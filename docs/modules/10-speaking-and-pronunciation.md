# 10: Speaking and pronunciation

## Goal

A learner can say a sentence out loud and see, word by word, what Parley heard: right, missed or
different, with a score and a pass mark. The take is recorded at 16 kHz mono, trimmed, and
transcribed by the provider or by local Whisper; the learner can replay their take next to the
voice. Speak-the-sentence exercises join the lessons.

## Depends on

09 (and 06 through it). Feature-map row **P6** (speaking check); §6.8 and §7 of
`docs/PUBLIK-INTEGRATION.md`. ADR-0015 (new here): `hound`, `rubato` and `whisper-rs`, each with
one maintainer, and their exits.

## Scope

- `parley-audio`: capture with `cpal` from the default input (`Microphone`), downmix to mono,
  resample to 16 kHz with `rubato`, cut at the take's limit (15 s for exercises; 30 s is module
  11's role-play turn and is already a parameter), trim silence, write 16-bit PCM WAV with `hound`;
  the level of every 50 ms of input; local transcription through `whisper-rs` behind the feature
  `local-whisper` (off by default; the app crate forwards it).
- Commands `mic_start`, `mic_stop`, `recording_play`, `speak_check`, `whisper_models`,
  `whisper_download`, `speaking_status`; events `mic://level`, `mic://limit`, `model://progress`.
- Takes in `cache/recordings/<uuid>.wav`, removed at `lesson_finish`, on quit and at the next
  launch unless `speaking.keep_recordings` is on.
- Transcription: `speaking.transcriber = provider` through `AiClient::transcribe` (P6, the request
  of §6.8; `gpt-4o-mini-transcribe` on publik API), or `local` through local Whisper with the
  course language set. Transcripts are held in memory only, while the app runs.
- `models/registry.toml` (Whisper `tiny`, `base`, `small`: URL, bytes, SHA-256), `whisper_models`,
  `whisper_download` with the size shown first and checksum verification; the new setting
  `speaking.whisper_model` picks the model (default `base`).
- Scoring: normalise both texts, fold accents, spell numbers 0 to 100 as words from
  `content/<lang>/numbers.yaml`, align words, report `ok`, `missed` or `substituted` per expected
  word plus the extra heard words, the score and the pass; the card's rating from the score.
- The speak exercise: hold-to-talk with Space or the button, the level meter, highlighted words
  (colour, icon and text label), **Replay my take**, **Play the voice**, **Try again**, **Can't
  speak now** (60 minutes); Settings > Speaking.
- `speak` enabled in the composer when a microphone exists, speaking is on and not skipped, and a
  transcriber can be used.
- The measured P6 charge and its cost line; gate G4 (OS voice to local Whisper loopback); the CI
  job `whisper`.

## Files

- `crates/parley-audio/src/{lib.rs,capture.rs,resample.rs,trim.rs,wav.rs,whisper_local.rs}`;
  `crates/parley-audio/Cargo.toml` (`cpal`, `rubato`, `hound`, optional `whisper-rs`, feature
  `local-whisper = ["dep:whisper-rs"]`)
- `crates/parley-core/src/audio.rs` (the capture and local-transcriber traits, `Speaking`),
  `crates/parley-core/src/speech/{mod.rs,align.rs,score.rs,models.rs}` (`pub mod speech` in
  `lib.rs`), `crates/parley-core/src/ai/transcribe.rs` (re-exported from `ai/mod.rs`),
  `TranscribeRequest` in `ai/client.rs`, `transcribe_charges` in `ai/cache.rs`, `Numbers` in
  `content/numbers.rs`, `crates/parley-core/src/app/speak.rs` (the service), the composer switch in
  `app/lesson.rs`, `LessonRun::spoken` in `exercise/run.rs`, recordings and models in `app_start`
  and `data_delete_all` (`app/mod.rs`), `speaking.whisper_model` in `settings.rs` and
  `schemas/settings.schema.json`, `schemas/ai/transcribe.v1.schema.json`, `api.rs`
- `crates/parley-publik/src/providers.rs` (the text fields from `TranscribeRequest::fields`); the
  two existing `TranscribeRequest` literals (the unit test in `providers.rs`, `transcribe_request()`
  in `crates/parley-publik/tests/m06_contract.rs`) gain `temperature: 0.0`
- `crates/parley-cli/src/cmd/{speak.rs,whisper.rs}`, `cmd/mod.rs`, `src/lib.rs` (dispatch, help)
- `models/registry.toml` (new, tracked; `models/whisper/` stays ignored)
- `app/src-tauri/src/commands/{mic.rs,speak.rs,models.rs}`, `commands/mod.rs`, `state.rs`
  (`desktop_speaking`), `lib.rs` (setup, handlers, the exit hook), `app/src-tauri/Cargo.toml`
  (`[features] local-whisper = ["parley-audio/local-whisper"]`)
- `app/src/components/{SpeakExercise,LevelMeter}.tsx`, `app/src/screens/Settings/{SpeakingSection,
  Settings}.tsx`, `app/src/screens/Lesson/{Lesson,ExerciseView}.tsx`, `app/src/api/commands.ts`,
  `app/src/strings/en.json`
- `.github/workflows/ci.yml` (job `whisper`), `scripts/gates/run`, `scripts/gates/run.ps1`
- Docs: `docs/adr/0015-speaking-crates.md` (new); ARCHITECTURE §1 (`cpal` direct, `rubato` 5.1,
  `hound` 3.5, `whisper-rs` 0.16 optional), §2, §4 (`models/whisper/ggml-<name>.bin`,
  `speaking.whisper_model`, `transcribe_charges`), §5 (the commands below) and §9 (ADR-0015);
  TESTING §2 (`scripts/gates/run --whisper`), §6 (G4's sentences) and §8 (the `whisper` job);
  PUBLIK-INTEGRATION §5, §6.8 and §8 (Design notes give the figures); `THIRD_PARTY.md`;
  `README.md`; `docs/HANDBOOK.md`; `docs/ROADMAP.md`
- Tests: `crates/parley-audio/tests/{m10_speaking_and_pronunciation.rs,gates.rs}`,
  `crates/parley-core/tests/m10_speaking_and_pronunciation.rs`,
  `crates/parley-publik/tests/m10_speaking_and_pronunciation.rs` (`mod fake;`),
  `app/src/test/m10/speaking.test.tsx`, `tests/cases/10-speaking-and-pronunciation/*/`; fixtures
  `tests/fixtures/ai/P6/*.json` with a README

## Interfaces

### parley-core: `parley_core::audio` (additions; no device code)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError { NoDevice, Permission, TooQuiet, NotRecording, Backend(String) }

/// A finished take: RIFF WAVE, 16 kHz mono 16-bit PCM, trimmed; its length in ms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Take { pub wav: Vec<u8>, pub duration_ms: u64 }

pub trait Capture: Send + Sync {
    /// Whether a default input device exists; opens nothing.
    fn has_device(&self) -> bool;
    /// Opens the default input and records until `stop`, keeping at most `max_ms` of input.
    /// `on_level` gets the level of each 50 ms of input (dBFS, −60 to 0); `on_limit` runs once
    /// when `max_ms` is reached. A capture in progress is discarded first.
    fn start(&self, max_ms: u32, on_level: Box<dyn FnMut(f32) + Send>,
             on_limit: Box<dyn FnOnce() + Send>) -> Result<(), CaptureError>;
    /// Stops; the take mixed to mono, resampled to 16 kHz, cut to `max_ms`, trimmed. TooQuiet
    /// when trimming finds under 300 ms of speech; NotRecording with no capture in progress.
    fn stop(&self) -> Result<Take, CaptureError>;
}

pub trait LocalTranscriber: Send + Sync {
    /// The text of a 16 kHz mono 16-bit WAV in `lang`, with the model file at `model`
    /// (absolute). The error is for logs only.
    fn transcribe(&self, model: &Path, wav: &[u8], lang: Lang) -> Result<String, String>;
}

/// No microphone (the default): `has_device` false, `start` NoDevice, `stop` NotRecording.
pub struct NoCapture;

#[derive(Clone)]
pub struct Speaking {
    pub capture: Arc<dyn Capture>,
    /// None in a build without the `local-whisper` feature.
    pub whisper: Option<Arc<dyn LocalTranscriber>>,
    /// `models/registry.toml`, parsed (tests may replace it).
    pub models: ModelRegistry,
}
impl Default for Speaking { /* NoCapture, None, bundled_models() */ }

/// The `mic_permission` message for `platform` (Messages).
pub fn mic_permission_message(platform: Platform) -> String;
```

### parley-core: `parley_core::speech` (new)

```rust
// align.rs: Wagner and Fischer (1974) over word keys, unit costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit { Match(usize, usize), Substitute(usize, usize), Delete(usize), Insert(usize) }
/// The edit script from the start: Match/Substitute (expected i, heard j), Delete (expected i
/// missed), Insert (heard j extra). Backtrace from the end; on equal cost it prefers the diagonal
/// (match or substitution), then Delete, then Insert.
pub fn align(expected: &[String], heard: &[String]) -> Vec<Edit>;

// score.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WordStatus { Ok, Missed, Substituted }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordResult { pub expected: String, pub heard: Option<String>, pub status: WordStatus }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeakScore { pub words: Vec<WordResult>, pub extra: Vec<String>, pub score: f64,
                        pub pass: bool }

