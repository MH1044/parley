# Contributing to Parley

Parley is pre-alpha and is built one module at a time, in the order of `docs/ROADMAP.md`, each
from its specification in `docs/modules/`. Read `docs/ARCHITECTURE.md` and `docs/TESTING.md`
first; for anything that makes an AI call, also `docs/PUBLIK-INTEGRATION.md` and
`docs/PUBLIK-CONTRACT.md`.

## The full check

`scripts/verify` (PowerShell: `scripts/verify.ps1`) is what continuous integration runs, minus the
end-to-end launch smoke. It must be green before every commit, with no warnings. Windows
PowerShell 5.1 has no `&&`: run the steps separately and read `$LASTEXITCODE`.

## The clean-room rule

Parley is permissively licensed (ADR-0002). Never have the source of a GPL, AGPL, SSPL, BUSL or
source-available project open, in an editor, a browser tab or an assistant's context, while writing
the corresponding part of Parley. `docs/PRODUCT.md` names LibreLingo and Anki, both AGPL-3.0:
study their documentation, never their source. Documentation, papers, issue trackers and
file-format specifications are fine. Permissive prior art (MIT, BSD, ISC, Apache-2.0, MPL-2.0,
CC0) may be studied and ported with attribution in `THIRD_PARTY.md`.

## Dependencies

- Permissive licences only. `deny.toml` (Rust, transitive dependencies included) and
  `scripts/check-npm-licences.mjs` (the front end) fail the build on anything else; both run in
  `scripts/verify`.
- Every new dependency needs a one-line justification in the commit body and a row in
  `THIRD_PARTY.md`. Prefer boring, widely used packages with more than one maintainer. A dependency
  with one maintainer gets a decision record in `docs/adr/` that names the exit.
- Models and datasets are dependencies too: each has a row in `THIRD_PARTY.md` with licence and
  source. Non-commercial or research-only weights are never shipped. Model weights are never
  committed: `models/registry.toml` pins the URL and SHA-256 of each file, the app downloads on
  explicit user action, verifies the checksum, and says how big the download is first.
- Public datasets under CC BY and similar licences are downloaded by checksum and never committed.
  Committed fixtures are generated, or CC0 or public domain with the source recorded.

## Determinism

- Core code never reads the clock, randomness, the filesystem or the network directly. They arrive
  as the traits in `crates/parley-core/src/env.rs`, and tests pass fakes.
- Every random fixture has a fixed seed. A fixture that is compared byte for byte is built with
  integer maths or a pure-software maths library, never the platform's `sin` or `exp`, so it is
  identical on Windows and Linux. `parley-fixtures --check` proves it.
- Floats are compared with a stated tolerance, never with `==`.
- No test reaches the network. Every HTTP call is faked. A test that needs a model file skips with a
  clear message when the file is absent.
- Golden files are the specification, not a cache. Expected values come from a reference (a
  published algorithm, a reference implementation run once and recorded with its version, or a
  fixture whose truth is known by construction), never from running the code. Regenerate an
  expected file only for a new case or a change the spec and commit body describe, filtered to the
  one module (`PARLEY_BLESS=NN-slug`), and read the diff line by line.

## Tests and commits

- Every feature and every user-facing error message has at least one test. `docs/TESTING.md`
  says where each kind of test lives.
- Nothing that identifies a person, an account or a machine goes into a file or a commit message:
  no home-directory paths, no email address except a GitHub noreply one, no keys or tokens, no
  hostnames, no personal data in fixtures. The source-hygiene test
  (`crates/parley-core/tests/hygiene.rs`) fails the build on any of them.
- One logical change per commit, revertable on its own, with a conventional subject
  (`feat`, `fix`, `test`, `docs`, `chore`, `refactor`, `perf`, `ci`) and a body that says what
  changed and why, in product terms.
- Talk in dollars. Interface text, the documentation and this file never say "credits".

## publik API
- Base URL: https://publikhq.com/api/v1 (OpenAI Chat Completions, Responses, Anthropic Messages)
- Models: publik-fast, publik-balanced, publik-smart only; never a vendor model name
- Key: per-install `pk_` from POST /installs, stored at the publik credential path; env PUBLIK_API_KEY / PUBLIK_API_BASE_URL override
- App token: publik-app-token.txt (public by design, committed) or PUBLIK_APP_TOKEN
- 402: show error.message plus error.top_up_url only
- Never print or log a key
- Self-test: PUBLIK_SELF_TEST=1 makes the built app print one `publik self-test:` line and exit
- Docs: https://publikhq.com/llms.txt, https://publikhq.com/developers, https://publikhq.com/skills/setup.md

### Parley's tiers
- publik-fast: connection check (ping.v1), answer judge (judge.v1), explanations (explain.v1), sentence translation (translate.v1)
- publik-balanced: lessons (lesson.v1), stories (story.v1), role-play turns (roleplay_turn.v1), role-play summaries (roleplay_summary.v1), photo words (photo_vocab.v1)
- publik-smart: not used in v1
- Audio: /audio/speech for natural voices, /audio/transcriptions for speaking checks and media; no developer share
- The token lives at crates/parley-publik/publik-app-token.txt; the UI never receives a key
- The self-test hook is the first lines of `main` in app/src-tauri/src/main.rs (desktop builds only)
