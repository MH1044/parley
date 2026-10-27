# 10: Speaking and pronunciation

## Goal

A learner can say a sentence out loud and see, word by word, what Parley heard: right, missed or
different, with a score and a pass mark. The take is recorded at 16 kHz mono, trimmed, and
transcribed by the provider or by local Whisper; the learner can replay their take next to the
voice. Speak-the-sentence exercises join the lessons.

## Depends on

09 (and 06 through it). Feature-map row **P6** (speaking check); §6.8 and §7 of
`docs/PUBLIK-INTEGRATION.md`.

## Scope

- Capture with `cpal` from the default input: `mic_start`, `mic_stop`, `mic://level` every 50 ms,
  automatic stop at 15 s (exercises) or 30 s (role-play turns); hold-to-talk with Space or the button.
- Downmix to mono, resample to 16 kHz with `rubato`, trim silence, write 16-bit WAV with `hound` to
  `cache/recordings/<uuid>.wav`; deleted when the session ends unless `speaking.keep_recordings`.
- Transcription: `speaking.transcriber = provider` through `AiClient::transcribe` (P6, request of
  §6.8), or `local` through `whisper-rs` (feature `local-whisper`) with the course language set.
- `models/registry.toml` (Whisper `tiny`, `base`, `small`: URL, SHA-256, bytes), `whisper_models`,
  `whisper_download` with the size shown first and checksum verification.
- Scoring: normalise both texts, fold accents, spell numbers as words from `content/<lang>/numbers.yaml`,
  align words, report `ok`, `missed` or `substituted` per expected word plus extra heard words,
  score and pass; rating from the score.
- `speak_check`, `recording_play` commands; the speak exercise UI with the level meter, highlighted
  words (colour, icon and text label), **Replay my take**, **Play the voice**, **Try again**, **Can't
  speak now** (60 minutes).
- `speak` enabled in the composer when a microphone exists and speaking is on.
- Gate G4 (TTS-to-Whisper loopback).

## Files

- `crates/parley-audio/src/{capture.rs,resample.rs,trim.rs,wav.rs,whisper_local.rs}`
- `crates/parley-core/src/speech/{align.rs,score.rs}`, `crates/parley-core/src/ai/transcribe.rs`
- `app/src-tauri/src/commands/{mic.rs,speak.rs,models.rs}`; `app/src/components/{SpeakExercise,LevelMeter}.tsx`
- `models/registry.toml`; `crates/parley-audio/tests/m10_speaking_and_pronunciation.rs`,
  `crates/parley-core/tests/m10_speaking_and_pronunciation.rs`

## Interfaces

```rust
pub trait Capture { fn start(&mut self, on_level: Box<dyn FnMut(f32) + Send>) -> Result<(), AudioError>; fn stop(&mut self) -> Result<Take, AudioError>; }
pub fn to_16k_mono(take: &Take) -> Vec<i16>;
pub fn trim(samples: &[i16]) -> Result<Trimmed, AudioError>;   // Trimmed { start, end, samples }
pub fn score(lang: Lang, expected: &str, heard: &str, numbers: &Numbers, pass: f64) -> SpeakScore;
// SpeakScore { words: Vec<WordResult{ expected, heard: Option<String>, status }>, extra: Vec<String>, score: f64, pass: bool }
```

Messages: "We didn't hear anything. Try again a little closer to the microphone." (`too_quiet`,
also for an empty transcript); "No microphone was found. Connect one, or choose Can't speak now."
(`no_microphone`); "Parley can't use the microphone. Open Settings > Privacy & security >
Microphone and allow desktop apps to use it." (Windows) and "…System Settings > Privacy & Security >
Microphone and turn on Parley." (macOS) (`mic_permission`); "The local speech model is not
downloaded. Download it in Settings > Speaking ({size})."; "The download was damaged. Try again.";
"Can't check speaking right now." with the reason of PUBLIK-INTEGRATION §9; "Speaking checks need a
connection or the local model." (offline, no model).

## Out of scope

- Phoneme-level scoring and intonation; role-play turns (11 reuses capture and transcription).
- Local Whisper on Android (13).

## Design notes

