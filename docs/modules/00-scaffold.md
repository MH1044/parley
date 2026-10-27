# 00: Scaffold

## Goal

A contributor can clone the repository, run `scripts/verify`, and see it pass on Windows and Linux;
`parley --version --json` answers; the desktop window opens and shows "Parley" with the core
version. The project documents, hygiene test, licence gate, golden harness, fixture generator,
CI and ADRs exist, so every later module only adds product code.

## Depends on

Nothing.

## Scope

- Cargo workspace with `crates/parley-core`, `crates/parley-publik`, `crates/parley-audio`,
  `crates/parley-cli` (binaries `parley` and `parley-fixtures`) and `app/src-tauri`; one shared
  version `0.1.0`; `rust-toolchain.toml` pinning a stable toolchain.
- React + TypeScript + Vite app in `app/` with one screen showing "Parley" and the version from the
  `app_info` command; Tauri identifier `org.parley.app` (placeholder, ADR-0001); capabilities with no
  filesystem or shell permissions.
- `crates/parley-core/src/env.rs`: `Clock`, `Rng`, `Fs`, `Net` traits with a `FailingNet` fake.
- `AppError { code, message, action }` in `parley-core/src/error.rs`, serialised as in
  `docs/ARCHITECTURE.md` §5.
- `scripts/verify` and `scripts/verify.ps1` running the steps of `docs/TESTING.md` §2 in order,
  except `parley content validate`, which module 01 adds when the command exists.
- Licence gate: `deny.toml` allowing only MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception,
  BSD-2-Clause, BSD-3-Clause, ISC, Zlib, 0BSD, Unicode-3.0, Unicode-DFS-2016, MPL-2.0, Unlicense,
  CC0-1.0, BSL-1.0; and `scripts/check-npm-licences.mjs` reading `app/package-lock.json`.
- The source-hygiene test (`docs/TESTING.md` §7), run by `verify` and CI.
- Golden harness `crates/parley-cli/tests/golden.rs` with the first case `tests/cases/00-scaffold/version`.
- `parley-fixtures --out <dir>` and `--check`: at this module it writes `audio/tone_gaps_a.wav` and
  `manifest.json` (SHA-256 of every file).
- CI `verify` job on `windows-latest` and `ubuntu-latest`; `release.yml` stub that runs
  `scripts/check-release-notes <version>` first.
- Documents: `README.md`, `CONTRIBUTING.md` (with the Publik block of
  `docs/PUBLIK-INTEGRATION.md` §12), `SECURITY.md`, `RELEASING.md`,
  `THIRD_PARTY.md`, `LICENSE-MIT`, `LICENSE-APACHE`, `docs/HANDBOOK.md` (stub),
  `docs/PUBLIK-CONTRACT.md`, and `docs/adr/` with ADR-0001 to ADR-0012 from `docs/ARCHITECTURE.md` §9.

## Files

- `Cargo.toml`, `rust-toolchain.toml`, `deny.toml`, `.gitignore` (template plus `target/`,
  `app/node_modules/`, `app/dist/`, `data-cache/`, `eval-data/`, `models/whisper/`),
  `.gitattributes`.
- `crates/*/Cargo.toml` and `src/lib.rs` or `src/main.rs`; `crates/parley-cli/src/fixtures.rs`.
- `app/` (Vite project), `app/src-tauri/` (`main.rs`, `commands/app.rs`, `tauri.conf.json`,
  `capabilities/default.json`).
- `scripts/verify`, `scripts/verify.ps1`, `scripts/check-npm-licences.mjs`,
  `scripts/check-release-notes`.
- `.github/workflows/ci.yml`, `.github/workflows/release.yml`.
- `tests/cases/00-scaffold/version/{args.txt,expected.json}`, `tests/fixtures/npm/gpl-lock.json`.

## Interfaces

- CLI: `parley --version --json` prints `{"name":"parley","version":"<version>"}`; unknown
  arguments exit 2 with `error: unknown argument <arg>. Run parley --help.` on stderr.
- Tauri command `app_info() -> AppInfo { version: String, data_dir: String }`.
- `trait Clock { fn now_ms(&self) -> i64; fn utc_offset_minutes(&self) -> i32; }`,
  `trait Rng { fn next_u64(&mut self) -> u64; }`, `trait Fs` (read, write, rename, remove in the
  data directory), `trait Net { fn send(&self, req: HttpRequest) -> Result<HttpResponse, NetError>; }`.
- `parley-fixtures --out <dir>` writes; `--check <dir>` exits 1 with `fixture <name> differs from
  manifest.json` on any difference.

## Out of scope

- Any product feature, database or content (modules 01 onwards).
- The launch smoke test and its CI job (module 04).
- macOS CI (module 14), Android (module 13).

## Design notes

- `tone_gaps_a.wav`: 16 kHz, mono, 16-bit; a square wave of period 40 samples (samples 0 to 19 are
  +16000, 20 to 39 are −16000), 16,000 samples of tone, 8,000 of silence (0), 16,000 of tone.
  Integer maths only, so it is bit-identical on every platform.
- `check-release-notes` exists so the release workflow can be tested before module 14.

Decisions made during implementation:

