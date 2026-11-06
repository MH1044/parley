# Parley

Learn Spanish, French or German from English by reading, listening, speaking and holding short
conversations, with spaced repetition underneath. Free and open source, for Windows, Linux and
macOS, with Android to follow.

**Status: pre-alpha.** The offline app works end to end: onboarding with a placement test, the
learning path, lessons and reviews with honest grading, the streak and stats, settings, backup and
restore, all with no network and no AI. The course covers A1 and A2 in Spanish, French and German
(16 units and about 64 skills, 540 words and 520 example sentences per language, most of them real
Tatoeba sentences), a labelled beta until native speakers have reviewed it. The sentence packs are
built and pinned by checksum but not yet published; `parley pack install` installs one for
development. Underneath: the
course as validated open data (a syllabus format with a validator, word-form tables, sentence
packs built from Tatoeba), the learner model (FSRS cards, an append-only review log, skill
mastery and states, an adaptive placement test) and the offline lesson engine (seven exercise
types, grading that forgives accents, articles and typos, template lessons, XP, streaks with
earned freezes and the weekly record), also usable from the command line. `docs/ROADMAP.md` lists
what comes next, in order; `docs/PRODUCT.md` describes the finished app.

## What it replaces

Parley replaces Duolingo Super, and covers what people pay Speak, Pimsleur and Language Reactor
for: a structured course with a real scheduler (FSRS), honest grading that forgives accents and
typos but never guesses, speaking checks, and conversation practice, without hearts, energy, ads
or a subscription.

The offline core is the whole course: placement, a learning path, lessons built from an open
CEFR-aligned syllabus and Tatoeba sentences, reviews, streaks with freezes you earn and never buy.
AI adds fresh lessons, judging of free translations, explanations, natural voices, speaking checks,
role-play, photo vocabulary and learning from your own audio and video, through publik API, your
own key or a local model, and every AI answer is checked before you see it.

## Build from source

You need:

- Rust 1.99.1 with rustfmt and clippy: rustup reads the exact version from `rust-toolchain.toml`
  and installs it on the first build. Also cargo-deny (`cargo install cargo-deny --locked`).
- Node.js LTS and npm.
- Windows: the Visual Studio C++ Build Tools and the WebView2 runtime (part of Windows 11; Windows
  10 gets it with Microsoft Edge).
- Linux: the Tauri prerequisites and the ALSA headers, on Debian and Ubuntu
  `sudo apt-get install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libasound2-dev`.
- macOS: the Xcode command line tools.

The full check builds the front end, then runs the format check, lint, typecheck, clippy, both
licence gates and every test. It is what continuous integration runs:

```sh
scripts/verify
```

PowerShell: `scripts/verify.ps1`.

The desktop app (the front end is embedded at compile time, so build it first):

```sh
npm --prefix app run build
cargo run -p parley-app
```

It opens on "Welcome to Parley" and walks you through choosing a language, the course pack, a
daily goal and the placement test; `docs/HANDBOOK.md` describes every screen. Your progress lives
in the app data folder (`parley.sqlite3` and `settings.json`), and Settings > Data exports it as
one JSON backup that restores on any computer.

The launch smoke test starts the debug build through `tauri-driver` and waits for the welcome
screen. It needs `tauri-driver` and the platform's WebDriver on the `PATH` (continuous
integration installs both):

```sh
npm --prefix app run build
cargo build -p parley-app
npm --prefix app run e2e
```

The command-line tool:

```sh
cargo run -q -p parley-cli -- --version --json
```

prints `{"name":"parley","version":"0.1.0"}`.

## Course content

The course is open data under `content/`, licensed CC BY-SA 4.0 (`content/LICENSE`,
ADR-0003), one folder per language:

| File | What |
|---|---|
| `content/<lang>/syllabus.yaml` | Units, skills, lemmas and example sentences with English translations |
| `content/<lang>/ids.lock` | Every item id ever published, active or retired, so an id is never renumbered or reused |
| `content/<lang>/forms.tsv` | Inflected form to lemma (`buenas` to `bueno`), derived from Wiktionary |
| `content/<lang>/numbers.yaml` | The words for 0 to 100 |
| `content/<lang>/exclude.txt` | Lemmas never taught or shown |
| `content/<lang>/checklist.yaml` | The grammar points and communicative functions of A1 and A2, each mapped to the skills that teach it |
| `content/<lang>/review.yaml` | Whether a native speaker has reviewed each unit, and since when |
| `content/scenarios/*.yaml` | The role-play scenarios: goals, a turn cap and, per language, the opening line and the skills it builds on |
| `content/packs.lock.toml` | The published sentence packs: URL, SHA-256, size per language |

