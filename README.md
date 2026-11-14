# Parley

Learn Spanish, French or German from English by reading, listening, speaking and holding short
conversations, with spaced repetition underneath. Free and open source, for Windows, Linux and
macOS, with Android to follow.

**Status: pre-alpha.** The offline app works end to end: onboarding with a placement test, the
learning path, lessons and reviews with honest grading, the streak and stats, settings, backup and
restore, all with no network and no AI. AI can be set up, when you choose to, through publik
API, your own OpenAI-compatible key or a local model server; with it, lessons are written fresh
for you and each unit ends with a short graded story, every AI exercise is checked before you
see it, a free translation the offline grader cannot match is checked by an AI judge (and
remembered, so the same answer is accepted offline next time), and **Explain** says why an answer
was almost right or wrong. Every sentence in the course language can be heard, at normal speed or
slowly, through your computer's own voice or, if you choose, a natural voice; lessons include
listen-and-type exercises, and say-this exercises that check, word by word, a sentence you say
into the microphone, through your AI provider or a local Whisper model. The course covers A1 and A2 in
Spanish, French and German
(16 units and about 64 skills, 540 words and 520 example sentences per language, most of them real
Tatoeba sentences), a labelled beta until native speakers have reviewed it. The sentence packs are
published as release assets and pinned by checksum, and the app downloads the chosen language's
pack in onboarding; `parley pack install` installs a locally built one for development.
Underneath: the
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

Or, as one command that builds an optimised app: `npm --prefix app run desktop`.

Local speaking checks (local Whisper through whisper.cpp) are an optional feature, off by default,
because building whisper.cpp also needs cmake and libclang (on Debian and Ubuntu `sudo apt-get
install cmake libclang-dev`):

