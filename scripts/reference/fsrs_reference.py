"""FSRS reference values for module 02 (docs/modules/02-srs-and-learner-model.md, test 5).

Runs five review scenarios through py-fsrs 6.3.2 (MIT) and records, after every review, the
stability, difficulty, due time and interval that py-fsrs computes. The Rust scheduler is checked
against this file; nothing here reads or runs the Rust code.

Rerun from the repository root (uv fetches py-fsrs; the network is used by uv only):

    uv run --no-project --with fsrs==6.3.2 python scripts/reference/fsrs_reference.py

It rewrites, relative to the repository root:

- tests/cases/02-srs-and-learner-model/fsrs_reference.json (the recorded run), and
- tests/cases/02-srs-and-learner-model/srs-replay-reference/ (the golden case that replays the
  same reviews through `parley srs replay`: args.txt, input.json, expected.json, tolerance.json).

The scheduler is configured as the Rust side is: the 21 default parameters of the `fsrs` crate
6.6.2 (identical to py-fsrs's defaults; the script refuses to run if they differ), desired
retention 0.9, no learning or relearning steps, a maximum interval of 365 days and no fuzz.

Each recorded review has its time, rating, the whole days elapsed since the previous review,
`retrievability_before` (py-fsrs's retrievability at the review time, over whole days; for
information only), and the stability, difficulty, due time and interval py-fsrs gives after it.

Times are UTC milliseconds since the Unix epoch. py-fsrs rounds intervals half to even and Rust
rounds half away from zero, so the script refuses any review whose stability lies within 0.01 of
a half day below the 365-day cap.

The golden case's `state`, `reps` and `lapses` follow the module's card-state rules, not
py-fsrs's states: Again leaves a card `relearning`, any other rating `review`; `reps` counts every
review; `lapses` counts Again given to a card in state `review`.
"""

import importlib.metadata
import json
import math
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

from fsrs import Card, Rating, Scheduler
from fsrs import scheduler as fsrs_scheduler

PY_FSRS_VERSION = "6.3.2"
FSRS_RS_VERSION = "6.6.2"

# fsrs-rs 6.6.2 `DEFAULT_PARAMETERS` (FSRS-6, 21 values), written out.
PARAMETERS = (
    0.212,
    1.2931,
    2.3065,
    8.2956,
    6.4133,
    0.8334,
    3.0194,
    0.001,
    1.8722,
    0.1666,
    0.796,
    1.4835,
    0.0614,
    0.2629,
    1.6483,
    0.6014,
    1.8729,
    0.5425,
    0.0912,
    0.0658,
    0.1542,
)

DESIRED_RETENTION = 0.9
MAXIMUM_INTERVAL = 365

HOUR_MS = 3_600_000
DAY_MS = 86_400_000
EPOCH = datetime(1970, 1, 1, tzinfo=timezone.utc)

# 2026-01-05T09:00:00Z
T0 = 1_767_603_600_000

CASE_DIR = "tests/cases/02-srs-and-learner-model"
CASE_NAME = "srs-replay-reference"

RATINGS = {1: Rating.Again, 2: Rating.Hard, 3: Rating.Good, 4: Rating.Easy}
VERDICTS = {1: "wrong", 2: "almost", 3: "correct", 4: "correct"}


def at(offset_ms, rating):
    """A review `offset_ms` after T0."""
    return ("at", offset_ms, rating)


def after_last(offset_ms, rating):
    """A review `offset_ms` after the card's previous review (a same-day review when < 1 day)."""
    return ("after_last", offset_ms, rating)


def after_due(offset_ms, rating):
    """A review `offset_ms` after the card's previous due time (0: exactly on time)."""
    return ("after_due", offset_ms, rating)