`parley content validate` checks a syllabus file, or every `<dir>/<lang>/syllabus.yaml`, and
prints an exact message for every problem: the schema (types, required fields, enumerations),
then the shape rules of `docs/modules/01-content-model-and-syllabus.md`: id shapes, ordering,
sizes (3 to 20 lemmas and 4 to 15 examples per skill), genders on nouns, every example word
taught by this or an earlier skill, no repeated sentence, Unicode NFC, and the id lock. In a
course folder (one with `review.yaml`) every single-word lemma also needs its frequency rank.
For a directory it then checks each language's checklist and review file and every scenario
(`docs/modules/05-course-content-a1-a2.md`).

```sh
cargo run -q -p parley-cli -- content validate content
cargo run -q -p parley-cli -- content validate content/es/syllabus.yaml --json
```

The first prints `ok`; the second `{"ok":true,"errors":[],"warnings":[]}`. Any error exits 1
with its line, for example `es.a1.u01.s01: example 5 uses "tardes", which no earlier skill
teaches`.

After adding or removing lemmas or examples, record the item ids:

```sh
cargo run -q -p parley-cli -- content lock --lang es
```

prints what changed, `{"added":[...],"retired":[...]}`, and rewrites `content/es/ids.lock`.

Word-form tables come from a wiktextract extract of Wiktionary, and sentence packs from
Tatoeba's per-language exports (both downloaded into `data-cache/`, never committed):

```sh
cargo run -q -p parley-cli -- forms extract --lang es --wiktextract data-cache/es.jsonl \
    --syllabus content/es/syllabus.yaml --out content/es/forms.tsv
cargo run -q -p parley-cli -- pack build --lang es --tatoeba-dir data-cache/tatoeba \
    --wiktextract data-cache/es.jsonl --syllabus content/es/syllabus.yaml --out es.sqlite3
cargo run -q -p parley-cli -- pack info es.sqlite3
```

A pack keeps every sentence's author (CC BY 2.0 FR) and reports why each dropped sentence was
dropped; `docs/HANDBOOK.md` shows each command's output.

The scenarios can be checked on their own, against the syllabi of `content/` (or `--content
<dir>`):

```sh
cargo run -q -p parley-cli -- content scenarios content/scenarios
```

prints `{"ok":true,"errors":[],"warnings":[]}`, or every problem, such as `cafe: es: opening
uses "pongo", which the listed skills do not teach`.

A course report counts the units, skills, lemmas and examples of a language, the checklist
entries no skill covers and the units still awaiting review, and with a pack the share of
examples that are real Tatoeba sentences:

```sh
cargo run -q -p parley-cli -- content report --lang es --pack data-cache/packs/es-2026-11-05.sqlite3 --json
```

Lemma ranks are the pack's frequency ranks. `freq check` lists every lemma whose `rank` differs
from the pack's and every lemma the pack does not rank, and exits 1 until there are none:

```sh
cargo run -q -p parley-cli -- freq check --lang es --pack data-cache/packs/es-2026-11-05.sqlite3
```

Until the packs are published as release assets, a locally built pack that
`content/packs.lock.toml` pins is installed into a course started in the app with

```sh
cargo run -q -p parley-cli -- pack install data-cache/packs/es-2026-11-05.sqlite3 --data-dir <app data folder>
```

which prints `{"lang":"es","version":"2026-11-05","path":"packs/es-2026-11-05.sqlite3"}`;
`docs/HANDBOOK.md` says where the app data folder is on each system.

Units still marked `unreviewed` in `review.yaml` ship as a labelled beta until a native speaker
has reviewed them.

## Scheduling and placement

Every item of the course becomes two FSRS cards, one for recognising it and one for producing
it, scheduled by the `fsrs` crate with its default FSRS-6 parameters, a desired retention from
0.80 to 0.95 (default 0.90), no fuzz and intervals of at most 365 days. Every review is appended
to a log in the learner's `parley.sqlite3` that is never edited, and the cards can always be
rebuilt from it:

```sh
cargo run -q -p parley-cli -- srs replay review-log.json
```

prints `{"cards":[...]}`, one card per item and facet the log mentions (`--retention <r>`
changes the due dates only). `docs/HANDBOOK.md` shows a complete log and its output.

