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

- `parley-audio`: playback through `rodio` (MP3 and WAV), OS voices through `tts` (Windows, macOS,
  Android) and Speech Dispatcher's `spd-say` command (Linux), both behind the `OsVoices` trait, voice
  listing per language tag; the decoder that measures a clip's duration; WSOLA time stretching.
- `voices_list`, `tts_play`, `tts_stop`, `tts_prefetch`, `audio_status` and `session_skip` commands
  and the `audio://ended` event.
- Source choice: natural when `audio.voice_source = natural`, a provider with speech is ready and
  `needs_credit` is not set; otherwise the OS voice.
- P5 through `AiClient::speech`, validation of §6.7, the audio cache in `cache/audio/`, one-ahead
  prefetch of the next exercise's sentence, fallbacks.
- Slow mode: the request's `speed` when `content/voices.toml` says the provider honours it, else
  local WSOLA time stretching; OS voices use their rate setting (normal rate × the slow speed,
  clamped to the backend's range).
- `content/voices.toml`: speech model id, whether `speed` is honoured, and a default voice per
  language, from the maintainer's setup run.
- `listen_type` enabled in the composer when a voice exists for the course language; **Can't listen
  now** turns it off for 60 minutes.
- Autoplay setting: prompts in the target language and the correct answer on the result sheet.
- Settings > Audio: source, voices per language, slow speed, autoplay, clear audio cache, and the
  measured cost line.

## Files

- `crates/parley-audio/src/{lib.rs,playback.rs,os_tts.rs,decode.rs,wsola.rs}`
- `crates/parley-core/src/audio.rs` (the audio traits and pure helpers),
  `crates/parley-core/src/ai/speech.rs` (re-exported from `ai/mod.rs`),
  `crates/parley-core/src/app/audio.rs` (the service methods), the `audio_cache` and
  `speech_charges` additions in `crates/parley-core/src/ai/cache.rs`, `Fs::list` in
  `crates/parley-core/src/env.rs`, `crates/parley-core/src/api.rs` (`COMMANDS` and the result
  types), the composer switch in `crates/parley-core/src/app/lesson.rs`
- `content/voices.toml`
- `app/src-tauri/src/commands/audio.rs`, the audio objects in `app/src-tauri/src/state.rs` and
  `lib.rs`, `DirFs::list` in `app/src-tauri/src/env.rs`; `app/src/components/{PlayButton,ListenType}.tsx`;
  `app/src/screens/Settings/AudioSection.tsx`; `app/src/screens/Lesson/*` (autoplay, prefetch,
  Can't listen now); `app/src/api/commands.ts`; `app/src/strings/en.json`
- Docs: `docs/adr/0014-os-voices.md` (new), ARCHITECTURE §1 (`tts` 0.26, `rodio` 0.23), §4
  (`speech_charges`), §5 (the commands below) and §9 (ADR-0014); PUBLIK-INTEGRATION §5 and §8 (the
  measured P5 charge); `THIRD_PARTY.md`; `README.md`; `docs/HANDBOOK.md`
- Tests: `crates/parley-audio/tests/m09_listening_and_voices.rs`,
  `crates/parley-core/tests/m09_listening_and_voices.rs`, `app/src/test/m09/*.test.tsx`; fixtures
  `tests/fixtures/ai/P5/*.json` with a README

## Interfaces

### parley-core: `parley_core::audio` (new; no device code)

```rust
/// An installed OS voice. `lang_tag` is its BCP 47 tag as the backend reports it (`es-ES`, `es_MX`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsVoice { pub id: String, pub name: String, pub lang_tag: String }

/// A backend's speaking rates (WinRT: 0.5, 1.0, 6.0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateRange { pub min: f32, pub normal: f32, pub max: f32 }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError { NoDevice, Backend(String), Decode }

pub trait OsVoices: Send + Sync {
    fn list(&self) -> Vec<OsVoice>;
    fn rates(&self) -> RateRange;
    /// Starts speaking and returns; a play in progress is interrupted.
    fn speak(&self, text: &str, voice: &str, rate: f32) -> Result<(), AudioError>;
    fn stop(&self);
}

pub trait Player: Send + Sync {
    /// Starts playing MP3 or WAV bytes and returns; a play in progress is interrupted. `stretch`
    /// is the length factor of WSOLA (1.0 plays as is; 1 ÷ 0.75 is slow mode stretched locally).
    fn play(&self, audio: &[u8], stretch: f32) -> Result<(), AudioError>;
    fn stop(&self);
}

pub trait AudioDecoder: Send + Sync {
    /// The clip's duration, or None when the bytes do not decode.
    fn duration_ms(&self, audio: &[u8]) -> Option<u64>;
}

/// RIFF WAVE with PCM data only: duration = data bytes ÷ byte rate (pure parsing, for tests and
/// as the core's default).
pub struct WavDecoder;
/// No OS voices (the default until the app sets its own; also Linux without Speech Dispatcher).
pub struct NoVoices;
/// Plays nothing and succeeds.
pub struct NoPlayer;

/// What the service plays through and with.
#[derive(Clone)]
pub struct Audio {
    pub os: Arc<dyn OsVoices>,
    pub player: Arc<dyn Player>,
    pub decoder: Arc<dyn AudioDecoder>,
    /// `content/voices.toml`, parsed (tests may replace it).
    pub natural: VoicesConfig,
}
impl Default for Audio { /* NoVoices, NoPlayer, WavDecoder, the bundled voices.toml */ }

/// The voices whose tag's primary subtag is `lang` (case-insensitive; `-` or `_`), in list order.
pub fn voices_for(voices: &[OsVoice], lang: Lang) -> Vec<OsVoice>;
/// normal, or normal × slow_speed clamped to [min, max] when `slow`.
pub fn os_rate(range: RateRange, slow: bool, slow_speed: f64) -> f32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform { Windows, MacOs, Linux }
impl Platform { pub fn current() -> Platform; }
/// The `no_voice` message for `lang` on `platform` (Messages below).
pub fn no_voice_message(lang: Lang, platform: Platform) -> String;
```

`Fs` gains `fn list(&self, dir: &Path) -> Result<Vec<String>, FsError>` (the file names in `dir`,
relative to the data directory; a missing directory is an empty list), with a default body that
returns an empty list so existing fakes keep compiling.

### parley-core: `parley_core::ai` (additions in `ai/speech.rs`, re-exported from `ai/mod.rs`)

```rust
pub const SPEECH_FEATURE: &str = "speech";      // cache key feature, version 1
pub const SPEECH_TIMEOUT_S: u64 = 20;
pub const NATURAL_UNAVAILABLE: &str = "Natural voice unavailable, using the system voice.";
/// The setup run's measured charge of one sentence (tts-1, alloy), the cost line's figure until
/// this install has measured its own.
pub const MEASURED_SPEECH_MICROS: i64 = 540;

/// `content/voices.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoicesConfig { pub model: String, pub speed_honoured: bool, pub voices: BTreeMap<Lang, String> }
pub fn parse_voices_toml(text: &str) -> Result<VoicesConfig, String>;
pub fn bundled_voices() -> VoicesConfig;          // include_str! of content/voices.toml

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceSource { Os, Natural }

/// Natural when `audio.voice_source = natural`, `status` is Some (a client was built: provider
/// chosen and consent given), its `needs_credit` is false, and a natural voice is configured for
/// `lang`: on `publik`, `voices.toml` has a voice for it; on `own_key`, `ai.own_key.speech_model`
/// is set; `local` never. Otherwise Os.
pub fn choose_source(settings: &Settings, status: Option<&AiStatus>, natural: &VoicesConfig, lang: Lang) -> VoiceSource;

/// The model and voice a natural request for `lang` uses: on publik, voices.toml's model and
/// `audio.natural_voice.<lang>` or voices.toml's voice; on own_key, `ai.own_key.speech_model` and
/// `audio.natural_voice.<lang>`, `ai.own_key.speech_voice` or voices.toml's voice, in that order.
pub fn natural_voice(settings: &Settings, natural: &VoicesConfig, lang: Lang) -> Option<(String, String)>;

/// The canonical cache-key JSON (§6.1) with input {"text_nfc", "voice", "speed"}, and its SHA-256.
pub fn speech_key_json(provider: ProviderKind, model: &str, text: &str, voice: &str, speed: f64) -> String;
pub fn speech_key(provider: ProviderKind, model: &str, text: &str, voice: &str, speed: f64) -> String;
/// The request of §6.7: the text NFC, `response_format` "mp3", timeout 20 s, the idempotency key of
/// the key JSON at attempt 1.
pub fn speech_request(provider: ProviderKind, model: &str, text: &str, voice: &str, speed: f64) -> SpeechRequest;
/// `cache/audio/<key>.mp3`.
pub fn audio_file(key: &str) -> String;
/// (200, 3 × 500 × words ÷ speed) in ms, words counted as UAX #29 words.
pub fn speech_bounds_ms(text: &str, speed: f64) -> (u64, u64);
/// §6.7: Content-Type `audio/*` (case-insensitive, parameters allowed), the bytes decode, and the
/// duration lies within `speech_bounds_ms`.
pub fn speech_valid(reply: &AudioReply, text: &str, speed: f64, decoder: &dyn AudioDecoder) -> bool;
/// "Natural voices cost about {format_dollars(mean)} a sentence (measured)." with the mean of
/// `charges` rounded half up to a whole micro, or MEASURED_SPEECH_MICROS when `charges` is empty.
pub fn speech_cost_line(charges: &[i64]) -> String;
```

`SpeechRequest` gains `pub fn body(&self) -> serde_json::Value`, the JSON body of §6.7
(`model`, `input`, `voice`, `response_format`, `speed`); `parley-publik` sends exactly it.

`Cache` gains `put_audio(&AudioEntry)` (`key, provider, voice, speed, file, bytes`; `created_at`
from its clock), `audio_len() -> usize`, `clear_audio() -> Result<usize, StoreError>` (the rows
removed), `add_speech_charge(key, micros)` and `speech_charges() -> Vec<i64>`. The new table, beside
`audio_cache` in `cache/ai.sqlite3`: `speech_charges(key TEXT PRIMARY KEY, charge_micros INTEGER
NOT NULL, created_at INTEGER NOT NULL)`. A cached sentence is found by its file: when
`cache/audio/<key>.mp3` reads, it plays with no request (I5), even after `cache/ai.sqlite3` was
removed; the `audio_cache` row is its record for clearing.

### parley-core: the service (`app/audio.rs`)

```rust
impl Parley {
    pub fn set_audio(&mut self, audio: Audio);             // Parley::new starts with Audio::default()
    pub fn voices_list(&self, lang: &str) -> Result<Vec<Voice>, AppError>;
    pub fn tts_begin(&mut self, text: &str, lang: &str, slow: bool, session_id: Option<&str>,
                     ai: Option<AiAccess>) -> Result<TtsBegin, AppError>;
    pub fn tts_complete(&mut self, job: SpeechJob, outcome: SpeechOutcome) -> Result<TtsPlayed, AppError>;
    /// Begin, run, complete; or wait for the fetch of the same sentence and begin again.
    pub fn tts_play_with(&mut self, text: &str, lang: &str, slow: bool, session_id: Option<&str>,
                         ai: Option<AiAccess>) -> Result<TtsPlayed, AppError>;
    pub fn tts_stop(&mut self);
    pub fn prefetch_begin(&mut self, session_id: &str, exercise_id: &str, ai: Option<AiAccess>)
        -> Result<PrefetchBegin, AppError>;
    pub fn prefetch_complete(&mut self, job: SpeechJob, outcome: SpeechOutcome) -> Result<Prefetched, AppError>;
    pub fn tts_prefetch_with(&mut self, session_id: &str, exercise_id: &str, ai: Option<AiAccess>)
        -> Result<Prefetched, AppError>;
    pub fn session_skip(&mut self, kind: &str) -> Result<(), AppError>;
    pub fn audio_status(&self) -> AudioStatus;
    /// Not skipped, and an OS voice for `lang` exists or `audio.voice_source = natural` with a
    /// natural voice configured for `lang` (`natural_voice` is Some).
    pub fn listen_available(&self, lang: Lang) -> bool;
}
pub enum TtsBegin { Done(TtsPlayed), Pending(SpeechJob), Waiting(SpeechWait) }
pub enum PrefetchBegin { Done(Prefetched), Pending(SpeechJob) }
pub struct SpeechJob { /* the client, the request, the key, the decoder, its fetch; no lock */ }
impl SpeechJob { pub fn request(&self) -> &SpeechRequest; pub fn run(&self) -> SpeechOutcome; }
impl Drop for SpeechJob { /* ends its fetch, completed or not */ }
pub struct SpeechOutcome { /* the reply or the error, and whether it passed §6.7 */ }
/// A fetch of the same sentence to wait for, with no lock held, before beginning again.
pub struct SpeechWait { /* the fetch */ }
impl SpeechWait { pub fn wait(self); }   // blocks until the fetch has ended
```

`tts_begin`, in order:

1. `lang` must be a course language (the module 04 unknown-value error otherwise); `text`, trimmed,
   must not be empty ("There is no text to read aloud.", `invalid_input`) or longer than 500
   characters ("This text is too long to read aloud.", `invalid_input`).
2. The source is `choose_source(settings, ai's client status when ai is Some(Ok), natural, lang)`.
3. Natural: model and voice from `natural_voice`; speed = `audio.slow_speed` when `slow` and
   `speed_honoured`, else 1.0; stretch = 1 ÷ `slow_speed` when `slow` and not `speed_honoured`,
   else 1.0. When `cache/audio/<key>.mp3` reads through `Fs`, the player plays it:
   `Done(TtsPlayed { source: natural, cached: true })`, no request. Otherwise, when the key is
   being fetched (below), `Waiting(wait)`, no request; otherwise `Pending(job)`.
4. Os (and every fallback): the voice is `audio.os_voice.<lang>` when that id is installed, else
   the first of `voices_for(os.list(), lang)`; none: `no_voice` with `no_voice_message(lang,
   Platform::current())`. `os.speak(text, voice, os_rate(os.rates(), slow, slow_speed))`.
   `Done(TtsPlayed { source: os, cached: false })`.

`SpeechJob::run` makes the one P5 call (never retried, I4) and checks `speech_valid`.
`tts_complete`: a valid reply is written to `cache/audio/<key>.mp3` through `Fs`, recorded with
`put_audio`, its charge recorded with `add_speech_charge` and added to the session's cost when
`session_id` names a lesson in progress, then played: `TtsPlayed { source: natural, cached: false }`.
A reply that fails §6.7 adds its charge the same way (it was charged), stores no file, counts as a
failed attempt in `ai_attempts` (so the next request for the sentence carries attempt n + 1's
idempotency key, as a Try again does in §6.1, and is not answered with the same unusable clip),
and falls back to the OS voice with `notice: NATURAL_UNAVAILABLE`. A 402 (`AiError::NeedsCredit`) falls back to the
OS voice with `ai_error` set to the `needs_credit` error and no notice; the client's status then
says `needs_credit`, so later plays choose the OS voice without a request and without a banner,
and no prefetch is made, until the status no longer says it. Any other error (offline, timeout,
provider error) falls back with the notice. A player error is `no_voice` "Parley could not play the
sound. Check that a speaker or headphones are connected."

Sentences being fetched: the service keeps in memory the cache keys of the natural sentences being
fetched. Every `Pending(job)` that `tts_begin` or `prefetch_begin` returns marks its key as being
fetched, and the mark ends when the job is completed or dropped (a call whose thread failed never
leaves a play waiting); a completion stores a valid clip before the mark ends. For a key being
fetched, `prefetch_begin` gives `Prefetched { prefetched: false }` and `tts_begin` gives
`Waiting(wait)`, both with no request. The caller calls `wait.wait()` with no lock held, which
returns once the mark has ended, then calls `tts_begin` again with the same arguments, so the play
goes exactly as if the fetch had ended just before it: a stored clip plays from the cache
(`cached: true`, no request); after a 402 the client's status says `needs_credit`, so the OS voice
speaks with no request; after a clip that failed §6.7 (counted in `ai_attempts`) its own request
carries attempt n + 1's idempotency key; after any other failure it makes its own request. Only the
same key waits: a slow play has its own speed, so its own key. `tts_play_with` loops the same way.

`prefetch_begin(session_id, exercise_id, ai)`: the run of `session_id` must be in progress
(`not_found` "This lesson is no longer in progress." otherwise); the exercise after `exercise_id`
in the run's order, if any, has a sentence to prefetch when it is `listen_type` (its `text`), or,
with `audio.autoplay` on, `pick_meaning` or `translate_to_en` (their `prompt_target`). When that
exercise exists, has a sentence, the source for the run's language is natural (as in `tts_begin`,
normal speed) and its file is neither cached nor being fetched: `Pending(job)`; otherwise
`Done(Prefetched { prefetched: false })`. `prefetch_complete` stores and records a valid reply
exactly as `tts_complete` does but plays nothing: `Prefetched { prefetched: true }`; a failure
stores nothing and is not shown: `Prefetched { prefetched: false }`.

`session_skip(kind)`: `listen` turns `listen_type` off until now + 60 minutes (injected clock, kept
in memory); `speak` records the same for module 10 and changes nothing yet; anything else is the
unknown-value error. `listen_available` is false while now < that time.

The composer: `plan_for` (every lesson a skill composes) sets `Capabilities.audio_out =
listen_available(lang)`, so a lesson gets up to 2 `listen_type` entries only when a voice exists
and listening is not skipped (module 03's rule turns the first 2 `translate_to_en` entries).

`cache_clear("audio")` removes every file `Fs::list("cache/audio")` names and the `audio_cache` rows,
and returns their count of files; `cache_clear("all")` does that and removes `cache/ai.sqlite3`;
`"ai"` is unchanged. Progress (`parley.sqlite3`) is never touched.

### Result types (`api.rs`, mirrored in `commands.ts`)

```rust
pub struct Voice { pub id: String, pub name: String, pub source: String, pub installed: bool }
pub struct TtsPlayed { pub source: VoiceSource, pub cached: bool, pub notice: Option<String>,
                       pub ai_error: Option<AppError> }
pub struct Prefetched { pub prefetched: bool }
pub struct AudioStatus { pub cost_line: String, pub listen_off_until: Option<i64> }
pub const AUDIO_ENDED_EVENT: &str = "audio://ended";
```

`voices_list(lang)`: the installed OS voices of `lang` (`voices_for`), each `{id, name, source:
"os", installed: true}`; then, only when `audio.voice_source = natural`, the natural voice
`natural_voice` gives, `{id: voice, name: voice, source: "natural", installed: true}`.

### Commands and events

| Command | Arguments | Result |
|---|---|---|
| `voices_list` | `lang` | `[Voice]` |
| `tts_play` | `text, lang, slow, session_id?` | `TtsPlayed` (+ `audio://ended` when the clip or OS voice ends) |
| `tts_stop` | | `()` |
| `tts_prefetch` | `session_id, exercise_id` | `Prefetched` |
| `audio_status` | | `AudioStatus` |
| `session_skip` | `kind: listen/speak` | `()` |

`tts_play` and `tts_prefetch` ask the AI service for a client only when `audio.voice_source =
natural` and `ai.provider` is `publik` or `own_key`; the call runs with no lock held between begin
and complete, as module 08's judge does, and the status is emitted (`ai://status`) after a call.
`tts_play` meets a `Waiting` begin by waiting with no lock held, then asking for the client and
beginning again.
`COMMANDS` gains the six names after `explain_answer`, in the order of this table.

### parley-audio

```rust
pub struct SystemVoices;   // impl OsVoices. tts on Windows (WinRT), macOS and Android, on one
                           // thread that owns the Tts; spd-say on Linux (absent: no voices)
impl SystemVoices { pub fn new(on_end: Arc<dyn Fn() + Send + Sync>) -> SystemVoices; }
pub struct RodioPlayer;    // impl Player: a thread owning the output stream, opened at the first play
impl RodioPlayer { pub fn new(on_end: Arc<dyn Fn() + Send + Sync>) -> RodioPlayer; }
pub struct MediaDecoder;   // impl AudioDecoder: MP3 and WAV through rodio's decoder
pub fn decode_mono_i16(audio: &[u8]) -> Option<(Vec<i16>, u32)>;   // samples and rate, channels mixed
pub fn wsola(samples: &[i16], rate_hz: u32, factor: f32) -> Vec<i16>;
```

WSOLA after Verhelst and Roelands (1993): frames of 20 ms, hop of 10 ms (50% overlap) in the output,
a search of ±10 ms around the nominal input position for the best cross-correlation with the
natural continuation of the previous frame, Hann cross-fade; output length = input × `factor`.
Linux rates for `spd-say`: the trait's rate is a multiplier (0.5, 1.0, 2.0); `spd-say -r` gets
round(100 × log2(rate)), clamped to −100..100.

### UI

Strings in `app/src/strings/en.json`. The `commands.ts` wrappers send snake_case arguments, as the
earlier ones do: `voices_list {lang}`, `tts_play {text, lang, slow, session_id}` (`session_id` null
outside a lesson), `tts_stop {}`, `tts_prefetch {session_id, exercise_id}`, `audio_status {}`,
`session_skip {kind}`. The UI reads `audio.autoplay`, `audio.voice_source` and `audio.slow_speed`
from `settings_get` and writes them with `settings_set`.

- `PlayButton`: **Play** and **Slow** (`aria-label` "Play the sentence" and "Play it slowly");
  calls `tts_play`; while playing shows **Stop** (`tts_stop`) until `audio://ended`; shows the
  notice under it, `ai_error` as `AiBanner` shows it, and a `no_voice` error's message.
- `ListenType`: the two buttons, the answer box, and **Can't listen now**, which calls
  `session_skip("listen")` and skips this and every later `listen_type` exercise of the lesson
  with no grade.
- The lesson screen calls `tts_prefetch` for each exercise it shows (errors ignored); with
  `audio.autoplay` on, it calls `tts_play` once for an exercise's target-language prompt
  (`pick_meaning` and `translate_to_en` `prompt_target`, `listen_type` `text`) when the exercise is
  shown, and once for the correct answer on the result sheet when the answer is in the target
  language (`translate_to_target` `target`, `fill_gap` `full_sentence`, `listen_type` `text`).
- Settings > Audio (`AudioSection`): **System voice** or **Natural voice** (turning on natural voices
  with no provider chosen or no consent opens the AI setup dialog, I12, and **Not now** leaves the
  system voice); per language, the OS voice picker from `voices_list` (or "No voice installed" and
  the `no_voice` help); **Slow speed** (0.5 to 1.0, step 0.05); **Play prompts aloud**
  (autoplay); **Clear audio cache** with "Removed {n} files."; and the cost line from
  `audio_status`.

## Messages

"No Spanish voice is installed. On Windows, open Settings > Time & language > Speech and add a
voice." (Windows); "No Spanish voice is installed. On macOS, open System Settings > Accessibility >
Spoken Content > System voice > Manage Voices." (macOS); "No Spanish voice is installed. On Linux,
install Speech Dispatcher and a Spanish voice, or turn on natural voices." (Linux), with French and
German for the other courses; "Natural voice unavailable, using the system voice."; "Natural voices
cost about {$x} a sentence (measured)."; "There is no text to read aloud."; "This text is too long to
read aloud."; "Parley could not play the sound. Check that a speaker or headphones are connected.";
"This lesson is no longer in progress."; "Removed {n} files."

## Out of scope

- Recording and speaking (10); reading role-play replies aloud (11 uses this module's `tts_play`).
- A choice among several natural voices per language: `voices.toml` names one per language, and
  `audio.natural_voice.<lang>` can override it by hand.

## Design notes

- The live check of this module was made in the maintainer's setup run of 2026-11-12, recorded in
  `docs/modules/06-publik-and-providers.md` (Design notes), and is used instead of a new one:
  `GET /models` lists no audio model, so the standard ids were probed by maintainer decision;
  `/audio/speech` accepts `tts-1` with voice `alloy`, returns MP3 (32 KB for one short sentence)
  and charged 540 micros, and refuses `gpt-4o-mini-tts`. `content/voices.toml` therefore names
  `tts-1` and `alloy` for Spanish, French and German (the voice is multilingual). Whether `speed`
  is honoured was not probed: the provider's published speech API takes `speed` from 0.25 to 4.0,
  so `speed = true`; if listening shows otherwise, setting it to false makes slow mode stretch
  locally and send 1.0. The charge of 10 sentences was not measured; the single measured charge
  (540 micros) replaces the estimate in PUBLIK-INTEGRATION §5 and §8, and each install measures
  its own from `x-publik-charge-micros`.
- Linux OS voices go through Speech Dispatcher's `spd-say` command rather than the `tts` crate's
  backend, which links `libspeechd` and needs `libclang` at build time: Speech Dispatcher stays
  optional at run time (ARCHITECTURE §10), and the build needs no new system library.
- `tts` has one maintainer, so ADR-0014 names the exit (the platform speech APIs directly).
  Its `Tts` handle is reference-counted without atomics, so one thread owns it and receives
  commands over a channel.
- `rodio` is a dependency on every platform (Linux plays sound and measures clips too, through
  ALSA, which CI already installs); only `tts` is limited to targets other than Linux. `tts`'s own
  Linux dependency, `speech-dispatcher` (LGPL-2.1 OR MIT OR Apache-2.0, taken under MIT), stays in
  `Cargo.lock` and in the licence gate's graph, which reads each dependency's target condition on
  its own, but is never built: no target the app is built for has both.
- One sound at a time: the OS voice is stopped before a clip plays, and the clip before the OS
  voice speaks. A natural voice that falls back to an OS voice when none is installed for the
  language gives the `no_voice` error with the platform's help (the notice is then not shown). An
  OS voice that fails to speak gives the same message as a player that fails ("Parley could not
  play the sound. …").
- A natural sentence is written as `cache/audio/<key>.mp3.part` and renamed into place, so a file
  under its own name is whole (only if the rename fails is the file written under its name
  directly); the cache is found by reading that file. A clip that fails §6.7 counts as a failed
  attempt, so asking for the sentence again sends a new idempotency key: without it, publik API
  would replay the unusable clip, and its charge would be counted again. `speech_charges`
  keeps one row per key (a sentence charged again keeps its latest charge), so the measured mean
  is per sentence. `audio_status` and `cache_clear("audio")` open `cache/ai.sqlite3` only when the
  file exists, so looking never creates it; clearing everything also forgets the measured charges,
  and the cost line falls back to the setup run's 540 micros. A charge is added to the session's
  cost while that session is a lesson, review or story in progress. **Delete everything** removes
  the cached sentences too.
- `voices.toml`: `model` is required, `speed` is optional and false when left out (slow mode then
  stretches locally, which always works), and any other key is refused, so a typo is caught when
  the file is edited. `speech_bounds_ms` rounds the upper bound to the nearest ms and treats a
  speed that is not a positive number as 1.0; a text with no words allows no clip at all.
- Prefetch: an exercise id that is not in the run, the last exercise, a sentence that is empty or
  over 500 characters, a `listen_type` while **Can't listen now** is on, and any client that is
  missing or failed give `prefetched: false` with no request. `session_skip` keeps its times in
  memory, so a restart brings listening back.
- One request and one charge per sentence, however the prefetch and the play overlap. A learner who
  answers quickly reaches the next exercise, whose autoplay asks for its sentence, before that
  sentence's prefetch has come back, and a double click on **Play** asks twice; asking the provider
  each time would buy, charge (in `speech_charges` and the lesson's cost) and store the same clip
  twice. So a sentence already being fetched is waited for, never asked for again: the play starts
  when the fetch ends, from the stored clip. If the fetch fails, the play decides afresh as it
  would have alone: the system voice after a 402, its own request with a new idempotency key after
  an unusable clip, its own request after any other error. Nothing is charged twice for one usable
  clip. What is being fetched is kept in memory only, and a fetch ends with its call even when the
  call never completes, so a play never waits for a reply that will not come. A slow play is
  another sentence to fetch (its own speed and cache key), so it never waits for a normal one.
- `WavDecoder` reads `WAVE_FORMAT_PCM` and `WAVE_FORMAT_EXTENSIBLE` with a PCM sub-format, skips
  other chunks (padding included), and counts no more data than is present. `MediaDecoder` decodes
  the whole clip and adds up its samples span by span (a stream may change rate or channels), so a
  clip's length is what plays, not what a header says; bytes that decode to nothing are refused.
  `decode_mono_i16` multiplies Symphonia's samples by 2^15 (its own divisor, so 16-bit PCM comes
  back exactly) and averages each frame's channels in integers, rounding toward zero.
- WSOLA: an even frame of 20 ms and a hop of half of it; the Hann window is sampled half a sample
  off its ends, w(i) = 0.5 − 0.5 cos(2π (i + ½) ÷ N), so it is never zero and two windows half a
  frame apart add up to exactly 1, and the output is divided by the summed window, which also
  keeps the first and last half-frames at full level. The search uses normalised cross-correlation
  (the dot product divided by the square root of the candidate's energy), tries offsets nearest
  first so equally good
  frames keep to the nominal place, and keeps its full ±10 ms width inside the input, so every
  frame is real input and a frame in phase can always be found. A factor of 1 returns the input,
  and an input shorter than one frame is only cut or padded with silence.
- `RodioPlayer` opens the default output at the first play and keeps it; each play makes a new
  rodio player and drops the previous one. The end of a clip is a callback queued after it, which
  announces `audio://ended` only while no later play or stop has come. `SystemVoices` on Windows,
  macOS and Android finds the voice by id in the engine's list for each sentence, clamps the rate
  to the engine's range, interrupts what is being said, and announces the end through the
  engine's end-of-utterance callback. On Linux, `spd-say -L` is read once per run (5 s at most:
  it may have to start Speech Dispatcher); a sentence is `spd-say --cancel --wait -r <rate> -l
  <language> -y <voice> -- <text>`, and its end is announced when that command exits normally and
  no later play or stop has come; **Stop** runs `spd-say --cancel`.
- Interface: while a sentence plays, **Stop** takes the place of **Play** and **Slow** until
  `audio://ended` (or **Stop**). Under the buttons: the notice, a natural voice's AI error as the AI
  banner shows it, and the message of a `no_voice` error; any other failure (a missing command, a
  lesson that ended) leaves the buttons as they were, so a broken speaker never blocks a lesson.
  Autoplay plays each sentence once per exercise shown, the first time the setting is known to
  be on. A `listen_type` exercise no longer shows its sentence: it is heard, and the result sheet
  gives the answer. Settings > Audio is its own tab after Learning; it lists a voice picker for
  each of the three course languages, and the help for a missing voice follows the platform the
  window reports. The slow speed is saved in whole steps of 0.05.

## Acceptance tests

Code tests in `crates/parley-audio/tests/m09_listening_and_voices.rs` and
`crates/parley-core/tests/m09_listening_and_voices.rs`, with fake `OsVoices` and `Player`, the
`WavDecoder`, and a fake `AiClient` that replies with the P5 fixtures: generated 16 kHz mono 16-bit
WAV bytes of the fixture's duration, served as `audio/wav`. UI tests in `app/src/test/m09/`.
Contract tests 1 to 18 keep passing; this module adds test 28 of `docs/PUBLIK-INTEGRATION.md` §11
(tests 4 to 6 here).

1. A fake backend with voices `es-ES`, `es-MX` and `fr-FR` → `voices_list("es")` returns two voices.
2. OS path, slow: `speak` is called with the backend's normal rate × 0.75, clamped to its range
   (normal 1.0 → 0.75; a range with min 0.8 → 0.8); normal speed → the normal rate.
3. No voice for German → `no_voice` with the platform's message (all three messages checked through
   `no_voice_message`), and the composer produces no `listen_type` for German.
4. Natural path: the request body equals §6.7 for `Quiero un café con leche.` at speed 0.75 with
   `tts-1` and `alloy` from `voices.toml`; the audio plays (the player gets the served bytes); a
   second play makes no request.
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
17. Slow mode when `speed_honoured` is false: the request sends speed 1.0 and the player gets
    stretch 1 ÷ 0.75; normal and slow plays share one cached file.
18. Errors: empty text and a 501-character text give their `invalid_input` messages; a player that
    fails gives the speaker message; `tts_prefetch` for a finished lesson gives `not_found`;
    `session_skip("sing")` gives the unknown-value error.
19. Charges reach the lesson: a natural play with a lesson's `session_id` adds its charge to that
    session's cost (contract test 34), and a charged reply that fails §6.7 adds it too.
20. parley-audio: `MediaDecoder` measures a generated 1.5 s WAV as 1500 ms ± 1 ms and refuses bytes
    that are not audio; `decode_mono_i16` mixes a stereo WAV to mono.
21. A sentence being fetched is asked for once: a play of it while its prefetch is in flight waits,
    then plays the stored clip (one request, one charge in `speech_charges` and the lesson's cost,
    one file); a second prefetch gives `prefetched: false` with no request; a second play (a double
    click) waits, then plays the stored clip; a job dropped unfinished wakes the waiting play, which
    then makes its own request; after an unusable prefetch the play's request carries attempt 2's
    idempotency key, and after a 402 the play uses the OS voice with no request; on two threads,
    the play returns only after the prefetch's call does, with one request in all; a slow play of
    a sentence being fetched at normal speed does not wait.

## Status

Done (2026-11-12)
