# Parley: testing

## 1. Layers and where they live

| Layer | Where | Runner |
|---|---|---|
| Unit tests | beside the code, `#[cfg(test)]` | `cargo test` |
| Module tests | `crates/<crate>/tests/mNN_<slug>.rs`, e.g. `crates/parley-core/tests/m03_exercise_engine_and_grading.rs` (the `m` keeps target names starting with a letter) | `cargo test` |
| Golden cases | `tests/cases/NN-slug/<case>/` with `args.txt` (one CLI argument per line), optional `stdin.txt` and `input.json`, and `expected.json` | `crates/parley-cli/tests/golden.rs` runs the `parley` binary |
| Property tests | inside module tests, `proptest` with fixed seeds recorded in `proptest-regressions/` | `cargo test` |
| UI unit tests | `app/src/**/<name>.test.tsx` | Vitest |
| Launch smoke | `app/e2e/launch.spec.ts` | tauri-driver + WebdriverIO |
| Quality gates | `scripts/gates/run` (offline) and `scripts/gates/run --live` (spends money, maintainer only); the directory and `crates/parley-core/tests/gates.rs` are created by module 03 | `cargo test --release -- --ignored gate_` |
| Hygiene | `crates/parley-core/tests/hygiene.rs` | `cargo test`, in `verify` and CI |

Golden comparison: stdout is parsed as JSON and compared structurally with `expected.json`.
Integers and strings compare exactly; floats compare with the tolerance in the case's optional
`tolerance.json` (`{"path.to.field": 1e-4}`), default 1e-9. `PARLEY_BLESS=NN-slug` rewrites the
expected files of that one module only, for a new case or a change the spec and commit body
describe; the diff is read line by line.

## 2. Commands

- Full check, green before every commit: `scripts/verify` (PowerShell: `scripts/verify.ps1`). In order,
  stopping at the first failure: `npm ci --prefix app` when `app/node_modules` is missing;
  `npm --prefix app run build` (so `frontendDist` exists before any cargo step); `cargo fmt --all --
  --check`; `npm --prefix app run lint`; `npm --prefix app run typecheck`; `cargo clippy --workspace
  --all-targets -- -D warnings`; `cargo deny check licenses`; `cargo test --workspace`;
  `npm --prefix app test -- --run`; and, added by module 01 once the command exists,
  `cargo run -q -p parley-cli -- content validate content`.
- One module: `cargo test -p parley-core --test m03_exercise_engine_and_grading` and
  `cargo test -p parley-cli --test golden -- 03-exercise-engine-and-grading`.
- Fixtures: `cargo run -q -p parley-cli --bin parley-fixtures -- --out tests/fixtures/generated`.
- Launch smoke: `npm --prefix app run e2e` (after `cargo tauri build --debug`).
- Local Whisper tests: `cargo test -p parley-audio --features local-whisper` (skip with
  "Whisper model not found at models/whisper/ggml-tiny.bin; run parley whisper download tiny" when
  the model is absent).
- Gates: `scripts/gates/run` (offline gates); `scripts/gates/run --live G1` (sets `PARLEY_LIVE=1`;
  costs are listed per gate; only the maintainer runs live gates). PowerShell: `scripts/gates/run.ps1`.

## 3. Rules

- **No test reaches the network.** The core gets `Clock`, `Rng`, `Fs` and `Net` as traits; tests pass
  fakes. The Publik fake server (`crates/parley-publik/tests/fake.rs`) replays status codes, headers
  and bodies from `tests/fixtures/ai/` and records every request for assertions.
- **Expected values come from references**, never from running the implementation: a published
  algorithm computed by hand in the spec, a recorded reference run with its version, or a fixture
  true by construction.
- **Determinism.** Every random fixture has a fixed seed. Anything compared byte for byte uses
  integer maths (no platform `sin` or `exp`). The generated fixtures are identical on Windows and
  Linux; `parley-fixtures --check` compares their SHA-256 with `tests/fixtures/generated/manifest.json`.
- **Personal data.** No real recordings, photos of people or documents. Speech in tests is text;
  audio fixtures are generated tones and silence; photos are generated images.

## 4. Fixture generators and what they prove

