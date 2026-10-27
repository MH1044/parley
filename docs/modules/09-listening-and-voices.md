# 09: Listening and voices

## Goal

Every target-language sentence can be heard: through an OS voice (free, offline) or a natural voice
through the provider, at normal speed or 0.75× slow mode. Listen-and-type exercises join the
lessons. Natural audio is cached, so each sentence is paid for once.

## Depends on

04, 06. Feature-map row **P5** (natural voice); §6.7 and §7 of `docs/PUBLIK-INTEGRATION.md`.
The default voice source is ADR-0009 (Proposed, the maintainer's choice). This module ships with
`os` as the default, which is the recommendation, and leaves the question open for the maintainer
without stopping; changing the default later is one line in `settings.rs`.

## Scope

- `parley-audio`: playback through `rodio` (MP3 and WAV), OS voices through `tts` behind an
  `OsVoices` trait, voice listing per language tag.
- `voices_list`, `tts_play`, `tts_stop` commands and the `audio://ended` event.
- Source choice: natural when `audio.voice_source = natural`, a provider with speech is ready and
  `needs_credit` is not set; otherwise the OS voice.
- P5 through `AiClient::speech`, validation of §6.7, the audio cache in `cache/audio/`, one-ahead
  prefetch of the next exercise's sentence, fallbacks.
- Slow mode: the request's `speed` when the live check shows it is honoured, else local WSOLA time
  stretching; OS voices use their rate setting (normal rate × 0.75, clamped to the backend's range).
- `content/voices.toml`: speech model id and a default voice per language, from the live check.
- `listen_type` enabled in the composer when a voice exists for the course language; **Can't listen
  now** turns it off for 60 minutes.
- Autoplay setting: prompts in the target language and the correct answer on the result sheet.
- Settings > Audio: source, voices per language, slow speed, autoplay, clear audio cache, and the
  measured cost line.

## Files

- `crates/parley-audio/src/{playback.rs,os_tts.rs,wsola.rs}`, `crates/parley-core/src/ai/speech.rs`
- `app/src-tauri/src/commands/audio.rs`; `app/src/components/{PlayButton,ListenType}.tsx`;
  `app/src/screens/Settings/AudioSection.tsx`; `content/voices.toml`
- `crates/parley-audio/tests/m09_listening_and_voices.rs`, `crates/parley-core/tests/m09_listening_and_voices.rs`

## Interfaces

```rust
pub trait OsVoices { fn list(&self) -> Vec<OsVoice>; fn speak(&self, text: &str, voice: &str, rate: f32) -> Result<(), AudioError>; fn stop(&self); }
pub fn choose_source(settings: &Settings, status: &AiStatus, lang: Lang, os: &dyn OsVoices) -> VoiceSource;
pub fn natural_audio(client: &dyn AiClient, cache: &AudioCache, text: &str, voice: &str, speed: f32) -> Result<PathBuf, AiError>;
pub fn wsola(samples: &[i16], rate_hz: u32, factor: f32) -> Vec<i16>;
```

Messages: "No {Spanish} voice is installed. On Windows, open Settings > Time & language > Speech and
add a voice." (Windows), "…open System Settings > Accessibility > Spoken Content > System voice >
Manage Voices." (macOS), "…install Speech Dispatcher and a {Spanish} voice, or turn on natural
voices." (Linux); "Natural voice unavailable, using the system voice."; "Natural voices cost about
{$x} a sentence (measured)."

## Out of scope

- Recording and speaking (10); reading role-play replies aloud (11 uses this module's `tts_play`).

## Design notes

- Live check at the start of implementation (record here): the speech model id that `GET /models`
  lists for `/audio/speech` (if it lists none, this module stops until the maintainer has decided,
  rather than guess), one accepted voice per language,
  whether `speed` is honoured, the returned `Content-Type`, and the measured charge of 10 sentences,
  which replaces the estimate in PUBLIK-INTEGRATION §8.
- WSOLA after Verhelst and Roelands (1993): 20 ms frames, 50% overlap, a search window of ±10 ms,
  Hann cross-fade.
- Decisions are filled in here during implementation.

## Acceptance tests

Code tests in `crates/parley-audio/tests/m09_listening_and_voices.rs` and
`crates/parley-core/tests/m09_listening_and_voices.rs`, with fake `OsVoices` and the fake AI server
(which returns generated WAV bytes as `audio/wav`). Contract tests 1 to 18 keep passing; this
module adds test 28 of `docs/PUBLIK-INTEGRATION.md` §11.

1. A fake backend with voices `es-ES`, `es-MX` and `fr-FR` → `voices_list("es")` returns two voices.
2. OS path, slow: `speak` is called with the backend's normal rate × 0.75, clamped to its range.
3. No voice for German → `no_voice` with the platform's message, and the composer produces no
   `listen_type` for German.
4. Natural path: the request body equals §6.7 for `Quiero un café con leche.` at speed 0.75 with the
   voice from `voices.toml`; the audio plays; a second play makes no request.
5. The cache key differs when only the voice or only the speed differs.
6. A reply with `Content-Type: text/html`, or a 0.05 s clip → OS voice and "Natural voice
   unavailable, using the system voice."
7. A 402 → OS voice for the rest of the session, one banner, no prefetch while `needs_credit`.
8. Offline: a cached sentence plays from cache; an uncached one uses the OS voice.
9. `listen_type` with target `Buenos días.`: `buenos dias` → almost (accent); `buenas días` → wrong
   (`buenas` is itself a known form, so it is not a typo).
10. **Can't listen now** at 10:00 (injected clock) → no `listen_type` until 11:00, then allowed again.
11. WSOLA on a 1 s square wave of period 40 samples with factor 1/0.75 → length 1.333 s ± 2%, and
    zero crossings per second within 2% of the input (pitch kept).
12. Duration bound: a 6-word sentence at speed 1.0 allows at most 3 × 0.5 × 6 = 9 s; a 10 s clip is
    rejected.
13. Clear audio cache removes the files and `audio_cache` rows and leaves progress unchanged.
14. Prefetch: showing exercise n makes exactly one P5 request, for exercise n + 1's sentence, when it
    is not cached and natural voices are on.
15. Charges of 600 and 640 micros → "Natural voices cost about $0.00062 a sentence (measured)." (the
    mean through `format_dollars`, module 06).
16. With autoplay on, showing a `pick_meaning` exercise calls `tts_play` with its prompt once; with
    autoplay off, it does not.

## Status

Planned
