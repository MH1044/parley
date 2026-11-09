# Third-party software, models and data

Every direct dependency, with its licence and source. Transitive dependencies are checked by the
licence gates (`deny.toml`, `scripts/check-npm-licences.mjs`) on every build. Models and datasets
get a row here when a module adds them.

## Rust crates (crates.io)

| Crate | Licence | Used for |
|---|---|---|
| tauri | Apache-2.0 OR MIT | The desktop shell |
| tauri-build | Apache-2.0 OR MIT | Build-time setup of the Tauri crate |
| tauri-plugin-dialog | Apache-2.0 OR MIT | The native file dialog for backups, called from Rust only so the web view needs no dialog permission (it brings tauri-plugin-fs, Apache-2.0 OR MIT, and rfd, MIT) |
| tauri-plugin-opener | Apache-2.0 OR MIT | Opening publik API's claim, top-up and add-credit links in the browser, called from Rust only (`open_url`) so the web view needs no opener permission (it brings open, MIT) |
| reqwest, blocking, with rustls and no crypto provider of its own | MIT OR Apache-2.0 | The one HTTP transport: AI calls (with server-sent events for streamed chat) and pack downloads; certificates checked against the operating system's store through rustls-platform-verifier (MIT OR Apache-2.0) |
| rustls, with the ring provider | Apache-2.0 OR ISC OR MIT; ring is Apache-2.0 AND ISC | TLS for every request; ring builds with the C compiler alone on every platform the app ships for |
| keyring, with its platform stores (keyring-core, windows-native-keyring-store, apple-native-keyring-store, zbus-secret-service-keyring-store) | MIT OR Apache-2.0 | The learner's own key in the operating system's keychain; on Linux the Secret Service is reached over zbus (MIT), so no system D-Bus library is needed (ADR-0013) |
| uuid, with v4 | Apache-2.0 OR MIT | The random install id of the publik API reference module |
| windows-sys (Windows only) | MIT OR Apache-2.0 | `AttachConsole`, so a release build started with `PUBLIK_SELF_TEST` prints its line in the console that started it |
| chrono, with the clock feature only | MIT OR Apache-2.0 | The local UTC offset of the desktop clock, so a day of the streak is the learner's day |
| serde | MIT OR Apache-2.0 | Serialisation of command results and errors |
| serde_json | MIT OR Apache-2.0 | JSON output of the command-line tool, the fixture manifest, and the untyped pass of a syllabus before its schema check |
| serde_yaml_ng | MIT | Reading `syllabus.yaml` and `numbers.yaml` (YAML 1.2: `no` and `yes` stay strings) |
| jsonschema | MIT | JSON Schema checks of content, settings, backups and every AI response; built without its network resolver |
| rusqlite, with bundled SQLite | MIT; SQLite is public domain | Sentence packs and the learner's store (`parley.sqlite3`: course, items, cards, review log) |
| fsrs | BSD-3-Clause | FSRS-6 scheduling of every card with the default parameters (no training); its `priority-queue` dependency, offered as LGPL-3.0-or-later OR MPL-2.0, is taken under MPL-2.0 |
| unicode-segmentation | MIT OR Apache-2.0 | UAX #29 word boundaries in the tokeniser |
| unicode-normalization | MIT OR Apache-2.0 | Unicode NFC for every comparison of words |
| strsim | MIT | Optimal string alignment distance between words, for typo tolerance in grading |
| toml | MIT OR Apache-2.0 | Reading `content/packs.lock.toml` (and `deny.toml`, `Cargo.toml` in tests) |
| sha2 | MIT OR Apache-2.0 | SHA-256 of generated fixtures, pack checksums, the content hash and forms-table headers, AI cache keys and idempotency keys |
| regex (tests only) | MIT OR Apache-2.0 | The source-hygiene test's patterns |
| ignore (tests only) | Unlicense OR MIT | The source-hygiene test's fallback walk, honouring `.gitignore` |
| libtest-mimic (tests only) | MIT OR Apache-2.0 | One test per golden case directory |
| tempfile (tests only) | MIT OR Apache-2.0 | Temporary directories in tests |
| tokio, with the rt feature (tests only; already part of reqwest) | MIT | Calling the HTTP transport from inside an async runtime in its tests, as the desktop app's commands do |

## npm packages (registry.npmjs.org)

| Package | Licence | Used for |
|---|---|---|
| react, react-dom | MIT | The interface |
| @tauri-apps/api | Apache-2.0 OR MIT | Calling commands from the web view |
| vite, @vitejs/plugin-react | MIT | Building the front end |
| typescript | Apache-2.0 | Type checking |
| vitest, jsdom | MIT | Interface unit tests |
| @testing-library/react, @testing-library/dom, @testing-library/jest-dom | MIT | Rendering and querying components in tests |
| eslint, @eslint/js, typescript-eslint, eslint-plugin-react-hooks, eslint-plugin-react-refresh, globals | MIT | Lint |
| @types/node, @types/react, @types/react-dom | MIT | Type definitions |

## Build and CI tools (not shipped)

| Tool | Licence | Used for |
|---|---|---|
| cargo-deny | MIT OR Apache-2.0 | The Rust licence gate |
| actions/checkout, actions/setup-node | MIT | Continuous integration |
| Swatinem/rust-cache | MIT | Caching Cargo builds in continuous integration |
| taiki-e/install-action | Apache-2.0 OR MIT | Installing cargo-deny in continuous integration |
| tauri-driver | Apache-2.0 OR MIT | The launch smoke in continuous integration: a W3C WebDriver server in front of the platform's driver |
| WebKitWebDriver (Ubuntu's webkitgtk-webdriver from 26.04, webkit2gtk-driver before), Microsoft Edge WebDriver, Xvfb | the platform's own terms | The platform WebDriver tauri-driver talks to, and a virtual display on Linux; installed on the runner by the e2e job, never shipped or linked |
| py-fsrs 6.3.2, run with uv | py-fsrs MIT; uv MIT OR Apache-2.0 | Recording the FSRS reference values in `tests/cases/02-srs-and-learner-model/fsrs_reference.json` with `scripts/reference/fsrs_reference.py`, run once, never in the build |

## Models

None yet. Local speech-recognition models arrive with module 10 and are pinned by URL and SHA-256
in `models/registry.toml`, never committed.

## Datasets

| Dataset | Licence | Used for |
|---|---|---|
| Tatoeba per-language exports (sentences, links) | CC BY 2.0 FR | Read by `parley pack build`; the pack keeps every author for attribution |
| Wiktionary through wiktextract (kaikki.org) extracts | CC BY-SA 4.0 and GFDL | Read by `parley forms extract` and `parley pack build` for inflected form to lemma |

Both are downloaded by checksum into the git-ignored `data-cache/` and never committed. What
ships is derived from them: sentence packs as release assets pinned by SHA-256 in
`content/packs.lock.toml`, and `content/<lang>/forms.tsv` under the content licence (CC BY-SA
4.0, ADR-0003), with the credits in `content/ATTRIBUTION.md`. The small fixtures under
`tests/fixtures/` are written for this project (CC0) and only imitate the file formats.

## Other

| Item | Licence | Note |
|---|---|---|
| `app/src-tauri/icons/*` | Project's own (MIT OR Apache-2.0) | Generated placeholder images until the packaging module designs the real icons |