- Trim: 20 ms frames (320 samples); frame RMS in dBFS; noise floor = the 10th percentile of frame
  RMS; threshold = max(floor + 10 dB, −50 dBFS); speech is a run of at least 3 frames above the
  threshold; keep from 150 ms before the first run to 300 ms after the last, clamped to the take
  (Rabiner and Sambur 1975, simplified). Less than 300 ms of speech → `too_quiet`.
- Alignment: Wagner and Fischer (1974) over words with unit costs; on equal cost the backtrace
  prefers a match or substitution, then a deletion (missed), then an insertion (extra).
- Ratings: pass → Good; score ≥ 0.5 → Hard; below → Again (productive card).
- Live check at the start of implementation (record here): the transcription model id that `GET /models` lists for
  `/audio/transcriptions` (if it lists none, this module stops until the maintainer has decided, rather than guess); the measured charge of 10
  takes, which replaces the P6 estimate in PUBLIK-INTEGRATION §8; the Whisper file URLs, sizes and
  SHA-256 values.
- Decisions are filled in here during implementation.

## Acceptance tests

Code tests in the two `m10_speaking_and_pronunciation.rs` files with a fake `Capture`, generated
audio and the fake AI server; golden cases for scoring in `tests/cases/10-speaking-and-pronunciation/`.
Contract tests 1 to 18 keep passing; this module adds test 29 of `docs/PUBLIK-INTEGRATION.md` §11.

1. A generated 48 kHz, 1 s square wave → 16,000 ± 1 samples at 16 kHz; stereo with L = +1000 and
   R = −1000 → mono 0.
2. Trim on 0.5 s silence, 1.0 s tone, 0.7 s silence (16 kHz) → start 0.35 s (sample 5,600) and end
   1.8 s (sample 28,800), each within one frame (320 samples).
3. A silence-only take → `too_quiet` and its message; 200 ms of tone → `too_quiet`.
4. A 16 s take → stopped at 15 s; the WAV is 480,044 bytes, 16 kHz, mono, 16-bit.
5. Scoring (es): `yo soy estudiante` against heard `yo es estudiante` → ok, substituted (`es`), ok;
   score 0.667 (tolerance 0.001); fail at 0.8. `quiero un café con leche` against `quiero café con
   leche` → missed `un`; 0.8; pass. `buenos días` against `buenos días señor` → 1.0, extra `señor`.
   `tengo tres gatos` against `tengo 3 gatos` → 1.0. `días` against `dias` → ok.
6. Ratings: score 1.0 → Good; 0.6 → Hard; 0.4 → Again.
7. The P6 request is multipart with `file`, `model`, `language=es`, `response_format=json`,
   `temperature=0`, no `prompt` field, and a body under 4,000,000 bytes.
8. The same take bytes and language again in the same session → no request; the transcript is held
   in memory only, so nothing about the take is written to `cache/ai.sqlite3` (PUBLIK-INTEGRATION §6.1).
9. An empty transcript → the `too_quiet` message; the card is not rated.
10. `transcriber = local` with no model → the not-downloaded message with the `base` size from the
    registry.
11. `whisper_download` shows the size first; a served file with one changed byte → "The download was
    damaged. Try again." and no file left in `models/whisper/`.
12. Fake capture with no device → `no_microphone`, and the composer then produces no `speak`; with
    permission denied → the platform message.
13. With `keep_recordings` off, the take files are gone after `lesson_finish` and after quitting.
14. **Can't speak now** at 10:00 → no `speak` exercise until 11:00.
15. A 402 → "Can't check speaking right now.", the message and link, and **Use the local model** when
    a model is downloaded.
16. `recording_play` plays the trimmed file (the playback fake receives its path).
17. (`local-whisper` job, model present) transcribing the generated tone returns without error;
    skipped with the message of `docs/TESTING.md` §2 when the model is absent.
18. Gate G4: mean loopback score recorded at first run; in `verify`, 5 sentences when a voice and the
    model exist, else skipped with a message.
19. A fake capture feeding a full-scale square wave emits `mic://level` every 50 ms at 0 dBFS
    (tolerance 0.1 dB), and digital silence emits −60 dBFS, the meter's floor (levels are clamped
    to −60 to 0 dBFS).

## Status

Planned
