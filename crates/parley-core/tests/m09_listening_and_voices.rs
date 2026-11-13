//! Acceptance tests of module 09 in the core (docs/modules/09-listening-and-voices.md, Acceptance
//! tests 1 to 10, 12 to 15 and 17 to 19; tests 11 and 20 are in crates/parley-audio and test 16 in
//! app/src/test/m09): OS voices per language and their rates, the no-voice messages, the natural
//! voice (P5) with its request, cache key, checks, fallbacks and charges, slow mode, the prefetch of
//! the next sentence, Can't listen now and the composer's listen_type switch, clearing the audio
//! cache, and the errors. Test 28 of docs/PUBLIK-INTEGRATION.md §11 is tests 4 to 6 here; contract
//! test 34 is test 19.
//!
//! References: the spec (its Interfaces, Messages and the figures worked out in its acceptance
//! tests), docs/PUBLIK-INTEGRATION.md §6.1, §6.7 and §9, module 03's grading rules, module 06's
//! format_dollars, and fixtures true by construction: the replies of tests/fixtures/ai/P5/ (its
//! README gives each clip's type, length and charge), turned here into 16 kHz mono 16-bit WAV bytes
//! of the stated length, so the WavDecoder measures exactly that; the course in
//! tests/fixtures/course07/es/; the German course of tests/fixtures/seed-course/de/; and the forms
//! table of content/es/ for test 9. No test reaches the network: speech comes from a scripted client
//! that records every request, and the OS voices and the player are fakes that record what they
//! are asked to do.
//!
//! Where the spec leaves a detail open, these tests assume that `Fs::list` returns the bare file
//! names of the directory it is given (`<key>.mp3`), as the fake filesystem here does; that the
//! unknown-value error of `session_skip` names its argument (`Unknown value sing for kind.`, the
//! module 04 form); and that a lesson composed with listening on keeps the composer's two
//! `listen_type` exercises (their template passes the same checks as `translate_to_en`).

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use parley_core::ai::{
    AiClient, AiError, AiStatus, AudioReply, Cache, ChatReply, ChatRequest, MEASURED_SPEECH_MICROS,
    NATURAL_UNAVAILABLE, ProviderKind, SPEECH_FEATURE, SPEECH_TIMEOUT_S, SpeechRequest,
    TranscribeRequest, TranscriptReply, VoiceSource, VoicesConfig, audio_file, bundled_voices,
    cache_key, cache_key_json, choose_source, format_dollars, idempotency_key, natural_voice,
    parse_voices_toml, speech_bounds_ms, speech_cost_line, speech_key, speech_key_json,
    speech_request, speech_valid,
};
use parley_core::api::{AUDIO_ENDED_EVENT, COMMANDS, TtsPlayed};
use parley_core::app::{AiAccess, Config, CourseFiles, Env, Parley};
use parley_core::audio::{
    Audio, AudioDecoder, AudioError, OsVoice, OsVoices, Platform, Player, RateRange, WavDecoder,
    no_voice_message, os_rate, voices_for,
};
use parley_core::content::{Forms, Lang};
use parley_core::env::{Clock, FailingNet, Fs, FsError, SplitMix64};
use parley_core::error::{AppError, ErrorCode};
use parley_core::exercise::{Body, Exercise, ExerciseType, Origin};
use parley_core::grading::{AccentLeniency, Answer, GradeCtx, Tag, Verdict, grade};
use parley_core::settings::{Settings, apply_patch};
use parley_core::srs::Facet;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------------------------
// The texts and figures of the spec.
// ---------------------------------------------------------------------------------------------

/// The §6.7 example sentence: five UAX #29 words.
const COFFEE: &str = "Quiero un café con leche.";
/// The sentence of acceptance test 9: two words.
const MORNING: &str = "Buenos días.";
/// Six UAX #29 words (Quiero, un, café, con, leche, gracias): at most 3 x 500 x 6 / 1.0 = 9,000 ms.
const SIX_WORDS: &str = "Quiero un café con leche, gracias.";
/// The speech model and voice of content/voices.toml (the spec's Design notes).
const MODEL: &str = "tts-1";
const VOICE: &str = "alloy";
const PUBLIK: ProviderKind = ProviderKind::Publik;

/// The skill of tests/fixtures/course07/es/: ten nouns and prepositions, then two example sentences.
const SKILL: &str = "es.a1.u03.s01";
/// The first skill of tests/fixtures/seed-course/de/: eleven lemmas, then nine example sentences.
const GERMAN_SKILL: &str = "de.a1.u01.s01";

/// The 402 of tests/fixtures/ai/P5/402.json.
const BALANCE: &str = "Your publik balance is $0.00. Add a plan or pack to keep using AI features.";
const TOP_UP: &str = "https://publikhq.com/wallet/add-credit?from=402";
const SPEAKER: &str =
    "Parley could not play the sound. Check that a speaker or headphones are connected.";
const NO_TEXT: &str = "There is no text to read aloud.";
const TOO_LONG: &str = "This text is too long to read aloud.";
const NOT_IN_PROGRESS: &str = "This lesson is no longer in progress.";

/// The fake OS voices.
const ES_ES: &str = "os-es-ES";
const ES_MX: &str = "os-es-MX";
const FR_FR: &str = "os-fr-FR";
const DE_DE: &str = "os-de-DE";

/// The speaking rates of WinRT (the spec's example): 0.5, 1.0, 6.0.
const WINRT: RateRange = RateRange {
    min: 0.5,
    normal: 1.0,
    max: 6.0,
};

/// 10:00 local time on 2026-11-12 at UTC+1, which is 09:00Z: 20,769 days after 1970-01-01.
const TEN: i64 = 20_769 * 86_400_000 + 9 * 3_600_000;
/// 11:00 local time the same day: sixty minutes later.
const ELEVEN: i64 = TEN + 60 * 60_000;
const OFFSET_MINUTES: i32 = 60;

// ---------------------------------------------------------------------------------------------
// Files of the repository.
// ---------------------------------------------------------------------------------------------

/// The repository root: the first ancestor of this crate that holds the workspace Cargo.toml.
fn repo_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if let Ok(text) = fs::read_to_string(dir.join("Cargo.toml"))
            && text.contains("[workspace]")
        {
            return dir;
        }
        assert!(dir.pop(), "no workspace Cargo.toml above the crate");
    }
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()))
}

fn json_file(relative: &str) -> Value {
    serde_json::from_str(&read(relative)).unwrap_or_else(|e| panic!("{relative}: {e}"))
}

// ---------------------------------------------------------------------------------------------
// The P5 fixtures.
// ---------------------------------------------------------------------------------------------

/// RIFF WAVE, 16 kHz mono 16-bit PCM: the 44-byte header, then 32 bytes for each millisecond of an
/// integer square wave (period 40 samples, levels +8000 and -8000). Data bytes / byte rate is `ms`.
fn wav(ms: u64) -> Vec<u8> {
    let samples = 16 * ms as usize;
    let data_len = u32::try_from(samples * 2).expect("a short clip");
    let mut out = Vec::with_capacity(44 + samples * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    // PCM, one channel, 16,000 samples a second, 32,000 bytes a second, 2 bytes a frame, 16 bits.
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&16_000u32.to_le_bytes());
    out.extend_from_slice(&32_000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..samples {
        let level: i16 = if i % 40 < 20 { 8_000 } else { -8_000 };
        out.extend_from_slice(&level.to_le_bytes());
    }
    out
}

/// What the client's `speech` returns for a fixture (format in tests/fixtures/ai/P5/README.md).
fn p5(name: &str) -> Result<AudioReply, AiError> {
    let fixture = json_file(&format!("tests/fixtures/ai/P5/{name}"));
    let text = |field: &str| -> String {
        fixture[field]
            .as_str()
            .unwrap_or_else(|| panic!("{name}: {field} is text"))
            .to_string()
    };
    if let Some(error) = fixture.get("error") {
        return Err(match error.as_str() {
            Some("needs_credit") => AiError::NeedsCredit {
                message: text("message"),
                top_up_url: text("top_up_url"),
            },
            Some("offline") => AiError::Offline,
            Some("timeout") => AiError::Timeout { provider: PUBLIK },
            Some("provider") => AiError::Provider {
                provider: PUBLIK,
                code: fixture["status"]
                    .as_u64()
                    .and_then(|code| u16::try_from(code).ok())
                    .unwrap_or_else(|| panic!("{name}: a status code")),
                message: text("message"),
            },
            _ => panic!("{name}: unknown error {error}"),
        });
    }
    assert_eq!(fixture["status"].as_u64(), Some(200), "{name}: a reply");
    let bytes = match (fixture["duration_ms"].as_u64(), fixture["body"].as_str()) {
        (Some(ms), None) => wav(ms),
        (None, Some(body)) => body.as_bytes().to_vec(),
        _ => panic!("{name}: either duration_ms or body"),
    };
    Ok(AudioReply {
        bytes,
        content_type: text("content_type"),
        charge_micros: fixture["charge_micros"].as_i64(),
    })
}

/// A fixture that is a reply.
fn reply(name: &str) -> AudioReply {
    p5(name).unwrap_or_else(|e| panic!("{name} is a reply: {e}"))
}

// ---------------------------------------------------------------------------------------------
// Fakes: the AI client, the OS voices, the player, the clock and the filesystem.
// ---------------------------------------------------------------------------------------------

/// An `AiClient` whose `speech` answers from P5 fixtures in turn (offline when none is left) and
/// records every request. Like the publik API client, a 402 sets `needs_credit` in its status and
/// a reply clears it (PUBLIK-INTEGRATION §9).
struct SpeechClient {
    needs_credit: AtomicBool,
    replies: Mutex<VecDeque<Result<AudioReply, AiError>>>,
    sent: Mutex<Vec<SpeechRequest>>,
}

impl SpeechClient {
    fn replying(fixtures: &[&str]) -> Arc<SpeechClient> {
        Arc::new(SpeechClient {
            needs_credit: AtomicBool::new(false),
            replies: Mutex::new(fixtures.iter().map(|fixture| p5(fixture)).collect()),
            sent: Mutex::new(Vec::new()),
        })
    }

    /// One more reply, after those already waiting.
    fn push(&self, fixture: &str) {
        self.replies.lock().unwrap().push_back(p5(fixture));
    }

    fn sent(&self) -> Vec<SpeechRequest> {
        self.sent.lock().unwrap().clone()
    }

    fn requests(&self) -> usize {
        self.sent.lock().unwrap().len()
    }

    fn set_needs_credit(&self, on: bool) {
        self.needs_credit.store(on, Ordering::SeqCst);
    }
}

impl AiClient for SpeechClient {
    fn chat(&self, _req: ChatRequest) -> Result<ChatReply, AiError> {
        Err(AiError::Offline)
    }

    fn chat_stream(
        &self,
        _req: ChatRequest,
        _on_delta: &mut dyn FnMut(&str),
    ) -> Result<ChatReply, AiError> {
        Err(AiError::Offline)
    }

    fn transcribe(&self, _req: TranscribeRequest) -> Result<TranscriptReply, AiError> {
        Err(AiError::Offline)
    }

    fn speech(&self, req: SpeechRequest) -> Result<AudioReply, AiError> {
        self.sent.lock().unwrap().push(req);
        let next = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(AiError::Offline));
        match &next {
            Ok(_) => self.set_needs_credit(false),
            Err(AiError::NeedsCredit { .. }) => self.set_needs_credit(true),
            Err(_) => {}
        }
        next
    }

    fn kind(&self) -> ProviderKind {
        PUBLIK
    }

    fn status(&self) -> AiStatus {
        AiStatus {
            provider: PUBLIK.as_str().to_string(),
            needs_credit: self.needs_credit.load(Ordering::SeqCst),
            ..AiStatus::default()
        }
    }

    fn chat_model(&self, tier: &str) -> String {
        tier.to_string()
    }
}