/// The words of `text` for scoring (rules in Design notes); `keep_case` keeps the text's case
/// (the display form of expected words), otherwise lower case. Spelled numbers are lower case.
pub fn speech_words(lang: Lang, text: &str, numbers: &Numbers, keep_case: bool) -> Vec<String>;
/// The comparison key of one word: marks folded (F2 of module 03), in German also ß → ss.
pub fn word_key(lang: Lang, word: &str) -> String;
/// Every `[...]` span of `heard` removed, both texts as `speech_words`, aligned on `word_key`.
/// One WordResult per expected word (display form; `heard` lower case, None when missed);
/// `extra` the inserted heard words in order; score = ok ÷ expected words (0.0 with none);
/// pass = score ≥ pass − 1e-9.
pub fn score(lang: Lang, expected: &str, heard: &str, numbers: &Numbers, pass: f64) -> SpeakScore;
/// pass → Correct; score ≥ 0.5 → Almost; otherwise Wrong.
pub fn speak_verdict(score: f64, pass: bool) -> Verdict;
/// pass → Good; score ≥ 0.5 → Hard; otherwise Again.
pub fn speak_rating(score: f64, pass: bool) -> Rating;
/// The grade a check shows: speak_verdict, no tags, no message, `corrected` the sentence unless
/// passed, `words` the expected words (display form) not ok, `rating` speak_rating.
pub fn speak_grade(sentence: &str, scored: &SpeakScore) -> Grade;

// models.rs
pub const WHISPER_DIR: &str = "models/whisper";
pub const DEFAULT_WHISPER_MODEL: &str = "base";
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelEntry { pub name: String, pub url: String, pub bytes: u64, pub sha256: String }
/// Ascending by bytes (tiny, base, small).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRegistry { pub whisper: Vec<ModelEntry> }
impl ModelRegistry { pub fn get(&self, name: &str) -> Option<&ModelEntry>; }
/// `schema = "parley.models/1"`, tables `[whisper.<name>]` with exactly `url` (https://), `bytes`
/// (> 0) and `sha256` (64 lowercase hex); names `[a-z0-9]+`; anything else is refused.
pub fn parse_model_registry(text: &str) -> Result<ModelRegistry, String>;
pub fn bundled_models() -> ModelRegistry;          // include_str! of models/registry.toml
pub fn model_file(name: &str) -> String;           // "models/whisper/ggml-<name>.bin"
/// The length equals `bytes` and the SHA-256 equals `sha256`.
pub fn model_verified(entry: &ModelEntry, bytes: &[u8]) -> bool;
```

### parley-core: `parley_core::content` (addition in `numbers.rs`)

```rust
/// The words for 0 to 100 of numbers.yaml as written, NFC; entry n is the number n.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Numbers { pub words: Vec<String> }
impl Numbers { pub fn spell(&self, n: u64) -> Option<&str>; }
/// The same checks as `load_numbers` (schema, 101 entries); the entries in order.
pub fn load_number_words(text: &str) -> Result<Numbers, ContentError>;
```

The service's `Course` gains `number_words: Numbers` (empty without a numbers file: digits then
stay as written).

### parley-core: `parley_core::ai` (additions in `ai/transcribe.rs`)

```rust
pub const TRANSCRIBE_FEATURE: &str = "transcribe";    // cache key feature, version 1
pub const TRANSCRIBE_TIMEOUT_S: u64 = 30;
/// The id the setup run of 2026-11-12 confirmed for /audio/transcriptions on publik API.
pub const PUBLIK_TRANSCRIBE_MODEL: &str = "gpt-4o-mini-transcribe";
/// The setup run's measured charge of one take; the cost line's figure until the install has its own.
pub const MEASURED_TRANSCRIBE_MICROS: i64 = 538;
pub const OFFLINE_LOCAL_NOTICE: &str = "You're offline, so the local model checked this.";

/// publik → PUBLIK_TRANSCRIBE_MODEL; own_key → `ai.own_key.transcribe_model` when set; else None.
pub fn transcribe_model(settings: &Settings) -> Option<String>;
/// The model in a local take's key: "whisper.cpp/ggml-<name>" (provider `local`).
pub fn local_model_id(name: &str) -> String;
/// Canonical key JSON (§6.1) with input {"audio_sha256", "language"}, and its SHA-256.
pub fn transcribe_key_json(provider: ProviderKind, model: &str, audio_sha256: &str,
                           lang: Lang) -> String;
pub fn transcribe_key(provider: ProviderKind, model: &str, audio_sha256: &str, lang: Lang)
    -> String;
/// §6.8: file_name "take.wav", mime "audio/wav", language, response_format "json",
/// temperature 0.0, timeout 30 s, the idempotency key of the key JSON at `attempt`.
pub fn transcribe_request(provider: ProviderKind, model: &str, wav: &[u8], lang: Lang,
                          attempt: u32) -> TranscribeRequest;
