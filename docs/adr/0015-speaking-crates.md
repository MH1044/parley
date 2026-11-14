# ADR-0015: Speaking through cpal, rubato, hound and whisper-rs

- **Status:** Accepted
- **Date:** 2026-11-13

## Context

Speaking exercises (module 10) record a take from the default microphone, mix it to mono,
resample it to 16 kHz, trim the silence around it and write it as 16-bit PCM WAV, the input every
transcriber takes (docs/PUBLIK-INTEGRATION.md §6.8 and §7). The take is then transcribed by the
learner's provider or, offline and for free, by local Whisper.

- `cpal` 0.18 (Apache-2.0) is already in the tree as the output device under `rodio` (module 09)
  and has a team of maintainers. It reads the default input on Windows (WASAPI), macOS (CoreAudio),
  Linux (ALSA) and Android (AAudio). Its input stream may not move between threads on every
  platform.
- `rubato` 5.1 (MIT OR Apache-2.0) resamples with a band-limited FFT or sinc resampler and
  removes its own start-up delay for a whole clip. It has one maintainer (crates.io and the
  repository, checked 2026-11-13).
- `hound` 3.5 (Apache-2.0) reads and writes WAV. It has one maintainer and few releases; the format
  it writes for us is fixed and small.
- `whisper-rs` 0.16 (Unlicense; whisper.cpp itself MIT; the repository moved to Codeberg) binds
  whisper.cpp, which runs OpenAI's Whisper models (MIT) on the CPU. It has one maintainer. Building
  it compiles whisper.cpp with cmake and generates bindings with libclang, neither of which a plain
  Windows Rust setup has.

CONTRIBUTING.md asks for a decision record that names the exit for each dependency with one
maintainer.

## Decision

`parley-audio` implements the core's `Capture` trait as `Microphone`, over an `InputDevice`
whose real implementation, `CpalInput`, is a thread that owns the cpal input stream and opens it
at each take, as `RodioPlayer` owns the output. Every sample format cpal delivers is
converted to 16 bits; channels are averaged; `rubato`'s FFT resampler takes the take to 16 kHz with
its cut-off below 8 kHz and its delay removed; energy trimming (Rabiner and Sambur 1975,
simplified) keeps the speech; `hound` writes the WAV. Local transcription is `WhisperLocal`, which
implements the core's `LocalTranscriber` with `whisper-rs`, only with the cargo feature
`local-whisper`, off by default; the desktop app forwards the feature, and the CI job `whisper`
(ubuntu-latest, with cmake and libclang) builds and tests it with the `tiny` model. The models are
whisper.cpp's ggml files, pinned in `models/registry.toml` by URL, size and SHA-256, downloaded only
when the learner asks and kept only once they match.

The exits, each local to one file behind a trait or a function the core does not see:

- **hound**: a WAV header is 44 bytes (`RIFF`, `fmt ` with PCM, one channel, 16,000 Hz, byte rate
  32,000, block align 2, 16 bits, `data`); `wav_16k_mono` writes it by hand.
- **rubato**: a windowed-sinc resampler of our own for the fixed cases the microphones give (48
  and 44.1 kHz to 16 kHz), behind `resample_to_16k`, which already fixes the output length.
- **whisper-rs**: whisper.cpp's C API through bindings of our own (a few functions: init from file,
  full, the segment text), or provider transcription only, which every build already has.

## Consequences

- The default build, `scripts/verify` and the `verify` job need no cmake and no libclang; a build
  without `local-whisper` still lists and downloads models, says that local checks are not
  available in it, and never falls back to the provider, which would spend money the learner did
  not choose.
- Local Whisper is tested only where whisper.cpp builds: the `whisper` job, and a maintainer's
  machine with the toolchain. A change to `whisper_local.rs` is checked by CI, not by every local
  run.
- A take never leaves the computer except in the learner's Check (invariant I13); local
  transcription never leaves it at all.
- Cost: three dependencies with one maintainer each, a C++ build in one CI job, and model files
  of 78 to 488 MB that a download holds in memory, because the transport returns whole bodies.

## Alternatives considered

- **Provider transcription only**: simplest, but every check needs a connection and costs money;
  speaking would be the one part of the course that is never offline and free.
- **A pure Rust Whisper (candle, burn)**: no C++ build, but much larger dependency trees, and
  models in another format to pin and host.
- **Resampling by linear interpolation**: no dependency, but no anti-alias filter, so 48 kHz
  input would fold energy above 8 kHz into the speech band the transcriber listens to.
- **Recording through the web view's `getUserMedia`**: the take would pass through the web view,
  which by design touches no file and holds nothing it does not need (docs/ARCHITECTURE.md §3).
