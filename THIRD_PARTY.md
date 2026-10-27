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
| serde_json | MIT OR Apache-2.0 | JSON output of the command-line tool and the fixture manifest |
| sha2 | MIT OR Apache-2.0 | SHA-256 of generated fixtures (later: cache keys, pack checksums) |
| regex (tests only) | MIT OR Apache-2.0 | The source-hygiene test's patterns |
| ignore (tests only) | Unlicense OR MIT | The source-hygiene test's fallback walk, honouring `.gitignore` |
| libtest-mimic (tests only) | MIT OR Apache-2.0 | One test per golden case directory |
| toml (tests only) | MIT OR Apache-2.0 | Reading `deny.toml` and `Cargo.toml` in tests |
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

None yet. Sentence packs are built from Tatoeba exports (CC BY 2.0 FR) and word forms from
Wiktionary through wiktextract (CC BY-SA 4.0 and GFDL) from module 01, with attribution in
`content/ATTRIBUTION.md`; nothing downloaded is committed.

## Other

| Item | Licence | Note |
|---|---|---|
| `app/src-tauri/icons/*` | Project's own (MIT OR Apache-2.0) | Generated placeholder images until the packaging module designs the real icons |