SCENARIOS = [
    {
        "name": "new-card-each-rating",
        "description": "Four new cards, each reviewed once, rated Again, Hard, Good and Easy.",
        "cards": [
            {"item_id": "ref.s1.again", "reviews": [at(0, 1)]},
            {"item_id": "ref.s1.hard", "reviews": [at(0, 2)]},
            {"item_id": "ref.s1.good", "reviews": [at(0, 3)]},
            {"item_id": "ref.s1.easy", "reviews": [at(0, 4)]},
        ],
    },
    {
        "name": "good-streak",
        "description": "Good eight times: on time, five hours late, three days late, then on time.",
        "cards": [
            {
                "item_id": "ref.s2.good-streak",
                "reviews": [
                    at(1 * HOUR_MS, 3),
                    after_due(0, 3),
                    after_due(5 * HOUR_MS, 3),
                    after_due(3 * DAY_MS, 3),
                    after_due(0, 3),
                    after_due(0, 3),
                    after_due(0, 3),
                    after_due(0, 3),
                ],
            }
        ],
    },
    {
        "name": "lapse-and-recovery",
        "description": (
            "Good three times, Again a day late on a reviewed card (a lapse), Again again on the "
            "relearning card (no second lapse), then Good, Good, Hard and Good on time."
        ),
        "cards": [
            {
                "item_id": "ref.s3.lapse",
                "reviews": [
                    at(2 * HOUR_MS, 3),
                    after_due(0, 3),
                    after_due(0, 3),
                    after_due(1 * DAY_MS, 1),
                    after_due(0, 1),
                    after_due(0, 3),
                    after_due(0, 3),
                    after_due(0, 2),
                    after_due(0, 3),
                ],
            }
        ],
    },
    {
        "name": "same-day-reviews",
        "description": (
            "Same-day reviews take the short-term formula: Good, then Hard, Good, Again and Good "
            "within hours; Good on time; Easy an hour later; Hard ten hours late; Good on time."
        ),
        "cards": [
            {
                "item_id": "ref.s4.same-day",
                "reviews": [
                    at(3 * HOUR_MS, 3),
                    after_last(2 * HOUR_MS, 2),
                    after_last(3 * HOUR_MS, 3),
                    after_last(2 * HOUR_MS, 1),
                    after_last(1 * HOUR_MS, 3),
                    after_due(0, 3),
                    after_last(1 * HOUR_MS, 4),
                    after_due(10 * HOUR_MS, 2),
                    after_due(0, 3),
                ],
            }
        ],
    },
    {
        "name": "easy-run-to-cap",
        "description": "Easy six times on time: the interval reaches the 365-day cap and stays there.",
        "cards": [
            {
                "item_id": "ref.s5.easy-cap",
                "reviews": [
                    at(4 * HOUR_MS, 4),
                    after_due(0, 4),
                    after_due(0, 4),
                    after_due(0, 4),
                    after_due(0, 4),
                    after_due(0, 4),
                ],
            }
        ],
    },
]


def to_datetime(ms):
    return EPOCH + timedelta(milliseconds=ms)


def to_ms(moment):
    return (moment - EPOCH) // timedelta(milliseconds=1)


def check_environment():
    version = importlib.metadata.version("fsrs")
    if version != PY_FSRS_VERSION:
        sys.exit(f"py-fsrs {PY_FSRS_VERSION} is required, found {version}")
    if tuple(fsrs_scheduler.DEFAULT_PARAMETERS) != PARAMETERS:
        sys.exit("py-fsrs's DEFAULT_PARAMETERS differ from the fsrs crate's vector written here")


def near_half_day(stability):
    """Whether rounding `stability` to whole days could differ between half-to-even and
    half-away-from-zero: within 0.01 of n + 0.5, below the cap."""
    if stability > MAXIMUM_INTERVAL + 0.5:
        return False
    fraction = stability - math.floor(stability)
    return abs(fraction - 0.5) < 0.01


