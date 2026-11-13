# ADR-0014: OS voices through tts, and Speech Dispatcher's spd-say on Linux

- **Status:** Accepted
- **Date:** 2026-11-12

## Context

Every target-language sentence must be audible for free and offline (module 09), so the app
speaks with the operating system's own voices unless the learner turns natural voices on
(ADR-0009). Each platform has its own speech API: WinRT's `SpeechSynthesizer` with the OneCore
voices on Windows (SAPI underneath), AVFoundation's `AVSpeechSynthesizer` on macOS and iOS,
`TextToSpeech` on Android, and Speech Dispatcher on Linux.

`tts` 0.26 (MIT) wraps all of them behind one `Tts` with `voices`, `set_voice`, `set_rate`,
`speak`, `stop` and an end-of-utterance callback. It has one maintainer (crates.io and the
repository, checked 2026-11-12), and CONTRIBUTING.md asks for a decision record that names the exit
for such a dependency. Its `Tts` handle is reference-counted without atomics (`Rc`), although the
crate marks it `Send` and `Sync`. Its Linux backend links `libspeechd` through the
`speech-dispatcher` crate, whose bindings need `libclang` at build time, so every Linux build and
CI runner would need two more system packages, for a part of the app that is optional at run time
(docs/ARCHITECTURE.md §10).

## Decision

`parley-audio` implements the core's `OsVoices` trait as `SystemVoices`:

- On Windows, macOS and Android, through `tts` 0.26 with its default features off (no Speech
  Dispatcher, no Tolk screen-reader backend). One thread owns the `Tts` handle and receives list,
  rate, speak and stop commands over a channel; the end-of-utterance callback announces the end
  of speech (`audio://ended`). `tts` is a dependency only on targets other than Linux.
- On Linux, through Speech Dispatcher's `spd-say` command: `-L` lists the voices (read once), and
  a sentence is spoken with `--cancel --wait -r <rate> -l <language> -y <voice> -- <text>`, the
  rate being round(100 × log2(multiplier)) within −100 to 100. A thread waits for the command, so
  its end is the end of speech. When `spd-say` is missing, the computer has no OS voices, and the
  app says how to add one or to turn on natural voices.

Everything else, and every test, sees only the `OsVoices` trait, so `tts` appears in one file
(`crates/parley-audio/src/os_tts.rs`).

The exit, if `tts` stops being maintained or a platform breaks: replace its part of `os_tts.rs`
with the platform speech APIs directly, behind the same trait: WinRT's
`Windows.Media.SpeechSynthesis` through the `windows` crate (which `tts` and `cpal` already bring
in) on Windows, `AVSpeechSynthesizer` through `objc2-avf-audio` on macOS, and Android's
`TextToSpeech` over JNI, which module 13 needs for the Android build anyway. The trait is four
methods (list, rates, speak, stop), so the replacement is local.

## Consequences

- Linux builds need no `libspeechd-dev` and no `libclang`; Speech Dispatcher stays an optional
  run-time package, as ARCHITECTURE §10 says.
- The `Rc` inside `Tts` never crosses threads, whatever its `Send` marker says.
- Linux voices are the output module's (`spd-say -L` lists those of the default module, usually
  espeak-ng); a voice installed while the app runs appears after a restart.
- On Linux each sentence starts a short-lived process; the waiting thread ends with it.
- Cost: a dependency with one maintainer on three platforms, and the `windows` 0.58 crate it pins
  beside the newer one the rest of the app uses, which lengthens Windows builds.

## Alternatives considered

- **`tts` on every platform, Linux included**: the simplest code, but two more system packages on
  every Linux build and CI runner (`libspeechd-dev`, `libclang`) for an optional feature; and a
  computer without Speech Dispatcher's library would not start the app at all if it were linked
  dynamically.
- **The platform speech APIs directly, now**: four implementations to write and test where `tts`
  covers three of them already; kept as the exit.
- **Natural voices only**: needs a connection and a provider for every sentence, and costs money
  per sentence; listening must work offline and free.
- **A bundled offline synthesiser (espeak-ng, Piper)**: GPL-3.0 (espeak-ng) is outside the licence
  policy (ADR-0002), and neural voice models add tens of megabytes per language to a download
  that should stay small.