| Fixture | Built by | Proves by construction |
|---|---|---|
| `audio/tone_gaps_*.wav` | `parley-fixtures`: integer square and triangle waves at 16 kHz with silence at known sample indices, and a noise floor from a seeded LCG | trim points and chunk boundaries are known exactly |
| `audio/media_23min` (generated at test time into a temp dir, 44 MB, never committed) | tones with 300 ms gaps every 106 s | 13 chunks, each under 4 MB |
| `images/exif_4000x3000.jpg` | a gradient with EXIF (orientation 6, GPS, camera), XMP, IPTC and ICC segments written by the generator | resize to 1568 px on the long side, rotation applied, all four blocks removed |
| `learners/*.json` | seeded simulated learners: a boundary ability and slip and guess rates | placement estimate tolerances |
| `syllabus-mini/xx/syllabus.yaml` | 80 synthetic skills in a pseudo-language `xx` with generated lemmas | ordering, unlocking and placement without real content |
| `tatoeba-mini/` (hand-written, CC0, committed) | 40 Spanish and 40 English project-authored sentences in Tatoeba's TSV formats with links | pack builder keeps and drops known sentences for known reasons |
| `forms-mini.jsonl` (hand-written, CC0) | 30 lemmas in wiktextract's line shape | forms table build and lookups |
| `ai/<row>/*.json` (hand-written) | one valid response per AI row and one failing response per validation rule | each rule rejects exactly what it should |
| `http/*.json` | header sets for 200, 402 (anonymous and claimed), 401, 403, 429, 503 | status parsing and the error table |

## 5. References

- **FSRS**: `scripts/reference/fsrs_reference.py`, run once with `uv run --with fsrs==<version>`
  (py-fsrs, MIT, verify at 02) over the scenarios in `tests/cases/02-srs-and-learner-model/`;
  output recorded in `fsrs_reference.json` with the py-fsrs and fsrs-rs versions and the parameter
  vector. The FSRS algorithm description in the open-spaced-repetition wiki gives the formulas.
- **Placement**: Levitt (1971); expected results from simulated learners whose boundary is known.
- **Edit distances and alignment**: Damerau (1964), Levenshtein (1966), Wagner and Fischer (1974);
  cases computed by hand in the module 03 and 10 specs.
- **Unicode**: UAX #15 and UAX #29; cases in the module 01 and 03 specs.
- **Language identification**: project-written sentences of known language.
- **Cost arithmetic**: `docs/PUBLIK-INTEGRATION.md` §8.

## 6. Quality gates

Thresholds marked "first run" are set by the first run that measures them, recorded in
`scripts/gates/thresholds.toml` with the date and version, and only tightened afterwards.

| Gate | What | Threshold | Cost | Scaled-down in `verify` |
|---|---|---|---|---|
| G1 lessons (live) | 30 lesson requests: 10 skills × es, fr, de on publik-balanced | ≥ 80% exercises valid; p95 ≤ 20 s | about $0.45 | recorded responses replayed through validators |
| G2 judge (live) | `tests/gates/judge-set.jsonl`: 300 labelled answers, 100 per language | ≥ 90% agreement; ≤ 3% false accepts; p95 ≤ 4 s | about $0.05 | 10 recorded verdicts |
| G3 templates (offline) | 1,000 seeded template lessons over the real syllabi | 100% pass the lesson validators | none | 50 seeds |
| G4 speaking loopback (offline, desktop) | 60 sentences per language synthesised by an OS voice, transcribed by local Whisper `base`, scored | mean score: first run | none | 5 sentences when voice and model exist, else skipped with a message |
| G5 role-play (live) | 20 turns in 2 scenarios | first delta p50 ≤ 2.5 s; ≥ 95% valid turns | about $0.10 | recorded streams |
| G6 placement (offline) | 1,000 noisy simulated learners | ≥ 90% within ±4 skills | none | 200 runs |
| G7 photo (live) | the maintainer's object photos in `eval-data/photos/` (git-ignored) with labels | ≥ 90% correct article and gender | about $0.20 | fixture responses |
| G8 daily cost (live) | a scripted daily-learner session | ≤ $0.10 in the recommended configuration | about $0.10 | none |

## 7. Source-hygiene test

A test that fails the build when a file about to be published contains something that would
identify a person or a machine, or something shaped like a secret. It runs in the normal test
suite, so in `verify` and CI. It names no person and no address.

**Where it lives**: `crates/parley-core/tests/hygiene.rs`, walking up from `CARGO_MANIFEST_DIR` to
the repository root (a root `tests/` folder is never compiled in a Cargo virtual workspace).

**Which files**: the output of `git ls-files -z --cached --others --exclude-standard`, skipping
paths that no longer exist, so ignored files are never scanned. Without git, walk the tree skipping
every path `.gitignore` ignores, plus `.git`, `node_modules`, `target`, `dist`, `build`, `out`, `coverage`, `release`, `.venv`,
`__pycache__`, and the root `models/` and `eval-data/` folders (except `models/registry.toml`).
Text is decided by extension: `.rs .ts .tsx .mts .cts .js .jsx .mjs .cjs .py .toml .json .jsonc .yml
.yaml .md .html .css .txt .sh .ps1 .xml .svg .csv .tsv .lock`, plus `LICENSE*` and `COPYING*`.
Exempt from the email rule only: `package-lock.json`, `Cargo.lock`, `uv.lock` and `THIRD_PARTY.md`.