def run_card(scheduler, item_id, reviews, card_number):
    card = Card(card_id=card_number)
    last_ms = None
    due_ms = None
    state = "new"
    reps = 0
    lapses = 0
    recorded = []
    for kind, offset, rating in reviews:
        if kind == "at":
            now_ms = T0 + offset
        elif kind == "after_last":
            now_ms = last_ms + offset
        else:
            now_ms = due_ms + offset
        elapsed_days = 0 if last_ms is None else max(0, (now_ms - last_ms) // DAY_MS)
        retrievability = float(
            scheduler.get_card_retrievability(card, current_datetime=to_datetime(now_ms))
        )
        card, _ = scheduler.review_card(card, RATINGS[rating], review_datetime=to_datetime(now_ms))
        stability = float(card.stability)
        difficulty = float(card.difficulty)
        due_ms = to_ms(card.due)
        interval_days = (due_ms - now_ms) // DAY_MS
        if (due_ms - now_ms) % DAY_MS != 0:
            sys.exit(f"{item_id}: py-fsrs gave an interval that is not whole days")
        expected_interval = min(MAXIMUM_INTERVAL, max(1, round(stability)))
        if interval_days != expected_interval:
            sys.exit(f"{item_id}: interval {interval_days} is not max(1, round(S)) at retention 0.9")
        if near_half_day(stability):
            sys.exit(f"{item_id}: stability {stability} is within 0.01 of a half day; move the review")

        # The module's card-state rules (Design notes), worked out here, not taken from py-fsrs.
        if rating == 1 and state == "review":
            lapses += 1
        state = "relearning" if rating == 1 else "review"
        reps += 1

        recorded.append(
            {
                "reviewed_at": now_ms,
                "rating": rating,
                "elapsed_days": elapsed_days,
                "retrievability_before": retrievability,
                "stability": stability,
                "difficulty": difficulty,
                "due": due_ms,
                "interval_days": interval_days,
            }
        )
        last_ms = now_ms
    final = {
        "item_id": item_id,
        "facet": "receptive",
        "state": state,
        "stability": recorded[-1]["stability"],
        "difficulty": recorded[-1]["difficulty"],
        "due": recorded[-1]["due"],
        "last_review": recorded[-1]["reviewed_at"],
        "reps": reps,
        "lapses": lapses,
    }
    return recorded, final


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(value, f, indent=2, ensure_ascii=False)
        f.write("\n")


def main():
    check_environment()
    scheduler = Scheduler(
        parameters=PARAMETERS,
        desired_retention=DESIRED_RETENTION,
        learning_steps=(),
        relearning_steps=(),
        maximum_interval=MAXIMUM_INTERVAL,
        enable_fuzzing=False,
    )

    scenarios_out = []
    finals = []
    log_entries = []
    card_number = 0
    for scenario_number, scenario in enumerate(SCENARIOS, start=1):
        cards_out = []
        for card in scenario["cards"]:
            card_number += 1
            recorded, final = run_card(scheduler, card["item_id"], card["reviews"], card_number)
            cards_out.append({"item_id": card["item_id"], "facet": "receptive", "reviews": recorded})
            finals.append(final)
            for n, review in enumerate(recorded):
                entry = {
                    "item_id": card["item_id"],
                    "facet": "receptive",
                    "reviewed_at": review["reviewed_at"],
                    "rating": review["rating"],
                    "exercise_type": "translate_to_en",
                    "verdict": VERDICTS[review["rating"]],
                    "session_id": f"reference-{scenario_number}",
                }
                # response_ms may be absent or null in a log; both appear here.
                if n % 2 == 1:
                    entry["response_ms"] = None
                elif n % 4 == 2:
                    entry["response_ms"] = 3000 + 100 * n
                log_entries.append(entry)
        scenarios_out.append(
            {"name": scenario["name"], "description": scenario["description"], "cards": cards_out}
        )

    root = Path(__file__).resolve().parents[2]
    case_dir = root / CASE_DIR

    reference = {
        "schema": "parley.fsrs-reference/1",
        "generated_by": "scripts/reference/fsrs_reference.py",
        "py_fsrs_version": PY_FSRS_VERSION,
        "fsrs_rs_version": FSRS_RS_VERSION,
        "parameters": list(PARAMETERS),
        "scheduler": {
            "desired_retention": DESIRED_RETENTION,
            "learning_steps": [],
            "relearning_steps": [],
            "maximum_interval": MAXIMUM_INTERVAL,
            "enable_fuzzing": False,
        },
        "time_unit": "UTC milliseconds since the Unix epoch",
        "scenarios": scenarios_out,
    }
    write_json(case_dir / "fsrs_reference.json", reference)

    # Log order: by review time, then item id (the scenarios interleave as a learner's day would).
    log_entries.sort(key=lambda e: (e["reviewed_at"], e["item_id"]))
    case = case_dir / CASE_NAME
    write_json(case / "input.json", {"schema": "parley.review-log/1", "entries": log_entries})

    # Replay's order: item id in byte order (every card here is receptive).
    finals.sort(key=lambda c: c["item_id"].encode("utf-8"))
    write_json(case / "expected.json", {"cards": finals})
    tolerance = {}
    for i, card in enumerate(finals):
        tolerance[f"cards.{i}.stability"] = 1e-4 * abs(card["stability"])
        tolerance[f"cards.{i}.difficulty"] = 1e-4 * abs(card["difficulty"])
    write_json(case / "tolerance.json", tolerance)
    with open(case / "args.txt", "w", encoding="utf-8", newline="\n") as f:
        f.write("srs\nreplay\n{input}\n--json\n")

    reviews = sum(len(c["reviews"]) for s in scenarios_out for c in s["cards"])
    print(f"{len(scenarios_out)} scenarios, {len(finals)} cards, {reviews} reviews")


if __name__ == "__main__":
    main()