/// The text when `body` passes schemas/ai/transcribe.v1.schema.json (§6.8's schema).
pub fn parse_transcript(body: &serde_json::Value) -> Option<String>;
/// Every `[...]` span removed, white space collapsed, trimmed.
pub fn heard_text(transcript: &str) -> String;
/// "Speaking checks cost about {format_dollars(mean)} a take (measured)." with the mean of
/// `charges` rounded half up to a whole micro, or MEASURED_TRANSCRIBE_MICROS when empty.
pub fn transcribe_cost_line(charges: &[i64]) -> String;
/// The AppError of a failed P6 call (Design notes, Transcriber); `use_local` goes into the action.
pub fn speaking_error(e: &AiError, use_local: bool) -> AppError;
```

`TranscribeRequest` gains `pub temperature: f64` and `pub fn fields(&self) -> Vec<(&'static str,
String)>`: `model`, `language`, `response_format`, `temperature` in that order, a whole temperature
written as an integer (`0`). `parley-publik` writes exactly these text parts, then the `file` part,
and never a `prompt` part.

`Cache` gains `add_transcribe_charge(micros)` and `transcribe_charges() -> Vec<i64>` over the new
table `transcribe_charges(id INTEGER PRIMARY KEY, charge_micros INTEGER NOT NULL)` in
`cache/ai.sqlite3`: an amount per charged take, with no key, hash, text or time.

### parley-core: the service (`app/speak.rs`)

```rust
impl Parley {
    pub fn set_speaking(&mut self, speaking: Speaking);   // Parley::new starts with Speaking::default()
    pub fn mic_start(&mut self, max_s: u32, on_level: Box<dyn FnMut(f32) + Send>,
                     on_limit: Box<dyn FnOnce() + Send>) -> Result<(), AppError>;
    pub fn mic_stop(&mut self) -> Result<Recording, AppError>;
    pub fn recording_play(&mut self, recording_id: &str) -> Result<(), AppError>;
    pub fn speak_begin(&mut self, session_id: &str, exercise_id: &str, recording_id: &str,
                       local: bool, ai: Option<AiAccess>) -> Result<SpeakBegin, AppError>;
    pub fn speak_complete(&mut self, job: TranscribeJob, outcome: TranscribeOutcome)
        -> Result<SpeakResult, AppError>;
    /// Begin, run, complete.
    pub fn speak_check_with(&mut self, session_id: &str, exercise_id: &str, recording_id: &str,
                            local: bool, ai: Option<AiAccess>) -> Result<SpeakResult, AppError>;
    pub fn whisper_models(&self) -> Vec<ModelInfo>;
    pub fn whisper_download_begin(&mut self, name: &str) -> Result<DownloadBegin, AppError>;
    pub fn whisper_download_complete(&mut self, job: ModelDownload, outcome: DownloadOutcome)
        -> Result<Vec<ModelInfo>, AppError>;
    /// Begin, run with the injected `Env::net`, complete.
    pub fn whisper_download_with(&mut self, name: &str, progress: &mut dyn FnMut(ModelProgress))
        -> Result<Vec<ModelInfo>, AppError>;
    pub fn speaking_status(&self) -> SpeakingStatus;
    pub fn speak_available(&self, lang: Lang) -> bool;
    /// Quitting: stops a capture and removes the takes unless `speaking.keep_recordings`.
    pub fn on_exit(&mut self);
}
pub enum SpeakBegin { Done(SpeakResult), Pending(TranscribeJob) }
/// The take, its key, the language, the provider call or the local model, the fallback; no lock.
pub struct TranscribeJob { .. }
impl TranscribeJob {
    pub fn request(&self) -> Option<&TranscribeRequest>;   // None for a local job
    pub fn run(&self) -> TranscribeOutcome;
}
/// The transcript or the error, the charge, and which transcriber answered.
pub struct TranscribeOutcome { .. }
pub enum DownloadBegin { Done(Vec<ModelInfo>), Pending(ModelDownload) }
pub struct ModelDownload { /* the registry entry; no lock */ }
impl ModelDownload {
    pub fn name(&self) -> &str;
    /// Reports {0, bytes} before the request and {bytes, bytes} after the body arrives.
    pub fn run(&self, net: &dyn Net, progress: &mut dyn FnMut(ModelProgress)) -> DownloadOutcome;
}
pub struct DownloadOutcome { /* the response or the transport error */ }
```

`app/mod.rs` re-exports `DownloadBegin, DownloadOutcome, ModelDownload, SpeakBegin, TranscribeJob,
TranscribeOutcome`.

**`mic_start(max_s, …)`**: `max_s` must be 15 or 30 ("Unknown value {v} for max_s.",
`invalid_input`); `capture.start(max_s × 1000, …)`; `NoDevice` → `no_microphone`, `Permission` →
`mic_permission` with `mic_permission_message(Platform::current())`, `Backend` → `no_microphone`
"Parley could not record from the microphone. Try again.".

**`mic_stop()`**: `capture.stop()`; `NotRecording` → `invalid_input` "Nothing is being recorded.";
`TooQuiet` → `too_quiet`; otherwise the id is a version-4 UUID (lowercase 8-4-4-4-12 hex) from two
`Rng::next_u64` values (the first is the high half; version nibble 4, variant bits 10); the WAV is
written to `cache/recordings/<id>.wav` through `Fs` (a failure: "Parley could not write that
file.", `storage`); the service keeps, in memory, the id, the take's SHA-256 and duration:
`Recording { id, duration_ms }`. Nothing is sent anywhere.

**`recording_play(id)`**: an id recorded while the app runs whose file reads through `Fs`, else
`not_found` "This recording is no longer available. Record it again."; the OS voice is stopped and
`player.play(<the file's bytes>, 1.0)`; a player error is module 09's `no_voice` "Parley could not
play the sound. Check that a speaker or headphones are connected."

**`speak_begin`**, in order:

1. The run of `session_id` is in progress and holds `exercise_id` (else `not_found` "This exercise
   is no longer available."); the exercise is `speak` (else `invalid_input` "This exercise is not a
   speaking exercise."); the recording passes `recording_play`'s check (same error).
2. The transcriber. `local_ready` = `whisper` is Some and `model_file(speaking.whisper_model)` is
   listed by `Fs::list("models/whisper")`.
   - `local` is true or `speaking.transcriber = local`: `whisper` None → `no_provider` "Local
     speech checks are not available in this build. Choose your AI provider in Settings >
     Speaking."; model not downloaded → `no_provider` "The local speech model is not downloaded.
     Download it in Settings > Speaking ({megabytes(bytes)})." with action `{"model": name}`;
     otherwise **local**.
   - Otherwise by `ai.provider`: `publik` or `own_key` with `ai` = `Some(Ok(client))` and
     `transcribe_model` Some → **provider** (with a local fallback when `local_ready`); `own_key`
     without a transcription model → local when `local_ready`, else `no_provider` "Add a
     transcription model for your own key in Settings > AI, or download the local model in Settings
     > Speaking."; `Some(Err(e))` → `speaking_error(&e, local_ready)`; `unset` (and `ai` None) →
     local when `local_ready`, else the `needs_consent` error of `AiError::NeedsConsent` (the UI
     opens the AI setup, I12); `local` (a chat server) → local when `local_ready`, else `offline`
     "Speaking checks need a connection or the local model."
3. The key: `transcribe_key(kind, model, sha256(wav), lang)` (local: `ProviderKind::Local`,
   `local_model_id(name)`). A transcript held for it is scored and rated as `speak_complete` does:
   `Done` with `cached: true` and no request (I5); one with no words → `too_quiet`, no request.
   Otherwise `Pending(job)`; a provider job's request is `transcribe_request(…, attempt)` with
   attempt = 1 + the failed attempts held for the key.

**`TranscribeJob::run`** makes the one P6 call (never retried, I4) or the one local transcription;
a provider `AiError::Offline` with the local fallback runs the local transcription instead and the
result carries `OFFLINE_LOCAL_NOTICE`.

**`speak_complete`**: a provider reply's charge is recorded (`add_transcribe_charge`, and the
session's cost while the run is in progress), usable or not. A body that fails `parse_transcript`
counts a failed attempt for the key (in memory) and gives `ai_invalid` "The AI answer could not be
used."; a valid one is held under the key. A local failure gives `provider_error` "The local speech
model could not be used. Download it again in Settings > Speaking." The heard text is
`heard_text(transcript)`; no words → `too_quiet`, the card is not rated. Otherwise `score(lang,
exercise text, heard, number_words, speaking.pass_threshold)`, then `LessonRun::spoken`: the first
scored take of the exercise reviews its card (item, productive) with `speak_rating`, verdict
`speak_verdict`, exercise type `speak`, no hint, no response time, and becomes its answer
(`Answer::Text(heard)`): `rated: true`; a later take reviews nothing and returns the first take's
grade: `rated: false`. A run that ended meanwhile → "This exercise is no longer available." (the
charge is still recorded). Errors map through `speaking_error`.

**`speak_available(lang)`**: `speaking.enabled`, not skipped (`session_skip("speak")` until now +
60 minutes, injected clock), `capture.has_device()`, and a transcriber can be used: with
`transcriber = local`, `whisper` is Some; with `provider`, `ai.provider` is `publik`, or `own_key`
with a transcription model, or `local_ready`. `plan_for` sets `Capabilities.mic =
speak_available(lang)`, so a learn lesson turns up to 2 `translate_to_target` phrase entries of at
most 8 words into `speak` (module 03's rule).

**Takes**: `lesson_finish` (after the run finishes, whatever its result), `on_exit` and `app_start`
remove every file `Fs::list("cache/recordings")` names and forget the takes, unless
`speaking.keep_recordings` is on. `data_delete_all` removes them and every file in
`models/whisper/` too.

**`whisper_models()`**: one `ModelInfo` per registry entry: `downloaded` when `model_file(name)` is
listed (only a verified file is ever renamed there), `chosen` when it is
`speaking.whisper_model`. **`whisper_download_begin(name)`**: an unknown name → "Unknown value {v}
for name."; downloaded → `Done(list)`, no request; being downloaded → `busy` "This model is
already being downloaded."; else `Pending(job)`. **`whisper_download_complete`**: transport error →
`offline` "You're offline. Download the speech model when you're connected."; status not 200 →
`not_found` "The speech model could not be downloaded. Try again later."; otherwise the body is
written to `models/whisper/ggml-<name>.bin.part` through `Fs`, checked with `model_verified`, and
renamed to `ggml-<name>.bin` only when it matches; a mismatch removes the part file:
`checksum_mismatch` "The download was damaged. Try again." Returns the list.

### Result types (`api.rs`, mirrored in `commands.ts`)

```rust
pub struct Recording { pub id: String, pub duration_ms: u64 }
#[serde(rename_all = "snake_case")] pub enum Transcriber { Provider, Local }
pub struct SpeakResult {
    #[serde(flatten)] pub scored: SpeakScore,   // words, extra, score, pass
    /// The transcript as heard_text gives it.
    pub heard: String,
    pub grade: Grade,
    pub rated: bool,
    pub transcriber: Transcriber,
    pub cached: bool,
    pub notice: Option<String>,
}
pub struct ModelInfo { pub name: String, pub bytes: u64, pub size: String, pub downloaded: bool,
                       pub chosen: bool }
pub struct ModelProgress { pub name: String, pub bytes_done: u64, pub bytes_total: u64 }
pub struct SpeakingStatus { pub cost_line: String, pub speak_off_until: Option<i64>,
                            pub local_available: bool, pub microphone: bool }
pub struct MicLevel { pub dbfs: f32 }
pub const MIC_LEVEL_EVENT: &str = "mic://level";
pub const MIC_LIMIT_EVENT: &str = "mic://limit";
pub const MODEL_PROGRESS_EVENT: &str = "model://progress";
```

`size` is `megabytes(bytes)`. `speaking_status`: the cost line from the recorded charges (the file
is opened only when it exists), the skip's end while it is in the future, `whisper.is_some()`,
`capture.has_device()`.

### Commands and events

| Command | Arguments | Result |
|---|---|---|
| `mic_start` | `max_s` (15 or 30) | `()` (+ `mic://level` `{dbfs}` every 50 ms of input, `mic://limit` `{}` once) |
| `mic_stop` | | `Recording` |
| `recording_play` | `recording_id` | `()` |
| `speak_check` | `session_id, exercise_id, recording_id, local` | `SpeakResult` |
| `whisper_models` | | `[ModelInfo]` |
| `whisper_download` | `name` | `[ModelInfo]` (+ `model://progress`) |
| `speaking_status` | | `SpeakingStatus` |

`COMMANDS` gains the seven names after `session_skip`, in this order. `mic_start`'s callbacks only
emit (they run on the audio thread and never take a lock). `speak_check` asks the AI service for a
client only when `local` is false, `speaking.transcriber = provider` and `ai.provider` is `publik`
or `own_key`; begin, the job's `run` on a blocking thread with no lock held, complete, then
`ai://status`. `whisper_download` runs the same way with `HttpNet::new()`, the transport the app
gives the core. `lib.rs` builds the app and calls `on_exit` on `RunEvent::Exit`.

### parley-audio

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputFormat { pub rate_hz: u32, pub channels: u16 }
pub trait InputDevice: Send + Sync {
    fn available(&self) -> bool;
    /// Delivers interleaved 16-bit chunks with their format until `close`, from any thread.
    fn open(&self, sink: Box<dyn FnMut(InputFormat, &[i16]) + Send>) -> Result<(), CaptureError>;
    fn close(&self);
}
pub struct CpalInput;                         // the default input through cpal
pub struct Microphone<D: InputDevice> { .. }  // impl Capture
impl Microphone<CpalInput> { pub fn system() -> Microphone<CpalInput>; }
impl<D: InputDevice> Microphone<D> { pub fn new(device: D) -> Microphone<D>; }
/// Each frame's mean, rounded toward zero; a trailing partial frame is dropped.
pub fn downmix(interleaved: &[i16], channels: u16) -> Vec<i16>;
/// Exactly (len × 16,000 + rate ÷ 2) ÷ rate samples (integers); 16 kHz is copied.
pub fn resample_to_16k(mono: &[i16], rate_hz: u32) -> Vec<i16>;
/// 20 log10(rms ÷ 32,768) clamped to −60..0; −60 for silence or an empty window.
pub fn level_dbfs(window: &[i16]) -> f32;
pub struct Trimmed { pub start: usize, pub end: usize }
/// None when the take holds under 300 ms of speech (Design notes).
pub fn trim(samples: &[i16]) -> Option<Trimmed>;
/// The 44-byte header (PCM, 1 channel, 16,000 Hz, byte rate 32,000, align 2, 16 bits), then data.
pub fn wav_16k_mono(samples: &[i16]) -> Vec<u8>;
#[cfg(feature = "local-whisper")] pub struct WhisperLocal;   // impl LocalTranscriber
#[cfg(feature = "local-whisper")] impl WhisperLocal { pub fn new() -> WhisperLocal; }
```

`Microphone::start` downmixes each chunk, emits a level per whole 50 ms window (`rate_hz ÷ 20`
frames, carried across chunks) and keeps at most ⌈max_ms × rate_hz ÷ 1000⌉ frames, calling
`on_limit` once when full. `stop` closes the device, resamples, cuts to `max_ms × 16` samples,
trims and writes the WAV; `duration_ms` = samples × 1000 ÷ 16,000, rounded.

### parley-cli

- `parley speak score <input.json> [--json]`: input `{"lang", "expected", "heard",
  "pass_mark"?}` (0.8 when left out), numbers from `content/<lang>/numbers.yaml`; prints
  `{"words", "extra", "score", "pass", "rating"}` (`rating` 1 to 4 from `speak_rating`). Exit 1 on
  a file that is not such an input.
- `parley whisper download <tiny|base|small> [--dir <dir>]`: downloads the bundled registry's file
  into `<dir>` (default `models/whisper`) as `ggml-<name>.bin.part`, renamed after `model_verified`;
  prints `{"name", "path", "bytes"}`; a mismatch removes the part file and exits 1 with "error:
  The download was damaged. Try again.". Real network (`HttpNet`): for people and the CI job, never
  for tests.

### `models/registry.toml`

```toml
# Local Whisper models (docs/modules/10-speaking-and-pronunciation.md): whisper.cpp's ggml files,
# pinned to one commit of the Hugging Face repository ggerganov/whisper.cpp. Weights: OpenAI
# Whisper, MIT; ggml conversion: MIT. Downloaded on request and checked; never committed.
schema = "parley.models/1"

[whisper.tiny]
url = "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-tiny.bin"
bytes = 77691713
sha256 = "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21"

[whisper.base]
url = "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.bin"
bytes = 147951465
sha256 = "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"

[whisper.small]
url = "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small.bin"
bytes = 487601967
sha256 = "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"
```

### UI

Strings in `app/src/strings/en.json`. Wrappers send snake_case: `mic_start {max_s}`, `mic_stop
{}`, `recording_play {recording_id}`, `speak_check {session_id, exercise_id, recording_id, local}`,
`whisper_models {}`, `whisper_download {name}`, `speaking_status {}`; listeners for the three
events. The UI reads and writes `speaking.*` through `settings_get` and `settings_set`.

- `SpeakExercise` (for `speak`): the heading "Say this", the sentence, its English, and module 09's
  **Play**/**Slow**. A button **Hold to talk** (`aria-label` "Hold to talk, or hold Space"): pointer
  down or Space down (not a repeat, not in a text field, default prevented) calls `mic_start` with
  15; release calls `mic_stop`; `mic://limit` calls `mic_stop` as a release would. While recording
  its text is **Recording…** and `LevelMeter` shows: `role="meter"`, `aria-label` "Microphone
  level", `aria-valuemin` −60, `aria-valuemax` 0, `aria-valuenow` the last `mic://level` dbfs
  rounded. After a take: "Your take: {s} s" (one decimal) and **Replay my take**
  (`recording_play`); holding again replaces the take. The lesson's **Check** calls `speak_check`
  with `local: false` and shows `grade` on the result sheet.
- The result: "Parley heard: {heard}"; a list labelled "What Parley heard" with one item per
  expected word, class `word-ok`, `word-missed` or `word-substituted`, an `aria-hidden` icon (✓, ✗,
  ≠) and the text "{word}: right", "{word}: missed" or "{word}: heard {heard}"; "Also heard:
  {extra, joined by ', '}" when there are extra words; "Score {round(score × 100)}% (pass mark
  {round(speaking.pass_threshold × 100)}%)"; the notice; **Play the voice** (`tts_play` with the
  sentence, not slow); **Try again** (clears take and result; a new take is checked again,
  unrated).
- **Can't speak now** (before a result) calls `session_skip("speak")` and skips this and every later
  `speak` exercise of the lesson with no grade. A lesson opened while `speaking.enabled` is off,
  `speak_off_until` is in the future or `microphone` is false skips its `speak` exercises the same
  way.
- Errors (`role="alert"`): the message; `needs_credit` with its link as `AiBanner` shows it, **Use
  the local model** when `action.use_local` (calls `speak_check` with `local: true`) and **Can't
  speak now**; `disconnected` with **Reconnect**; `busy` with **Try again** (the same take);
  `needs_consent` opens the AI setup dialog and checks again after its Continue.
- Settings > Speaking (`SpeakingSection`, tab button **Speaking** after **Audio**): **Speaking
  exercises** (`speaking.enabled`); **Check my speaking with** **AI provider** or **Local model**
  (`speaking.transcriber`; Local model offered only when `local_available`, else the line "Local
  speech checks are not available in this build." and no model list); per model of
  `whisper_models`: its name and size, **Download ({size})** or "Downloaded", and a radio **Use
  {name}** (`speaking.whisper_model`); while downloading "Downloading {name}: {percent}%" from
  `model://progress`; **Pass mark** (0.5 to 1.0, step 0.05, shown as a percentage); **Keep my
  recordings** with "Recordings stay on this computer."; the cost line of `speaking_status`.

## Messages

`too_quiet`: "We didn't hear anything. Try again a little closer to the microphone." (also for a
transcript with no words); `no_microphone`: "No microphone was found. Connect one, or choose Can't
speak now." and "Parley could not record from the microphone. Try again."; `mic_permission`:
"Parley can't use the microphone. Open Settings > Privacy & security > Microphone and allow
desktop apps to use it." (Windows), "Parley can't use the microphone. Open System Settings >
Privacy & Security > Microphone and turn on Parley." (macOS), "Parley can't use the microphone.
Check that it is connected, not muted, and not in use by another app." (Linux); "The local speech
model is not downloaded. Download it in Settings > Speaking ({size})."; "The download was damaged.
Try again."; "Can't check speaking right now. {reason}" with the reason of PUBLIK-INTEGRATION §9;
"Speaking checks need a connection or the local model." (offline, no model); "Local speech checks
are not available in this build. Choose your AI provider in Settings > Speaking."; "Add a
transcription model for your own key in Settings > AI, or download the local model in Settings >
Speaking."; "The local speech model could not be used. Download it again in Settings > Speaking.";
"You're offline, so the local model checked this."; "Speaking checks cost about {$x} a take
(measured)."; "Nothing is being recorded."; "This recording is no longer available. Record it
again."; "This exercise is not a speaking exercise."; "You're offline. Download the speech model
when you're connected."; "The speech model could not be downloaded. Try again later."; "This model
is already being downloaded."; "Local speech checks are not available in this build."; "Recordings
stay on this computer."

## Out of scope

- Phoneme-level scoring and intonation; role-play turns (11 reuses `mic_start` with 30, the
  transcription job and the in-memory transcripts).
- Local Whisper on Android (13); the macOS microphone usage string (14).
- Streaming a model download to disk: the transport returns whole bodies (Design notes).

## Design notes

- Live check, from the maintainer's setup run of 2026-11-12 (module 06 Design notes), used instead
  of a new one: `GET /models` lists no audio id, so the standard id was probed by maintainer
  decision; `/audio/transcriptions` accepts `gpt-4o-mini-transcribe` with `language=es`,
  `response_format=json` and `temperature=0`, returned the spoken sentence exactly, and charged
  538 micros. That one measured take ($0.00054) replaces the $0.00040 estimate: PUBLIK-INTEGRATION
  §5's P6 row reads "$0.00054 (measured, `gpt-4o-mini-transcribe`)" and its lead says P5 and P6 are
  measured; §6.8 names the id; §8's P6 sentence gives the measured charge, and the daily totals
  keep the per-minute estimate (spoken turns are module 11's) with "(at the measured $0.00054 a
  take, the 10 speaking checks alone are 10 × $0.00054 = $0.0054)"; the lighter learner's row
  becomes $0.030 + $0.0030 + $0.00066 + $0.0054 = $0.039, $1.17 for 30 days, $0.78 for 20. Ten
  takes were not measured: each install measures its own from `x-publik-charge-micros`, and the
  cost line shows its mean, or 538 micros until it has one. Live calls spend money and are the
  maintainer's.
- Whisper models are whisper.cpp's ggml files pinned to commit
  `5359861c739e955e79d9a303bcbc70fb988958b1` of `ggerganov/whisper.cpp` on Hugging Face: tiny
  77,691,713 bytes (77.7 MB), base 147,951,465 (148.0 MB), small 487,601,967 (487.6 MB); MIT
  (OpenAI's weights and the conversion). `base` is the default. They live in the data directory
  under `models/whisper/ggml-<name>.bin` (ARCHITECTURE §4 is corrected from `<name>.bin`). The
  `resolve` URL answers with a redirect to Hugging Face's file store, which the transport follows.
  The download goes through the app's transport, never core code; the body is written to a `.part`
  name and renamed only after its length and SHA-256 match, so a file under its own name is always
  whole and verified, and "downloaded" means the file is listed. The transport returns whole
  bodies, so a download holds the file in memory (about 490 MB for `small`). The service lock is
  never held during the request.
- `local-whisper` needs cmake and libclang to build whisper.cpp, which a plain Windows Rust setup
  lacks, so the feature is off by default: `scripts/verify` and the `verify` job build without it.
  A build without it still has the registry, `whisper_models` and `whisper_download`
  (no whisper.cpp needed), and says that local transcription is unavailable: `local_available` is
  false, Settings > Speaking offers no Local model option and no model list, and a stored
  `transcriber = local` gives no `speak` exercise and, at a check, "Local speech checks are not
  available in this build. …". It never falls back to the provider, because that would spend money
  the learner did not choose.
- The CI job `whisper` (ubuntu-latest): apt installs `build-essential cmake libclang-dev
  libasound2-dev espeak-ng`; the toolchain from `rust-toolchain.toml`; `actions/cache` keeps
  `models/whisper/ggml-tiny.bin` under the key `whisper-ggml-tiny-<sha256>`, the tiny model's
  SHA-256 read from `models/registry.toml`; on a miss `cargo run -q -p parley-cli -- whisper
  download tiny` fetches and checks it; then `cargo deny --all-features check licenses` (the only
  feature in the workspace is `local-whisper`), `cargo clippy -p parley-audio --features
  local-whisper --all-targets -- -D warnings` and `cargo test -p parley-audio --features
  local-whisper -- --show-output` (so G4's mean or skip line shows in the log). espeak-ng is a
  separate program the gate runs; nothing links it.
- Dependencies: `cpal` 0.18 (Apache-2.0, already in the tree through rodio, now direct), `rubato`
  5.1 (MIT), `hound` 3.5 (Apache-2.0), `whisper-rs` 0.16 (Unlicense; whisper.cpp MIT). `hound`,
  `rubato` and `whisper-rs` each have one maintainer, so ADR-0015 names the exits: a WAV header is
  44 bytes, written by hand; a windowed-sinc resampler of our own for the fixed 48 and 44.1 to 16
  kHz cases; whisper.cpp's C API through our own bindings, or provider transcription only.
  `THIRD_PARTY.md` gets a row per crate and per model.
- Capture: a thread owns the cpal stream (it is not `Send` everywhere), as `RodioPlayer` owns its
  output. Samples become i16: f32 as round(clamp(x, −1, 1) × 32,767), u16 as x − 32,768, wider
  integers shifted to 16 bits. A build or play error whose text contains "denied" or "0x80070005"
  (case-insensitive) is `Permission`; others are `Backend`. The capture keeps at most the limit of
  input, and the 16 kHz take is cut again to exactly `max_ms × 16` samples, so a 15 s take is
  240,000 samples, 480,044 bytes. The UI stops a take at `mic://limit`, not on its own timer.
- Levels: RMS, not peak, of each 50 ms window of the mixed input, 20 log10(rms ÷ 32,768),
  clamped to −60..0 dBFS: a full-scale square reads 0 dBFS, silence −60, the meter's floor.
- Resampling: rubato's band-limited resampler, cut-off at or below 8 kHz, its delay removed so the
  output lines up with the input, then cut or padded with zeros to the exact length.
- Trim (Rabiner and Sambur 1975, simplified to energy): 20 ms frames of 320 samples (whole frames
  only); a frame's level is 20 log10(max(rms, 1) ÷ 32,768); the noise floor is the level at index
  ⌊n ÷ 10⌋ of the n levels sorted ascending; threshold = min(max(floor + 10 dB, −50 dBFS), −30
  dBFS); a speech frame is above the threshold; runs of under 3 frames are ignored; fewer than 15
  speech frames in all (300 ms), or none, is too quiet; keep from max(0, first run's start × 320 −
  2,400) to min(len, last run's end × 320 + 4,800). The −30 dBFS cap is new at planning: without it
  a take with no pause has its own speech as its noise floor and would be judged silent.
- Words for scoring: module 03's normaliser steps (NFC, typographic apostrophes to `'`, no-break
  spaces, lower case unless `keep_case`, every character other than a letter, digit or mark
  becomes a space except `'` or `-` between letters); then a hyphen between letters splits the word
  (`est-ce` → `est ce`, so a transcript with or without the hyphen scores the same); an apostrophe
  stays inside its word (`l'eau`, `j'ai`, `aujourd'hui` are one word each); a word of ASCII digits
  whose value is 0 to 100 becomes the words of that `numbers.yaml` entry, split the same way (`70`
  → `soixante dix`); larger numbers, decimals and words mixing digits and letters stay as written.
  Comparison folds marks (`días` = `dias`) and, in German, ß to ss; German compounds are not
  split. `numbers.yaml` keeps one form per number, so a transcript's `1` does not match `un`,
  `una`, `une` or `ein`: a known limit. `[...]` spans (whisper.cpp's `[BLANK_AUDIO]`) are removed
  from heard text only.
- Score = ok words ÷ expected words; extra words do not lower it; pass = score ≥ the pass mark
  (`speaking.pass_threshold`, default 0.8, a setting from 0 to 1; the UI offers 0.5 to 1.0), with
  a 1e-9 allowance, so 4 of 5 passes at 0.8.
- Ratings: pass → Good; score ≥ 0.5 → Hard; below → Again, on the productive card; verdicts
  correct, almost and wrong, so XP counts as for typed answers. Only the first scored take of an
  exercise rates its card; **Try again** is practice and appends nothing to the review log. An
  empty transcript and every error leave the card unrated. `exercise_submit` still grades typed
  text for a `speak` exercise (module 03); the UI no longer offers it.
- Transcriber: the learner's choice, never a silent switch to something that costs money. A
  provider failure maps as §9's speaking row: 402 → `needs_credit` "Can't check speaking right now.
  {402 message}" with action `{"top_up_url", "use_local"}`; disconnected → "Can't check speaking
  right now. publik API is disconnected." with `{"use_local"}`; 429 → its message; busy and timeout
  → their messages; offline → the local model when downloaded (with the notice), else "Speaking
  checks need a connection or the local model."; invalid → "The AI answer could not be used.";
  other provider errors → "Can't check speaking right now. {message}"; consent and key errors as
  module 06 gives them. Every check is a learner's action, so it goes out once even while
  `needs_credit` is set (§9).
- Transcripts are held in memory only, keyed by provider, model, the take's SHA-256 and language,
  with the failed-attempt count of each key (so a Try again after an unusable reply sends attempt
  2's idempotency key); nothing about a take is written to `cache/ai.sqlite3`. The cost line needs
  charges that outlive the app, so each charge is kept as an amount alone in `transcribe_charges`;
  clearing all caches forgets them and the line returns to 538 micros.
- Takes are local only (I13): `mic_stop` sends nothing; the one request that carries a take is the
  learner's Check, after consent. Removing every take at launch means a crash leaves none behind.
  With **Keep my recordings** on they stay in `cache/recordings/` until the setting is turned off
  and the next lesson ends.
- Local Whisper: one model context is kept loaded (reloaded when the chosen model changes); greedy
  decoding, temperature 0 with no fallback, the course language set, no translation, no
  timestamps, no context from earlier takes, non-speech tokens suppressed, threads min(4, the
  available parallelism); the take is padded with silence to at least 1 s.
- Gate G4 (TESTING.md §6), in `crates/parley-audio/tests/gates.rs`: sentences come from the course
  content: each language's `content/<lang>/syllabus.yaml` examples (`target`) in syllabus order with
  3 to 8 UAX #29 words, the first 60 (`gate_g4_speaking_loopback`, ignored, run by
  `scripts/gates/run --whisper`) or the first 5 (`g4_loopback_scaled`, in every `cargo test`). Each
  is synthesised to a WAV by gate-only code with no new dependency: Windows, PowerShell's
  System.Speech `SpeechSynthesizer` (`SetOutputToWaveFile`) with the first enabled installed voice
  whose culture's two-letter name is the language; Linux, `espeak-ng -v <lang> -w <out> -f <text
  file>`; macOS, `say -v <voice> --file-format=WAVE --data-format=LEI16@16000 -o <out> -f <text
  file>` with a voice whose locale starts `<lang>_` in `say -v '?'`; the text always goes through
  a file. Each WAV is decoded (`decode_mono_i16`), resampled to 16 kHz, trimmed (too quiet scores
  0), written with `wav_16k_mono`, transcribed by `WhisperLocal` (`base` for the full gate; the
  scaled one takes `base`, else `tiny`, from `models/whisper/` at the repository root) and scored
  with `score` at pass 0.8. It prints "G4 {lang}: mean {score} over {n} sentences ({model})". No
  threshold: the first full run's means are recorded in `scripts/gates/thresholds.toml` (`[g4]`,
  with date and version). It is skipped with one line naming the reason when the build has no
  `local-whisper` ("G4 skipped: this build has no local Whisper; build with --features
  local-whisper"), the model file is absent, or no voice exists for a language (that language
  only). `scripts/gates/run` runs the parley-audio gates too (G4 then prints its skip line);
  `--whisper` builds them with the feature; `run.ps1` matches.
- `tests/fixtures/ai/P6/*.json`, read by the core tests' scripted client as module 09's P5 files
  are: `{"status": 200, "body": {...}, "charge_micros": n}` for a reply, `{"error": "needs_credit",
  "message", "top_up_url"}` (the 402 body of `tests/fixtures/http/402-claimed.json`), `{"error":
  "offline"}` or `{"error": "timeout"}` for a failure. Files: `valid.json` (`{"text": "Yo es
  estudiante."}`, 538), `empty.json` (`""`), `blank-audio.json` (`"[BLANK_AUDIO]"`),
  `not-a-string.json` (`{"text": 42}`), `402.json`, `offline.json`, `timeout.json`.
- Decisions are filled in here during implementation.
- Licence: `rubato` 5.1 is MIT OR Apache-2.0 (its Cargo.toml, checked 2026-11-13), not MIT alone;
  ARCHITECTURE §1, ADR-0015 and `THIRD_PARTY.md` say so. `cpal` is a direct dependency with its
  default features off, the same 0.18 rodio uses.
- Resampling uses rubato's synchronous FFT resampler (`Fft`, both chunk sizes fixed, chunk 1,024)
  through `process_all`, which removes the start-up delay; its anti-alias cut-off is rubato's own
  for the output's Nyquist frequency, below 8 kHz. Should rubato refuse the rates (only a rate of
  zero), the take is decimated to the nearest earlier sample instead; a rate of zero gives no
  samples.
- Capture: `CpalInput` opens the default input in its default configuration and converts every
  sample format cpal delivers (8 to 64-bit integers, signed and unsigned, 24-bit packed in 32, f32
  and f64); DSD streams deliver nothing. cpal's own `PermissionDenied` kind also counts as
  `Permission`. `Microphone::start` answers `NoDevice` when the device says it is unavailable,
  and discards a capture in progress first; `stop` with no input at all is `TooQuiet`. Levels are
  reported for every whole window of input until `stop`, past the limit too; only the take stops
  growing. The callbacks run on the audio thread and must only pass the news on.
- Takes: the service keeps, in memory, each take's id and SHA-256 (the key's `audio_sha256`); the
  length goes back in `Recording` and is not kept. `lesson_finish` removes the takes whatever its
  result (an unknown or a story session too). **Delete everything** also forgets the held
  transcripts and failed attempts.
- Words for scoring: a `.` or `,` between two ASCII digits stays inside its word, so a decimal
  (`3,5`) stays as written rather than reading as two numbers; this flag is used by speaking
  only, and module 03's normaliser is otherwise unchanged. A word of digits is spelled by its
  value, so `03` is `tres`. `word_key` lowers the word before folding, so expected words keep
  their case for display and still compare with the lower-case heard words. A `[...]` span is
  replaced by a space, so the words on either side stay apart; a `[` with no `]` after it stays.
- `LessonRun::spoken(store, settings, exercise_id, heard, &SpeakScore, clock)` returns the grade
  and whether it rated. An exercise that already has an answer (a typed one through
  `exercise_submit` included) returns that answer's grade, unrated; an exercise that is not
  `speak` is "This exercise is no longer available."
- The transcriber: `own_key` without a transcription model is decided by the core before the
  client is used (the desktop command may already have built one; building it makes no call);
  `publik` or `own_key` with no client given is treated as `unset` (the local model when ready,
  else the consent error). Whether the local model is ready is decided at the begin, and
  that is the `use_local` of a 402's or a disconnection's action. A take the local model checked
  because the provider was offline is held under the local key and reports `transcriber: local`.
  `speak_check` emits `ai://status` after a provider job only.
- A 200 the client could not read at all (its own `AiError::Invalid`) counts a failed attempt
  for the take exactly as a transcript that fails the schema does (PUBLIK-INTEGRATION §6.1), so
  the next check sends attempt n + 1's idempotency key instead of replaying attempt n.
- Model downloads: the second progress report comes only with a 200 response. A download counts
  as being downloaded until its job is dropped, completed or not, so a failed thread never leaves
  a model "already being downloaded". `speaking.whisper_model` takes `tiny`, `base` or `small`
  (settings rule and schema), the bundled registry's names; when the chosen model is missing
  from a replaced registry, the not-downloaded message takes the bundled registry's size.
- Interface: **Check** with no take says "Hold to talk and say the sentence first."; the lesson asks
  for `speaking_status` only when it holds a `speak` exercise; Explain is not offered on a speaking
  result; a new hold clears the previous take, and holding is ignored while a check runs or a
  result shows; Space is ignored with a key repeat or in a text field, and its default is
  prevented on both press and release; a pointer that leaves the button or is cancelled stops the
  take as a release would. **Can't speak now** sits inside the 402's alert while it shows, so
  there is one such button at a time. **Try again** also clears the result sheet's grade.
  Settings > Speaking asks for `whisper_models` only when `local_available`, disables the other
  Download buttons while one model downloads, shows the percentage rounded down, and saves the
  pass mark in whole steps of 0.05.
- CLI: `parley speak score` reads `content/<lang>/numbers.yaml` from the working directory (none:
  digits stay as written), refuses unknown fields, and exits 1 on a pass mark outside 0 to 1.
  `parley whisper download` with a name the bundled registry lacks is a usage error (exit 2,
  "error: unknown model {name}; use tiny, base, small. Run parley --help."); it creates the
  directory, and an offline or failed request exits 1 with the service's messages.
- Desktop: the app is built and then run with an event loop that calls `on_exit` at
  `RunEvent::Exit`. `TranscribeRequest` is no longer `Eq` (its temperature is a float).
- Gates: `scripts/gates/run --whisper` and `scripts/gates/run.ps1 --whisper` take the same
  argument; `scripts/gates/thresholds.toml` gets `[g4]` at G4's first full run, not before.
- The CI job `whisper` reads the tiny model's SHA-256 from `models/registry.toml` with `awk`,
  caches the file with `actions/cache@v4`, and keeps its own Rust cache (`key: whisper`).
- Local Whisper is compiled only by the CI job `whisper`: building whisper.cpp needs cmake and
  libclang, so `whisper_local.rs` was written against whisper-rs 0.16's API and checked there.
  It routes whisper.cpp's console logging away (`install_logging_hooks`), reads the take with
  `hound`, and reports a missing model as an error, never a panic.

## Acceptance tests

Code tests in three files named `m10_speaking_and_pronunciation.rs`: in `crates/parley-audio/tests/`
(a fake `InputDevice` fed generated integer tones, and the pure functions), in
`crates/parley-core/tests/` (a fake `Capture` returning WAV bytes or errors, a fake
`LocalTranscriber`, a scripted `AiClient` replying with the P6 fixtures, a fake `Player`, a
scripted `Net`, an injected clock, the bundled courses) and in `crates/parley-publik/tests/` (the
fake server); golden cases in `tests/cases/10-speaking-and-pronunciation/`; UI tests in
`app/src/test/m10/speaking.test.tsx`. "Tone" below is an integer square wave of period 40 samples
at ±16,000 unless it says full scale (±32,767). Where a core test needs `speak` composed, the
transcriber is made ready with a fake `LocalTranscriber` in `Speaking`, `speaking.transcriber =
local` and a file at `models/whisper/ggml-base.bin`. Contract tests 1 to 18 keep passing; this
module adds test 29 of `docs/PUBLIK-INTEGRATION.md` §11.

1. `resample_to_16k` of a 48 kHz, 1 s square wave (period 48 samples) → 16,000 ± 1 samples whose
   zero crossings are 2,000 a second ± 2%; 44.1 kHz, 1 s → 16,000 ± 1; `downmix` of stereo with
   L = +1000 and R = −1000 → mono 0.
2. `trim` on 0.5 s silence, 1.0 s tone, 0.7 s silence (16 kHz) → start 0.35 s (sample 5,600) and
   end 1.8 s (sample 28,800), each within one frame (320 samples).
3. A silence-only take → `trim` None and `stop` `TooQuiet`; 200 ms of tone between silences → the
   same; in the core, `TooQuiet` gives `too_quiet` and its message, and no file is written.
4. A 16 s take of tone at 16 kHz (and again at 48 kHz) → `on_limit` once, stopped at 15 s; the WAV
   is 480,044 bytes, 16 kHz, mono, 16-bit, `duration_ms` 15,000.
5. Scoring (es): `yo soy estudiante` against heard `yo es estudiante` → ok, substituted (`es`), ok;
   score 0.667 (tolerance 0.001); fail at 0.8. `quiero un café con leche` against `quiero café con
   leche` → missed `un`; 0.8; pass. `buenos días` against `buenos días señor` → 1.0, extra `señor`.
   `tengo tres gatos` against `tengo 3 gatos` → 1.0. `días` against `dias` → ok. (Through `score`
   and the golden cases below.)
6. Ratings: `speak_rating` 1.0 → Good; 0.6 → Hard; 0.4 → Again (pass mark 0.8); a check scoring
   0.667 appends one `review_log` row for the productive card with rating 2, verdict `almost` and
   exercise type `speak`.
7. The P6 request is multipart with `file` (`take.wav`, `audio/wav`, the take's bytes), `model`
   (`gpt-4o-mini-transcribe`), `language=es`, `response_format=json`, `temperature=0`, no `prompt`
   field, and a body under 4,000,000 bytes; in the core, `transcribe_request` and `fields()` give
   exactly those values, timeout 30 and the idempotency key of the key JSON.
8. The same take bytes and language again while the app runs (the same recording, or a second
   recording with identical bytes) → no request, `cached: true`; the transcript is held in memory
   only, so nothing about the take is written to `cache/ai.sqlite3`: it has no `ai_cache`,
   `ai_attempts` or `audio_cache` row, and its bytes contain neither the take's SHA-256 nor the
   transcript (the charge is an amount in `transcribe_charges`, test 20).
9. An empty transcript (and `[BLANK_AUDIO]`) → the `too_quiet` message; the card is not rated (no
   `review_log` row).
10. `transcriber = local` with no model (local Whisper in the build) → the not-downloaded message
    with the `base` size from the registry: "… Download it in Settings > Speaking (148.0 MB).";
    the local transcriber is not called.
11. `whisper_download` shows the size first (the first progress is {0, bytes} and comes before the
    request; in the UI the button reads **Download ({size})** before any call); with a test
    registry, a served file with one changed byte → "The download was damaged. Try again." and no
    file left in `models/whisper/`; the right bytes → `ggml-<name>.bin` and `downloaded: true`.
12. Fake capture with no device → `no_microphone`, and the composer then produces no `speak` (with
    a device it produces one or two for the same skill); with permission denied → the platform
    message (all three checked through `mic_permission_message`).
13. With `keep_recordings` off, the take files are gone after `lesson_finish` and after quitting
    (`on_exit`), and a take left in `cache/recordings/` is gone after the next `app_start`; with it
    on, all stay.
14. **Can't speak now** at 10:00 (injected clock) → no `speak` exercise until 11:00, then allowed
    again; `speak_off_until` is 11:00 meanwhile; in the UI, the button calls `session_skip` with
    `speak` and skips this and later `speak` exercises with no grade.
15. A 402 → "Can't check speaking right now.", the message and link, and **Use the local model**
    when a model is downloaded (`action.use_local` true, false without); in the UI that button
    calls `speak_check` with `local: true`.
16. `recording_play` plays the trimmed file: the fake player receives exactly the bytes of
    `cache/recordings/<id>.wav` with stretch 1.0; an unknown id → "This recording is no longer
    available. Record it again."; in the UI, **Replay my take** calls it with the take's id.
17. (`local-whisper` job, model present) `WhisperLocal` transcribing the generated tone returns
    without error; skipped with the message of `docs/TESTING.md` §2 when the model is absent.
18. Gate G4: mean loopback score recorded at first run; in `verify`, 5 sentences when a voice and
    the model exist, else skipped with a message.
19. A fake device feeding 1 s of a full-scale square wave at 48 kHz in 10 ms chunks gives exactly
    20 levels, each 0 dBFS (tolerance 0.1 dB), and 1 s of digital silence 20 levels of −60 dBFS,
    the meter's floor (levels are clamped to −60 to 0 dBFS); `MIC_LEVEL_EVENT` is `mic://level`;
    in the UI the meter's value follows a `mic://level` event.
20. Cost line: no charges → "Speaking checks cost about $0.00054 a take (measured)."; charges 500
    and 600 → "$0.00055"; a check charged 538 adds 538 to the session's cost (contract test 34)
    and to `transcribe_charges`.
21. A build without local Whisper (`whisper` None): `local_available` false; with `transcriber =
    local` no `speak` is composed, and a check gives "Local speech checks are not available in this
    build. …" with no request; the UI shows that line and no Local model option.
22. The bundled registry lists tiny, base and small with the URLs, bytes and SHA-256 above;
    `whisper_models` gives sizes "77.7 MB", "148.0 MB", "487.6 MB" with `base` chosen; a registry
    with an `http://` URL, a 63-character SHA-256 or an unknown key is refused.
23. Offline with a model downloaded → the local transcription scores the take with the notice
    "You're offline, so the local model checked this."; without one → "Speaking checks need a
    connection or the local model."; a reply that fails the schema (`not-a-string.json`) →
    "The AI answer could not be used.", and the next check of the take carries attempt 2's key.
24. A second take on a rated exercise → `rated: false`, the first grade, no new `review_log` row.
25. UI, the speak exercise: Space down and up (and the button's pointer down and up) call
    `mic_start {max_s: 15}` and `mic_stop {}`; `mic://limit` calls `mic_stop`; **Check** calls
    `speak_check {session_id, exercise_id, recording_id, local: false}`; each word shows its class,
    icon and label ("soy: heard es"), "Also heard: señor", the score line; **Play the voice** calls
    `tts_play` with the sentence; **Try again** allows a new take; the too-quiet message shows.
26. Privacy and consent: `mic_start`, `mic_stop`, `recording_play` and `lesson_finish` make no call
    on the scripted `AiClient` or `Net` (I13); with `ai.provider = unset` and no local model, a
    check gives the `needs_consent` error and no call (I12); with publik API chosen and consent
    given, the check makes exactly one call, whose `audio` is exactly the take file's bytes (I13).

Invariants: I2 (tests 9 and 23: a transcript that fails the schema is neither scored nor held),
I4 (test 23: one call per check; the next attempt is the learner's), I5 (test 8), I10 (test 7 and
contract test 29), I12 and I13 (test 26).

Golden cases, one directory each with `input.json` (`lang`, `expected`, `heard`, `pass_mark`;
0.8 unless shown), `args.txt` (`speak`, `score`, `{input}`, `--json`), `expected.json` and
`tolerance.json` (`{"score": 0.001}`). Expected values come from the rules above. Each word is
`{"expected", "heard", "status"}`: `expected` as written in the sentence, `heard` the lower-case
heard word (the expected word in lower case unless shown, `null` when missed).

| Case | lang, expected, heard, pass mark | words (status, heard) | extra | score, pass, rating |
|---|---|---|---|---|
| `score-es-substituted-fails` | es, `yo soy estudiante`, `yo es estudiante`, 0.8 | yo ok yo; soy substituted es; estudiante ok | – | 0.667, false, 2 |
| `score-es-pass-mark-0.6` | the same, 0.6 | the same | – | 0.667, true, 3 |
| `score-es-missed-passes-at-the-mark` | es, `quiero un café con leche`, `quiero café con leche` | quiero ok; un missed null; café, con, leche ok | – | 0.8, true, 3 |
| `score-es-extra-word` | es, `buenos días`, `buenos días señor` | buenos ok; días ok | señor | 1.0, true, 3 |
| `score-es-digits-spelled` | es, `tengo tres gatos`, `tengo 3 gatos` | tengo ok; tres ok tres; gatos ok | – | 1.0, true, 3 |
| `score-es-accent-folded` | es, `días`, `dias` | días ok dias | – | 1.0, true, 3 |
| `score-es-empty-heard` | es, `Buenos días.`, `` | Buenos missed; días missed | – | 0.0, false, 1 |
| `score-es-bracket-dropped` | es, `hola`, `[BLANK_AUDIO] hola` | hola ok hola | – | 1.0, true, 3 |
| `score-fr-hyphen-and-elision` | fr, `Est-ce que l'eau est froide ?`, `est ce que l'eau est froide` | Est, ce, que, l'eau, est, froide ok | – | 1.0, true, 3 |
| `score-fr-number-with-hyphen` | fr, `J'ai soixante-dix ans.`, `J'ai 70 ans.` | J'ai ok j'ai; soixante ok; dix ok; ans ok | – | 1.0, true, 3 |
| `score-de-eszett` | de, `Die Straße ist lang.`, `die Strasse ist lang` | Die ok die; Straße ok strasse; ist, lang ok | – | 1.0, true, 3 |
| `score-de-number-beyond-100` | de, `Das kostet hundertzehn Euro.`, `Das kostet 110 Euro.` | Das, kostet ok; hundertzehn substituted 110; Euro ok euro | – | 0.75, false, 2 |

Where each test lives: 1, 2, 3 (`trim`, `stop`), 4, 17 and 19 (levels) in the parley-audio m10
file (17 under `#[cfg(feature = "local-whisper")]`); 18 in `crates/parley-audio/tests/gates.rs`; 3
(message), 5 to 16, 19 to 24 and 26 in the core m10 file; 5 also in the golden cases; 7 also, and
contract test 29 (the request carries `language`, no prompt field, and a WAV under 4 MB; a body of
4,000,000 bytes or more is refused before any connection), in the parley-publik m10 file; the UI
halves of 11, 14, 15, 16, 19, 21 and 25 in `app/src/test/m10/speaking.test.tsx`.

## Status

Done (2026-11-14)