**Tests**:

1. The walk found more than N files (N about half the current count).
2. No control characters other than tab, LF and CR.
3. No home-directory paths: `(?:[A-Za-z]:[\\/]+Users[\\/]+|/home/|/Users/)[A-Za-z0-9._-]+`.
4. No email addresses except `^\d+\+[A-Za-z0-9-]+@users\.noreply\.github\.com$`,
   `^noreply@(?:anthropic|github)\.com$`, `@(?:example|invalid|localhost|test)(?:\.[A-Za-z]{2,})?$`,
   `\.(?:example|invalid|test)$`.
5. No secret-shaped strings: `pk_(?:live|test)_[A-Za-z0-9]{16,}`, `pbt_[A-Za-z0-9]{16,}`,
   `sk-[A-Za-z0-9_-]{20,}`, `AIza[0-9A-Za-z_-]{35}`, `gh[pousr]_[A-Za-z0-9]{30,}`,
   `github_pat_[A-Za-z0-9_]{30,}`, `xox[baprs]-[A-Za-z0-9-]{10,}`,
   `-----BEGIN (?:RSA |EC |OPENSSH |)PRIVATE KEY-----`. The one allowed token is the `pat_…` app
   token in `crates/parley-publik/publik-app-token.txt`. Test fakes stay short (`pk_live_a`).

Failure messages say what to do: "use a path relative to the repository root, or an environment
variable"; "a real address in a public repository cannot be taken back"; "write the character as an
escape sequence"; "remove the key and rotate it".

**Proving it works** (module 00, never committed): plant in an untracked temp file one home path,
one email at a real-looking domain, a 40-character fake `sk-` key and a control character; see
exactly four failures; delete the file.

## 8. CI matrix

| Job | Runners | From module | Steps |
|---|---|---|---|
| verify | `windows-latest`, `ubuntu-latest` | 00 | Node LTS, Rust from `rust-toolchain.toml`; on Ubuntu `libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libasound2-dev`; `scripts/verify` |
| e2e | `windows-latest`, `ubuntu-latest` (xvfb) | 04 | debug build, launch smoke |
| whisper | `ubuntu-latest` | 10 | `--features local-whisper`; ggml-tiny cached by SHA-256 from `models/registry.toml` |
| android | `ubuntu-latest` | 13 | Android SDK and NDK; `cargo tauri android build --debug`; unit tests only |
| macos | `macos-latest` | 14 | `scripts/verify` and a bundle build |
| release | on a `v*` tag | 14 | refuses without `docs/releases/<version>.md`; verify, build, draft, upload, publish when green |

## 9. Manual checklist, by screen

Run before each release on Windows, and on each other platform where it ships.

- **Onboarding**: offline first launch reaches the first exercise in ≤ 90 s; pack download shows its
  size; a corrupted pack (edit one byte of the downloaded file) gives "The download was damaged. Try
  again."; placement ends between 12 and 20 questions; no AI dialog appears.
- **Path**: skill states match module 02's rules after finishing a skill; locked skills name the
  skill that opens them; AI chip changes with the provider.
- **Lesson player**: every exercise type by keyboard only; word bank and typing; accent and typo
  verdicts; I was right; quit and resume; 20 lessons in a row with no limit.
- **Result sheet**: differences highlighted; Explain appears only after almost or wrong; screen
  reader reads the verdict.
- **Reviews**: due count matches the badge; empty state shows the next due time.
- **Stats**: streak calendar after a missed day with a freeze; weekly record message.
- **AI setup**: opens from each entry point, never at launch; three buttons equal; $0.00 first-run
  card; the claim link opens the browser; 402 shows one link; Not now cancels the action.
- **Listening**: OS voice per language; a language with no voice shows the install help; slow mode;
  natural voice replays from cache offline.
- **Speaking**: microphone permission denied path; too quiet; replay own take; Can't speak now.
- **Conversation**: streaming text, withdrawal on a forced invalid reply (fake provider), turn cap,
  cost line, summary items saved.
- **Photo and media**: metadata stripped (inspect the sent file in the fake server log); batch
  confirmation for a long file; Continue after a stop.
- **Settings and data**: backup, delete everything, restore, and the progress is identical; clear
  caches keeps progress.
- **Accessibility**: keyboard only through a full lesson; contrast in light and dark themes;
  reduced motion honoured.