fn access(client: &Arc<SpeechClient>) -> Option<AiAccess> {
    let client: Arc<dyn AiClient> = client.clone();
    Some(Ok(client))
}

/// An OS voice backend with a fixed list and rate range that records every speak.
struct FakeVoices {
    voices: Vec<OsVoice>,
    range: RateRange,
    spoken: Mutex<Vec<(String, String, f32)>>,
}

impl FakeVoices {
    fn shared(voices: Vec<OsVoice>, range: RateRange) -> Arc<FakeVoices> {
        Arc::new(FakeVoices {
            voices,
            range,
            spoken: Mutex::new(Vec::new()),
        })
    }

    /// (text, voice id, rate) of every speak, in order.
    fn spoken(&self) -> Vec<(String, String, f32)> {
        self.spoken.lock().unwrap().clone()
    }
}

impl OsVoices for FakeVoices {
    fn list(&self) -> Vec<OsVoice> {
        self.voices.clone()
    }

    fn rates(&self) -> RateRange {
        self.range
    }

    fn speak(&self, text: &str, voice: &str, rate: f32) -> Result<(), AudioError> {
        self.spoken
            .lock()
            .unwrap()
            .push((text.to_string(), voice.to_string(), rate));
        Ok(())
    }

    fn stop(&self) {}
}

/// A player that records (bytes, stretch) of every play, or fails every play.
struct FakePlayer {
    fails: bool,
    played: Mutex<Vec<(Vec<u8>, f32)>>,
}

impl FakePlayer {
    fn shared(fails: bool) -> Arc<FakePlayer> {
        Arc::new(FakePlayer {
            fails,
            played: Mutex::new(Vec::new()),
        })
    }

    fn played(&self) -> Vec<(Vec<u8>, f32)> {
        self.played.lock().unwrap().clone()
    }
}

impl Player for FakePlayer {
    fn play(&self, audio: &[u8], stretch: f32) -> Result<(), AudioError> {
        if self.fails {
            return Err(AudioError::NoDevice);
        }
        self.played.lock().unwrap().push((audio.to_vec(), stretch));
        Ok(())
    }

    fn stop(&self) {}
}

fn os_voice(id: &str, name: &str, lang_tag: &str) -> OsVoice {
    OsVoice {
        id: id.to_string(),
        name: name.to_string(),
        lang_tag: lang_tag.to_string(),
    }
}

/// The backend of acceptance test 1: voices es-ES, es-MX and fr-FR.
fn spanish_and_french() -> Vec<OsVoice> {
    vec![
        os_voice(ES_ES, "Spanish (Spain)", "es-ES"),
        os_voice(ES_MX, "Spanish (Mexico)", "es-MX"),
        os_voice(FR_FR, "French (France)", "fr-FR"),
    ]
}

/// A clock the test moves, at UTC+1.
struct SharedClock(Arc<AtomicI64>);

impl Clock for SharedClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
    fn utc_offset_minutes(&self) -> i32 {
        OFFSET_MINUTES
    }
}

/// Files under a directory; absolute paths as they are.
struct DirFs(PathBuf);

fn fs_error(path: &Path, e: std::io::Error) -> FsError {
    if e.kind() == std::io::ErrorKind::NotFound {
        FsError::NotFound(path.to_path_buf())
    } else {
        FsError::Io(e.to_string())
    }
}

impl DirFs {
    fn full(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.0.join(path)
        }
    }
}

impl Fs for DirFs {
    fn read(&self, path: &Path) -> Result<Vec<u8>, FsError> {
        fs::read(self.full(path)).map_err(|e| fs_error(path, e))
    }
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), FsError> {
        let full = self.full(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(|e| fs_error(path, e))?;
        }
        fs::write(full, bytes).map_err(|e| fs_error(path, e))
    }
    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        fs::rename(self.full(from), self.full(to)).map_err(|e| fs_error(from, e))
    }
    fn remove(&self, path: &Path) -> Result<(), FsError> {
        fs::remove_file(self.full(path)).map_err(|e| fs_error(path, e))
    }
    /// The names of the files in `dir`, sorted; a missing directory has none.
    fn list(&self, dir: &Path) -> Result<Vec<String>, FsError> {
        let entries = match fs::read_dir(self.full(dir)) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(fs_error(dir, e)),
        };
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| fs_error(dir, e))?;
            if entry.path().is_file() {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        names.sort();
        Ok(names)
    }
}

// ---------------------------------------------------------------------------------------------
// The app.
// ---------------------------------------------------------------------------------------------

/// The service over `dir` with the test courses (Spanish from course07, German from the seed
/// course), the Spanish course started, and the default audio of `Parley::new`.
fn bare_app(dir: &Path, now: Arc<AtomicI64>) -> Parley {
    let mut courses = BTreeMap::new();
    courses.insert(
        Lang::Es,
        CourseFiles {
            syllabus_yaml: read("tests/fixtures/course07/es/syllabus.yaml"),
            forms_tsv: read("tests/fixtures/course07/es/forms.tsv"),
            numbers_yaml: None,
        },
    );
    courses.insert(
        Lang::De,
        CourseFiles {
            syllabus_yaml: read("tests/fixtures/seed-course/de/syllabus.yaml"),
            forms_tsv: read("tests/fixtures/seed-course/de/forms.tsv"),
            numbers_yaml: Some(read("tests/fixtures/seed-course/de/numbers.yaml")),
        },
    );
    let mut app = Parley::new(
        Config {
            data_dir: dir.to_path_buf(),
            courses,
            packs_lock: "schema = \"parley.packs/1\"\n".to_string(),
        },
        Env {
            clock: Box::new(SharedClock(now)),
            rng: Box::new(SplitMix64::new(9)),
            fs: Box::new(DirFs(dir.to_path_buf())),
            net: Box::new(FailingNet),
        },
    );
    app.course_start("es")
        .expect("the Spanish test course starts");
    app
}

fn audio_of(voices: &Arc<FakeVoices>, player: &Arc<FakePlayer>, natural: VoicesConfig) -> Audio {
    let os: Arc<dyn OsVoices> = voices.clone();
    let player: Arc<dyn Player> = player.clone();
    Audio {
        os,
        player,
        decoder: Arc::new(WavDecoder),
        natural,
    }
}

/// The app with fake voices and player, in a temporary data directory, at 10:00.
struct Setup {
    app: Parley,
    dir: PathBuf,
    now: Arc<AtomicI64>,
    voices: Arc<FakeVoices>,
    player: Arc<FakePlayer>,
    _temp: tempfile::TempDir,
}

impl Setup {
    /// Voices es-ES, es-MX and fr-FR at WinRT's rates, a working player, the bundled voices.toml.
    fn new() -> Setup {
        Setup::with(spanish_and_french(), WINRT, false, bundled_voices())
    }

    fn with(
        voices: Vec<OsVoice>,
        range: RateRange,
        player_fails: bool,
        natural: VoicesConfig,
    ) -> Setup {
        let temp = tempfile::tempdir().expect("a temporary directory");
        let dir = temp.path().to_path_buf();
        let now = Arc::new(AtomicI64::new(TEN));
        let mut app = bare_app(&dir, now.clone());
        let voices = FakeVoices::shared(voices, range);
        let player = FakePlayer::shared(player_fails);
        app.set_audio(audio_of(&voices, &player, natural));
        Setup {
            app,
            dir,
            now,
            voices,
            player,
            _temp: temp,
        }
    }

    /// Natural voices on, through publik API.
    fn natural(mut self) -> Setup {
        set(
            &mut self.app,
            json!({"ai": {"provider": "publik"}, "audio": {"voice_source": "natural"}}),
        );
        self
    }

    /// Other OS voices; the player and the bundled voices.toml stay.
    fn install(&mut self, voices: Vec<OsVoice>) {
        self.voices = FakeVoices::shared(voices, WINRT);
        self.app
            .set_audio(audio_of(&self.voices, &self.player, bundled_voices()));
    }

    fn set_now(&self, ms: i64) {
        self.now.store(ms, Ordering::SeqCst);
    }

