# 13: Android

## Goal

Parley runs on Android phones with the same course, progress format and features as the desktop,
adapted to touch: word bank answers by default, Android voices, the microphone permission asked
when first needed, and transcription through the provider. A debug APK builds in CI.

## Depends on

12. iOS is ADR-0010 (Proposed, the maintainer's choice) and is not part of this module, so it does
not stop for it; the release signing key is a maintainer step at module 14.

## Scope

- Tauri mobile Android target (`cargo tauri android init` output under `app/src-tauri/gen/android/`),
  minimum SDK as Tauri 2 requires (verify here), permissions `INTERNET` and `RECORD_AUDIO`.
- Publik credential and the own key in app-private storage (contract adaptation 3): the credential at
  `<app data>/publik/apps/parley.json`, mode 0600; the own key in `<app data>/secrets/own-key.json`,
  mode 0600; `SECURITY.md` updated.
- `device_name` sends "Android phone" only when the learner opts in.
- Android TTS through `tts`; microphone through `cpal`'s Android backend (verify AAudio support here).
- The `RECORD_AUDIO` runtime request on the first speaking exercise or spoken turn; the denial path.
- Local Whisper off on Android: the transcriber is always the provider; the Speaking settings say so.
- First-run default `answer_input = word_bank` on Android.
- Layout below 600 px wide: bottom navigation, full-width exercise controls, touch targets of at
  least 48 dp; the system back button asks before leaving a lesson.
- Photo and media picking through the system picker; a `content://` media file is copied to
  `cache/` for decoding and deleted straight after.
- CI job `android`: `cargo tauri android build --debug` on `ubuntu-latest`.

## Files

- `app/src-tauri/gen/android/**`, `app/src-tauri/tauri.conf.json` (mobile section),
  `app/src-tauri/capabilities/mobile.json`
- `crates/parley-publik/src/providers.rs` (Android storage paths), `crates/parley-core/src/settings.rs`
  (platform defaults), `crates/parley-audio/src/{capture.rs,os_tts.rs}` (Android paths)
- `app/src/layout/{BottomNav,useNarrow}.tsx`; `.github/workflows/ci.yml` (android job)
- `crates/parley-core/tests/m13_android.rs`, `app/src/layout/*.test.tsx`

## Interfaces

- `fn credential_location(platform: Platform, data_dir: &Path) -> PathBuf` (Android: app-private;
  desktop: the contract's `credential_path()`).
- `fn default_settings(platform: Platform) -> Settings`.
- `whisper_models()` on Android returns an empty list and the notice below.

Messages: "Parley needs the microphone for speaking exercises. Allow it in Android settings, or
choose Can't speak now."; "Local speech recognition is not available on Android, so speaking
checks use your AI provider."; "Leave this lesson? Your answers so far are saved." with **Leave**
and **Stay**.

## Out of scope

- iOS (ADR-0010); Play Store listing and signing (maintainer, 14); a camera capture screen (the
  system picker offers the camera where available); local Whisper on Android.

## Design notes

- The Android WebView renders the same React app; narrow layout is chosen by width, not platform,
  so it is testable in Vitest.
- Decisions are filled in here during implementation.

## Acceptance tests

Code tests in `crates/parley-core/tests/m13_android.rs` (platform passed as a parameter, so they run
on the desktop CI runners); UI tests in `app/src/layout/*.test.tsx`; the build in the CI `android`
job; a manual checklist on an emulator or device.

1. The CI `android` job builds a debug APK.
2. `credential_location(Android, D)` → `D/publik/apps/parley.json`; desktop → `credential_path()`.
3. Setting an own key with `Platform::Android` writes `secrets/own-key.json` with mode 0600, and
   `settings.json` contains no key.
4. With the device-name opt-in off, the provisioning body has no `device_name`; with it on, exactly
   "Android phone".
5. `default_settings(Android).answer_input` = `word_bank`; desktop = `typing`.
6. `whisper_models()` on Android → empty with "Local speech recognition is not available on Android,
   so speaking checks use your AI provider."
7. A denied microphone permission (fake) → the microphone message and **Can't speak now**.
8. At a mocked width of 390 px the bottom navigation renders and the side navigation does not; at
   1024 px the reverse.
9. A back event during a lesson shows "Leave this lesson? Your answers so far are saved." with
   **Leave** and **Stay**.
10. A `content://` media import (fake file system) leaves no copy in `cache/` after decoding.
11. Manual (emulator): onboarding, a word-bank lesson, an Android voice, the permission prompt on the
    first speaking exercise, a conversation turn, a photo from the picker.

## Status

Planned