The placement test is a staircase over the course's skills that asks 12 to 20 questions (fewer
for a beginner), estimates where to start and gives a CEFR label. Against a simulated learner
who knows exactly the skills below a boundary:

```sh
cargo run -q -p parley-cli -- placement simulate --syllabus content/es/syllabus.yaml --boundary 5
```

prints

```
{"skills":8,"questions":[2,4,6,5,4,5,4,5,4,5,4,5],"answers":[true,true,false,false,true,false,true,false,true,false,true,false],"reversals":[6,4,5,4,5,4,5,4,5],"estimate":5,"label":"around A1","tested_out":["es.a1.u01.s01","es.a1.u01.s02","es.a1.u01.s03","es.a1.u01.s04","es.a1.u02.s01"]}
```

`--slip <p>` flips each answer with probability `p` and `--seed <s>` seeds the generator.

## Lessons, grading and streaks

A lesson is up to 12 exercises of seven types (pick the meaning, translate either way, listen and
type, speak, fill the gap, match the pairs), planned from the skill's new items and your due
cards and built offline from the syllabus with a seeded generator, so the same seed gives the
same lesson on every machine. Answers are graded in a fixed order: exact (case, punctuation and
spacing never count), a missing or wrong article, an accent slip (almost by default; strict or
ignore in the settings), one wrong letter in a few long words, then a stored judgement; a free
translation nothing matches is left unchecked for "I was right", never guessed.

```sh
cargo run -q -p parley-cli -- grade --lang es --type listen_type --expected "Buenos días." --answer "Buenos dias"
```

prints

```
{"verdict":"almost","tags":["accent"],"message":"Almost: mind the accent: días","corrected":"Buenos días.","words":["días"],"rating":2}
```

`parley lesson template --lang es --skill es.a1.u01.s01 --seed 1` prints the first template lesson
of a skill (`schemas/lesson.schema.json`), and `parley streak simulate days.json` the streak,
freezes and weekly records a list of sessions gives: every 7 days in a row earn a freeze (at most
2 banked) that covers a missed day; freezes are never bought. There are no hearts, no energy and
no limit on lessons. `docs/HANDBOOK.md` shows each command with its output.

## Repository

| Path | What |
|---|---|
| `crates/parley-core` | The learning logic: content, scheduling, grading, exercises, storage, AI features. Deterministic; clock, randomness, filesystem and network are injected |
| `crates/parley-publik` | The AI client: publik API, your own key, a local model |
| `crates/parley-audio` | Microphone, playback, OS voices, local transcription, media decoding |
| `crates/parley-cli` | The `parley` command-line tool and the `parley-fixtures` generator |
| `app/` | The React and TypeScript interface; `app/src-tauri/` is the Tauri shell |
| `content/` | The course: one folder per language with the syllabus, id lock, forms table, number words and exclude list; the pack lock file; licence and attribution |
| `schemas/` | JSON Schemas for the content formats, lessons, settings and backups |
| `scripts/` | `verify`, the licence gates, the release-notes check; `gates/` holds the quality gates (`scripts/gates/run`) and their thresholds |
| `tests/cases/` | Golden cases run through the `parley` binary; `tests/fixtures/` holds generated and hand-written fixtures |
| `docs/` | Product, architecture, Publik integration and contract, testing, roadmap, one spec per module, decision records, the handbook |

## Documentation

- `docs/PRODUCT.md`: who it is for, every feature, every screen.
- `docs/ARCHITECTURE.md`: stack, layout, data model, commands, invariants, decisions.
- `docs/PUBLIK-INTEGRATION.md` and `docs/PUBLIK-CONTRACT.md`: every AI call, its prompt, schema,
  cost and errors.
- `docs/TESTING.md`: test layers, commands, fixtures, quality gates, the manual checklist.
- `docs/ROADMAP.md` and `docs/modules/`: the modules, in order, with their specifications.
- `docs/adr/`: decision records.
- `docs/HANDBOOK.md`: the user guide, grown with the app.
- `CONTRIBUTING.md`, `SECURITY.md`, `RELEASING.md`, `THIRD_PARTY.md`.

## Licence

The code is licensed under either the MIT licence (`LICENSE-MIT`) or the Apache License, Version
2.0 (`LICENSE-APACHE`), at your option (ADR-0002). The course content under `content/` is
CC BY-SA 4.0 (`content/LICENSE`, ADR-0003), which lets its forms tables be derived from
Wiktionary; sentence packs built from Tatoeba are CC BY 2.0 FR with every author kept.
`THIRD_PARTY.md` lists every dependency with its licence.