    /// tts_play of a Spanish text with the client.
    fn play(
        &mut self,
        text: &str,
        slow: bool,
        session: Option<&str>,
        client: &Arc<SpeechClient>,
    ) -> TtsPlayed {
        self.app
            .tts_play_with(text, "es", slow, session, access(client))
            .unwrap_or_else(|e| panic!("{text} plays: {e}"))
    }

    /// tts_play of a Spanish text with no client.
    fn play_os(&mut self, text: &str, slow: bool) -> TtsPlayed {
        self.app
            .tts_play_with(text, "es", slow, None, None)
            .unwrap_or_else(|e| panic!("{text} plays: {e}"))
    }
}

fn set(app: &mut Parley, patch: Value) {
    app.settings_set(&patch)
        .unwrap_or_else(|e| panic!("{patch} is saved: {e}"));
}

/// The default settings with `patch` applied.
fn settings(patch: Value) -> Settings {
    apply_patch(&Settings::default(), &patch).unwrap_or_else(|e| panic!("{patch}: {e}"))
}

fn status(provider: &str, needs_credit: bool) -> AiStatus {
    AiStatus {
        provider: provider.to_string(),
        needs_credit,
        ..AiStatus::default()
    }
}

/// The AI cache of the data directory, which holds `audio_cache` and `speech_charges`.
fn cache(dir: &Path) -> Cache {
    Cache::open(
        &dir.join("cache").join("ai.sqlite3"),
        Box::new(SharedClock(Arc::new(AtomicI64::new(TEN)))),
    )
    .expect("the AI cache opens")
}

/// The files in cache/audio.
fn audio_files(dir: &Path) -> usize {
    match fs::read_dir(dir.join("cache").join("audio")) {
        Ok(entries) => entries
            .filter(|entry| entry.as_ref().is_ok_and(|entry| entry.path().is_file()))
            .count(),
        Err(_) => 0,
    }
}

/// Every backed-up table and the settings, from a backup of the app's data, without its time.
fn tables(app: &mut Parley, dir: &Path) -> Value {
    static EXPORTS: AtomicUsize = AtomicUsize::new(0);
    let n = EXPORTS.fetch_add(1, Ordering::SeqCst);
    let path = dir.join(format!("backup-{n}.json"));
    app.backup_export(&path).expect("the backup is written");
    let mut value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    value["exported_at"] = Value::Null;
    value
}

/// The session's `cost_micros`.
fn session_cost(tables: &Value, session: &str) -> i64 {
    tables["sessions"]
        .as_array()
        .expect("the backup has sessions")
        .iter()
        .find(|row| row["id"].as_str() == Some(session))
        .and_then(|row| row["cost_micros"].as_i64())
        .unwrap_or_else(|| panic!("session {session} has a cost"))
}

/// The right answer to an exercise.
fn answer_of(ex: &Exercise) -> Answer {
    match &ex.body {
        Body::PickMeaning { answer_index, .. } => Answer::Choice(*answer_index),
        Body::MatchPairs { pairs } => {
            Answer::Pairs((0..pairs.len() as u32).map(|i| [i, i]).collect())
        }
        Body::TranslateToTarget { target, .. } => Answer::Text(target.clone()),
        Body::TranslateToEn { accepted, .. } => Answer::Text(accepted[0].clone()),
        Body::ListenType { text, .. } | Body::Speak { text, .. } => Answer::Text(text.clone()),
        Body::FillGap { answer, .. } => Answer::Text(answer.clone()),
    }
}

/// Plays a whole lesson of `skill`, every answer right, and finishes it.
fn finish_lesson(app: &mut Parley, skill: &str) {
    let lesson = app
        .lesson_start(skill, "learn")
        .unwrap_or_else(|e| panic!("a lesson of {skill} starts: {e}"));
    for ex in &lesson.exercises {
        app.exercise_submit(
            &lesson.session_id,
            &ex.id,
            &answer_of(ex),
            Some(4_000),
            false,
        )
        .unwrap_or_else(|e| panic!("{} is graded: {e}", ex.id));
    }
    app.lesson_finish(&lesson.session_id)
        .unwrap_or_else(|e| panic!("the lesson of {skill} finishes: {e}"));
}

/// With ten new items a lesson, the first lesson of the skill introduces its ten vocabulary items,
/// so the next lesson introduces its example sentences: receptive phrases, which the composer
/// plans as `translate_to_en` and, with listening on, turns the first two of into `listen_type`.
fn practise_vocabulary(app: &mut Parley, skill: &str) {
    set(app, json!({"new_items_per_lesson": 10}));
    finish_lesson(app, skill);
}

/// The exercise types of a new lesson of `skill`.
fn next_lesson_kinds(app: &mut Parley, skill: &str) -> Vec<ExerciseType> {
    app.lesson_start(skill, "learn")
        .unwrap_or_else(|e| panic!("a lesson of {skill} starts: {e}"))
        .exercises
        .iter()
        .map(Exercise::kind)
        .collect()
}

fn count(kinds: &[ExerciseType], kind: ExerciseType) -> usize {
    kinds.iter().filter(|k| **k == kind).count()
}

/// The sentence a prefetch of `ex` reads with autoplay on: a `listen_type`'s text, a
/// `pick_meaning`'s or `translate_to_en`'s prompt; none for the other types.
fn sentence_of(ex: &Exercise) -> Option<&str> {
    match &ex.body {
        Body::ListenType { text, .. } => Some(text.as_str()),
        Body::PickMeaning { prompt_target, .. } | Body::TranslateToEn { prompt_target, .. } => {
            Some(prompt_target.as_str())
        }
        _ => None,
    }
}

/// The index of the first exercise followed by one with a sentence to prefetch.
fn before_a_sentence(exercises: &[Exercise]) -> usize {
    exercises
        .windows(2)
        .position(|pair| sentence_of(&pair[1]).is_some())
        .expect("an exercise followed by one with a sentence")
}

#[track_caller]
fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 1e-4,
        "{actual} where {expected} was expected"
    );
}

#[track_caller]
fn assert_played(played: &TtsPlayed, source: VoiceSource, cached: bool, notice: Option<&str>) {
    assert_eq!(played.source, source, "source");
    assert_eq!(played.cached, cached, "cached");
    assert_eq!(played.notice.as_deref(), notice, "notice");
}

/// The speak calls, as (text, voice id, rate).
#[track_caller]
fn assert_spoken(voices: &FakeVoices, expected: &[(&str, &str, f32)]) {
    let spoken = voices.spoken();
    assert_eq!(spoken.len(), expected.len(), "speak calls: {spoken:?}");
    for ((text, voice, rate), (want_text, want_voice, want_rate)) in spoken.iter().zip(expected) {
        assert_eq!((text.as_str(), voice.as_str()), (*want_text, *want_voice));
        close(*rate, *want_rate);
    }
}

