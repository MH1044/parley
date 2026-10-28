# Third-party software, models and data

Every direct dependency, with its licence and source. Transitive dependencies are checked by the
licence gates (`deny.toml`, `scripts/check-npm-licences.mjs`) on every build. Models and datasets
get a row here when a module adds them.

## Rust crates (crates.io)

| Crate | Licence | Used for |
|---|---|---|
| tauri | Apache-2.0 OR MIT | The desktop shell |
| tauri-build | Apache-2.0 OR MIT | Build-time setup of the Tauri crate |
| serde | MIT OR Apache-2.0 | Serialisation of command results and errors |
| serde_json | MIT OR Apache-2.0 | JSON output of the command-line tool, the fixture manifest, and the untyped pass of a syllabus before its schema check |
| serde_yaml_ng | MIT | Reading `syllabus.yaml` and `numbers.yaml` (YAML 1.2: `no` and `yes` stay strings) |
| jsonschema | MIT | JSON Schema checks of content (later: backups and every AI response); built without its network resolver |
| rusqlite, with bundled SQLite | MIT; SQLite is public domain | Sentence packs (later: the learner's store) |
| unicode-segmentation | MIT OR Apache-2.0 | UAX #29 word boundaries in the tokeniser |
| unicode-normalization | MIT OR Apache-2.0 | Unicode NFC for every comparison of words |
| toml | MIT OR Apache-2.0 | Reading `content/packs.lock.toml` (and `deny.toml`, `Cargo.toml` in tests) |
| sha2 | MIT OR Apache-2.0 | SHA-256 of generated fixtures, pack checksums, the content hash and forms-table headers (later: cache keys) |
| regex (tests only) | MIT OR Apache-2.0 | The source-hygiene test's patterns |
| ignore (tests only) | Unlicense OR MIT | The source-hygiene test's fallback walk, honouring `.gitignore` |
| libtest-mimic (tests only) | MIT OR Apache-2.0 | One test per golden case directory |
| tempfile (tests only) | MIT OR Apache-2.0 | Temporary directories in tests |

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