- **Toolchain**: `rust-toolchain.toml` pins `channel = "stable"` with rustfmt and clippy; CI
  installs from the file. An exact version pin is a maintainer choice recorded in the roadmap's
  open questions. The workspace uses edition 2024.
- **One version**: `[workspace.package].version` in `Cargo.toml` is inherited by every crate;
  `app/src-tauri/tauri.conf.json` and `app/package.json` carry the same number, and a test fails
  when they differ (`RELEASING.md` lists the three places).
- **Golden harness**: built on `libtest-mimic` so each case directory is its own named test and
  `cargo test -p parley-cli --test golden -- NN-slug` filters by module. `args.txt` may use the
  token `{input}` for the path of the case's `input.json`; `stdin.txt` goes to standard input; the
  command must exit 0; `PARLEY_BLESS=NN-slug` rewrites that module's expected files.
- **Hygiene test**: `regex` and `ignore` as test-only dependencies; the file-count floor is 45; the
  fallback walk (no git) honours `.gitignore` through `ignore`.
- **Fixtures**: `manifest.json` is `{"schema": "parley.fixtures/1", "files": {<path>: {"bytes",
  "sha256"}}}` with sorted keys and LF line ends. The tone file (80,044 bytes) is committed next
  to the manifest under `tests/fixtures/generated/` and re-included below the `*.wav` ignore rule,
  so `--check` can prove the committed copy on every platform. `sha2` therefore arrives at this
  module rather than at 01.
- **Errors**: `AppError.code` is an enum `ErrorCode` of the codes in `docs/ARCHITECTURE.md` §5,
  serialised in snake case; `action` is a JSON value so each error carries the one action it
  offers. `HttpRequest`'s debug form prints header names only (invariant I1).
- **CLI**: arguments are parsed by hand (no parser crate yet); `--json` without a command is a
  usage error; no arguments prints the help.
- **Tauri**: no development URL is configured, so `cargo run -p parley-app` embeds `app/dist`,
  which `verify` builds first; CSP `default-src 'self'`; the capability grants `core:default`
  only; the window is 960 × 680 with a 640 × 480 minimum. The icons are generated placeholder
  images until module 14 designs real ones.
- **Front end**: Vite with the React plugin, Vitest with jsdom and Testing Library, ESLint's flat
  configuration with the TypeScript and React Hooks rules; `npm run typecheck` is `tsc -b`.
- **Licence gates**: `deny.toml` checks the Windows, Linux, macOS (x64 and arm64) and Android
  targets; the npm gate parses SPDX expressions (`OR`, `AND`, `WITH`, parentheses) and also
  allows the data and documentation licences a few widely used packages carry (CC-BY-4.0,
  BlueOak-1.0.0, Python-2.0), reading an installed `package.json` when the lockfile omits the field.
- **CI**: cargo-deny is installed as a prebuilt binary and Cargo builds are cached. The release
  workflow has the notes gate only, and a manual trigger with a version input so it can be
  exercised before module 14.

## Acceptance tests

Golden cases in `tests/cases/00-scaffold/`; code tests in `crates/parley-cli/tests/m00_scaffold.rs`
and `crates/parley-core/tests/hygiene.rs`; UI in `app/src/App.test.tsx`.

1. `parley --version --json` → exactly `{"name":"parley","version":"0.1.0"}` (golden `version`).
2. `parley --bogus` → exit code 2 and stderr `error: unknown argument --bogus. Run parley --help.`
3. `scripts/verify` exits 0 on both CI runners (the CI run is the evidence).
4. Hygiene: the five tests of `docs/TESTING.md` §7 pass on the tree; a home path, an email at a
   real-looking domain, a 40-character fake `sk-` key and a control character in an untracked temp
   file give exactly four failures; the file is then deleted.
5. `parley-fixtures --out A` and `--out B` produce identical `manifest.json`; reading
   `tone_gaps_a.wav` gives sample 0 = 16000, sample 20 = −16000, samples 16,000 to 23,999 = 0,
   40,000 samples in total (by construction, see Design notes).
6. `deny.toml`'s allow list contains `MIT` and `Apache-2.0` and no identifier containing `GPL`,
   `AGPL`, `SSPL` or `BUSL` (a test parses the file).
7. `node scripts/check-npm-licences.mjs tests/fixtures/npm/gpl-lock.json` exits 1 with
   `left-pad-gpl@1.0.0 is GPL-3.0; the licence gate allows only permissive licences`; on the real
   lockfile it exits 0.
8. `scripts/check-release-notes 0.1.0` with no notes file exits 1 with
   `docs/releases/0.1.0.md is missing; write the release notes first.`; with the file it exits 0.
9. `app_info()` returns `version` = `0.1.0`.
10. `App` renders the text "Parley" and the version passed by a mocked `app_info` (Vitest).
11. `FailingNet::send` returns `NetError::Disabled` for any request.
12. `docs/adr/README.md` lists 12 ADRs; each file exists and has a `Status:` line; 0003, 0009 and
    0010 are `Proposed`, the rest `Accepted`; every document named in Scope exists.
13. `AppError { code: "not_found", message: "x", action: None }` serialises to exactly
    `{"code":"not_found","message":"x","action":null}`.

## Status

Done (2026-10-27)
