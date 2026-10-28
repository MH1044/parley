# Parley

Learn Spanish, French or German from English by reading, listening, speaking and holding short
conversations, with spaced repetition underneath. Free and open source, for Windows, Linux and
macOS, with Android to follow.

**Status: pre-alpha.** Nothing teaches yet. The repository, the build, the tests and continuous
integration exist, the desktop window opens and shows the version, and the course exists as
validated open data: a syllabus format with a validator, word-form tables, and sentence packs
built from Tatoeba. `docs/ROADMAP.md` lists what comes next, in order; `docs/PRODUCT.md`
describes the finished app.

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

- Rust, stable, with rustfmt and clippy (rustup reads `rust-toolchain.toml`), and cargo-deny
  (`cargo install cargo-deny --locked`).
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

The desktop window (the front end is embedded at compile time, so build it first):

```sh
npm --prefix app run build
cargo run -p parley-app
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
| `content/packs.lock.toml` | The published sentence packs: URL, SHA-256, size per language |

`parley content validate` checks a syllabus file, or every `<dir>/<lang>/syllabus.yaml`, and
prints an exact message for every problem: the schema (types, required fields, enumerations),
then the shape rules of `docs/modules/01-content-model-and-syllabus.md`: id shapes, ordering,
sizes (3 to 20 lemmas and 4 to 15 examples per skill), genders on nouns, every example word
taught by this or an earlier skill, no repeated sentence, Unicode NFC, and the id lock.

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

## Repository

| Path | What |
|---|---|
| `crates/parley-core` | The learning logic: content, scheduling, grading, exercises, storage, AI features. Deterministic; clock, randomness, filesystem and network are injected |
| `crates/parley-publik` | The AI client: publik API, your own key, a local model |
| `crates/parley-audio` | Microphone, playback, OS voices, local transcription, media decoding |
| `crates/parley-cli` | The `parley` command-line tool and the `parley-fixtures` generator |
| `app/` | The React and TypeScript interface; `app/src-tauri/` is the Tauri shell |
| `content/` | The course: one folder per language with the syllabus, id lock, forms table, number words and exclude list; the pack lock file; licence and attribution |
| `schemas/` | JSON Schemas for the content formats |
| `scripts/` | `verify`, the licence gates, the release-notes check |
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