```sh
cargo run -p parley-app --features local-whisper
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

The app downloads the published packs itself. For development, a locally built pack that
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
translation nothing matches is checked by the AI judge when AI is set up, and otherwise left
unchecked for **Check with AI** or "I was right", never guessed.

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

## Listening

Every sentence in the course language has **Play** and **Slow** beside it: the prompt of a
pick-the-meaning or translate-into-English exercise, and the correct answer on the result sheet
when it is in the course language. With **Play prompts aloud** on (the default), each plays once
by itself. A lesson includes up to two **Type what you hear** exercises while a voice is available;
**Can't listen now** skips them for an hour, with no grade.

The voice is your computer's own by default, free and offline: Windows' speech voices (Settings >
Time & language > Speech adds one), the macOS system voices, or Speech Dispatcher on Linux. When
none is installed for the language, Parley says how to add one. Settings > Audio chooses **System
voice** or **Natural voice**, the voice of each language, the **Slow speed** (0.5 to 1.0, 0.75 by
default), **Play prompts aloud**, and **Clear audio cache**. A natural voice comes from your AI
provider (publik API, or your own key with a speech model); each sentence is fetched once, checked
(it must be audio of a plausible length) and kept in the audio cache, so it is never paid for twice
and replays offline, and the next exercise's sentence is fetched while you answer. Settings > Audio
shows what a sentence costs ("Natural voices cost about $0.00054 a sentence (measured)."), from
the charges your own sentences have had. When a natural voice cannot be used, the system voice
reads the sentence instead ("Natural voice unavailable, using the system voice."); at $0.00 the
system voice is used without asking again.

## Speaking

A lesson includes up to two **Say this** exercises while speaking is possible: a microphone,
**Speaking exercises** on (Settings > Speaking), and a way to check them: your AI provider (publik
API, or your own key with a transcription model) or a downloaded local model. Hold **Hold to talk**
(or Space) and say the sentence; the level meter shows the microphone hears you, and a take stops
by itself at 15 seconds. **Replay my take** plays it back. **Check** sends that one take (16 kHz
mono WAV with the silence trimmed, and never the expected sentence) to be transcribed, and scores
it on your computer: "Parley heard: …", each word right, missed or heard as another (each with a
colour, an icon and a text label), the extra words, and "Score 80% (pass mark 80%)". Digits count
as the numbers they are (`3` is `tres`), and accents, hyphens and case are forgiven. The first
checked take of an exercise rates its card; **Try again** is practice. **Play the voice** plays the
sentence, and **Can't speak now** skips speaking exercises for an hour, with no grade.

Takes stay on your computer, in `cache/recordings/`, and are removed when the lesson ends, when
Parley quits and at the next launch, unless **Keep my recordings** is on. Settings > Speaking holds
**Speaking exercises**, **Check my speaking with** (**AI provider** or **Local model**), the local
models (tiny 77.7 MB, base 148.0 MB, small 487.6 MB) with **Download** and **Use**, the **Pass
mark** (50% to 100%, 80% by default), **Keep my recordings**, and what a check costs ("Speaking
checks cost about $0.00054 a take (measured)."). A local model is downloaded only when you ask,
and kept only once its size and SHA-256 match `models/registry.toml`; offline, with your provider
chosen, a downloaded model checks the take instead ("You're offline, so the local model checked
this."). A build without local Whisper says "Local speech checks are not available in this build."
and never sends a take to a provider you did not choose.

`parley speak score input.json` scores a transcript against a sentence the same way, offline. With
`{"lang": "es", "expected": "Tengo tres gatos y un perro.", "heard": "pues tengo 3 gatos y perro"}`:

```sh
cargo run -q -p parley-cli -- speak score input.json
```

prints the word missed, the extra word, and a pass at the 0.8 pass mark:

```json
{"words":[{"expected":"Tengo","heard":"tengo","status":"ok"},{"expected":"tres","heard":"tres","status":"ok"},{"expected":"gatos","heard":"gatos","status":"ok"},{"expected":"y","heard":"y","status":"ok"},{"expected":"un","heard":null,"status":"missed"},{"expected":"perro","heard":"perro","status":"ok"}],"extra":["pues"],"score":0.8333333333333334,"pass":true,"rating":3}
```

`parley whisper download base` downloads one local model into `models/whisper/` (or `--dir`) and
checks it; it reaches the network (148 MB for `base`).

## AI providers

Parley works without AI, and never asks about it at launch or during onboarding. When you choose
**Set up AI** on the path, a dialog says what would leave your computer, shows publik's own
disclosure, and offers three equal ways: **Continue with publik API** (no key to paste, priced
per use in dollars from a publik balance that starts at $0.00; linking this computer to a publik
account gives $0.05 of free use, once), **Use my own key** (any OpenAI-compatible API) or **Use a
local model** (Ollama or another OpenAI-compatible server on this computer). Settings > AI shows
all three, which one is in use, the publik API balance, and **Check connection**, which makes one
tiny call and says what it cost ("Connected. This check cost $0.000017."). Only the provider you
chose is ever contacted. Your own key is kept in the operating system's keychain, never in
`settings.json`, a backup or anything the interface receives; `SECURITY.md` lists every network
call the app can make.

With a provider set up, each lesson is written fresh for your skill and the words you know
("Writing a fresh lesson…"; after 5 seconds **Use the course pack instead** starts the offline
lesson at once). Every exercise is checked before you see it: the right language, only words the
course has taught, answers that fit, no repeats, the level's length, and the word being practised;
an exercise that fails is replaced from the course pack, and if more than half fail the whole
lesson comes from the course pack with a note saying so. The same lesson is never paid for twice,
and the next lesson of a skill is prepared while you finish one (Settings > AI, **Prepare the next
lesson**). Each unit ends with **Read a story**, a short graded story in the course language with
questions in English, once the unit's last skill is practised; stories earn XP and never change
your reviews. At $0.00, or offline, lessons simply come from the course pack. Settings > Data
exports the lessons you have as a course pack and imports one from someone else, checked the same
way.

A free translation the offline grader cannot match ("Checking…" for a few seconds) is judged
correct, almost or wrong, with the corrected answer and a short reason; a correct answer is
remembered, so the same answer is accepted offline from then on, whichever provider judged it.
Without a provider, or when the check could not be made, the answer shows "Not checked" with the
reason, **Check with AI** (which opens the AI setup dialog when AI is not set up yet) and **I was
right**; at $0.00 the judge stops for the rest of the lesson. After an answer that was almost right
or wrong, **Explain** gives up to 80 words of plain English about the mistake and, often, a tip; an
explanation is never paid for twice.

For the maintainer only, `scripts/publik-proof` makes one real publik API call and prints the
reply, the charge and the `x-publik-*` headers; without `PUBLIK_PROOF=1` it sends nothing. A
built app started with `PUBLIK_SELF_TEST=1` prints one `publik self-test:` line and exits, without
opening a window.

## Repository

| Path | What |
|---|---|
| `crates/parley-core` | The learning logic: content, scheduling, grading, exercises, storage, AI features. Deterministic; clock, randomness, filesystem and network are injected |
| `crates/parley-publik` | The AI client: publik API, your own key, a local model |
| `crates/parley-audio` | Microphone, playback, OS voices, local transcription, media decoding |
| `crates/parley-cli` | The `parley` command-line tool and the `parley-fixtures` generator |
| `app/` | The React and TypeScript interface; `app/src-tauri/` is the Tauri shell |
| `content/` | The course: one folder per language with the syllabus, id lock, forms table, number words and exclude list; the pack lock file; `voices.toml`, the natural voice of each language; licence and attribution |
| `models/registry.toml` | The local Whisper models: URL, size and SHA-256 of each; the files themselves are downloaded on request into the app data folder and never committed |
| `schemas/` | JSON Schemas for the content formats, lessons, settings and backups; `schemas/ai/` one per AI answer |
| `prompts/` | The AI prompts, one file per feature and version (`ping.v1.md` the connection check, `lesson.v1.md` fresh lessons, `story.v1.md` graded stories, `judge.v1.md` answer checks, `explain.v1.md` explanations), compiled into the app |
| `scripts/` | `verify`, the licence gates, the release-notes check, `publik-proof` (the maintainer's one real publik API call); `gates/` holds the quality gates (`scripts/gates/run`) and their thresholds |
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
