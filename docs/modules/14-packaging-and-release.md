# 14: Packaging and release

## Goal

A tester can download an installer for Windows (first), macOS or Linux from a tagged release, with
the course packs published beside it, and Parley can tell them when a newer version exists. The
Publik listing, made by the maintainer's setup run after module 06, picks up the tagged release, and
`docs/LISTING.md` records what it says.

## Depends on

13. Maintainer steps, each given with the exact command; this module stops at each until the
maintainer has done it: pushing the release tag; publishing the packs release; the Apple developer
account for signing and notarisation (ADR-0010); the Android signing key; the Publik setup run of
`docs/PUBLIK-INTEGRATION.md` §2, if it has not been done since module 06 (Publik picks up each new
release within a day). Whether units still marked `unreviewed` may ship was decided on 2026-11-04
(module 05's Design notes): yes, as a labelled beta, the About page listing them and the release
notes saying the course awaits native-speaker review.

## Scope

- Tauri bundles: Windows NSIS installer (x64), macOS `.dmg` (x64 and arm64), Linux AppImage and
  `.deb`; icons; version from the workspace.
- `release.yml` on a `v*` tag: `scripts/check-release-notes`, `scripts/verify`, the build matrix,
  a draft release with `docs/releases/<version>.md` as notes, uploads, publish only when every job
  is green; an Android APK only when the signing secrets exist.
- Smaller sentence packs, before the first app release (the full-size packs are already published on
  `packs-2026-11-05`): module 05 pinned packs of 247 MB (es),
  175 MB (fr) and 401 MB (de), most of it the whole Wiktionary forms table and, for German, the
  lemma keys of personal pronouns that the extract lists as forms of one another. Each pack is
  brought to at most 60 MB (for example: keep only the forms of lemmas the pack's sentences use,
  drop the pronoun cross-listing, and compress the download), as a new version of the pack format
  with module 01's reader, `pack info` and checksums kept consistent; the packs are rebuilt and
  `content/packs.lock.toml` re-pinned.
- `packs.yml`: on a `packs-<YYYY-MM-DD>` tag, uploads the three packs of `data-cache/packs/`
  (built by module 05's command in the job) and checks their SHA-256 against `content/packs.lock.toml`,
  whose URLs point at that release.
- The update check: one `GET https://github.com/{repo}/releases/latest` without following the
  redirect, reading the tag from `Location`; `User-Agent: Parley/<version>` and nothing else; off in
  development builds and switchable in Settings > About.
- Release builds enable local Whisper (`--features local-whisper`, module 10), so the release
  runners install cmake and libclang; without it a shipped app has no offline speaking checks.
  whisper.cpp builds for the build machine's own processor unless `GGML_NATIVE=OFF` is set, so
  release builds set it; the Windows build of the feature is first proven there.
- CI `macos` job running `scripts/verify` and a bundle build.
- Usage reports to publik (maintainer decision, 2026-11-09; publik's `skills/telemetry.md` and its
  Rust file fetched again with `curl` at implementation): `app_opened` once per launch and
  `active_session` every 30 minutes after it, `POST {base_url}/telemetry`, sent **only while
  `ai.provider = publik`**, never before the learner has chosen publik API, so a launch with
  `unset`, `own_key` or `local` sends nothing and invariant I9 holds. No key and no content: only
  the fields publik's file lists (the slug `parley`, the version, the event, the platform). A switch
  in Settings > AI, "Send publik anonymous usage reports" (`ai.usage_reports`, default on), stops
  them. `SECURITY.md` and ARCHITECTURE §8 list the call and its condition.
- `RELEASING.md`: the tag flow, the notes rule, what Windows SmartScreen and macOS Gatekeeper show
  for unsigned builds and how testers get past them.
- `THIRD_PARTY.md` complete: every direct dependency, the Whisper models, Tatoeba and Wiktionary.
- `docs/LISTING.md` from PRODUCT §9; the About page lists unreviewed units from `review.yaml`.
- `docs/releases/0.9.0.md`, the first release's notes, including the manual checklist result.

## Files

- `.github/workflows/{release.yml,packs.yml,ci.yml}`, `app/src-tauri/icons/*`, `app/src-tauri/tauri.conf.json`
- `crates/parley-core/src/update.rs`, `app/src/screens/Settings/About.tsx`
- `crates/parley-publik/src/telemetry.rs`, `app/src/screens/Settings/AiSection.tsx`, `schemas/settings.schema.json`
- `RELEASING.md`, `THIRD_PARTY.md`, `docs/LISTING.md`, `docs/releases/0.9.0.md`, `docs/HANDBOOK.md`
- `crates/parley-core/tests/m14_packaging_and_release.rs`

## Interfaces

`fn check_update(net: &dyn Net, current: &Version, repo: &str) -> Result<Option<Version>, NetError>`;
command `update_check() -> { available: Option<String> }`. Messages: "Parley {version} is available."
with **Open the releases page**; "Android: build from source for now." (release notes when no APK).

## Out of scope

- An auto-updater that installs by itself; app stores; iOS.

## Design notes

- Unsigned installers are expected until a certificate exists (`RELEASING.md` says what testers see).
- Decisions are filled in here during implementation.

## Acceptance tests

Code tests in `crates/parley-core/tests/m14_packaging_and_release.rs`; workflow checks by parsing the
YAML files; the release run itself is the evidence for the bundles.

1. `scripts/check-release-notes 0.9.0` exits 0 with `docs/releases/0.9.0.md` present.
2. `release.yml` runs, in order, the notes check, verify, the build matrix, the draft, the uploads and
   the publish step, and the publish step depends on every build job.
3. The release run for `v0.9.0` produces a Windows `-setup.exe`, a `.dmg` per architecture, an
   `.AppImage` and a `.deb`.
4. Update check with a fake `Location: …/releases/tag/v0.9.1` and current 0.9.0 → "Parley 0.9.1 is
   available."; `v0.9.0` → nothing; a development build or the setting off → no request; the
   request carries only `User-Agent: Parley/0.9.0` and no query string.
5. Every `url` in `content/packs.lock.toml` starts with this repository's release download base
   (from the `repository` field of the workspace `Cargo.toml`) and a `packs-` tag.
6. `THIRD_PARTY.md` names every dependency in the crates' `Cargo.toml` files and `app/package.json`,
   every model in `models/registry.toml`, Tatoeba and Wiktionary.
7. The About page lists the units that `review.yaml` marks `unreviewed`, and none when all are reviewed.
8. The `macos` CI job runs `scripts/verify` green.
9. `docs/LISTING.md`: name at most 18 characters, summary at most 80 (counted by the test), category
   Education, alternativeTo Duolingo, Speak and Pimsleur, and products exactly `ai.chat`,
   `ai.transcribe`, `ai.speech` (as in PUBLIK-INTEGRATION §1).
10. `RELEASING.md` contains the SmartScreen steps ("More info", then "Run anyway") and the Gatekeeper
    steps (System Settings > Privacy & Security > "Open Anyway").
11. `release.yml` attaches an APK only when the signing secrets are set, and otherwise the notes
    contain "Android: build from source for now."
12. Every pack named in `content/packs.lock.toml` has `bytes` of at most 60,000,000, and the built
    file in `data-cache/packs/` has exactly that size (measured from the file; skipped with module
    05's message when the pack is not built).
13. Usage reports, against the fake server with a clock the test sets: with `ai.provider` =
    `unset`, `own_key` or `local`, or with `ai.usage_reports` off, a launch and two hours of running
    send no request to publik API; with `publik` and the switch on, one `app_opened` at launch, then
    one `active_session` at 30, 60, 90 and 120 minutes; choosing publik API while running starts
    them, and choosing another provider or turning the switch off stops them before the next one.
14. A usage report carries no `Authorization` or `x-api-key` header, no `pk_` string anywhere, and
    no content: its body has exactly the fields publik's telemetry file lists, and none of the
    learner's text, answers, settings or file names.
15. `SECURITY.md` and `docs/ARCHITECTURE.md` §8 list `POST /telemetry` with its condition (only
    while publik API is the chosen provider, and the switch).

## Status

Planned