#[track_caller]
fn assert_error(error: Option<AppError>, code: ErrorCode, message: &str) {
    let error = error.expect("the command is refused");
    assert_eq!((error.code, error.message.as_str()), (code, message));
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 1: voices per language.
// ---------------------------------------------------------------------------------------------

/// Test 1: "A fake backend with voices es-ES, es-MX and fr-FR → voices_list("es") returns two voices."
#[test]
fn t01_voices_list_returns_the_two_spanish_voices() {
    let setup = Setup::new();
    let voices = setup
        .app
        .voices_list("es")
        .unwrap_or_else(|e| panic!("the Spanish voices: {e}"));
    let shown: Vec<(&str, &str, &str, bool)> = voices
        .iter()
        .map(|v| {
            (
                v.id.as_str(),
                v.name.as_str(),
                v.source.as_str(),
                v.installed,
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            (ES_ES, "Spanish (Spain)", "os", true),
            (ES_MX, "Spanish (Mexico)", "os", true)
        ]
    );
    let german = setup
        .app
        .voices_list("de")
        .unwrap_or_else(|e| panic!("the German voices: {e}"));
    assert!(german.is_empty(), "the backend has no German voice");
}

/// 1, voices_for: "The voices whose tag's primary subtag is lang (case-insensitive; - or _), in list
/// order."
#[test]
fn t01_voices_for_matches_the_primary_subtag_in_any_case_with_either_separator() {
    let voices = [
        os_voice("a", "A", "es-ES"),
        os_voice("b", "B", "fr-FR"),
        os_voice("c", "C", "es_MX"),
        os_voice("d", "D", "ES"),
        os_voice("e", "E", "est-EE"),
        os_voice("f", "F", "de-DE"),
        os_voice("g", "G", "Es-419"),
    ];
    let ids = |lang: Lang| -> Vec<String> {
        voices_for(&voices, lang)
            .into_iter()
            .map(|voice| voice.id)
            .collect()
    };
    assert_eq!(ids(Lang::Es), ["a", "c", "d", "g"], "est is Estonian");
    assert_eq!(ids(Lang::Fr), ["b"]);
    assert_eq!(ids(Lang::De), ["f"]);
}

/// 1, voices_list: "then, only when audio.voice_source = natural, the natural voice natural_voice
/// gives, {id: voice, name: voice, source: "natural", installed: true}."
#[test]
fn t01_voices_list_adds_the_natural_voice_when_natural_voices_are_on() {
    let setup = Setup::new().natural();
    let voices = setup
        .app
        .voices_list("es")
        .unwrap_or_else(|e| panic!("the Spanish voices: {e}"));
    let shown: Vec<(&str, &str, &str, bool)> = voices
        .iter()
        .map(|v| {
            (
                v.id.as_str(),
                v.name.as_str(),
                v.source.as_str(),
                v.installed,
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            (ES_ES, "Spanish (Spain)", "os", true),
            (ES_MX, "Spanish (Mexico)", "os", true),
            (VOICE, VOICE, "natural", true)
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 2: OS rates.
// ---------------------------------------------------------------------------------------------

/// Test 2: "OS path, slow: speak is called with the backend's normal rate × 0.75, clamped to its range
/// (normal 1.0 → 0.75; a range with min 0.8 → 0.8); normal speed → the normal rate."
#[test]
fn t02_os_rate_is_the_normal_rate_times_the_slow_speed_clamped_to_the_range() {
    close(os_rate(WINRT, true, 0.75), 0.75);
    close(
        os_rate(
            RateRange {
                min: 0.8,
                normal: 1.0,
                max: 6.0,
            },
            true,
            0.75,
        ),
        0.8,
    );
    close(os_rate(WINRT, false, 0.75), 1.0);
    let fast = RateRange {
        min: 0.5,
        normal: 2.0,
        max: 4.0,
    };
    close(os_rate(fast, true, 0.75), 1.5);
    close(os_rate(fast, false, 0.75), 2.0);
    close(os_rate(WINRT, true, 0.5), 0.5);
}

/// Test 2: Through tts_play: the OS voice speaks slowly at 0.75 on WinRT's rates and at 0.8 on a
/// backend whose slowest rate is 0.8; at normal speed at the normal rate.
#[test]
fn t02_a_slow_os_play_speaks_at_the_clamped_rate() {
    let mut setup = Setup::new();
    let slow = setup.play_os(MORNING, true);
    assert_played(&slow, VoiceSource::Os, false, None);
    assert_eq!(slow.ai_error, None);
    let normal = setup.play_os(MORNING, false);
    assert_played(&normal, VoiceSource::Os, false, None);
    assert_spoken(
        &setup.voices,
        &[(MORNING, ES_ES, 0.75), (MORNING, ES_ES, 1.0)],
    );
    assert!(
        setup.player.played().is_empty(),
        "an OS voice speaks itself"
    );

    let floor = RateRange {
        min: 0.8,
        normal: 1.0,
        max: 6.0,
    };
    let mut setup = Setup::with(spanish_and_french(), floor, false, bundled_voices());
    setup.play_os(MORNING, true);
    setup.play_os(MORNING, false);
    assert_spoken(
        &setup.voices,
        &[(MORNING, ES_ES, 0.8), (MORNING, ES_ES, 1.0)],
    );
}

/// 2, the voice of the OS path: "audio.os_voice.<lang> when that id is installed, else the first of
/// voices_for(os.list(), lang)".
#[test]
fn t02_the_os_voice_is_the_chosen_one_when_installed_else_the_first() {
    let mut setup = Setup::new();
    set(
        &mut setup.app,
        json!({"audio": {"os_voice": {"es": ES_MX}}}),
    );
    setup.play_os(MORNING, false);
    set(
        &mut setup.app,
        json!({"audio": {"os_voice": {"es": "os-es-AR"}}}),
    );
    setup.play_os(MORNING, false);
    set(
        &mut setup.app,
        json!({"audio": {"os_voice": {"es": null}, "slow_speed": 0.6}}),
    );
    setup.play_os(MORNING, true);
    assert_spoken(
        &setup.voices,
        &[
            (MORNING, ES_MX, 1.0),
            (MORNING, ES_ES, 1.0),
            (MORNING, ES_ES, 0.6),
        ],
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 3: no voice.
// ---------------------------------------------------------------------------------------------

/// Test 3: "all three messages checked through no_voice_message" (Messages, with French and German for
/// the other courses).
#[test]
fn t03_the_no_voice_messages_name_the_language_and_the_platform() {
    assert_eq!(
        no_voice_message(Lang::Es, Platform::Windows),
        "No Spanish voice is installed. On Windows, open Settings > Time & language > Speech and add a voice."
    );
    assert_eq!(
        no_voice_message(Lang::Es, Platform::MacOs),
        "No Spanish voice is installed. On macOS, open System Settings > Accessibility > Spoken Content > System voice > Manage Voices."
    );
    assert_eq!(
        no_voice_message(Lang::Es, Platform::Linux),
        "No Spanish voice is installed. On Linux, install Speech Dispatcher and a Spanish voice, or turn on natural voices."
    );
    for (lang, language) in [(Lang::Fr, "French"), (Lang::De, "German")] {
        assert_eq!(
            no_voice_message(lang, Platform::Windows),
            format!(
                "No {language} voice is installed. On Windows, open Settings > Time & language > Speech and add a voice."
            )
        );
        assert_eq!(
            no_voice_message(lang, Platform::MacOs),
            format!(
                "No {language} voice is installed. On macOS, open System Settings > Accessibility > Spoken Content > System voice > Manage Voices."
            )
        );
        assert_eq!(
            no_voice_message(lang, Platform::Linux),
            format!(
                "No {language} voice is installed. On Linux, install Speech Dispatcher and a {language} voice, or turn on natural voices."
            )
        );
    }
    let here = if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Linux
    };
    assert_eq!(Platform::current(), here);
}

/// Test 3: "No voice for German → no_voice with the platform's message ..., and the composer produces no
/// listen_type for German."
#[test]
fn t03_german_without_a_voice_is_no_voice_and_its_lessons_have_no_listen_type() {
    let mut setup = Setup::new();
    setup
        .app
        .course_start("de")
        .expect("the German test course starts");
    assert_error(
        setup
            .app
            .tts_play_with("Guten Tag.", "de", false, None, None)
            .err(),
        ErrorCode::NoVoice,
        &no_voice_message(Lang::De, Platform::current()),
    );
    assert!(setup.voices.spoken().is_empty());
    assert!(!setup.app.listen_available(Lang::De));
    assert!(setup.app.listen_available(Lang::Es));

    practise_vocabulary(&mut setup.app, GERMAN_SKILL);
    let kinds = next_lesson_kinds(&mut setup.app, GERMAN_SKILL);
    assert_eq!(count(&kinds, ExerciseType::ListenType), 0, "{kinds:?}");
    assert!(
        count(&kinds, ExerciseType::TranslateToEn) >= 2,
        "the lesson has sentences that a voice would turn into listen_type: {kinds:?}"
    );

    // With a German voice installed, the same lesson has its two listen_type exercises.
    let mut voices = spanish_and_french();
    voices.push(os_voice(DE_DE, "German (Germany)", "de-DE"));
    setup.install(voices);
    assert!(setup.app.listen_available(Lang::De));
    let kinds = next_lesson_kinds(&mut setup.app, GERMAN_SKILL);
    assert_eq!(count(&kinds, ExerciseType::ListenType), 2, "{kinds:?}");
}

/// 3, the default audio: "No OS voices (the default until the app sets its own ...)", so a new app
/// has no voice and no listen_type; Audio::default() holds the bundled voices.toml.
#[test]
fn t03_the_default_audio_has_no_os_voice() {
    let audio = Audio::default();
    assert!(audio.os.list().is_empty());
    assert_eq!(audio.natural, bundled_voices());

    let temp = tempfile::tempdir().expect("a temporary directory");
    let mut app = bare_app(temp.path(), Arc::new(AtomicI64::new(TEN)));
    let voices = app
        .voices_list("es")
        .unwrap_or_else(|e| panic!("the Spanish voices: {e}"));
    assert!(voices.is_empty());
    assert!(!app.listen_available(Lang::Es));
    assert_error(
        app.tts_play_with(MORNING, "es", false, None, None).err(),
        ErrorCode::NoVoice,
        &no_voice_message(Lang::Es, Platform::current()),
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 4: the natural path (PUBLIK-INTEGRATION test 28).
// ---------------------------------------------------------------------------------------------

/// Test 4: "Natural path: the request body equals §6.7 for Quiero un café con leche. at speed 0.75 with
/// tts-1 and alloy from voices.toml; the audio plays (the player gets the served bytes); a second
/// play makes no request."
#[test]
fn t04_the_natural_voice_sends_the_section_6_7_body_plays_the_reply_and_caches_it() {
    let mut setup = Setup::new().natural();
    let client = SpeechClient::replying(&["valid.json"]);
    let played = setup.play(COFFEE, true, None, &client);
    assert_played(&played, VoiceSource::Natural, false, None);
    assert_eq!(played.ai_error, None);

    let sent = client.sent();
    assert_eq!(sent.len(), 1, "one request");
    assert_eq!(
        sent[0].body(),
        json!({
            "model": "tts-1",
            "input": "Quiero un café con leche.",
            "voice": "alloy",
            "response_format": "mp3",
            "speed": 0.75
        })
    );
    assert_eq!(SPEECH_TIMEOUT_S, 20);
    assert_eq!(sent[0].timeout_s, SPEECH_TIMEOUT_S);
    assert_eq!(
        sent[0].idempotency_key,
        Some(idempotency_key(
            &speech_key_json(PUBLIK, MODEL, COFFEE, VOICE, 0.75),
            1
        ))
    );
    assert_eq!(sent[0], speech_request(PUBLIK, MODEL, COFFEE, VOICE, 0.75));

    // The player gets the served bytes, unstretched: the provider honours speed.
    let played_audio = setup.player.played();
    assert_eq!(played_audio.len(), 1);
    assert_eq!(played_audio[0].0, wav(1_500));
    close(played_audio[0].1, 1.0);
    assert!(setup.voices.spoken().is_empty());

    // The file is cache/audio/<key>.mp3, with the served bytes.
    let key = speech_key(PUBLIK, MODEL, COFFEE, VOICE, 0.75);
    assert_eq!(audio_file(&key), format!("cache/audio/{key}.mp3"));
    assert_eq!(
        fs::read(setup.dir.join(audio_file(&key))).ok(),
        Some(wav(1_500))
    );
    assert_eq!(cache(&setup.dir).audio_len(), 1);

    // A second play makes no request and plays the same bytes.
    let again = setup.play(COFFEE, true, None, &client);
    assert_played(&again, VoiceSource::Natural, true, None);
    assert_eq!(client.requests(), 1, "a cached sentence makes no request");
    let played_audio = setup.player.played();
    assert_eq!(played_audio.len(), 2);
    assert_eq!(played_audio[1].0, wav(1_500));
}

/// 4, voices.toml: "tts-1 and alloy for Spanish, French and German", speed honoured (Design notes).
#[test]
fn t04_the_bundled_voices_toml_names_tts_1_and_alloy() {
    let natural = bundled_voices();
    assert_eq!(natural.model, MODEL);
    assert!(natural.speed_honoured);
    for lang in [Lang::Es, Lang::Fr, Lang::De] {
        assert_eq!(
            natural.voices.get(&lang).map(String::as_str),
            Some(VOICE),
            "{lang:?}"
        );
    }
    assert!(parse_voices_toml("model = = \"tts-1\"").is_err());
}

/// 4, choose_source: "Natural when audio.voice_source = natural, status is Some ..., its
/// needs_credit is false, and a natural voice is configured for lang: on publik, voices.toml has a
/// voice for it; on own_key, ai.own_key.speech_model is set; local never. Otherwise Os."
#[test]
fn t04_choose_source_needs_natural_voices_a_client_no_402_and_a_voice() {
    let natural = bundled_voices();
    let on = settings(json!({"audio": {"voice_source": "natural"}, "ai": {"provider": "publik"}}));
    let ready = status("publik", false);
    assert_eq!(
        choose_source(&on, Some(&ready), &natural, Lang::Es),
        VoiceSource::Natural
    );
    assert_eq!(
        choose_source(&on, None, &natural, Lang::Es),
        VoiceSource::Os
    );
    assert_eq!(
        choose_source(&on, Some(&status("publik", true)), &natural, Lang::Es),
        VoiceSource::Os
    );
    let off = settings(json!({"ai": {"provider": "publik"}}));
    assert_eq!(
        choose_source(&off, Some(&ready), &natural, Lang::Es),
        VoiceSource::Os
    );
    let mut no_german = natural.clone();
    no_german.voices.remove(&Lang::De);
    assert_eq!(
        choose_source(&on, Some(&ready), &no_german, Lang::De),
        VoiceSource::Os
    );
    assert_eq!(
        choose_source(&on, Some(&ready), &no_german, Lang::Es),
        VoiceSource::Natural
    );

    let own = settings(json!({
        "audio": {"voice_source": "natural"},
        "ai": {"provider": "own_key", "own_key": {"speech_model": "tts-1-hd"}}
    }));
    assert_eq!(
        choose_source(&own, Some(&status("own_key", false)), &natural, Lang::Es),
        VoiceSource::Natural
    );
    let own_without = settings(json!({
        "audio": {"voice_source": "natural"},
        "ai": {"provider": "own_key"}
    }));
    assert_eq!(
        choose_source(
            &own_without,
            Some(&status("own_key", false)),
            &natural,
            Lang::Es
        ),
        VoiceSource::Os
    );
    let local = settings(json!({
        "audio": {"voice_source": "natural"},
        "ai": {"provider": "local"}
    }));
    assert_eq!(
        choose_source(&local, Some(&status("local", false)), &natural, Lang::Es),
        VoiceSource::Os
    );
}

/// 4, natural_voice: "on publik, voices.toml's model and audio.natural_voice.<lang> or voices.toml's
/// voice; on own_key, ai.own_key.speech_model and audio.natural_voice.<lang>,
/// ai.own_key.speech_voice or voices.toml's voice, in that order."
#[test]
fn t04_natural_voice_takes_the_model_and_voice_in_the_documented_order() {
    let natural = bundled_voices();
    let pair = |model: &str, voice: &str| Some((model.to_string(), voice.to_string()));

    let publik = settings(json!({"ai": {"provider": "publik"}}));
    assert_eq!(
        natural_voice(&publik, &natural, Lang::Es),
        pair(MODEL, VOICE)
    );
    let chosen = settings(json!({
        "ai": {"provider": "publik"},
        "audio": {"natural_voice": {"es": "nova"}}
    }));
    assert_eq!(
        natural_voice(&chosen, &natural, Lang::Es),
        pair(MODEL, "nova")
    );
    assert_eq!(
        natural_voice(&chosen, &natural, Lang::Fr),
        pair(MODEL, VOICE)
    );

    let own = settings(json!({
        "ai": {"provider": "own_key", "own_key": {"speech_model": "tts-1-hd", "speech_voice": "echo"}}
    }));
    assert_eq!(
        natural_voice(&own, &natural, Lang::Es),
        pair("tts-1-hd", "echo")
    );
    let own_chosen = settings(json!({
        "ai": {"provider": "own_key", "own_key": {"speech_model": "tts-1-hd", "speech_voice": "echo"}},
        "audio": {"natural_voice": {"es": "fable"}}
    }));
    assert_eq!(
        natural_voice(&own_chosen, &natural, Lang::Es),
        pair("tts-1-hd", "fable")
    );
    let own_model_only = settings(json!({
        "ai": {"provider": "own_key", "own_key": {"speech_model": "tts-1-hd"}}
    }));
    assert_eq!(
        natural_voice(&own_model_only, &natural, Lang::Es),
        pair("tts-1-hd", VOICE)
    );
    let own_without = settings(json!({"ai": {"provider": "own_key"}}));
    assert_eq!(natural_voice(&own_without, &natural, Lang::Es), None);
    let local = settings(json!({"ai": {"provider": "local"}}));
    assert_eq!(natural_voice(&local, &natural, Lang::Es), None);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 5: the cache key (PUBLIK-INTEGRATION test 28).
// ---------------------------------------------------------------------------------------------

/// Test 5: "The cache key differs when only the voice or only the speed differs."
#[test]
fn t05_the_speech_cache_key_changes_with_the_voice_and_with_the_speed() {
    let base = speech_key(PUBLIK, MODEL, COFFEE, VOICE, 1.0);
    assert_eq!(base, speech_key(PUBLIK, MODEL, COFFEE, VOICE, 1.0));
    assert_ne!(
        base,
        speech_key(PUBLIK, MODEL, COFFEE, "nova", 1.0),
        "voice"
    );
    assert_ne!(
        base,
        speech_key(PUBLIK, MODEL, COFFEE, VOICE, 0.75),
        "speed"
    );
    assert_ne!(
        base,
        speech_key(PUBLIK, "tts-1-hd", COFFEE, VOICE, 1.0),
        "model"
    );
    assert_ne!(
        base,
        speech_key(ProviderKind::OwnKey, MODEL, COFFEE, VOICE, 1.0),
        "provider"
    );
    assert_ne!(base, speech_key(PUBLIK, MODEL, MORNING, VOICE, 1.0), "text");
}

/// 5, §6.1 and §6.7: the key is the SHA-256 of the canonical JSON of feature speech, version 1, the
/// provider and model, and input {"text_nfc", "voice", "speed"}; the text is NFC in the key and in
/// the request.
#[test]
fn t05_the_speech_key_is_that_of_section_6_1_with_the_input_of_section_6_7() {
    assert_eq!(SPEECH_FEATURE, "speech");
    let key_json = speech_key_json(PUBLIK, MODEL, COFFEE, VOICE, 0.75);
    assert_eq!(
        key_json,
        cache_key_json(
            "speech",
            1,
            PUBLIK,
            MODEL,
            &json!({"text_nfc": COFFEE, "voice": VOICE, "speed": 0.75})
        )
    );
    assert_eq!(
        speech_key(PUBLIK, MODEL, COFFEE, VOICE, 0.75),
        cache_key(&key_json)
    );
    let decomposed = "Quiero un cafe\u{301} con leche.";
    assert_ne!(decomposed, COFFEE);
    assert_eq!(
        speech_key(PUBLIK, MODEL, decomposed, VOICE, 0.75),
        speech_key(PUBLIK, MODEL, COFFEE, VOICE, 0.75)
    );
    let req = speech_request(PUBLIK, MODEL, decomposed, VOICE, 0.75);
    assert_eq!(req.input, COFFEE);
    assert_eq!(req.response_format, "mp3");
    assert_eq!(
        req.idempotency_key,
        Some(idempotency_key(&key_json, 1)),
        "the idempotency key of the key JSON at attempt 1"
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 6: §6.7 failures fall back (PUBLIK-INTEGRATION test 28).
// ---------------------------------------------------------------------------------------------

/// Test 6: "A reply with Content-Type: text/html, or a 0.05 s clip → OS voice and "Natural voice
/// unavailable, using the system voice."" (and bytes that do not decode, the third rule of §6.7)
#[test]
fn t06_an_html_reply_or_a_50_ms_clip_falls_back_to_the_os_voice_with_the_notice() {
    assert_eq!(
        NATURAL_UNAVAILABLE,
        "Natural voice unavailable, using the system voice."
    );
    for fixture in ["text-html.json", "clip-50ms.json", "not-audio.json"] {
        let mut setup = Setup::new().natural();
        let client = SpeechClient::replying(&[fixture]);
        let played = setup.play(COFFEE, false, None, &client);
        assert_played(&played, VoiceSource::Os, false, Some(NATURAL_UNAVAILABLE));
        assert_eq!(played.ai_error, None, "{fixture}");
        assert_eq!(client.requests(), 1, "{fixture}");
        assert_spoken(&setup.voices, &[(COFFEE, ES_ES, 1.0)]);
        assert!(setup.player.played().is_empty(), "{fixture}: nothing plays");
        assert_eq!(audio_files(&setup.dir), 0, "{fixture}: no file is stored");
        assert_eq!(cache(&setup.dir).audio_len(), 0, "{fixture}: no row");
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 7: a 402.
// ---------------------------------------------------------------------------------------------

/// Test 7: "A 402 → OS voice for the rest of the session, one banner, no prefetch while needs_credit."
#[test]
fn t07_a_402_gives_the_os_voice_one_banner_and_no_prefetch_while_it_lasts() {
    let mut setup = Setup::new().natural();
    let client = SpeechClient::replying(&["402.json"]);
    let first = setup.play(COFFEE, false, None, &client);
    assert_played(&first, VoiceSource::Os, false, None);
    let banner = first.ai_error.expect("the 402 shows once");
    assert_eq!(banner.code, ErrorCode::NeedsCredit);
    assert_eq!(banner.message, BALANCE);
    assert_eq!(banner.action, Some(json!({"top_up_url": TOP_UP})));
    assert!(client.status().needs_credit);

    // Later plays choose the OS voice with no request and no banner.
    let second = setup.play(MORNING, false, None, &client);
    assert_played(&second, VoiceSource::Os, false, None);
    assert_eq!(second.ai_error, None, "one banner");
    assert_eq!(client.requests(), 1);
    assert_spoken(
        &setup.voices,
        &[(COFFEE, ES_ES, 1.0), (MORNING, ES_ES, 1.0)],
    );

    // No prefetch while the status says needs_credit.
    let lesson = setup
        .app
        .lesson_start(SKILL, "learn")
        .unwrap_or_else(|e| panic!("the lesson starts: {e}"));
    let shown = lesson.exercises[before_a_sentence(&lesson.exercises)]
        .id
        .clone();
    let prefetched = setup
        .app
        .tts_prefetch_with(&lesson.session_id, &shown, access(&client))
        .unwrap_or_else(|e| panic!("the prefetch: {e}"));
    assert!(!prefetched.prefetched);
    assert_eq!(client.requests(), 1, "no prefetch while needs_credit");

    // Once the status no longer says it, the next sentence is prefetched again.
    client.set_needs_credit(false);
    client.push("valid.json");
    let prefetched = setup
        .app
        .tts_prefetch_with(&lesson.session_id, &shown, access(&client))
        .unwrap_or_else(|e| panic!("the prefetch: {e}"));
    assert!(prefetched.prefetched);
    assert_eq!(client.requests(), 2);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 8: offline.
// ---------------------------------------------------------------------------------------------

/// Test 8: "Offline: a cached sentence plays from cache; an uncached one uses the OS voice."
#[test]
fn t08_offline_a_cached_sentence_plays_from_the_cache_and_another_uses_the_os_voice() {
    let mut setup = Setup::new().natural();
    let client = SpeechClient::replying(&["valid.json", "offline.json"]);
    let first = setup.play(COFFEE, false, None, &client);
    assert_played(&first, VoiceSource::Natural, false, None);

    // Offline from here on.
    let cached = setup.play(COFFEE, false, None, &client);
    assert_played(&cached, VoiceSource::Natural, true, None);
    assert_eq!(client.requests(), 1, "the cached sentence makes no request");

    let uncached = setup.play(MORNING, false, None, &client);
    assert_played(&uncached, VoiceSource::Os, false, Some(NATURAL_UNAVAILABLE));
    assert_eq!(client.requests(), 2);
    assert_spoken(&setup.voices, &[(MORNING, ES_ES, 1.0)]);

    // "A cached sentence is found by its file ... even after cache/ai.sqlite3 was removed."
    let database = setup.dir.join("cache").join("ai.sqlite3");
    if database.exists() {
        fs::remove_file(&database).expect("cache/ai.sqlite3 is removed");
    }
    let from_file = setup.play(COFFEE, false, None, &client);
    assert_played(&from_file, VoiceSource::Natural, true, None);
    assert_eq!(client.requests(), 2);
    let played_audio = setup.player.played();
    assert_eq!(played_audio.len(), 3);
    assert!(played_audio.iter().all(|(bytes, _)| *bytes == wav(1_500)));
}

/// 8, the other failures: "Any other error (offline, timeout, provider error) falls back with the
/// notice."
#[test]
fn t08_a_timeout_or_a_provider_error_also_falls_back_with_the_notice() {
    for fixture in ["timeout.json", "500.json"] {
        let mut setup = Setup::new().natural();
        let client = SpeechClient::replying(&[fixture]);
        let played = setup.play(COFFEE, false, None, &client);
        assert_played(&played, VoiceSource::Os, false, Some(NATURAL_UNAVAILABLE));
        assert_eq!(client.requests(), 1, "{fixture}: one attempt (I4)");
        assert_spoken(&setup.voices, &[(COFFEE, ES_ES, 1.0)]);
        assert_eq!(audio_files(&setup.dir), 0, "{fixture}");
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 9: grading what was heard.
// ---------------------------------------------------------------------------------------------

/// Test 9: "listen_type with target Buenos días.: buenos dias → almost (accent); buenas días → wrong
/// (buenas is itself a known form, so it is not a typo)."
#[test]
fn t09_listen_type_grades_an_accent_slip_almost_and_a_known_form_wrong() {
    let forms = Forms::load_tsv(read("content/es/forms.tsv").as_bytes())
        .unwrap_or_else(|e| panic!("the Spanish forms table loads: {e:?}"));
    assert!(
        !forms.lemmas_of("buenas").is_empty(),
        "buenas is a form of the Spanish course"
    );
    let listen = Exercise {
        id: "e1".to_string(),
        item_id: "es.a1.u01.s01#e:2".to_string(),
        facet: Facet::Receptive,
        origin: Origin::Template,
        body: Body::ListenType {
            text: MORNING.to_string(),
            accepted: vec![MORNING.to_string()],
        },
    };
    let ctx = GradeCtx {
        lang: Lang::Es,
        leniency: AccentLeniency::Lenient,
        forms: &forms,
        noun: None,
        stored: Vec::new(),
        judgement: None,
    };

    let accent = grade(&ctx, &listen, &Answer::Text("buenos dias".to_string()));
    assert_eq!(accent.verdict, Verdict::Almost);
    assert_eq!(accent.tags, [Tag::Accent]);
    assert_eq!(
        accent.message.as_deref(),
        Some("Almost: mind the accent: días")
    );

    let known = grade(&ctx, &listen, &Answer::Text("buenas días".to_string()));
    assert_eq!(known.verdict, Verdict::Wrong);
    assert!(known.tags.is_empty(), "not a typo: {:?}", known.tags);
    assert_eq!(known.corrected.as_deref(), Some(MORNING));

    // The same one-letter slip in a word that is no form is a typo.
    let none = Forms::default();
    let typo_ctx = GradeCtx {
        forms: &none,
        ..ctx.clone()
    };
    assert_eq!(
        grade(&typo_ctx, &listen, &Answer::Text("buenas días".to_string())).verdict,
        Verdict::Typo
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 10: Can't listen now.
// ---------------------------------------------------------------------------------------------

/// Test 10: "Can't listen now at 10:00 (injected clock) → no listen_type until 11:00, then allowed
/// again."
#[test]
fn t10_cant_listen_now_at_ten_turns_listen_type_off_until_eleven() {
    let mut setup = Setup::new();
    practise_vocabulary(&mut setup.app, SKILL);

    // 10:00, a Spanish voice installed: the next lesson has its two listen_type exercises.
    assert!(setup.app.listen_available(Lang::Es));
    assert_eq!(setup.app.audio_status().listen_off_until, None);
    let kinds = next_lesson_kinds(&mut setup.app, SKILL);
    assert_eq!(count(&kinds, ExerciseType::ListenType), 2, "{kinds:?}");

    setup
        .app
        .session_skip("listen")
        .unwrap_or_else(|e| panic!("Can't listen now: {e}"));
    assert!(!setup.app.listen_available(Lang::Es));
    assert_eq!(
        setup.app.audio_status().listen_off_until,
        Some(ELEVEN),
        "now + 60 minutes"
    );
    let kinds = next_lesson_kinds(&mut setup.app, SKILL);
    assert_eq!(count(&kinds, ExerciseType::ListenType), 0, "{kinds:?}");
    assert!(
        count(&kinds, ExerciseType::TranslateToEn) >= 2,
        "the sentences stay translate_to_en: {kinds:?}"
    );

    // 10:59:59.999: still off.
    setup.set_now(ELEVEN - 1);
    assert!(!setup.app.listen_available(Lang::Es));
    let kinds = next_lesson_kinds(&mut setup.app, SKILL);
    assert_eq!(count(&kinds, ExerciseType::ListenType), 0, "{kinds:?}");

    // 11:00: allowed again.
    setup.set_now(ELEVEN);
    assert!(setup.app.listen_available(Lang::Es));
    let kinds = next_lesson_kinds(&mut setup.app, SKILL);
    assert_eq!(count(&kinds, ExerciseType::ListenType), 2, "{kinds:?}");
}

/// 10, session_skip("speak"): "records the same for module 10 and changes nothing yet".
#[test]
fn t10_cant_speak_now_changes_nothing_for_listening() {
    let mut setup = Setup::new();
    setup
        .app
        .session_skip("speak")
        .unwrap_or_else(|e| panic!("Can't speak now: {e}"));
    assert!(setup.app.listen_available(Lang::Es));
    assert_eq!(setup.app.audio_status().listen_off_until, None);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 12: the duration bound.
// ---------------------------------------------------------------------------------------------

/// Test 12: "Duration bound: a 6-word sentence at speed 1.0 allows at most 3 × 0.5 × 6 = 9 s; a 10 s clip
/// is rejected."
#[test]
fn t12_a_six_word_sentence_allows_nine_seconds_and_a_ten_second_clip_is_rejected() {
    assert_eq!(speech_bounds_ms(SIX_WORDS, 1.0), (200, 9_000));
    // 3 x 500 x 5 / 0.75 = 10,000 and 3 x 500 x 5 / 1.0 = 7,500.
    assert_eq!(speech_bounds_ms(COFFEE, 0.75), (200, 10_000));
    assert_eq!(speech_bounds_ms(COFFEE, 1.0), (200, 7_500));

    assert!(!speech_valid(
        &reply("clip-10s.json"),
        SIX_WORDS,
        1.0,
        &WavDecoder
    ));
    assert!(speech_valid(
        &reply("clip-9s.json"),
        SIX_WORDS,
        1.0,
        &WavDecoder
    ));

    // Through tts_play: the 10 s clip falls back to the OS voice with the notice.
    let mut setup = Setup::new().natural();
    let client = SpeechClient::replying(&["clip-10s.json"]);
    let played = setup.play(SIX_WORDS, false, None, &client);
    assert_played(&played, VoiceSource::Os, false, Some(NATURAL_UNAVAILABLE));
    assert_eq!(audio_files(&setup.dir), 0);
}

/// 12, the other rules of speech_valid (§6.7): "Content-Type audio/* (case-insensitive, parameters
/// allowed), the bytes decode, and the duration lies within speech_bounds_ms".
#[test]
fn t12_speech_valid_checks_the_type_the_bytes_and_the_duration() {
    for (fixture, valid) in [
        ("valid.json", true),
        ("valid-type-parameters.json", true),
        ("text-html.json", false),
        ("not-audio.json", false),
        ("clip-50ms.json", false),
    ] {
        assert_eq!(
            speech_valid(&reply(fixture), COFFEE, 1.0, &WavDecoder),
            valid,
            "{fixture}"
        );
    }
}

/// 12, WavDecoder: "RIFF WAVE with PCM data only: duration = data bytes ÷ byte rate", None when the
/// bytes do not decode.
#[test]
fn t12_the_wav_decoder_measures_data_bytes_over_the_byte_rate() {
    assert_eq!(WavDecoder.duration_ms(&wav(1_500)), Some(1_500));
    assert_eq!(WavDecoder.duration_ms(&wav(50)), Some(50));
    assert_eq!(WavDecoder.duration_ms(&wav(10_000)), Some(10_000));
    assert_eq!(WavDecoder.duration_ms(b"<!doctype html><p>no</p>"), None);
    assert_eq!(WavDecoder.duration_ms(&[]), None);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 13: clearing the audio cache.
// ---------------------------------------------------------------------------------------------

/// Test 13: "Clear audio cache removes the files and audio_cache rows and leaves progress unchanged."
#[test]
fn t13_clear_audio_cache_removes_the_files_and_rows_and_keeps_progress() {
    let mut setup = Setup::new().natural();
    finish_lesson(&mut setup.app, SKILL);
    let client = SpeechClient::replying(&["valid.json", "valid-640.json"]);
    setup.play(COFFEE, false, None, &client);
    setup.play(MORNING, false, None, &client);
    assert_eq!(audio_files(&setup.dir), 2);
    assert_eq!(cache(&setup.dir).audio_len(), 2);
    let before = tables(&mut setup.app, &setup.dir);
    assert!(
        !before["review_log"]
            .as_array()
            .expect("the backup has a review log")
            .is_empty(),
        "there is progress to keep"
    );

    let cleared = setup
        .app
        .cache_clear("audio")
        .unwrap_or_else(|e| panic!("Clear audio cache: {e}"));
    assert_eq!(cleared.files_removed, 2);
    assert_eq!(audio_files(&setup.dir), 0);
    assert_eq!(cache(&setup.dir).audio_len(), 0);
    assert_eq!(
        tables(&mut setup.app, &setup.dir),
        before,
        "progress is unchanged"
    );

    // A cleared sentence is asked for again.
    let again = SpeechClient::replying(&["valid.json"]);
    let played = setup.play(COFFEE, false, None, &again);
    assert_played(&played, VoiceSource::Natural, false, None);
    assert_eq!(again.requests(), 1);

    // "ai" leaves the audio files; "all" removes them and cache/ai.sqlite3.
    setup
        .app
        .cache_clear("ai")
        .unwrap_or_else(|e| panic!("clear the AI cache: {e}"));
    assert_eq!(audio_files(&setup.dir), 1);
    setup
        .app
        .cache_clear("all")
        .unwrap_or_else(|e| panic!("clear every cache: {e}"));
    assert_eq!(audio_files(&setup.dir), 0);
    assert!(!setup.dir.join("cache").join("ai.sqlite3").exists());
    assert_eq!(
        tables(&mut setup.app, &setup.dir),
        before,
        "progress is unchanged"
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 14: prefetch.
// ---------------------------------------------------------------------------------------------

/// Test 14: "Prefetch: showing exercise n makes exactly one P5 request, for exercise n + 1's sentence,
/// when it is not cached and natural voices are on."
#[test]
fn t14_showing_an_exercise_prefetches_the_next_sentence_once() {
    let mut setup = Setup::new().natural();
    let lesson = setup
        .app
        .lesson_start(SKILL, "learn")
        .unwrap_or_else(|e| panic!("the lesson starts: {e}"));
    let n = before_a_sentence(&lesson.exercises);
    let shown = lesson.exercises[n].id.clone();
    let next = sentence_of(&lesson.exercises[n + 1])
        .expect("a sentence")
        .to_string();

    let client = SpeechClient::replying(&["valid.json"]);
    let prefetched = setup
        .app
        .tts_prefetch_with(&lesson.session_id, &shown, access(&client))
        .unwrap_or_else(|e| panic!("the prefetch: {e}"));
    assert!(prefetched.prefetched);
    let sent = client.sent();
    assert_eq!(sent.len(), 1, "exactly one request");
    assert_eq!(
        sent[0],
        speech_request(PUBLIK, MODEL, &next, VOICE, 1.0),
        "for the next exercise's sentence, at normal speed"
    );
    let key = speech_key(PUBLIK, MODEL, &next, VOICE, 1.0);
    assert_eq!(
        fs::read(setup.dir.join(audio_file(&key))).ok(),
        Some(wav(1_500)),
        "the reply is stored"
    );
    assert!(setup.player.played().is_empty(), "a prefetch plays nothing");
    assert!(setup.voices.spoken().is_empty());

    // Already cached: no request.
    let again = setup
        .app
        .tts_prefetch_with(&lesson.session_id, &shown, access(&client))
        .unwrap_or_else(|e| panic!("the prefetch: {e}"));
    assert!(!again.prefetched);
    assert_eq!(client.requests(), 1);

    // Showing exercise n + 1 plays its sentence from the cache.
    let played = setup.play(&next, false, Some(lesson.session_id.as_str()), &client);
    assert_played(&played, VoiceSource::Natural, true, None);
    assert_eq!(client.requests(), 1);

    // The last exercise has nothing after it.
    let last = lesson.exercises.last().expect("exercises").id.clone();
    let after_last = setup
        .app
        .tts_prefetch_with(&lesson.session_id, &last, access(&client))
        .unwrap_or_else(|e| panic!("the prefetch: {e}"));
    assert!(!after_last.prefetched);
    assert_eq!(client.requests(), 1);
}

/// 14, the conditions: no prefetch with the system voice chosen, and none for a `pick_meaning`
/// prompt while autoplay is off ("with audio.autoplay on, pick_meaning or translate_to_en").
#[test]
fn t14_no_prefetch_without_natural_voices_or_for_a_prompt_autoplay_would_not_read() {
    let mut setup = Setup::new().natural();
    let lesson = setup
        .app
        .lesson_start(SKILL, "learn")
        .unwrap_or_else(|e| panic!("the lesson starts: {e}"));
    let before_pick = lesson
        .exercises
        .windows(2)
        .position(|pair| matches!(pair[1].body, Body::PickMeaning { .. }))
        .expect("a pick_meaning exercise after the first");
    let shown = lesson.exercises[before_pick].id.clone();
    let client = SpeechClient::replying(&["valid.json", "valid.json"]);

    set(&mut setup.app, json!({"audio": {"voice_source": "os"}}));
    let os = setup
        .app
        .tts_prefetch_with(&lesson.session_id, &shown, access(&client))
        .unwrap_or_else(|e| panic!("the prefetch: {e}"));
    assert!(!os.prefetched);

    set(
        &mut setup.app,
        json!({"audio": {"voice_source": "natural", "autoplay": false}}),
    );
    let quiet = setup
        .app
        .tts_prefetch_with(&lesson.session_id, &shown, access(&client))
        .unwrap_or_else(|e| panic!("the prefetch: {e}"));
    assert!(!quiet.prefetched);
    assert_eq!(client.requests(), 0);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 15: the cost line.
// ---------------------------------------------------------------------------------------------

/// Test 15: "Charges of 600 and 640 micros → "Natural voices cost about $0.00062 a sentence (measured)."
/// (the mean through format_dollars, module 06)."
#[test]
fn t15_charges_of_600_and_640_give_the_measured_cost_line() {
    assert_eq!(format_dollars(620), "$0.00062");
    assert_eq!(
        speech_cost_line(&[600, 640]),
        "Natural voices cost about $0.00062 a sentence (measured)."
    );
    // No charge yet: the setup run's 540 micros.
    assert_eq!(MEASURED_SPEECH_MICROS, 540);
    assert_eq!(
        speech_cost_line(&[]),
        "Natural voices cost about $0.00054 a sentence (measured)."
    );
    // The mean rounds half up to a whole micro: 1.5 → 2.
    assert_eq!(
        speech_cost_line(&[1, 2]),
        "Natural voices cost about $0.000002 a sentence (measured)."
    );

    // Through the service: the charges of two natural plays.
    let mut setup = Setup::new().natural();
    assert_eq!(setup.app.audio_status().cost_line, speech_cost_line(&[]));
    let client = SpeechClient::replying(&["valid.json", "valid-640.json"]);
    setup.play(COFFEE, false, None, &client);
    setup.play(MORNING, false, None, &client);
    let mut charges = cache(&setup.dir).speech_charges();
    charges.sort_unstable();
    assert_eq!(charges, [600, 640]);
    assert_eq!(
        setup.app.audio_status().cost_line,
        "Natural voices cost about $0.00062 a sentence (measured)."
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 17: slow mode stretched locally.
// ---------------------------------------------------------------------------------------------

/// Test 17: "Slow mode when speed_honoured is false: the request sends speed 1.0 and the player gets
/// stretch 1 ÷ 0.75; normal and slow plays share one cached file."
#[test]
fn t17_slow_mode_stretches_locally_when_speed_is_not_honoured_and_shares_one_file() {
    let natural = VoicesConfig {
        model: MODEL.to_string(),
        speed_honoured: false,
        voices: BTreeMap::from([
            (Lang::Es, VOICE.to_string()),
            (Lang::Fr, VOICE.to_string()),
            (Lang::De, VOICE.to_string()),
        ]),
    };
    let mut setup = Setup::with(spanish_and_french(), WINRT, false, natural).natural();
    let client = SpeechClient::replying(&["valid.json"]);

    let slow = setup.play(COFFEE, true, None, &client);
    assert_played(&slow, VoiceSource::Natural, false, None);
    let sent = client.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].body()["speed"].as_f64(), Some(1.0));
    assert_eq!(sent[0], speech_request(PUBLIK, MODEL, COFFEE, VOICE, 1.0));

    let normal = setup.play(COFFEE, false, None, &client);
    assert_played(&normal, VoiceSource::Natural, true, None);
    let slow_again = setup.play(COFFEE, true, None, &client);
    assert_played(&slow_again, VoiceSource::Natural, true, None);
    assert_eq!(client.requests(), 1, "one cached file for both speeds");
    assert_eq!(audio_files(&setup.dir), 1);

    let played_audio = setup.player.played();
    assert_eq!(played_audio.len(), 3);
    assert!(played_audio.iter().all(|(bytes, _)| *bytes == wav(1_500)));
    close(played_audio[0].1, 1.0 / 0.75);
    close(played_audio[1].1, 1.0);
    close(played_audio[2].1, 1.0 / 0.75);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 18: errors.
// ---------------------------------------------------------------------------------------------

/// Test 18: "Errors: empty text and a 501-character text give their invalid_input messages".
#[test]
fn t18_empty_and_too_long_texts_are_refused_with_their_messages() {
    let mut setup = Setup::new();
    for text in ["", "   ", "\n\t "] {
        assert_error(
            setup.app.tts_play_with(text, "es", false, None, None).err(),
            ErrorCode::InvalidInput,
            NO_TEXT,
        );
    }
    let long = "a".repeat(501);
    assert_error(
        setup
            .app
            .tts_play_with(&long, "es", false, None, None)
            .err(),
        ErrorCode::InvalidInput,
        TOO_LONG,
    );
    assert!(setup.voices.spoken().is_empty());

    // 500 characters, trimmed, are read: characters, not bytes.
    let accents = "é".repeat(500);
    assert_eq!(accents.len(), 1_000);
    setup.play_os(&accents, false);
    let padded = format!("  {}  ", "a".repeat(500));
    setup.play_os(&padded, false);
    assert_eq!(setup.voices.spoken().len(), 2);

    // The language must be a course language (the module 04 unknown-value error).
    assert_error(
        setup
            .app
            .tts_play_with(MORNING, "it", false, None, None)
            .err(),
        ErrorCode::InvalidInput,
        "Unknown value it for lang.",
    );
}

/// Test 18: "a player that fails gives the speaker message"
#[test]
fn t18_a_player_that_fails_gives_the_speaker_message() {
    let mut setup = Setup::with(spanish_and_french(), WINRT, true, bundled_voices()).natural();
    let client = SpeechClient::replying(&["valid.json"]);
    assert_error(
        setup
            .app
            .tts_play_with(COFFEE, "es", false, None, access(&client))
            .err(),
        ErrorCode::NoVoice,
        SPEAKER,
    );
}

/// Test 18: "tts_prefetch for a finished lesson gives not_found; session_skip("sing") gives the
/// unknown-value error."
#[test]
fn t18_a_prefetch_for_a_finished_lesson_and_an_unknown_skip_are_refused() {
    let mut setup = Setup::new();
    let lesson = setup
        .app
        .lesson_start(SKILL, "learn")
        .unwrap_or_else(|e| panic!("the lesson starts: {e}"));
    let first = lesson.exercises[0].id.clone();
    setup
        .app
        .lesson_finish(&lesson.session_id)
        .unwrap_or_else(|e| panic!("the lesson finishes: {e}"));
    assert_error(
        setup
            .app
            .tts_prefetch_with(&lesson.session_id, &first, None)
            .err(),
        ErrorCode::NotFound,
        NOT_IN_PROGRESS,
    );
    assert_error(
        setup
            .app
            .tts_prefetch_with("s-0000000000000000", "e1", None)
            .err(),
        ErrorCode::NotFound,
        NOT_IN_PROGRESS,
    );
    assert_error(
        setup.app.session_skip("sing").err(),
        ErrorCode::InvalidInput,
        "Unknown value sing for kind.",
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 19: charges reach the lesson (contract test 34).
// ---------------------------------------------------------------------------------------------

/// Test 19: "Charges reach the lesson: a natural play with a lesson's session_id adds its charge to that
/// session's cost (contract test 34), and a charged reply that fails §6.7 adds it too."
#[test]
fn t19_natural_plays_in_a_lesson_add_their_charges_to_its_cost() {
    let mut setup = Setup::new().natural();
    let lesson = setup
        .app
        .lesson_start(SKILL, "learn")
        .unwrap_or_else(|e| panic!("the lesson starts: {e}"));
    let session = lesson.session_id.clone();
    assert_eq!(
        session_cost(&tables(&mut setup.app, &setup.dir), &session),
        0
    );
    let client = SpeechClient::replying(&["valid.json", "text-html.json", "valid-640.json"]);

    let played = setup.play(COFFEE, false, Some(session.as_str()), &client);
    assert_played(&played, VoiceSource::Natural, false, None);
    assert_eq!(
        session_cost(&tables(&mut setup.app, &setup.dir), &session),
        600
    );

    // A charged reply that fails §6.7 counts too.
    let failed = setup.play(MORNING, false, Some(session.as_str()), &client);
    assert_played(&failed, VoiceSource::Os, false, Some(NATURAL_UNAVAILABLE));
    assert_eq!(
        session_cost(&tables(&mut setup.app, &setup.dir), &session),
        600 + 580
    );

    // A prefetch records its reply exactly as a play does.
    let shown = lesson.exercises[before_a_sentence(&lesson.exercises)]
        .id
        .clone();
    let prefetched = setup
        .app
        .tts_prefetch_with(&session, &shown, access(&client))
        .unwrap_or_else(|e| panic!("the prefetch: {e}"));
    assert!(prefetched.prefetched);
    assert_eq!(
        session_cost(&tables(&mut setup.app, &setup.dir), &session),
        600 + 580 + 640
    );

    // A replay from the cache costs nothing.
    let cached = setup.play(COFFEE, false, Some(session.as_str()), &client);
    assert_played(&cached, VoiceSource::Natural, true, None);
    assert_eq!(client.requests(), 3);
    assert_eq!(
        session_cost(&tables(&mut setup.app, &setup.dir), &session),
        600 + 580 + 640
    );
}

// ---------------------------------------------------------------------------------------------
// Commands and events (the spec's Interfaces).
// ---------------------------------------------------------------------------------------------

/// Interfaces: "COMMANDS gains the six names after explain_answer, in the order of this table", and
/// the event is audio://ended.
#[test]
fn interfaces_the_six_commands_follow_explain_answer_and_the_ended_event_is_named() {
    let at = COMMANDS
        .iter()
        .position(|c| *c == "explain_answer")
        .expect("explain_answer is listed");
    assert_eq!(
        &COMMANDS[at + 1..at + 7],
        [
            "voices_list",
            "tts_play",
            "tts_stop",
            "tts_prefetch",
            "audio_status",
            "session_skip"
        ]
    );
    assert_eq!(AUDIO_ENDED_EVENT, "audio://ended");
}

// ---------------------------------------------------------------------------------------------
// More cases: a new attempt after an unusable clip, Delete everything, no consent.
// ---------------------------------------------------------------------------------------------

/// Test 6 with §6.1: a reply that failed §6.7 is a failed attempt, so asking for the same sentence
/// again sends attempt 2's idempotency key (the key JSON with "attempt": 2), and the valid clip it
/// gets is stored under the plain cache key.
#[test]
fn t06_a_sentence_asked_for_again_after_an_unusable_clip_is_a_new_attempt() {
    let mut setup = Setup::new().natural();
    let client = SpeechClient::replying(&["clip-50ms.json", "valid.json"]);
    let first = setup.play(COFFEE, false, None, &client);
    assert_played(&first, VoiceSource::Os, false, Some(NATURAL_UNAVAILABLE));
    let second = setup.play(COFFEE, false, None, &client);
    assert_played(&second, VoiceSource::Natural, false, None);

    let key_json = speech_key_json(PUBLIK, MODEL, COFFEE, VOICE, 1.0);
    let keys: Vec<Option<String>> = client
        .sent()
        .iter()
        .map(|request| request.idempotency_key.clone())
        .collect();
    assert_eq!(
        keys,
        vec![
            Some(idempotency_key(&key_json, 1)),
            Some(idempotency_key(&key_json, 2)),
        ]
    );
    assert_ne!(keys[0], keys[1]);
    let stored = setup
        .dir
        .join(audio_file(&speech_key(PUBLIK, MODEL, COFFEE, VOICE, 1.0)));
    assert!(stored.is_file(), "the clip is stored under the plain key");

    // A third play is a cache hit: no request.
    let third = setup.play(COFFEE, false, None, &client);
    assert_played(&third, VoiceSource::Natural, true, None);
    assert_eq!(client.requests(), 2);
}

/// Delete everything (module 04) also removes the cached natural sentences under cache/audio.
#[test]
fn t13_delete_everything_removes_the_cached_sentences() {
    let mut setup = Setup::new().natural();
    let client = SpeechClient::replying(&["valid.json", "valid-640.json"]);
    setup.play(COFFEE, false, None, &client);
    setup.play(MORNING, false, None, &client);
    assert_eq!(audio_files(&setup.dir), 2);
    setup
        .app
        .data_delete_all("delete")
        .expect("Delete everything with the word");
    assert_eq!(audio_files(&setup.dir), 0, "no cached sentence is left");
}

/// I12: natural voices chosen but no consent yet (the client cannot be built) → the OS voice
/// speaks, with no request, no notice and no banner; consent is asked at an AI action, never here.
#[test]
fn t04_natural_voices_without_consent_use_the_os_voice_quietly() {
    let mut setup = Setup::new().natural();
    let played = setup
        .app
        .tts_play_with(COFFEE, "es", false, None, Some(Err(AiError::NeedsConsent)))
        .expect("the OS voice speaks");
    assert_played(&played, VoiceSource::Os, false, None);
    assert_eq!(played.ai_error, None);
    assert_spoken(&setup.voices, &[(COFFEE, ES_ES, 1.0)]);
    assert!(setup.player.played().is_empty());
}
