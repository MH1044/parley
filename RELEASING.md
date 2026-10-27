# Releasing

Releases are cut by pushing a tag. The Release workflow (`.github/workflows/release.yml`) refuses
to run without release notes, and, from the packaging module onwards, verifies, builds the
installers, drafts the GitHub release with the notes, uploads the artifacts and publishes only when
every job is green.

## One version, three places

The version lives in `Cargo.toml` under `[workspace.package]`, in `app/src-tauri/tauri.conf.json`
and in `app/package.json`. A test fails when they differ. `parley --version` and the window's
About line both report it.

## Steps

1. Write the notes: `docs/releases/<version>.md`, with what is new, what is known to be broken, and
   the result of the manual checklist in `docs/TESTING.md` §9 on every platform the release ships
   on. `scripts/check-release-notes <version>` confirms the file exists; the workflow runs it first.
2. Bump the version in the three places above.
3. `scripts/verify` green, then commit: `chore(release): v<version>`.
4. Tag and push: `git tag v<version>`, `git push origin main`, `git push origin v<version>`.
5. Watch the Release run to completion. A red run is fixed by the next commit and a new tag; a tag
   is never moved.
6. Course packs are published separately, on a `packs-<YYYY-MM-DD>` tag, and pinned by checksum in
   `content/packs.lock.toml` (from module 14).

## What a tester sees with an unsigned installer

Installers are unsigned until a signing certificate exists.

- **Windows**: SmartScreen shows "Windows protected your PC". Click **More info**, then
  **Run anyway**. The installer then runs normally.
- **macOS**: Gatekeeper refuses to open the app the first time. Open **System Settings** >
  **Privacy & Security**, scroll to the message about Parley, and click **Open Anyway**; confirm
  once more. Signing and notarisation need an Apple developer account (ADR-0010).
- **Linux**: mark the AppImage executable (`chmod +x`) and run it, or install the `.deb` with the
  package manager.

Testers are told this in the release notes of every unsigned release.
