//! Acceptance tests of module 10 in the core (docs/modules/10-speaking-and-pronunciation.md,
//! Acceptance tests 3 (the message), 5 to 16, 19 to 24 and 26; tests 1 to 4, 17 and the levels of
//! 19 are in crates/parley-audio, 18 in its gates.rs, the wire form of 7 and contract test 29 in
//! crates/parley-publik, and the UI halves in app/src/test/m10): taking a recording, scoring what
//! was heard, the P6 request and its in-memory transcripts, the local transcriber and its models,
//! the composer's speak switch, Can't speak now, the cost line, and the privacy and consent rules
//! (I2, I4, I5, I10, I12, I13). Contract test 34 is test 20 here.
//!
//! References: the spec (its Interfaces, Messages, Design notes and the figures worked out in its
//! acceptance tests), Wagner and Fischer (1974) for the alignments (each script worked by hand,
//! with the spec's tie order: diagonal, then delete, then insert), docs/PUBLIK-INTEGRATION.md §6.1,
//! §6.8 and §9, module 03's normaliser and folding, module 04's megabytes, module 06's
//! format_dollars, and fixtures true by construction: the replies of tests/fixtures/ai/P6/ (its
//! README gives each body and charge), takes that are 16 kHz mono WAV files of integer square
//! waves written here field by field, the bundled Spanish course (content/es) and the number words
//! of content/<lang>/numbers.yaml. No test reaches the network: transcripts come from a scripted
//! client and a fake local transcriber, model downloads from a scripted Net, and the microphone and
//! the player are fakes that record what they are asked to do.
//!
//! A lesson with `speak` exercises is the second lesson of es.a1.u01.s01: the first, with ten new
//! items, introduces the skill's ten words; the second, with two, introduces its first two example
//! sentences, whose productive entries (module 03) are `translate_to_target` and become `speak`
//! when a microphone and a transcriber can be used. Their texts are "Hola, ¿qué tal?" (three
//! words: Hola, qué, tal) and "Buenos días.".
//!
//! Where the spec leaves a detail open, these tests assume that `Fs::list` gives the bare names of
//! the files in a directory (as module 09's tests do); that `Numbers` and `load_number_words` are
//! reachable as `parley_core::content::numbers::{Numbers, load_number_words}` (the file the spec
//! names, already a public module); that `speak_verdict` and `Grade` use the grader's `Verdict`;
//! that a later take's `SpeakResult` carries its own words and score beside the first take's grade;
//! and that the `speaking.whisper_model` setting is written as the patch
//! `{"speaking": {"whisper_model": name}}`.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use parley_core::ai::{
    AiClient, AiError, AiStatus, AudioReply, Cache, ChatReply, ChatRequest, MAX_REQUEST_BYTES,
    MEASURED_TRANSCRIBE_MICROS, NO_TOKEN_MESSAGE, OFFLINE_LOCAL_NOTICE, PUBLIK_TRANSCRIBE_MODEL,
    ProviderKind, SpeechRequest, TRANSCRIBE_FEATURE, TRANSCRIBE_TIMEOUT_S, TranscribeRequest,
    TranscriptReply, bundled_voices, cache_key, cache_key_json, format_dollars, heard_text,
    idempotency_key, local_model_id, parse_transcript, speaking_error, transcribe_cost_line,
    transcribe_key, transcribe_key_json, transcribe_model, transcribe_request,
};
use parley_core::api::{
    COMMANDS, LessonStarted, MIC_LEVEL_EVENT, MIC_LIMIT_EVENT, MODEL_PROGRESS_EVENT, MicLevel,
    ModelProgress, Recording, SpeakResult, Transcriber, megabytes,
};
use parley_core::app::{AiAccess, Config, CourseFiles, DownloadBegin, Env, Parley, SpeakBegin};
use parley_core::audio::{
    Audio, AudioError, Capture, CaptureError, LocalTranscriber, NoCapture, OsVoice, OsVoices,
    Platform, Player, RateRange, Speaking, Take, WavDecoder, mic_permission_message,
};
use parley_core::content::Lang;
use parley_core::content::numbers::{Numbers, load_number_words};
use parley_core::env::{
    Clock, Fs, FsError, HttpRequest, HttpResponse, Net, NetError, Rng, SplitMix64,
};
use parley_core::error::{AppError, ErrorCode};
use parley_core::exercise::{Body, Exercise, ExerciseType};
use parley_core::grading::{Answer, Verdict};
use parley_core::settings::{Settings, apply_patch};
use parley_core::speech::{
    DEFAULT_WHISPER_MODEL, Edit, ModelEntry, ModelRegistry, WHISPER_DIR, WordResult, WordStatus,
    align, bundled_models, model_file, model_verified, parse_model_registry, score, speak_grade,
    speak_rating, speak_verdict, speech_words, word_key,
};
use parley_core::srs::Rating;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------------------------
// The texts and figures of the spec.
// ---------------------------------------------------------------------------------------------

/// The first skill of the bundled Spanish course: ten words, then seven example sentences.
const SKILL: &str = "es.a1.u01.s01";
/// Its first two example sentences: the lesson's `speak` exercises.
const HOLA: &str = "Hola, ¿qué tal?";
const BUENOS: &str = "Buenos días.";
/// What the fake local transcriber hears unless a test says otherwise: all three words of HOLA.
const HOLA_HEARD: &str = "hola qué tal";

const PUBLIK: ProviderKind = ProviderKind::Publik;

// Messages.
const TOO_QUIET: &str = "We didn't hear anything. Try again a little closer to the microphone.";
const NO_MICROPHONE: &str = "No microphone was found. Connect one, or choose Can't speak now.";
const MIC_FAILED: &str = "Parley could not record from the microphone. Try again.";
const NOTHING_RECORDED: &str = "Nothing is being recorded.";
const RECORDING_GONE: &str = "This recording is no longer available. Record it again.";
const NOT_AVAILABLE: &str = "This exercise is no longer available.";
const NOT_SPEAKING: &str = "This exercise is not a speaking exercise.";
const CANNOT_WRITE: &str = "Parley could not write that file.";
const SPEAKER: &str =
    "Parley could not play the sound. Check that a speaker or headphones are connected.";
const NOT_IN_BUILD: &str = "Local speech checks are not available in this build. Choose your AI provider in Settings > Speaking.";
const BASE_NOT_DOWNLOADED: &str =
    "The local speech model is not downloaded. Download it in Settings > Speaking (148.0 MB).";
const OWN_KEY_NO_MODEL: &str = "Add a transcription model for your own key in Settings > AI, or download the local model in Settings > Speaking.";
const NEED_CONNECTION: &str = "Speaking checks need a connection or the local model.";
const LOCAL_FAILED: &str =
    "The local speech model could not be used. Download it again in Settings > Speaking.";
const INVALID: &str = "The AI answer could not be used.";
const CANT_CHECK: &str = "Can't check speaking right now.";
const DAMAGED: &str = "The download was damaged. Try again.";
const DOWNLOAD_OFFLINE: &str = "You're offline. Download the speech model when you're connected.";
const DOWNLOAD_MISSING: &str = "The speech model could not be downloaded. Try again later.";
const DOWNLOAD_BUSY: &str = "This model is already being downloaded.";

/// The 402 of tests/fixtures/ai/P6/402.json.
const BALANCE: &str = "Your publik balance is $0.00. Add a plan or pack to keep using AI features.";
const TOP_UP: &str = "https://publikhq.com/wallet/add-credit?from=402";
/// The transcript of tests/fixtures/ai/P6/valid.json and its charge.
const VALID_TEXT: &str = "Yo es estudiante.";
const VALID_CHARGE: i64 = 538;
/// The charge of tests/fixtures/ai/P6/not-a-string.json.
const UNUSABLE_CHARGE: i64 = 600;

/// The registry of the spec's Interfaces.
const PIN: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1";
const TINY_SHA: &str = "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21";
const BASE_SHA: &str = "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe";
const SMALL_SHA: &str = "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b";

/// 10:00 local time on 2026-11-13 at UTC+1, which is 09:00Z: 20,770 days after 1970-01-01.
const TEN: i64 = 20_770 * 86_400_000 + 9 * 3_600_000;
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

/// The bundled Spanish course, read once.
fn spanish() -> CourseFiles {
    static SPANISH: OnceLock<CourseFiles> = OnceLock::new();
    SPANISH
        .get_or_init(|| CourseFiles {
            syllabus_yaml: read("content/es/syllabus.yaml"),
            forms_tsv: read("content/es/forms.tsv"),
            numbers_yaml: Some(read("content/es/numbers.yaml")),
        })
        .clone()
}

/// The number words of content/<lang>/numbers.yaml.
fn numbers(lang: Lang) -> Numbers {
    let file = format!("content/{}/numbers.yaml", lang.as_str());
    load_number_words(&read(&file)).unwrap_or_else(|e| panic!("{file} loads: {e:?}"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

// ---------------------------------------------------------------------------------------------
// Takes: 16 kHz mono 16-bit WAV files of an integer square wave.
// ---------------------------------------------------------------------------------------------

/// RIFF WAVE, 16 kHz mono 16-bit PCM: the 44-byte header written field by field, then the samples.
fn wav16(samples: &[i16]) -> Vec<u8> {
    let data_len = u32::try_from(samples.len() * 2).expect("a short take");
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&16_000u32.to_le_bytes());
    out.extend_from_slice(&32_000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

/// A take of `ms` milliseconds of a square wave (period 40 samples) at `level`: takes of
/// different levels have different bytes, so different SHA-256.
fn take_of(level: i16, ms: u64) -> Take {
    let samples: Vec<i16> = (0..16 * ms as usize)
        .map(|i| if i % 40 < 20 { level } else { -level })
        .collect();
    Take {
        wav: wav16(&samples),
        duration_ms: ms,
    }
}

/// A one-second take.
fn take(level: i16) -> Take {
    take_of(level, 1_000)
}

// ---------------------------------------------------------------------------------------------
// The P6 fixtures.
// ---------------------------------------------------------------------------------------------

/// What the client's `transcribe` returns for a fixture (format in tests/fixtures/ai/P6/README.md).
fn p6(name: &str) -> Result<TranscriptReply, AiError> {
    let fixture = json_file(&format!("tests/fixtures/ai/P6/{name}"));
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
            Some("invalid") => AiError::Invalid,
            _ => panic!("{name}: unknown error {error}"),
        });
    }
    assert_eq!(fixture["status"].as_u64(), Some(200), "{name}: a reply");
    Ok(TranscriptReply {
        body: fixture["body"].clone(),
        charge_micros: fixture["charge_micros"].as_i64(),
    })
}

// ---------------------------------------------------------------------------------------------
// Fakes: the AI client, the microphone, the local transcriber, the speakers, the clock, the
// filesystem, the network and the random source.
// ---------------------------------------------------------------------------------------------

/// An `AiClient` whose `transcribe` answers from P6 fixtures in turn (offline when none is left)
/// and records every request; every other call is refused.
struct TranscribeClient {
    kind: ProviderKind,
    needs_credit: AtomicBool,
    replies: Mutex<VecDeque<Result<TranscriptReply, AiError>>>,
    sent: Mutex<Vec<TranscribeRequest>>,
}

impl TranscribeClient {
    fn of(kind: ProviderKind, fixtures: &[&str]) -> Arc<TranscribeClient> {
        Arc::new(TranscribeClient {
            kind,
            needs_credit: AtomicBool::new(false),
            replies: Mutex::new(fixtures.iter().map(|fixture| p6(fixture)).collect()),
            sent: Mutex::new(Vec::new()),
        })
    }

    /// A publik API client.
    fn replying(fixtures: &[&str]) -> Arc<TranscribeClient> {
        TranscribeClient::of(PUBLIK, fixtures)
    }

    fn sent(&self) -> Vec<TranscribeRequest> {
        self.sent.lock().unwrap().clone()
    }

    fn requests(&self) -> usize {
        self.sent.lock().unwrap().len()
    }
}

impl AiClient for TranscribeClient {
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

    fn transcribe(&self, req: TranscribeRequest) -> Result<TranscriptReply, AiError> {
        self.sent.lock().unwrap().push(req);
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(AiError::Offline))
    }

    fn speech(&self, _req: SpeechRequest) -> Result<AudioReply, AiError> {
        Err(AiError::Offline)
    }

    fn kind(&self) -> ProviderKind {
        self.kind
    }

    fn status(&self) -> AiStatus {
        AiStatus {
            provider: self.kind.as_str().to_string(),
            needs_credit: self.needs_credit.load(Ordering::SeqCst),
            ..AiStatus::default()
        }
    }

    fn chat_model(&self, tier: &str) -> String {
        tier.to_string()
    }
}

fn access(client: &Arc<TranscribeClient>) -> Option<AiAccess> {
    let client: Arc<dyn AiClient> = client.clone();
    Some(Ok(client))
}

/// A microphone that hands out scripted takes and records how it was used.
struct FakeCapture {
    device: bool,
    start_error: Mutex<Option<CaptureError>>,
    takes: Mutex<VecDeque<Result<Take, CaptureError>>>,
    /// The `max_ms` of every start.
    starts: Mutex<Vec<u32>>,
    recording: AtomicBool,
    /// Levels reported to `on_level` at each start.
    levels: Mutex<Vec<f32>>,
    on_limit: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl FakeCapture {
    fn shared(device: bool) -> Arc<FakeCapture> {
        Arc::new(FakeCapture {
            device,
            start_error: Mutex::new(None),
            takes: Mutex::new(VecDeque::new()),
            starts: Mutex::new(Vec::new()),
            recording: AtomicBool::new(false),
            levels: Mutex::new(Vec::new()),
            on_limit: Mutex::new(None),
        })
    }

    fn push(&self, take: Result<Take, CaptureError>) {
        self.takes.lock().unwrap().push_back(take);
    }

    fn refuse_start(&self, error: CaptureError) {
        *self.start_error.lock().unwrap() = Some(error);
    }

    fn starts(&self) -> Vec<u32> {
        self.starts.lock().unwrap().clone()
    }

    fn recording(&self) -> bool {
        self.recording.load(Ordering::SeqCst)
    }

    /// The capture reaches its limit: `on_limit` runs.
    fn reach_limit(&self) {
        let on_limit = self.on_limit.lock().unwrap().take();
        if let Some(on_limit) = on_limit {
            on_limit();
        }
    }
}

impl Capture for FakeCapture {
    fn has_device(&self) -> bool {
        self.device
    }

    fn start(
        &self,
        max_ms: u32,
        mut on_level: Box<dyn FnMut(f32) + Send>,
        on_limit: Box<dyn FnOnce() + Send>,
    ) -> Result<(), CaptureError> {
        self.starts.lock().unwrap().push(max_ms);
        if !self.device {
            return Err(CaptureError::NoDevice);
        }
        if let Some(error) = self.start_error.lock().unwrap().clone() {
            return Err(error);
        }
        for level in self.levels.lock().unwrap().iter() {
            on_level(*level);
        }
        *self.on_limit.lock().unwrap() = Some(on_limit);
        self.recording.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn stop(&self) -> Result<Take, CaptureError> {
        if !self.recording.swap(false, Ordering::SeqCst) {
            return Err(CaptureError::NotRecording);
        }
        self.takes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(CaptureError::TooQuiet))
    }
}

/// A local transcriber with scripted texts (HOLA_HEARD when none is left) that records every call.
struct FakeTranscriber {
    replies: Mutex<VecDeque<Result<String, String>>>,
    calls: Mutex<Vec<(PathBuf, Vec<u8>, Lang)>>,
}

impl FakeTranscriber {
    fn shared() -> Arc<FakeTranscriber> {
        Arc::new(FakeTranscriber {
            replies: Mutex::new(VecDeque::new()),
            calls: Mutex::new(Vec::new()),
        })
    }

    fn hears(&self, text: &str) {
        self.replies.lock().unwrap().push_back(Ok(text.to_string()));
    }

    fn fails(&self) {
        self.replies
            .lock()
            .unwrap()
            .push_back(Err("the model file is not a ggml file".to_string()));
    }

    fn calls(&self) -> Vec<(PathBuf, Vec<u8>, Lang)> {
        self.calls.lock().unwrap().clone()
    }
}

impl LocalTranscriber for FakeTranscriber {
    fn transcribe(&self, model: &Path, wav: &[u8], lang: Lang) -> Result<String, String> {
        self.calls
            .lock()
            .unwrap()
            .push((model.to_path_buf(), wav.to_vec(), lang));
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Ok(HOLA_HEARD.to_string()))
    }
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

/// No OS voice (so lessons have no `listen_type`); counts how often the OS voice is stopped.
struct FakeVoices {
    stops: AtomicUsize,
}

impl OsVoices for FakeVoices {
    fn list(&self) -> Vec<OsVoice> {
        Vec::new()
    }

    fn rates(&self) -> RateRange {
        RateRange {
            min: 0.5,
            normal: 1.0,
            max: 6.0,
        }
    }

    fn speak(&self, _text: &str, _voice: &str, _rate: f32) -> Result<(), AudioError> {
        Ok(())
    }

    fn stop(&self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
    }
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

/// Files under a directory; absolute paths as they are. Writes fail while `refuse` is set.
struct DirFs {
    root: PathBuf,
    refuse: Arc<AtomicBool>,
}

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
            self.root.join(path)
        }
    }
}

impl Fs for DirFs {
    fn read(&self, path: &Path) -> Result<Vec<u8>, FsError> {
        fs::read(self.full(path)).map_err(|e| fs_error(path, e))
    }
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), FsError> {
        if self.refuse.load(Ordering::SeqCst) {
            return Err(FsError::Io("the disk is full".to_string()));
        }
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

/// A network that answers from a script (refusing when none is left) and logs every request in
/// `events`, which the progress of a download writes to as well, so their order can be checked.
struct NetScript {
    replies: Mutex<VecDeque<Result<HttpResponse, NetError>>>,
    sent: Mutex<Vec<HttpRequest>>,
    events: Arc<Mutex<Vec<String>>>,
}

impl NetScript {
    fn shared() -> Arc<NetScript> {
        Arc::new(NetScript {
            replies: Mutex::new(VecDeque::new()),
            sent: Mutex::new(Vec::new()),
            events: Arc::new(Mutex::new(Vec::new())),
        })
    }

    fn push(&self, reply: Result<HttpResponse, NetError>) {
        self.replies.lock().unwrap().push_back(reply);
    }

    fn serve(&self, status: u16, body: &[u8]) {
        self.push(Ok(HttpResponse {
            status,
            headers: Vec::new(),
            body: body.to_vec(),
        }));
    }

    fn sent(&self) -> Vec<HttpRequest> {
        self.sent.lock().unwrap().clone()
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    /// A progress callback that logs into `events`.
    fn progress(&self) -> impl FnMut(ModelProgress) + use<> {
        let events = Arc::clone(&self.events);
        move |p: ModelProgress| {
            events.lock().unwrap().push(format!(
                "progress {} {} of {}",
                p.name, p.bytes_done, p.bytes_total
            ));
        }
    }
}

struct SharedNet(Arc<NetScript>);

impl Net for SharedNet {
    fn send(&self, req: HttpRequest) -> Result<HttpResponse, NetError> {
        self.0
            .events
            .lock()
            .unwrap()
            .push(format!("request {} {}", req.method, req.url));
        self.0.sent.lock().unwrap().push(req);
        self.0
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(NetError::Disabled))
    }
}

/// A random source that always draws the same value.
struct Constant(u64);

impl Rng for Constant {
    fn next_u64(&mut self) -> u64 {
        self.0
    }
}

// ---------------------------------------------------------------------------------------------
// The app.
// ---------------------------------------------------------------------------------------------

const PACKS_LOCK: &str = "schema = \"parley.packs/1\"\n";

/// A speak lesson: its session and its `speak` exercises.
struct SpeakLesson {
    session: String,
    hola: Exercise,
    exercises: Vec<Exercise>,
}

/// The service over a temporary data directory with the bundled Spanish course started, at 10:00,
/// with a fake microphone, a fake local transcriber (or none), the bundled model registry, a fake
/// player and no OS voice.
struct Rig {
    app: Parley,
    dir: PathBuf,
    now: Arc<AtomicI64>,
    capture: Arc<FakeCapture>,
    whisper: Arc<FakeTranscriber>,
    local_whisper: bool,
    player: Arc<FakePlayer>,
    voices: Arc<FakeVoices>,
    net: Arc<NetScript>,
    refuse_writes: Arc<AtomicBool>,
    _temp: tempfile::TempDir,
}

impl Rig {
    /// A microphone and local Whisper in the build.
    fn new() -> Rig {
        Rig::build(true, true)
    }

    fn build(device: bool, local_whisper: bool) -> Rig {
        let temp = tempfile::tempdir().expect("a temporary directory");
        let dir = temp.path().to_path_buf();
        let now = Arc::new(AtomicI64::new(TEN));
        let net = NetScript::shared();
        let refuse_writes = Arc::new(AtomicBool::new(false));
        let mut app = Parley::new(
            Config {
                data_dir: dir.clone(),
                courses: BTreeMap::from([(Lang::Es, spanish())]),
                packs_lock: PACKS_LOCK.to_string(),
            },
            Env {
                clock: Box::new(SharedClock(now.clone())),
                rng: Box::new(SplitMix64::new(10)),
                fs: Box::new(DirFs {
                    root: dir.clone(),
                    refuse: refuse_writes.clone(),
                }),
                net: Box::new(SharedNet(net.clone())),
            },
        );
        app.course_start("es")
            .unwrap_or_else(|e| panic!("the Spanish course starts: {e}"));
        let player = FakePlayer::shared(false);
        let voices = Arc::new(FakeVoices {
            stops: AtomicUsize::new(0),
        });
        let mut rig = Rig {
            app,
            dir,
            now,
            capture: FakeCapture::shared(device),
            whisper: FakeTranscriber::shared(),
            local_whisper,
            player,
            voices,
            net,
            refuse_writes,
            _temp: temp,
        };
        rig.set_audio();
        rig.set_models(bundled_models());
        rig
    }

    fn set_audio(&mut self) {
        let os: Arc<dyn OsVoices> = self.voices.clone();
        let player: Arc<dyn Player> = self.player.clone();
        self.app.set_audio(Audio {
            os,
            player,
            decoder: Arc::new(WavDecoder),
            natural: bundled_voices(),
        });
    }

    /// The speaking objects, with `models` as the registry.
    fn set_models(&mut self, models: ModelRegistry) {
        let capture: Arc<dyn Capture> = self.capture.clone();
        let whisper: Option<Arc<dyn LocalTranscriber>> = if self.local_whisper {
            let whisper: Arc<dyn LocalTranscriber> = self.whisper.clone();
            Some(whisper)
        } else {
            None
        };
        self.app.set_speaking(Speaking {
            capture,
            whisper,
            models,
        });
    }

    /// A player that fails every play.
    fn break_player(&mut self) {
        self.player = FakePlayer::shared(true);
        self.set_audio();
    }

    fn set(&mut self, patch: Value) {
        self.app
            .settings_set(&patch)
            .unwrap_or_else(|e| panic!("{patch} is saved: {e}"));
    }

    fn set_now(&self, ms: i64) {
        self.now.store(ms, Ordering::SeqCst);
    }

    /// publik API chosen (consent given: the tests pass its client).
    fn publik(&mut self) {
        self.set(json!({"ai": {"provider": "publik"}}));
    }

    /// A file at `models/whisper/ggml-<name>.bin`.
    fn download(&self, name: &str) {
        let path = self.dir.join(model_file(name));
        fs::create_dir_all(path.parent().expect("a folder")).expect("models/whisper is made");
        fs::write(path, b"a test model, not a real one").expect("the model file is written");
    }

    /// The spec's speak-ready set-up: the fake transcriber, transcriber = local and a file at
    /// models/whisper/ggml-base.bin.
    fn ready_local(&mut self) {
        self.set(json!({"speaking": {"transcriber": "local"}}));
        self.download("base");
    }

    /// One take recorded: mic_start with 15 s, mic_stop.
    fn record(&mut self, take: Take) -> Recording {
        self.capture.push(Ok(take));
        self.app
            .mic_start(15, Box::new(|_| {}), Box::new(|| {}))
            .unwrap_or_else(|e| panic!("mic_start: {e}"));
        self.app
            .mic_stop()
            .unwrap_or_else(|e| panic!("mic_stop: {e}"))
    }

    fn recording_path(&self, id: &str) -> PathBuf {
        self.dir
            .join("cache")
            .join("recordings")
            .join(format!("{id}.wav"))
    }

    /// The files in a folder of the data directory.
    fn files_in(&self, folder: &str) -> Vec<String> {
        let mut names: Vec<String> = match fs::read_dir(self.dir.join(folder)) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().is_file())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect(),
            Err(_) => Vec::new(),
        };
        names.sort();
        names
    }

    fn recordings(&self) -> Vec<String> {
        self.files_in("cache/recordings")
    }

    /// The first lesson of the skill with ten new items introduces its ten words; lessons after it
    /// introduce two items each.
    fn practise_vocabulary(&mut self) {
        self.set(json!({"new_items_per_lesson": 10}));
        finish_lesson(&mut self.app, SKILL);
        self.set(json!({"new_items_per_lesson": 2}));
    }

    fn next_lesson(&mut self) -> LessonStarted {
        self.app
            .lesson_start(SKILL, "learn")
            .unwrap_or_else(|e| panic!("a lesson of {SKILL} starts: {e}"))
    }

    /// The lesson whose new items are the skill's first two sentences, and its speak exercise for
    /// HOLA.
    fn speak_lesson(&mut self) -> SpeakLesson {
        self.practise_vocabulary();
        let lesson = self.next_lesson();
        let hola = lesson
            .exercises
            .iter()
            .find(|ex| matches!(&ex.body, Body::Speak { text, .. } if text == HOLA))
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "the lesson has a speak exercise for {HOLA}: {:?}",
                    kinds(&lesson)
                )
            });
        SpeakLesson {
            session: lesson.session_id.clone(),
            hola,
            exercises: lesson.exercises,
        }
    }

    fn check(
        &mut self,
        lesson: &SpeakLesson,
        recording: &str,
        local: bool,
        ai: Option<AiAccess>,
    ) -> Result<SpeakResult, AppError> {
        self.app
            .speak_check_with(&lesson.session, &lesson.hola.id, recording, local, ai)
    }

    fn checked(
        &mut self,
        lesson: &SpeakLesson,
        recording: &str,
        local: bool,
        ai: Option<AiAccess>,
    ) -> SpeakResult {
        self.check(lesson, recording, local, ai)
            .unwrap_or_else(|e| panic!("the speaking check: {e}"))
    }

    /// Every backed-up table, from a backup of the app's data.
    fn tables(&mut self) -> Value {
        static EXPORTS: AtomicUsize = AtomicUsize::new(0);
        let n = EXPORTS.fetch_add(1, Ordering::SeqCst);
        let path = self.dir.join(format!("backup-{n}.json"));
        self.app
            .backup_export(&path)
            .expect("the backup is written");
        serde_json::from_str(&fs::read_to_string(path).expect("the backup reads"))
            .expect("the backup is JSON")
    }

    fn reviews(&mut self) -> Vec<Value> {
        self.tables()["review_log"]
            .as_array()
            .expect("the backup has a review log")
            .clone()
    }

    fn session_cost(&mut self, session: &str) -> i64 {
        self.tables()["sessions"]
            .as_array()
            .expect("the backup has sessions")
            .iter()
            .find(|row| row["id"].as_str() == Some(session))
            .and_then(|row| row["cost_micros"].as_i64())
            .unwrap_or_else(|| panic!("session {session} has a cost"))
    }

    fn ai_cache_path(&self) -> PathBuf {
        self.dir.join("cache").join("ai.sqlite3")
    }

    /// The charges of `transcribe_charges`, sorted.
    fn charges(&self) -> Vec<i64> {
        let cache = Cache::open(
            &self.ai_cache_path(),
            Box::new(SharedClock(Arc::new(AtomicI64::new(TEN)))),
        )
        .expect("the AI cache opens");
        let mut charges = cache.transcribe_charges();
        charges.sort_unstable();
        charges
    }
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

fn kinds(lesson: &LessonStarted) -> Vec<ExerciseType> {
    lesson.exercises.iter().map(Exercise::kind).collect()
}

fn speak_count(lesson: &LessonStarted) -> usize {
    kinds(lesson)
        .iter()
        .filter(|kind| **kind == ExerciseType::Speak)
        .count()
}

/// The default settings with `patch` applied.
fn settings(patch: Value) -> Settings {
    apply_patch(&Settings::default(), &patch).unwrap_or_else(|e| panic!("{patch}: {e}"))
}

#[track_caller]
fn assert_error(error: Option<AppError>, code: ErrorCode, message: &str) {
    let error = error.expect("the command is refused");
    assert_eq!((error.code, error.message.as_str()), (code, message));
}

#[track_caller]
fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} where {expected} (within {tolerance}) was expected"
    );
}

fn word(expected: &str, heard: Option<&str>, status: WordStatus) -> WordResult {
    WordResult {
        expected: expected.to_string(),
        heard: heard.map(str::to_string),
        status,
    }
}

fn ok(expected: &str, heard: &str) -> WordResult {
    word(expected, Some(heard), WordStatus::Ok)
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|w| w.to_string()).collect()
}

/// A lowercase 8-4-4-4-12 hex id with version nibble 4 and variant bits 10.
fn is_v4_uuid(id: &str) -> bool {
    let groups: Vec<&str> = id.split('-').collect();
    let shaped = groups.iter().map(|g| g.len()).collect::<Vec<_>>() == [8, 4, 4, 4, 12];
    let hex = id
        .chars()
        .all(|c| c == '-' || c.is_ascii_digit() || ('a'..='f').contains(&c));
    shaped && hex && groups[2].starts_with('4') && groups[3].starts_with(['8', '9', 'a', 'b'])
}

// ---------------------------------------------------------------------------------------------
// Recording: mic_start and mic_stop (Interfaces, and Acceptance tests 3 and 12).
// ---------------------------------------------------------------------------------------------

/// Acceptance 3: "in the core, TooQuiet gives too_quiet and its message, and no file is written."
#[test]
fn t03_a_take_that_is_too_quiet_gives_too_quiet_and_writes_no_file() {
    let mut rig = Rig::new();
    rig.capture.push(Err(CaptureError::TooQuiet));
    rig.app
        .mic_start(15, Box::new(|_| {}), Box::new(|| {}))
        .unwrap_or_else(|e| panic!("mic_start: {e}"));
    assert_error(rig.app.mic_stop().err(), ErrorCode::TooQuiet, TOO_QUIET);
    assert!(rig.recordings().is_empty(), "no file is written");
}

/// Interfaces, mic_start: "max_s must be 15 or 30 ("Unknown value {v} for max_s.", invalid_input);
/// capture.start(max_s × 1000, …)".
#[test]
fn interfaces_mic_start_takes_15_or_30_seconds() {
    let mut rig = Rig::new();
    for max_s in [0, 20, 60] {
        assert_error(
            rig.app
                .mic_start(max_s, Box::new(|_| {}), Box::new(|| {}))
                .err(),
            ErrorCode::InvalidInput,
            &format!("Unknown value {max_s} for max_s."),
        );
    }
    assert!(rig.capture.starts().is_empty(), "nothing is opened");
    for max_s in [15, 30] {
        rig.app
            .mic_start(max_s, Box::new(|_| {}), Box::new(|| {}))
            .unwrap_or_else(|e| panic!("mic_start {max_s}: {e}"));
    }
    assert_eq!(rig.capture.starts(), [15_000, 30_000]);
}

/// Interfaces, mic_start: "Backend → no_microphone "Parley could not record from the microphone.
/// Try again."".
#[test]
fn interfaces_a_backend_error_is_no_microphone_with_try_again() {
    let mut rig = Rig::new();
    rig.capture.refuse_start(CaptureError::Backend(
        "the stream could not be built".into(),
    ));
    assert_error(
        rig.app
            .mic_start(15, Box::new(|_| {}), Box::new(|| {}))
            .err(),
        ErrorCode::NoMicrophone,
        MIC_FAILED,
    );
}

/// Interfaces, mic_stop: "NotRecording → invalid_input "Nothing is being recorded.""
#[test]
fn interfaces_mic_stop_with_nothing_recorded() {
    let mut rig = Rig::new();
    assert_error(
        rig.app.mic_stop().err(),
        ErrorCode::InvalidInput,
        NOTHING_RECORDED,
    );
}

/// Interfaces, mic_stop: the take is written to cache/recordings/<id>.wav, `Recording { id,
/// duration_ms }` comes back, and the id is a version-4 UUID.
#[test]
fn interfaces_mic_stop_writes_the_take_under_its_id() {
    let mut rig = Rig::new();
    let first = take_of(9_000, 2_500);
    let recording = rig.record(first.clone());
    assert!(is_v4_uuid(&recording.id), "{} is a v4 UUID", recording.id);
    assert_eq!(recording.duration_ms, 2_500);
    assert_eq!(
        fs::read(rig.recording_path(&recording.id)).ok(),
        Some(first.wav)
    );
    let second = rig.record(take(7_000));
    assert_ne!(second.id, recording.id, "each take has its own id");
    assert_eq!(rig.recordings().len(), 2);
}

/// Interfaces, mic_stop: "the id is a version-4 UUID (lowercase 8-4-4-4-12 hex) from two
/// Rng::next_u64 values (the first is the high half; version nibble 4, variant bits 10)".
/// By hand: 0x0123456789abcdef twice is 0123456789abcdef0123456789abcdef; hex digit 12 becomes 4
/// and digit 16 (0000) becomes 10 00 = 8. All ones give f...f with 4 and 10 11 = b.
#[test]
fn interfaces_the_recording_id_is_built_from_two_draws() {
    for (value, expected) in [
        (
            0x0123_4567_89ab_cdef_u64,
            "01234567-89ab-4def-8123-456789abcdef",
        ),
        (u64::MAX, "ffffffff-ffff-4fff-bfff-ffffffffffff"),
    ] {
        let temp = tempfile::tempdir().expect("a temporary directory");
        let mut app = Parley::new(
            Config {
                data_dir: temp.path().to_path_buf(),
                courses: BTreeMap::new(),
                packs_lock: PACKS_LOCK.to_string(),
            },
            Env {
                clock: Box::new(SharedClock(Arc::new(AtomicI64::new(TEN)))),
                rng: Box::new(Constant(value)),
                fs: Box::new(DirFs {
                    root: temp.path().to_path_buf(),
                    refuse: Arc::new(AtomicBool::new(false)),
                }),
                net: Box::new(SharedNet(NetScript::shared())),
            },
        );
        let capture = FakeCapture::shared(true);
        capture.push(Ok(take(8_000)));
        let shared: Arc<dyn Capture> = capture.clone();
        app.set_speaking(Speaking {
            capture: shared,
            whisper: None,
            models: bundled_models(),
        });
        app.mic_start(15, Box::new(|_| {}), Box::new(|| {}))
            .unwrap_or_else(|e| panic!("mic_start: {e}"));
        let recording = app.mic_stop().unwrap_or_else(|e| panic!("mic_stop: {e}"));
        assert_eq!(recording.id, expected);
        assert!(
            temp.path()
                .join("cache")
                .join("recordings")
                .join(format!("{expected}.wav"))
                .is_file()
        );
    }
}

/// Interfaces, mic_stop: "the WAV is written to cache/recordings/<id>.wav through Fs (a failure:
/// "Parley could not write that file.", storage)".
#[test]
fn interfaces_a_take_that_cannot_be_written_is_a_storage_error() {
    let mut rig = Rig::new();
    rig.capture.push(Ok(take(8_000)));
    rig.app
        .mic_start(15, Box::new(|_| {}), Box::new(|| {}))
        .unwrap_or_else(|e| panic!("mic_start: {e}"));
    rig.refuse_writes.store(true, Ordering::SeqCst);
    assert_error(rig.app.mic_stop().err(), ErrorCode::Storage, CANNOT_WRITE);
    rig.refuse_writes.store(false, Ordering::SeqCst);
    assert!(rig.recordings().is_empty());
}

/// Interfaces: "No microphone (the default): has_device false, start NoDevice, stop NotRecording";
/// `Speaking::default()` is NoCapture, no local Whisper and the bundled registry, and Parley::new
/// starts with it.
#[test]
fn interfaces_the_default_has_no_microphone_and_no_local_whisper() {
    assert!(!NoCapture.has_device());
    assert_eq!(
        NoCapture.start(15_000, Box::new(|_| {}), Box::new(|| {})),
        Err(CaptureError::NoDevice)
    );
    assert_eq!(NoCapture.stop(), Err(CaptureError::NotRecording));

    let speaking = Speaking::default();
    assert!(!speaking.capture.has_device());
    assert!(speaking.whisper.is_none());
    assert_eq!(speaking.models, bundled_models());

    let temp = tempfile::tempdir().expect("a temporary directory");
    let mut app = Parley::new(
        Config {
            data_dir: temp.path().to_path_buf(),
            courses: BTreeMap::from([(Lang::Es, spanish())]),
            packs_lock: PACKS_LOCK.to_string(),
        },
        Env {
            clock: Box::new(SharedClock(Arc::new(AtomicI64::new(TEN)))),
            rng: Box::new(SplitMix64::new(3)),
            fs: Box::new(DirFs {
                root: temp.path().to_path_buf(),
                refuse: Arc::new(AtomicBool::new(false)),
            }),
            net: Box::new(SharedNet(NetScript::shared())),
        },
    );
    app.course_start("es")
        .unwrap_or_else(|e| panic!("the course starts: {e}"));
    let status = app.speaking_status();
    assert!(!status.microphone);
    assert!(!status.local_available);
    assert!(!app.speak_available(Lang::Es));
    assert_error(
        app.mic_start(15, Box::new(|_| {}), Box::new(|| {})).err(),
        ErrorCode::NoMicrophone,
        NO_MICROPHONE,
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 5: scoring.
// ---------------------------------------------------------------------------------------------

/// Acceptance 5: "yo soy estudiante against heard yo es estudiante → ok, substituted (es), ok;
/// score 0.667 (tolerance 0.001); fail at 0.8."
#[test]
fn t05_a_substituted_word_scores_two_thirds_and_fails_at_0_8() {
    let es = numbers(Lang::Es);
    let scored = score(Lang::Es, "yo soy estudiante", "yo es estudiante", &es, 0.8);
    assert_eq!(
        scored.words,
        [
            ok("yo", "yo"),
            word("soy", Some("es"), WordStatus::Substituted),
            ok("estudiante", "estudiante"),
        ]
    );
    assert!(scored.extra.is_empty());
    close(scored.score, 2.0 / 3.0, 0.001);
    assert!(!scored.pass);
    assert!(
        score(Lang::Es, "yo soy estudiante", "yo es estudiante", &es, 0.6).pass,
        "2 of 3 passes at 0.6"
    );
}

/// Acceptance 5: "quiero un café con leche against quiero café con leche → missed un; 0.8; pass."
#[test]
fn t05_a_missed_word_scores_four_fifths_and_passes_at_the_mark() {
    let scored = score(
        Lang::Es,
        "quiero un café con leche",
        "quiero café con leche",
        &numbers(Lang::Es),
        0.8,
    );
    assert_eq!(
        scored.words,
        [
            ok("quiero", "quiero"),
            word("un", None, WordStatus::Missed),
            ok("café", "café"),
            ok("con", "con"),
            ok("leche", "leche"),
        ]
    );
    close(scored.score, 0.8, 1e-12);
    assert!(scored.pass, "4 of 5 passes at 0.8");
}

/// Acceptance 5: "buenos días against buenos días señor → 1.0, extra señor"; extra words keep their
/// order and never lower the score.
#[test]
fn t05_extra_words_are_listed_in_order_and_do_not_lower_the_score() {
    let es = numbers(Lang::Es);
    let scored = score(Lang::Es, "buenos días", "buenos días señor", &es, 0.8);
    assert_eq!(scored.words, [ok("buenos", "buenos"), ok("días", "días")]);
    assert_eq!(scored.extra, words(&["señor"]));
    close(scored.score, 1.0, 1e-12);
    assert!(scored.pass);

    let around = score(Lang::Es, "buenos días", "señor buenos días amigo", &es, 0.8);
    assert_eq!(around.extra, words(&["señor", "amigo"]));
    close(around.score, 1.0, 1e-12);
}

/// Acceptance 5: "tengo tres gatos against tengo 3 gatos → 1.0. días against dias → ok."
#[test]
fn t05_digits_are_spelled_and_accents_are_folded() {
    let es = numbers(Lang::Es);
    let digits = score(Lang::Es, "tengo tres gatos", "tengo 3 gatos", &es, 0.8);
    assert_eq!(
        digits.words,
        [
            ok("tengo", "tengo"),
            ok("tres", "tres"),
            ok("gatos", "gatos")
        ]
    );
    close(digits.score, 1.0, 1e-12);
    let folded = score(Lang::Es, "días", "dias", &es, 0.8);
    assert_eq!(folded.words, [ok("días", "dias")]);
    assert!(folded.pass);
}

/// Acceptance 5 (the golden table): an empty transcript misses every word (0.0); `[...]` spans are
/// removed from the heard text; the expected words keep the sentence's case and the heard words are
/// lower case.
#[test]
fn t05_empty_heard_brackets_and_case() {
    let es = numbers(Lang::Es);
    let empty = score(Lang::Es, "Buenos días.", "", &es, 0.8);
    assert_eq!(
        empty.words,
        [
            word("Buenos", None, WordStatus::Missed),
            word("días", None, WordStatus::Missed),
        ]
    );
    close(empty.score, 0.0, 1e-12);
    assert!(!empty.pass);

    let bracket = score(Lang::Es, "hola", "[BLANK_AUDIO] hola", &es, 0.8);
    assert_eq!(bracket.words, [ok("hola", "hola")]);
    assert!(bracket.extra.is_empty(), "the span is not an extra word");

    let case = score(Lang::Es, "Buenos días.", "BUENOS DÍAS", &es, 0.8);
    assert_eq!(case.words, [ok("Buenos", "buenos"), ok("días", "días")]);

    // No expected words: 0.0, and no pass at 0.8.
    let none = score(Lang::Es, "", "hola", &es, 0.8);
    assert!(none.words.is_empty());
    close(none.score, 0.0, 1e-12);
    assert!(!none.pass);
}

/// Acceptance 5 (the golden table): French hyphens split and elisions stay whole; German folds ß to
/// ss; a number above 100 stays as written.
#[test]
fn t05_french_and_german_cases_of_the_golden_table() {
    let fr = numbers(Lang::Fr);
    let elision = score(
        Lang::Fr,
        "Est-ce que l'eau est froide ?",
        "est ce que l'eau est froide",
        &fr,
        0.8,
    );
    assert_eq!(
        elision.words,
        [
            ok("Est", "est"),
            ok("ce", "ce"),
            ok("que", "que"),
            ok("l'eau", "l'eau"),
            ok("est", "est"),
            ok("froide", "froide"),
        ]
    );
    close(elision.score, 1.0, 1e-12);
    let seventy = score(Lang::Fr, "J'ai soixante-dix ans.", "J'ai 70 ans.", &fr, 0.8);
    assert_eq!(
        seventy.words,
        [
            ok("J'ai", "j'ai"),
            ok("soixante", "soixante"),
            ok("dix", "dix"),
            ok("ans", "ans"),
        ]
    );

    let de = numbers(Lang::De);
    let eszett = score(
        Lang::De,
        "Die Straße ist lang.",
        "die Strasse ist lang",
        &de,
        0.8,
    );
    assert_eq!(
        eszett.words,
        [
            ok("Die", "die"),
            ok("Straße", "strasse"),
            ok("ist", "ist"),
            ok("lang", "lang"),
        ]
    );
    let beyond = score(
        Lang::De,
        "Das kostet hundertzehn Euro.",
        "Das kostet 110 Euro.",
        &de,
        0.8,
    );
    assert_eq!(
        beyond.words,
        [
            ok("Das", "das"),
            ok("kostet", "kostet"),
            word("hundertzehn", Some("110"), WordStatus::Substituted),
            ok("Euro", "euro"),
        ]
    );
    close(beyond.score, 0.75, 1e-12);
    assert!(!beyond.pass);
}

/// Acceptance 5, Interfaces: "pass = score ≥ pass − 1e-9": 3 of 10 words is 0.3, which passes a mark
/// of 0.1 + 0.2 (0.30000000000000004 in binary) only with the allowance; 0.31 fails.
#[test]
fn t05_the_pass_mark_has_a_one_billionth_allowance() {
    let es = numbers(Lang::Es);
    let expected = "uno dos tres cuatro cinco seis siete ocho nueve diez";
    let mark = 0.1 + 0.2;
    assert!(mark > 0.3, "the mark is a hair above 0.3");
    let scored = score(Lang::Es, expected, "uno dos tres", &es, mark);
    close(scored.score, 0.3, 1e-12);
    assert!(scored.pass);
    assert!(!score(Lang::Es, expected, "uno dos tres", &es, 0.31).pass);
}

/// Acceptance 5, Design notes, words for scoring: module 03's normaliser steps, a hyphen between
/// letters splits, an apostrophe stays inside its word, ASCII digits 0 to 100 become the words of
/// numbers.yaml (split the same way, lower case), and larger numbers and words mixing digits and
/// letters stay as written.
#[test]
fn t05_speech_words_follow_the_design_notes() {
    let es = numbers(Lang::Es);
    let fr = numbers(Lang::Fr);
    let de = numbers(Lang::De);
    assert_eq!(
        speech_words(Lang::Es, "Yo soy estudiante.", &es, true),
        words(&["Yo", "soy", "estudiante"])
    );
    assert_eq!(
        speech_words(Lang::Es, "Yo soy estudiante.", &es, false),
        words(&["yo", "soy", "estudiante"])
    );
    assert_eq!(
        speech_words(Lang::Es, "¡Hola, ¿qué tal?", &es, true),
        words(&["Hola", "qué", "tal"])
    );
    assert_eq!(
        speech_words(Lang::Es, "Tengo 3 gatos", &es, true),
        words(&["Tengo", "tres", "gatos"])
    );
    assert_eq!(
        speech_words(Lang::Es, "Ana tiene 31 años y 100 euros", &es, false),
        words(&[
            "ana", "tiene", "treinta", "y", "uno", "años", "y", "cien", "euros"
        ])
    );
    assert_eq!(
        speech_words(Lang::Es, "Son 101 y el 3a", &es, false),
        words(&["son", "101", "y", "el", "3a"])
    );
    assert_eq!(
        speech_words(Lang::Es, "buenos\u{a0}días", &es, false),
        words(&["buenos", "días"])
    );
    assert_eq!(
        speech_words(Lang::Es, "cafe\u{301}", &es, false),
        words(&["café"]),
        "NFC"
    );
    assert!(speech_words(Lang::Es, " ¿? ", &es, false).is_empty());

    assert_eq!(
        speech_words(Lang::Fr, "Est-ce que l'eau est froide ?", &fr, true),
        words(&["Est", "ce", "que", "l'eau", "est", "froide"])
    );
    assert_eq!(
        speech_words(Lang::Fr, "J\u{2019}ai 70 ans, aujourd'hui.", &fr, false),
        words(&["j'ai", "soixante", "dix", "ans", "aujourd'hui"])
    );
    assert_eq!(
        speech_words(Lang::Fr, "-oui-", &fr, false),
        words(&["oui"]),
        "a hyphen not between letters"
    );
    assert_eq!(
        speech_words(Lang::De, "Das kostet 110 Euro.", &de, true),
        words(&["Das", "kostet", "110", "Euro"])
    );
    assert_eq!(
        speech_words(Lang::De, "Ich bin 21.", &de, true),
        words(&["Ich", "bin", "einundzwanzig"])
    );
    // Without a numbers file, digits stay as written.
    assert_eq!(
        speech_words(Lang::Es, "Tengo 3 gatos", &Numbers::default(), false),
        words(&["tengo", "3", "gatos"])
    );
}

/// Acceptance 5, Interfaces: "The comparison key of one word: marks folded (F2 of module 03), in
/// German also ß → ss." German umlauts fold as marks (F2), not to ae, oe, ue.
#[test]
fn t05_word_key_folds_marks_and_in_german_eszett() {
    assert_eq!(word_key(Lang::Es, "días"), "dias");
    assert_eq!(word_key(Lang::Es, "dias"), "dias");
    assert_eq!(word_key(Lang::Fr, "élève"), "eleve");
    assert_eq!(word_key(Lang::De, "straße"), "strasse");
    assert_eq!(word_key(Lang::De, "mädchen"), "madchen");
    assert_eq!(word_key(Lang::Es, "señor"), "senor");
}

/// Acceptance 5, Interfaces: Wagner and Fischer (1974) over word keys with unit costs; the edit
/// script from the start; on equal cost the diagonal, then Delete, then Insert. Each script was
/// worked by hand from the distance table.
#[test]
fn t05_align_gives_the_wagner_fischer_script_with_the_stated_ties() {
    let w = |list: &[&str]| words(list);
    use Edit::{Delete, Insert, Match, Substitute};
    assert_eq!(
        align(
            &w(&["yo", "soy", "estudiante"]),
            &w(&["yo", "es", "estudiante"])
        ),
        [Match(0, 0), Substitute(1, 1), Match(2, 2)]
    );
    assert_eq!(
        align(
            &w(&["quiero", "un", "cafe", "con", "leche"]),
            &w(&["quiero", "cafe", "con", "leche"])
        ),
        [
            Match(0, 0),
            Delete(1),
            Match(2, 1),
            Match(3, 2),
            Match(4, 3)
        ]
    );
    assert_eq!(
        align(&w(&["buenos", "dias"]), &w(&["buenos", "dias", "senor"])),
        [Match(0, 0), Match(1, 1), Insert(2)]
    );
    // D(2,1) = 2 by the diagonal or by Delete: the diagonal first.
    assert_eq!(
        align(&w(&["a", "b"]), &w(&["c"])),
        [Delete(0), Substitute(1, 0)]
    );
    // D(1,2) = 2 by the diagonal or by Insert: the diagonal first.
    assert_eq!(
        align(&w(&["hola"]), &w(&["adios", "amigo"])),
        [Insert(0), Substitute(0, 1)]
    );
    // D(3,3) = 2 by Delete or by Insert, 3 by the diagonal: Delete first.
    assert_eq!(
        align(&w(&["a", "b", "a"]), &w(&["b", "a", "b"])),
        [Insert(0), Match(0, 1), Match(1, 2), Delete(2)]
    );
    assert_eq!(
        align(&w(&["a", "b"]), &w(&["b", "a"])),
        [Substitute(0, 0), Substitute(1, 1)]
    );
    assert_eq!(align(&w(&[]), &w(&["a", "b"])), [Insert(0), Insert(1)]);
    assert_eq!(align(&w(&["a", "b"]), &w(&[])), [Delete(0), Delete(1)]);
    assert!(align(&w(&[]), &w(&[])).is_empty());
}

/// Acceptance 5, Interfaces: Numbers holds the 101 entries of numbers.yaml as written, entry n
/// being the number n; the same checks as load_numbers.
#[test]
fn t05_numbers_spell_the_entries_of_numbers_yaml() {
    let es = numbers(Lang::Es);
    assert_eq!(es.words.len(), 101);
    assert_eq!(es.spell(0), Some("cero"));
    assert_eq!(es.spell(3), Some("tres"));
    assert_eq!(es.spell(31), Some("treinta y uno"));
    assert_eq!(es.spell(100), Some("cien"));
    assert_eq!(es.spell(101), None);
    assert_eq!(numbers(Lang::Fr).spell(70), Some("soixante-dix"));
    assert_eq!(numbers(Lang::De).spell(21), Some("einundzwanzig"));
    assert_eq!(Numbers::default().spell(0), None);

    let mut short = String::from("schema: parley.numbers/1\nlang: es\nwords:\n");
    for n in 0..100 {
        short.push_str(&format!("  - n{n}\n"));
    }
    assert!(load_number_words(&short).is_err(), "100 entries");
    let wrong = read("content/es/numbers.yaml").replace("parley.numbers/1", "parley.numbers/9");
    assert!(load_number_words(&wrong).is_err(), "another schema");
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 6: ratings.
// ---------------------------------------------------------------------------------------------

/// Acceptance 6: "speak_rating 1.0 → Good; 0.6 → Hard; 0.4 → Again (pass mark 0.8)"; the verdicts
/// follow the same bands.
#[test]
fn t06_ratings_and_verdicts_follow_the_score() {
    assert_eq!(speak_rating(1.0, true), Rating::Good);
    assert_eq!(speak_rating(0.6, false), Rating::Hard);
    assert_eq!(speak_rating(0.5, false), Rating::Hard);
    assert_eq!(speak_rating(0.4, false), Rating::Again);
    assert_eq!(speak_rating(0.0, false), Rating::Again);
    assert_eq!(speak_verdict(1.0, true), Verdict::Correct);
    assert_eq!(speak_verdict(0.6, false), Verdict::Almost);
    assert_eq!(speak_verdict(0.5, false), Verdict::Almost);
    assert_eq!(speak_verdict(0.4, false), Verdict::Wrong);
}

/// Acceptance 6, Interfaces: "speak_grade: speak_verdict, no tags, no message, corrected the
/// sentence unless passed, words the expected words (display form) not ok, rating speak_rating."
#[test]
fn t06_speak_grade_names_the_words_that_were_not_right() {
    let es = numbers(Lang::Es);
    let scored = score(Lang::Es, HOLA, "hola que", &es, 0.8);
    let grade = speak_grade(HOLA, &scored);
    assert_eq!(grade.verdict, Verdict::Almost);
    assert!(grade.tags.is_empty());
    assert_eq!(grade.message, None);
    assert_eq!(grade.corrected.as_deref(), Some(HOLA));
    assert_eq!(grade.words, words(&["tal"]));
    assert_eq!(grade.rating, Some(Rating::Hard));

    let passed = speak_grade(HOLA, &score(Lang::Es, HOLA, "hola que tal", &es, 0.8));
    assert_eq!(passed.verdict, Verdict::Correct);
    assert_eq!(passed.corrected, None);
    assert!(passed.words.is_empty());
    assert_eq!(passed.rating, Some(Rating::Good));

    let wrong = speak_grade(BUENOS, &score(Lang::Es, BUENOS, "", &es, 0.8));
    assert_eq!(wrong.verdict, Verdict::Wrong);
    assert_eq!(wrong.words, words(&["Buenos", "días"]));
    assert_eq!(wrong.rating, Some(Rating::Again));
}

/// Acceptance 6: "a check scoring 0.667 appends one review_log row for the productive card with
/// rating 2, verdict almost and exercise type speak" (no hint, no response time).
#[test]
fn t06_a_check_scoring_two_thirds_appends_one_review_of_the_productive_card() {
    let mut rig = Rig::new();
    rig.ready_local();
    let lesson = rig.speak_lesson();
    let before = rig.reviews().len();
    let recording = rig.record(take(8_000));
    rig.whisper.hears("hola que");
    let result = rig.checked(&lesson, &recording.id, false, None);
    close(result.scored.score, 2.0 / 3.0, 0.001);
    assert!(!result.scored.pass);
    assert!(result.rated);
    assert_eq!(result.heard, "hola que");
    assert_eq!(result.grade.verdict, Verdict::Almost);
    assert_eq!(result.grade.rating, Some(Rating::Hard));
    assert_eq!(result.grade.words, words(&["tal"]));

    let reviews = rig.reviews();
    assert_eq!(reviews.len(), before + 1, "one row");
    let row = &reviews[reviews.len() - 1];
    assert_eq!(row["item_id"].as_str(), Some(lesson.hola.item_id.as_str()));
    assert_eq!(row["facet"], "productive");
    assert_eq!(row["rating"], 2);
    assert_eq!(row["verdict"], "almost");
    assert_eq!(row["exercise_type"], "speak");
    assert_eq!(row["response_ms"], Value::Null);
    assert_eq!(row["session_id"].as_str(), Some(lesson.session.as_str()));

    // The result serialises with the score's fields flattened beside the others.
    let shown = serde_json::to_value(&result).expect("a SpeakResult serialises");
    let mut keys: Vec<&str> = shown
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "cached",
            "extra",
            "grade",
            "heard",
            "notice",
            "pass",
            "rated",
            "score",
            "transcriber",
            "words"
        ]
    );
    assert_eq!(shown["transcriber"], "local");
    assert_eq!(shown["words"][2]["status"], "missed");
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 7: the P6 request.
// ---------------------------------------------------------------------------------------------

/// Acceptance 7: "in the core, transcribe_request and fields() give exactly those values, timeout 30
/// and the idempotency key of the key JSON."
#[test]
fn t07_transcribe_request_gives_the_section_6_8_fields() {
    assert_eq!(PUBLIK_TRANSCRIBE_MODEL, "gpt-4o-mini-transcribe");
    assert_eq!(TRANSCRIBE_TIMEOUT_S, 30);
    let wav = take_of(16_000, 15_000).wav;
    assert_eq!(wav.len(), 480_044, "a 15 s take");
    let request = transcribe_request(PUBLIK, PUBLIK_TRANSCRIBE_MODEL, &wav, Lang::Es, 1);
    assert_eq!(request.model, PUBLIK_TRANSCRIBE_MODEL);
    assert!(request.audio == wav, "the take's bytes");
    assert_eq!(request.file_name, "take.wav");
    assert_eq!(request.mime, "audio/wav");
    assert_eq!(request.language, "es");
    assert_eq!(request.response_format, "json");
    assert_eq!(request.temperature, 0.0);
    assert_eq!(request.timeout_s, 30);
    let key_json =
        transcribe_key_json(PUBLIK, PUBLIK_TRANSCRIBE_MODEL, &sha256_hex(&wav), Lang::Es);
    assert_eq!(request.idempotency_key, Some(idempotency_key(&key_json, 1)));
    assert!(request.audio.len() < MAX_REQUEST_BYTES);
    assert_eq!(
        request.fields(),
        [
            ("model", PUBLIK_TRANSCRIBE_MODEL.to_string()),
            ("language", "es".to_string()),
            ("response_format", "json".to_string()),
            ("temperature", "0".to_string()),
        ]
    );
    assert!(
        request.fields().iter().all(|(name, _)| *name != "prompt"),
        "the expected sentence is never sent"
    );

    let second = transcribe_request(PUBLIK, PUBLIK_TRANSCRIBE_MODEL, &wav, Lang::Fr, 2);
    assert_eq!(second.language, "fr");
    let key_json =
        transcribe_key_json(PUBLIK, PUBLIK_TRANSCRIBE_MODEL, &sha256_hex(&wav), Lang::Fr);
    assert_eq!(second.idempotency_key, Some(idempotency_key(&key_json, 2)));

    // Only a whole temperature is written as an integer.
    let warm = TranscribeRequest {
        temperature: 0.5,
        ..request
    };
    assert_eq!(warm.fields()[3], ("temperature", "0.5".to_string()));
}

/// Acceptance 7, Interfaces: "Canonical key JSON (§6.1) with input {"audio_sha256", "language"},
/// and its SHA-256"; the feature is transcribe, version 1. The key changes with the provider, the
/// model, the take and the language.
#[test]
fn t07_the_transcribe_key_is_that_of_section_6_1() {
    assert_eq!(TRANSCRIBE_FEATURE, "transcribe");
    let sha = sha256_hex(&take(8_000).wav);
    let key_json = transcribe_key_json(PUBLIK, PUBLIK_TRANSCRIBE_MODEL, &sha, Lang::Es);
    assert_eq!(
        key_json,
        cache_key_json(
            "transcribe",
            1,
            PUBLIK,
            PUBLIK_TRANSCRIBE_MODEL,
            &json!({"audio_sha256": sha, "language": "es"})
        )
    );
    let key = transcribe_key(PUBLIK, PUBLIK_TRANSCRIBE_MODEL, &sha, Lang::Es);
    assert_eq!(key, cache_key(&key_json));
    let other = sha256_hex(&take(7_000).wav);
    for different in [
        transcribe_key(
            ProviderKind::OwnKey,
            PUBLIK_TRANSCRIBE_MODEL,
            &sha,
            Lang::Es,
        ),
        transcribe_key(PUBLIK, "whisper-1", &sha, Lang::Es),
        transcribe_key(PUBLIK, PUBLIK_TRANSCRIBE_MODEL, &other, Lang::Es),
        transcribe_key(PUBLIK, PUBLIK_TRANSCRIBE_MODEL, &sha, Lang::De),
        transcribe_key(ProviderKind::Local, &local_model_id("base"), &sha, Lang::Es),
    ] {
        assert_ne!(different, key);
    }
}

/// Acceptance 7, through the service: the provider job's request is `transcribe_request` of the
/// take with attempt 1, under 4,000,000 bytes, and it is the one request the client receives.
#[test]
fn t07_a_provider_check_sends_the_take_as_section_6_8_says() {
    let mut rig = Rig::new();
    rig.publik();
    let lesson = rig.speak_lesson();
    let fifteen = take_of(16_000, 15_000);
    let recording = rig.record(fifteen.clone());
    let client = TranscribeClient::replying(&["valid.json"]);
    let job = match rig.app.speak_begin(
        &lesson.session,
        &lesson.hola.id,
        &recording.id,
        false,
        access(&client),
    ) {
        Ok(SpeakBegin::Pending(job)) => job,
        Ok(SpeakBegin::Done(_)) => panic!("a new take is transcribed"),
        Err(e) => panic!("speak_begin: {e}"),
    };
    let request = job
        .request()
        .cloned()
        .expect("a provider job has its request");
    assert_eq!(
        request,
        transcribe_request(PUBLIK, PUBLIK_TRANSCRIBE_MODEL, &fifteen.wav, Lang::Es, 1)
    );
    assert_eq!(request.audio.len(), 480_044);
    assert!(request.audio.len() < MAX_REQUEST_BYTES);
    assert_eq!(client.requests(), 0, "begin makes no call");
    let outcome = job.run();
    let result = rig
        .app
        .speak_complete(job, outcome)
        .unwrap_or_else(|e| panic!("speak_complete: {e}"));
    assert_eq!(client.sent(), [request]);
    assert!(matches!(result.transcriber, Transcriber::Provider));
    assert!(!result.cached);
    assert_eq!(result.heard, "Yo es estudiante.");
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 8: transcripts held in memory (I5).
// ---------------------------------------------------------------------------------------------

/// Acceptance 8: "The same take bytes and language again while the app runs (the same recording, or
/// a second recording with identical bytes) → no request, cached: true; ... nothing about the take
/// is written to cache/ai.sqlite3: it has no ai_cache, ai_attempts or audio_cache row, and its
/// bytes contain neither the take's SHA-256 nor the transcript".
#[test]
fn t08_the_same_take_again_is_answered_from_memory_and_never_written_to_the_ai_cache() {
    let mut rig = Rig::new();
    rig.publik();
    let lesson = rig.speak_lesson();
    let same = take(12_345);
    let first = rig.record(same.clone());
    let client = TranscribeClient::replying(&["valid.json"]);
    let checked = rig.checked(&lesson, &first.id, false, access(&client));
    assert!(!checked.cached);
    assert_eq!(client.requests(), 1);

    let again = rig.checked(&lesson, &first.id, false, access(&client));
    assert!(again.cached, "the same recording");
    assert_eq!(again.heard, checked.heard);
    let twin = rig.record(same.clone());
    assert_ne!(twin.id, first.id);
    let from_twin = rig.checked(&lesson, &twin.id, false, access(&client));
    assert!(from_twin.cached, "a second recording with identical bytes");
    assert_eq!(
        client.requests(),
        1,
        "no request for a held transcript (I5)"
    );

    // Another take is another key.
    let other = rig.record(take(4_321));
    let client = TranscribeClient::replying(&["valid.json"]);
    let fresh = rig.checked(&lesson, &other.id, false, access(&client));
    assert!(!fresh.cached);
    assert_eq!(client.requests(), 1);

    let database = rig.ai_cache_path();
    assert!(
        database.is_file(),
        "the charges are kept in cache/ai.sqlite3"
    );
    {
        let conn = rusqlite::Connection::open(&database).expect("cache/ai.sqlite3 opens");
        for table in ["ai_cache", "ai_attempts", "audio_cache"] {
            let rows: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap_or(0);
            assert_eq!(rows, 0, "{table} has no row");
        }
        let mut columns: Vec<String> = conn
            .prepare("PRAGMA table_info(transcribe_charges)")
            .expect("transcribe_charges exists")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("its columns")
            .map(|column| column.expect("a column name"))
            .collect();
        columns.sort();
        assert_eq!(columns, ["charge_micros", "id"], "an amount alone");
    }
    let bytes = fs::read(database).expect("cache/ai.sqlite3 reads");
    let sha = sha256_hex(&same.wav);
    assert!(!contains(&bytes, sha.as_bytes()), "the take's SHA-256");
    assert!(
        !contains(&bytes, &Sha256::digest(&same.wav)),
        "the take's SHA-256 as bytes"
    );
    assert!(!contains(&bytes, VALID_TEXT.as_bytes()), "the transcript");
    let key = transcribe_key(PUBLIK, PUBLIK_TRANSCRIBE_MODEL, &sha, Lang::Es);
    assert!(!contains(&bytes, key.as_bytes()), "the transcript's key");
    assert_eq!(rig.charges(), [VALID_CHARGE, VALID_CHARGE]);
}

/// Acceptance 8, Design notes: transcripts are "keyed by provider, model, the take's SHA-256 and
/// language", so a take checked locally is still sent to the provider when the provider is asked.
#[test]
fn t08_a_local_transcript_does_not_answer_a_provider_check() {
    let mut rig = Rig::new();
    rig.publik();
    rig.download("base");
    let lesson = rig.speak_lesson();
    let recording = rig.record(take(8_000));
    let local = rig.checked(&lesson, &recording.id, true, None);
    assert!(matches!(local.transcriber, Transcriber::Local));
    let client = TranscribeClient::replying(&["valid.json"]);
    let provider = rig.checked(&lesson, &recording.id, false, access(&client));
    assert!(!provider.cached);
    assert!(matches!(provider.transcriber, Transcriber::Provider));
    assert_eq!(client.requests(), 1);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 9: an empty transcript.
// ---------------------------------------------------------------------------------------------

/// Acceptance 9: "An empty transcript (and [BLANK_AUDIO]) → the too_quiet message; the card is not
/// rated (no review_log row)". The reply was charged, so its charge is recorded.
#[test]
fn t09_an_empty_or_blank_transcript_is_too_quiet_and_rates_nothing() {
    let mut rig = Rig::new();
    rig.publik();
    let lesson = rig.speak_lesson();
    let before = rig.reviews().len();
    for (fixture, level) in [("empty.json", 9_001), ("blank-audio.json", 9_002)] {
        let recording = rig.record(take(level));
        let client = TranscribeClient::replying(&[fixture]);
        assert_error(
            rig.check(&lesson, &recording.id, false, access(&client))
                .err(),
            ErrorCode::TooQuiet,
            TOO_QUIET,
        );
        assert_eq!(client.requests(), 1, "{fixture}");
    }
    assert_eq!(rig.reviews().len(), before, "the card is not rated");
    assert_eq!(rig.charges(), [520, 530]);

    // The local transcriber hearing nothing, or only a span, is the same.
    rig.download("base");
    for heard in ["", "[BLANK_AUDIO]", "  [MUSIC]  "] {
        let recording = rig.record(take(9_100 + heard.len() as i16));
        rig.whisper.hears(heard);
        assert_error(
            rig.check(&lesson, &recording.id, true, None).err(),
            ErrorCode::TooQuiet,
            TOO_QUIET,
        );
    }
    assert_eq!(rig.reviews().len(), before);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 10: no local model.
// ---------------------------------------------------------------------------------------------

/// Acceptance 10: "transcriber = local with no model (local Whisper in the build) → the
/// not-downloaded message with the base size from the registry: "… Download it in Settings >
/// Speaking (148.0 MB)."; the local transcriber is not called."
#[test]
fn t10_local_without_a_model_names_its_size_and_transcribes_nothing() {
    let mut rig = Rig::new();
    rig.set(json!({"speaking": {"transcriber": "local"}}));
    assert!(
        rig.app.speak_available(Lang::Es),
        "local Whisper is in the build"
    );
    let lesson = rig.speak_lesson();
    let recording = rig.record(take(8_000));
    let error = rig
        .check(&lesson, &recording.id, false, None)
        .expect_err("no model is downloaded");
    assert_eq!(error.code, ErrorCode::NoProvider);
    assert_eq!(error.message, BASE_NOT_DOWNLOADED);
    assert_eq!(error.action, Some(json!({"model": "base"})));
    assert!(rig.whisper.calls().is_empty());

    // The chosen model's size: tiny is 77.7 MB.
    rig.set(json!({"speaking": {"whisper_model": "tiny"}}));
    let error = rig
        .check(&lesson, &recording.id, false, None)
        .expect_err("no model is downloaded");
    assert_eq!(
        error.message,
        "The local speech model is not downloaded. Download it in Settings > Speaking (77.7 MB)."
    );
    assert_eq!(error.action, Some(json!({"model": "tiny"})));
    assert!(rig.whisper.calls().is_empty());
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 11: downloading a model.
// ---------------------------------------------------------------------------------------------

const TEST_MODEL: &[u8] = b"a ggml model for tests, written by hand\n";
const TEST_URL: &str = "https://example.invalid/whisper/ggml-tiny.bin";

/// A registry with one model whose bytes are TEST_MODEL.
fn test_registry() -> ModelRegistry {
    ModelRegistry {
        whisper: vec![ModelEntry {
            name: "tiny".to_string(),
            url: TEST_URL.to_string(),
            bytes: TEST_MODEL.len() as u64,
            sha256: sha256_hex(TEST_MODEL),
        }],
    }
}

/// Acceptance 11: "whisper_download shows the size first (the first progress is {0, bytes} and comes
/// before the request ...); with a test registry, a served file with one changed byte → "The
/// download was damaged. Try again." and no file left in models/whisper/; the right bytes →
/// ggml-<name>.bin and downloaded: true."
#[test]
fn t11_a_download_shows_its_size_first_and_keeps_only_a_verified_file() {
    let mut rig = Rig::new();
    rig.set_models(test_registry());
    let total = TEST_MODEL.len() as u64;
    let mut damaged = TEST_MODEL.to_vec();
    damaged[7] ^= 0x01;

    rig.net.serve(200, &damaged);
    let mut progress = rig.net.progress();
    assert_error(
        rig.app.whisper_download_with("tiny", &mut progress).err(),
        ErrorCode::ChecksumMismatch,
        DAMAGED,
    );
    assert_eq!(
        rig.net.events(),
        [
            format!("progress tiny 0 of {total}"),
            format!("request GET {TEST_URL}"),
            format!("progress tiny {total} of {total}"),
        ],
        "the size comes before the request"
    );
    assert!(
        rig.files_in(WHISPER_DIR).is_empty(),
        "no file is left in models/whisper/"
    );
    assert!(!rig.app.whisper_models()[0].downloaded);

    rig.net.serve(200, TEST_MODEL);
    let mut progress = rig.net.progress();
    let list = rig
        .app
        .whisper_download_with("tiny", &mut progress)
        .unwrap_or_else(|e| panic!("the right bytes download: {e}"));
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "tiny");
    assert!(list[0].downloaded);
    assert_eq!(rig.files_in(WHISPER_DIR), ["ggml-tiny.bin"]);
    assert_eq!(
        fs::read(rig.dir.join(model_file("tiny"))).ok(),
        Some(TEST_MODEL.to_vec())
    );

    // Downloaded: Done with the list, and no request.
    let sent = rig.net.sent().len();
    match rig.app.whisper_download_begin("tiny") {
        Ok(DownloadBegin::Done(list)) => assert!(list[0].downloaded),
        Ok(DownloadBegin::Pending(_)) => panic!("a downloaded model is not fetched again"),
        Err(e) => panic!("whisper_download_begin: {e}"),
    }
    assert_eq!(rig.net.sent().len(), sent);
}

/// Acceptance 11, Interfaces: ModelDownload reports {0, bytes} before the request and {bytes,
/// bytes} after the body arrives; a model being downloaded is busy; a transport error and a
/// status other than 200 have their messages; an unknown name is refused.
#[test]
fn t11_download_jobs_and_their_errors() {
    let mut rig = Rig::new();
    rig.set_models(test_registry());
    let total = TEST_MODEL.len() as u64;
    assert_error(
        rig.app.whisper_download_begin("huge").err(),
        ErrorCode::InvalidInput,
        "Unknown value huge for name.",
    );

    let job = match rig.app.whisper_download_begin("tiny") {
        Ok(DownloadBegin::Pending(job)) => job,
        Ok(DownloadBegin::Done(_)) => panic!("nothing is downloaded yet"),
        Err(e) => panic!("whisper_download_begin: {e}"),
    };
    assert_eq!(job.name(), "tiny");
    assert_error(
        rig.app.whisper_download_begin("tiny").err(),
        ErrorCode::Busy,
        DOWNLOAD_BUSY,
    );
    let script = NetScript::shared();
    script.serve(200, TEST_MODEL);
    let mut seen: Vec<(String, u64, u64)> = Vec::new();
    let outcome = job.run(&SharedNet(script.clone()), &mut |p: ModelProgress| {
        seen.push((p.name, p.bytes_done, p.bytes_total));
    });
    assert_eq!(
        seen,
        [
            ("tiny".to_string(), 0, total),
            ("tiny".to_string(), total, total)
        ]
    );
    assert_eq!(script.sent().len(), 1);
    assert_eq!(script.sent()[0].method, "GET");
    assert_eq!(script.sent()[0].url, TEST_URL);
    let list = rig
        .app
        .whisper_download_complete(job, outcome)
        .unwrap_or_else(|e| panic!("whisper_download_complete: {e}"));
    assert!(list[0].downloaded);

    // A fresh data directory for the failures.
    let mut rig = Rig::new();
    rig.set_models(test_registry());
    rig.net.push(Err(NetError::Offline));
    assert_error(
        rig.app
            .whisper_download_with("tiny", &mut |_: ModelProgress| {})
            .err(),
        ErrorCode::Offline,
        DOWNLOAD_OFFLINE,
    );
    rig.net.serve(404, b"Not Found");
    assert_error(
        rig.app
            .whisper_download_with("tiny", &mut |_: ModelProgress| {})
            .err(),
        ErrorCode::NotFound,
        DOWNLOAD_MISSING,
    );
    assert!(rig.files_in(WHISPER_DIR).is_empty());
}

/// Acceptance 11, Interfaces: "The length equals bytes and the SHA-256 equals sha256."
#[test]
fn t11_model_verified_checks_the_length_and_the_sha_256() {
    let entry = test_registry().whisper[0].clone();
    assert!(model_verified(&entry, TEST_MODEL));
    let mut changed = TEST_MODEL.to_vec();
    changed[0] ^= 0x20;
    assert!(!model_verified(&entry, &changed), "one byte changed");
    assert!(!model_verified(&entry, &TEST_MODEL[1..]), "one byte short");
    let mut longer = TEST_MODEL.to_vec();
    longer.push(b'\n');
    assert!(!model_verified(&entry, &longer), "one byte more");
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 12: no microphone, or no permission.
// ---------------------------------------------------------------------------------------------

/// Acceptance 12: "Fake capture with no device → no_microphone, and the composer then produces no
/// speak (with a device it produces one or two for the same skill)".
#[test]
fn t12_no_microphone_means_no_speak_exercise() {
    let mut rig = Rig::build(false, true);
    rig.ready_local();
    assert_error(
        rig.app
            .mic_start(15, Box::new(|_| {}), Box::new(|| {}))
            .err(),
        ErrorCode::NoMicrophone,
        NO_MICROPHONE,
    );
    assert!(!rig.app.speak_available(Lang::Es));
    assert!(!rig.app.speaking_status().microphone);
    rig.practise_vocabulary();
    let lesson = rig.next_lesson();
    assert_eq!(speak_count(&lesson), 0, "{:?}", kinds(&lesson));
    assert!(
        kinds(&lesson).contains(&ExerciseType::TranslateToTarget),
        "the sentences stay translations: {:?}",
        kinds(&lesson)
    );

    let mut rig = Rig::new();
    rig.ready_local();
    assert!(rig.app.speak_available(Lang::Es));
    assert!(rig.app.speaking_status().microphone);
    rig.practise_vocabulary();
    let lesson = rig.next_lesson();
    let speaking = speak_count(&lesson);
    assert!(
        (1..=2).contains(&speaking),
        "one or two speak exercises: {:?}",
        kinds(&lesson)
    );
}

/// Acceptance 12: "with permission denied → the platform message (all three checked through
/// mic_permission_message)".
#[test]
fn t12_permission_denied_gives_the_platforms_message() {
    assert_eq!(
        mic_permission_message(Platform::Windows),
        "Parley can't use the microphone. Open Settings > Privacy & security > Microphone and allow desktop apps to use it."
    );
    assert_eq!(
        mic_permission_message(Platform::MacOs),
        "Parley can't use the microphone. Open System Settings > Privacy & Security > Microphone and turn on Parley."
    );
    assert_eq!(
        mic_permission_message(Platform::Linux),
        "Parley can't use the microphone. Check that it is connected, not muted, and not in use by another app."
    );
    let mut rig = Rig::new();
    rig.capture.refuse_start(CaptureError::Permission);
    assert_error(
        rig.app
            .mic_start(15, Box::new(|_| {}), Box::new(|| {}))
            .err(),
        ErrorCode::MicPermission,
        &mic_permission_message(Platform::current()),
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 13: takes are removed.
// ---------------------------------------------------------------------------------------------

/// Acceptance 13: "With keep_recordings off, the take files are gone after lesson_finish and after
/// quitting (on_exit), and a take left in cache/recordings/ is gone after the next app_start".
#[test]
fn t13_takes_are_removed_at_lesson_finish_on_exit_and_at_launch() {
    let mut rig = Rig::new();
    rig.ready_local();
    let lesson = rig.speak_lesson();
    let first = rig.record(take(8_000));
    rig.record(take(7_000));
    assert_eq!(rig.recordings().len(), 2);
    rig.app
        .lesson_finish(&lesson.session)
        .unwrap_or_else(|e| panic!("lesson_finish: {e}"));
    assert!(rig.recordings().is_empty(), "gone after lesson_finish");
    assert_error(
        rig.app.recording_play(&first.id).err(),
        ErrorCode::NotFound,
        RECORDING_GONE,
    );

    rig.record(take(6_000));
    rig.capture.push(Ok(take(5_000)));
    rig.app
        .mic_start(15, Box::new(|_| {}), Box::new(|| {}))
        .unwrap_or_else(|e| panic!("mic_start: {e}"));
    assert!(rig.capture.recording());
    rig.app.on_exit();
    assert!(!rig.capture.recording(), "quitting stops the capture");
    assert!(rig.recordings().is_empty(), "gone after on_exit");

    let left = rig.dir.join("cache").join("recordings");
    fs::create_dir_all(&left).expect("cache/recordings is made");
    fs::write(
        left.join("6f1c2a52-6e0b-4c55-9a3e-2a3f1d9b7c11.wav"),
        take(4_000).wav,
    )
    .expect("a take is left behind");
    rig.app
        .app_start()
        .unwrap_or_else(|e| panic!("app_start: {e}"));
    assert!(rig.recordings().is_empty(), "gone after the next launch");
}

/// Acceptance 13: "with it on, all stay."
#[test]
fn t13_kept_recordings_stay() {
    let mut rig = Rig::new();
    rig.ready_local();
    rig.set(json!({"speaking": {"keep_recordings": true}}));
    let lesson = rig.speak_lesson();
    let first = rig.record(take(8_000));
    rig.app
        .lesson_finish(&lesson.session)
        .unwrap_or_else(|e| panic!("lesson_finish: {e}"));
    assert_eq!(rig.recordings().len(), 1);
    rig.app
        .recording_play(&first.id)
        .unwrap_or_else(|e| panic!("a kept take plays: {e}"));
    rig.app.on_exit();
    assert_eq!(rig.recordings().len(), 1);
    rig.app
        .app_start()
        .unwrap_or_else(|e| panic!("app_start: {e}"));
    assert_eq!(rig.recordings().len(), 1);
}

/// Acceptance 13, Interfaces: "data_delete_all removes them and every file in models/whisper/ too."
#[test]
fn t13_delete_everything_removes_takes_and_models() {
    let mut rig = Rig::new();
    rig.set(json!({"speaking": {"keep_recordings": true}}));
    rig.download("base");
    rig.download("tiny");
    fs::write(rig.dir.join("models/whisper/ggml-small.bin.part"), b"half").expect("a part file");
    rig.record(take(8_000));
    rig.app
        .data_delete_all("delete")
        .unwrap_or_else(|e| panic!("Delete everything: {e}"));
    assert!(rig.recordings().is_empty());
    assert!(rig.files_in(WHISPER_DIR).is_empty());
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 14: Can't speak now.
// ---------------------------------------------------------------------------------------------

/// Acceptance 14: "Can't speak now at 10:00 (injected clock) → no speak exercise until 11:00, then
/// allowed again; speak_off_until is 11:00 meanwhile".
#[test]
fn t14_cant_speak_now_at_ten_turns_speak_off_until_eleven() {
    let mut rig = Rig::new();
    rig.ready_local();
    rig.practise_vocabulary();
    assert_eq!(rig.app.speaking_status().speak_off_until, None);
    let lesson = rig.next_lesson();
    assert!(speak_count(&lesson) >= 1, "{:?}", kinds(&lesson));

    rig.app
        .session_skip("speak")
        .unwrap_or_else(|e| panic!("Can't speak now: {e}"));
    assert!(!rig.app.speak_available(Lang::Es));
    assert_eq!(rig.app.speaking_status().speak_off_until, Some(ELEVEN));
    let lesson = rig.next_lesson();
    assert_eq!(speak_count(&lesson), 0, "{:?}", kinds(&lesson));

    rig.set_now(ELEVEN - 1);
    assert!(!rig.app.speak_available(Lang::Es));
    assert_eq!(speak_count(&rig.next_lesson()), 0);

    rig.set_now(ELEVEN);
    assert!(rig.app.speak_available(Lang::Es));
    assert_eq!(
        rig.app.speaking_status().speak_off_until,
        None,
        "only while it is in the future"
    );
    assert!(speak_count(&rig.next_lesson()) >= 1);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 15: a 402.
// ---------------------------------------------------------------------------------------------

/// Acceptance 15: "A 402 → "Can't check speaking right now.", the message and link, and Use the local
/// model when a model is downloaded (action.use_local true, false without)".
#[test]
fn t15_a_402_offers_the_local_model_only_when_it_is_downloaded() {
    let mut rig = Rig::new();
    rig.publik();
    let lesson = rig.speak_lesson();
    let before = rig.reviews().len();
    let recording = rig.record(take(8_000));

    let client = TranscribeClient::replying(&["402.json"]);
    let without = rig
        .check(&lesson, &recording.id, false, access(&client))
        .expect_err("a 402");
    assert_eq!(without.code, ErrorCode::NeedsCredit);
    assert_eq!(without.message, format!("{CANT_CHECK} {BALANCE}"));
    assert_eq!(
        without.action,
        Some(json!({"top_up_url": TOP_UP, "use_local": false}))
    );

    rig.download("base");
    let client = TranscribeClient::replying(&["402.json"]);
    let with = rig
        .check(&lesson, &recording.id, false, access(&client))
        .expect_err("a 402");
    assert_eq!(with.message, format!("{CANT_CHECK} {BALANCE}"));
    assert_eq!(
        with.action,
        Some(json!({"top_up_url": TOP_UP, "use_local": true}))
    );
    assert_eq!(rig.reviews().len(), before, "an error rates nothing");

    // Use the local model: the same take, checked with local true, makes no request.
    let local = rig.checked(&lesson, &recording.id, true, access(&client));
    assert!(matches!(local.transcriber, Transcriber::Local));
    assert_eq!(client.requests(), 1);
    assert_eq!(rig.whisper.calls().len(), 1);
}

/// Acceptance 15, Design notes: "Every check is a learner's action, so it goes out once even while
/// needs_credit is set (§9)."
#[test]
fn t15_a_check_goes_out_once_even_while_needs_credit_is_set() {
    let mut rig = Rig::new();
    rig.publik();
    let lesson = rig.speak_lesson();
    let recording = rig.record(take(8_000));
    let client = TranscribeClient::replying(&["valid.json"]);
    client.needs_credit.store(true, Ordering::SeqCst);
    rig.checked(&lesson, &recording.id, false, access(&client));
    assert_eq!(client.requests(), 1);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 16: Replay my take.
// ---------------------------------------------------------------------------------------------

/// Acceptance 16: "recording_play plays the trimmed file: the fake player receives exactly the
/// bytes of cache/recordings/<id>.wav with stretch 1.0; an unknown id → "This recording is no
/// longer available. Record it again.""
#[test]
fn t16_replay_plays_the_files_bytes_at_stretch_one() {
    let mut rig = Rig::new();
    let recording = rig.record(take(8_000));
    let path = rig.recording_path(&recording.id);
    let stops = rig.voices.stops.load(Ordering::SeqCst);
    rig.app
        .recording_play(&recording.id)
        .unwrap_or_else(|e| panic!("recording_play: {e}"));
    let file = fs::read(&path).expect("the take's file");
    let played = rig.player.played();
    assert_eq!(played.len(), 1);
    assert!(played[0].0 == file, "the file's bytes");
    assert_eq!(played[0].1, 1.0);
    assert!(
        rig.voices.stops.load(Ordering::SeqCst) > stops,
        "the OS voice is stopped"
    );

    // What plays is what the file holds.
    let changed = take(3_000).wav;
    fs::write(&path, &changed).expect("the file is rewritten");
    rig.app
        .recording_play(&recording.id)
        .unwrap_or_else(|e| panic!("recording_play: {e}"));
    assert!(rig.player.played()[1].0 == changed);

    assert_error(
        rig.app.recording_play("not-a-recording").err(),
        ErrorCode::NotFound,
        RECORDING_GONE,
    );
    fs::remove_file(path).expect("the file is removed");
    assert_error(
        rig.app.recording_play(&recording.id).err(),
        ErrorCode::NotFound,
        RECORDING_GONE,
    );
}

/// Acceptance 16, Interfaces: "a player error is module 09's no_voice "Parley could not play the
/// sound. Check that a speaker or headphones are connected.""
#[test]
fn t16_a_player_that_fails_gives_the_speaker_message() {
    let mut rig = Rig::new();
    rig.break_player();
    let recording = rig.record(take(8_000));
    assert_error(
        rig.app.recording_play(&recording.id).err(),
        ErrorCode::NoVoice,
        SPEAKER,
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 19: events.
// ---------------------------------------------------------------------------------------------

/// Acceptance 19: "MIC_LEVEL_EVENT is mic://level"; the other events and the level's payload
/// {dbfs}; mic_start passes its callbacks to the capture, so each level and the limit reach them.
#[test]
fn t19_levels_and_the_limit_reach_mic_starts_callbacks() {
    assert_eq!(MIC_LEVEL_EVENT, "mic://level");
    assert_eq!(MIC_LIMIT_EVENT, "mic://limit");
    assert_eq!(MODEL_PROGRESS_EVENT, "model://progress");
    assert_eq!(
        serde_json::to_value(MicLevel { dbfs: -12.5 }).expect("MicLevel serialises"),
        json!({"dbfs": -12.5})
    );

    let mut rig = Rig::new();
    *rig.capture.levels.lock().unwrap() = vec![-12.5, -60.0, 0.0];
    let heard = Arc::new(Mutex::new(Vec::<f32>::new()));
    let limited = Arc::new(AtomicUsize::new(0));
    let (levels, limit) = (Arc::clone(&heard), Arc::clone(&limited));
    rig.app
        .mic_start(
            15,
            Box::new(move |dbfs: f32| levels.lock().unwrap().push(dbfs)),
            Box::new(move || {
                limit.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .unwrap_or_else(|e| panic!("mic_start: {e}"));
    assert_eq!(*heard.lock().unwrap(), [-12.5, -60.0, 0.0]);
    rig.capture.reach_limit();
    assert_eq!(limited.load(Ordering::SeqCst), 1);
}

/// Interfaces: "COMMANDS gains the seven names after session_skip, in this order."
#[test]
fn interfaces_the_seven_commands_follow_session_skip() {
    let at = COMMANDS
        .iter()
        .position(|c| *c == "session_skip")
        .expect("session_skip is listed");
    assert_eq!(
        &COMMANDS[at + 1..at + 8],
        [
            "mic_start",
            "mic_stop",
            "recording_play",
            "speak_check",
            "whisper_models",
            "whisper_download",
            "speaking_status"
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 20: the cost line and charges (contract test 34).
// ---------------------------------------------------------------------------------------------

/// Acceptance 20: "Cost line: no charges → "Speaking checks cost about $0.00054 a take (measured).";
/// charges 500 and 600 → "$0.00055"". The mean rounds half up to a whole micro.
#[test]
fn t20_the_cost_line_is_the_measured_mean() {
    assert_eq!(MEASURED_TRANSCRIBE_MICROS, 538);
    assert_eq!(format_dollars(538), "$0.00054");
    assert_eq!(
        transcribe_cost_line(&[]),
        "Speaking checks cost about $0.00054 a take (measured)."
    );
    assert_eq!(
        transcribe_cost_line(&[500, 600]),
        "Speaking checks cost about $0.00055 a take (measured)."
    );
    assert_eq!(
        transcribe_cost_line(&[1, 2]),
        "Speaking checks cost about $0.000002 a take (measured)."
    );
}

/// Acceptance 20: "a check charged 538 adds 538 to the session's cost (contract test 34) and to
/// transcribe_charges"; an unusable reply's charge is recorded too; speaking_status opens the
/// cache only when it exists, and clearing every cache brings the line back to 538 micros.
#[test]
fn t20_charges_reach_the_lesson_and_the_cost_line() {
    let mut rig = Rig::new();
    rig.publik();
    assert_eq!(
        rig.app.speaking_status().cost_line,
        transcribe_cost_line(&[])
    );
    assert!(
        !rig.ai_cache_path().exists(),
        "looking at the status creates no cache"
    );
    let lesson = rig.speak_lesson();
    assert_eq!(rig.session_cost(&lesson.session), 0);

    let recording = rig.record(take(8_000));
    let client = TranscribeClient::replying(&["valid.json"]);
    rig.checked(&lesson, &recording.id, false, access(&client));
    assert_eq!(rig.session_cost(&lesson.session), VALID_CHARGE);
    assert_eq!(rig.charges(), [VALID_CHARGE]);

    let other = rig.record(take(7_000));
    let client = TranscribeClient::replying(&["not-a-string.json"]);
    assert_error(
        rig.check(&lesson, &other.id, false, access(&client)).err(),
        ErrorCode::AiInvalid,
        INVALID,
    );
    assert_eq!(
        rig.session_cost(&lesson.session),
        VALID_CHARGE + UNUSABLE_CHARGE
    );
    assert_eq!(rig.charges(), [VALID_CHARGE, UNUSABLE_CHARGE]);
    assert_eq!(
        rig.app.speaking_status().cost_line,
        transcribe_cost_line(&[VALID_CHARGE, UNUSABLE_CHARGE])
    );

    rig.app
        .cache_clear("all")
        .unwrap_or_else(|e| panic!("clear every cache: {e}"));
    assert_eq!(
        rig.app.speaking_status().cost_line,
        "Speaking checks cost about $0.00054 a take (measured)."
    );
}

/// Acceptance 20, Interfaces: "A run that ended meanwhile → "This exercise is no longer
/// available." (the charge is still recorded)."
#[test]
fn t20_a_lesson_that_ended_during_the_call_still_records_the_charge() {
    let mut rig = Rig::new();
    rig.publik();
    let lesson = rig.speak_lesson();
    let recording = rig.record(take(8_000));
    let client = TranscribeClient::replying(&["valid.json"]);
    let job = match rig.app.speak_begin(
        &lesson.session,
        &lesson.hola.id,
        &recording.id,
        false,
        access(&client),
    ) {
        Ok(SpeakBegin::Pending(job)) => job,
        Ok(SpeakBegin::Done(_)) => panic!("a new take is transcribed"),
        Err(e) => panic!("speak_begin: {e}"),
    };
    let outcome = job.run();
    rig.app
        .lesson_finish(&lesson.session)
        .unwrap_or_else(|e| panic!("lesson_finish: {e}"));
    assert_error(
        rig.app.speak_complete(job, outcome).err(),
        ErrorCode::NotFound,
        NOT_AVAILABLE,
    );
    assert_eq!(rig.charges(), [VALID_CHARGE]);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 21: a build without local Whisper.
// ---------------------------------------------------------------------------------------------

/// Acceptance 21: "A build without local Whisper (whisper None): local_available false; with
/// transcriber = local no speak is composed, and a check gives "Local speech checks are not
/// available in this build. …" with no request". It never falls back to the provider.
#[test]
fn t21_without_local_whisper_local_checks_are_refused_and_never_sent() {
    let mut rig = Rig::build(true, false);
    rig.download("base");
    assert!(!rig.app.speaking_status().local_available);
    rig.publik();
    let lesson = rig.speak_lesson();
    let recording = rig.record(take(8_000));

    rig.set(json!({"speaking": {"transcriber": "local"}}));
    assert!(!rig.app.speak_available(Lang::Es));
    let client = TranscribeClient::replying(&["valid.json"]);
    let error = rig
        .check(&lesson, &recording.id, false, access(&client))
        .expect_err("no local Whisper");
    assert_eq!(
        (error.code, error.message.as_str()),
        (ErrorCode::NoProvider, NOT_IN_BUILD)
    );
    assert_eq!(client.requests(), 0, "no request");

    rig.set(json!({"speaking": {"transcriber": "provider"}}));
    assert_error(
        rig.check(&lesson, &recording.id, true, access(&client))
            .err(),
        ErrorCode::NoProvider,
        NOT_IN_BUILD,
    );
    assert_eq!(client.requests(), 0);

    rig.set(json!({"speaking": {"transcriber": "local"}}));
    rig.app
        .lesson_finish(&lesson.session)
        .unwrap_or_else(|e| panic!("lesson_finish: {e}"));
    let next = rig.next_lesson();
    assert_eq!(speak_count(&next), 0, "{:?}", kinds(&next));
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 22: the model registry.
// ---------------------------------------------------------------------------------------------

/// Acceptance 22: "The bundled registry lists tiny, base and small with the URLs, bytes and SHA-256
/// above"; models/registry.toml parses to the same registry.
#[test]
fn t22_the_bundled_registry_pins_three_models() {
    let registry = bundled_models();
    let expected = [
        ("tiny", 77_691_713_u64, TINY_SHA),
        ("base", 147_951_465, BASE_SHA),
        ("small", 487_601_967, SMALL_SHA),
    ];
    assert_eq!(registry.whisper.len(), 3);
    for (entry, (name, bytes, sha)) in registry.whisper.iter().zip(expected) {
        assert_eq!(entry.name, name);
        assert_eq!(entry.url, format!("{PIN}/ggml-{name}.bin"));
        assert_eq!(entry.bytes, bytes);
        assert_eq!(entry.sha256, sha);
    }
    assert_eq!(
        registry.get("base").map(|entry| entry.bytes),
        Some(147_951_465)
    );
    assert_eq!(registry.get("large"), None);
    assert_eq!(
        parse_model_registry(&read("models/registry.toml")).as_ref(),
        Ok(&registry)
    );
    assert_eq!(WHISPER_DIR, "models/whisper");
    assert_eq!(DEFAULT_WHISPER_MODEL, "base");
    assert_eq!(model_file("base"), "models/whisper/ggml-base.bin");
    assert_eq!(
        serde_json::to_value(Settings::default()).expect("settings serialise")["speaking"]["whisper_model"],
        "base"
    );
}

/// Acceptance 22: "whisper_models gives sizes "77.7 MB", "148.0 MB", "487.6 MB" with base chosen";
/// downloaded follows the file, chosen follows speaking.whisper_model.
#[test]
fn t22_whisper_models_gives_sizes_downloads_and_the_choice() {
    assert_eq!(megabytes(147_951_465), "148.0 MB");
    let mut rig = Rig::new();
    let shown = |rig: &Rig| -> Vec<(String, u64, String, bool, bool)> {
        rig.app
            .whisper_models()
            .into_iter()
            .map(|m| (m.name, m.bytes, m.size, m.downloaded, m.chosen))
            .collect()
    };
    let row = |name: &str, bytes: u64, size: &str, downloaded: bool, chosen: bool| {
        (
            name.to_string(),
            bytes,
            size.to_string(),
            downloaded,
            chosen,
        )
    };
    assert_eq!(
        shown(&rig),
        [
            row("tiny", 77_691_713, "77.7 MB", false, false),
            row("base", 147_951_465, "148.0 MB", false, true),
            row("small", 487_601_967, "487.6 MB", false, false),
        ]
    );
    rig.download("tiny");
    rig.set(json!({"speaking": {"whisper_model": "tiny"}}));
    assert_eq!(
        shown(&rig),
        [
            row("tiny", 77_691_713, "77.7 MB", true, true),
            row("base", 147_951_465, "148.0 MB", false, false),
            row("small", 487_601_967, "487.6 MB", false, false),
        ]
    );
    // A part file is not a download.
    fs::write(rig.dir.join("models/whisper/ggml-base.bin.part"), b"half").expect("a part file");
    assert!(!rig.app.whisper_models()[1].downloaded);
}

/// Acceptance 22: "a registry with an http:// URL, a 63-character SHA-256 or an unknown key is
/// refused", and the other rules of parse_model_registry; entries come in ascending bytes.
#[test]
fn t22_parse_model_registry_refuses_anything_else() {
    let sha = "0".repeat(64);
    let table = |name: &str, url: &str, bytes: &str, sha256: &str| {
        format!("[whisper.{name}]\nurl = \"{url}\"\nbytes = {bytes}\nsha256 = \"{sha256}\"\n")
    };
    let head = "schema = \"parley.models/1\"\n\n";
    let good = format!(
        "{head}{}",
        table("tiny", "https://example.invalid/t.bin", "10", &sha)
    );
    let parsed = parse_model_registry(&good).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        parsed.whisper,
        [ModelEntry {
            name: "tiny".to_string(),
            url: "https://example.invalid/t.bin".to_string(),
            bytes: 10,
            sha256: sha.clone(),
        }]
    );

    let sorted = format!(
        "{head}{}{}{}",
        table("aaa", "https://example.invalid/a.bin", "30", &sha),
        table("bbb", "https://example.invalid/b.bin", "10", &sha),
        table("ccc", "https://example.invalid/c.bin", "20", &sha)
    );
    let names: Vec<String> = parse_model_registry(&sorted)
        .unwrap_or_else(|e| panic!("{e}"))
        .whisper
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(names, ["bbb", "ccc", "aaa"], "ascending by bytes");

    let refused = [
        (
            "an http:// URL",
            format!(
                "{head}{}",
                table("tiny", "http://example.invalid/t.bin", "10", &sha)
            ),
        ),
        (
            "a 63-character SHA-256",
            format!(
                "{head}{}",
                table("tiny", "https://example.invalid/t.bin", "10", &sha[..63])
            ),
        ),
        (
            "an unknown key",
            format!(
                "{head}{}size = \"77.7 MB\"\n",
                table("tiny", "https://example.invalid/t.bin", "10", &sha)
            ),
        ),
        (
            "an upper-case SHA-256",
            format!(
                "{head}{}",
                table(
                    "tiny",
                    "https://example.invalid/t.bin",
                    "10",
                    &"A".repeat(64)
                )
            ),
        ),
        (
            "zero bytes",
            format!(
                "{head}{}",
                table("tiny", "https://example.invalid/t.bin", "0", &sha)
            ),
        ),
        (
            "a name with a hyphen",
            format!(
                "{head}{}",
                table("ggml-tiny", "https://example.invalid/t.bin", "10", &sha)
            ),
        ),
        (
            "a name in capitals",
            format!(
                "{head}{}",
                table("Tiny", "https://example.invalid/t.bin", "10", &sha)
            ),
        ),
        (
            "another schema",
            format!(
                "schema = \"parley.models/2\"\n\n{}",
                table("tiny", "https://example.invalid/t.bin", "10", &sha)
            ),
        ),
        (
            "no schema",
            table("tiny", "https://example.invalid/t.bin", "10", &sha),
        ),
        (
            "an unknown top-level key",
            format!(
                "{head}extra = 1\n\n{}",
                table("tiny", "https://example.invalid/t.bin", "10", &sha)
            ),
        ),
        (
            "a missing url",
            format!("{head}[whisper.tiny]\nbytes = 10\nsha256 = \"{sha}\"\n"),
        ),
        ("not TOML", "schema = = 1".to_string()),
    ];
    for (why, text) in refused {
        assert!(parse_model_registry(&text).is_err(), "{why} is refused");
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 23: offline, and a reply that fails the schema.
// ---------------------------------------------------------------------------------------------

/// Acceptance 23: "Offline with a model downloaded → the local transcription scores the take with
/// the notice "You're offline, so the local model checked this."; without one → "Speaking checks
/// need a connection or the local model.""
#[test]
fn t23_offline_falls_back_to_a_downloaded_local_model_with_the_notice() {
    assert_eq!(
        OFFLINE_LOCAL_NOTICE,
        "You're offline, so the local model checked this."
    );
    let mut rig = Rig::new();
    rig.publik();
    let lesson = rig.speak_lesson();
    let recording = rig.record(take(8_000));

    let client = TranscribeClient::replying(&["offline.json"]);
    assert_error(
        rig.check(&lesson, &recording.id, false, access(&client))
            .err(),
        ErrorCode::Offline,
        NEED_CONNECTION,
    );
    assert!(rig.whisper.calls().is_empty());

    rig.download("base");
    let client = TranscribeClient::replying(&["offline.json"]);
    let result = rig.checked(&lesson, &recording.id, false, access(&client));
    assert_eq!(client.requests(), 1, "one call, never retried (I4)");
    assert!(matches!(result.transcriber, Transcriber::Local));
    assert_eq!(result.notice.as_deref(), Some(OFFLINE_LOCAL_NOTICE));
    assert_eq!(result.heard, HOLA_HEARD);
    close(result.scored.score, 1.0, 1e-12);
    let calls = rig.whisper.calls();
    assert_eq!(calls.len(), 1);
    let (model, wav, lang) = &calls[0];
    assert!(model.is_absolute(), "the model path is absolute");
    assert!(model.ends_with("models/whisper/ggml-base.bin"));
    assert!(*wav == take(8_000).wav, "the take's bytes");
    assert_eq!(*lang, Lang::Es);
}

/// Acceptance 23: "a reply that fails the schema (not-a-string.json) → "The AI answer could not be
/// used.", and the next check of the take carries attempt 2's key." It is neither scored nor held
/// (I2), and the attempts are counted in memory, not in cache/ai.sqlite3.
#[test]
fn t23_an_unusable_reply_is_not_held_and_the_next_check_is_attempt_two() {
    let mut rig = Rig::new();
    rig.publik();
    let lesson = rig.speak_lesson();
    let before = rig.reviews().len();
    let same = take(8_000);
    let recording = rig.record(same.clone());
    let client =
        TranscribeClient::replying(&["not-a-string.json", "not-a-string.json", "valid.json"]);
    for _ in 0..2 {
        assert_error(
            rig.check(&lesson, &recording.id, false, access(&client))
                .err(),
            ErrorCode::AiInvalid,
            INVALID,
        );
    }
    assert_eq!(rig.reviews().len(), before, "nothing is scored");
    let job = match rig.app.speak_begin(
        &lesson.session,
        &lesson.hola.id,
        &recording.id,
        false,
        access(&client),
    ) {
        Ok(SpeakBegin::Pending(job)) => job,
        Ok(SpeakBegin::Done(_)) => panic!("an unusable reply is not held"),
        Err(e) => panic!("speak_begin: {e}"),
    };
    let key_json = transcribe_key_json(
        PUBLIK,
        PUBLIK_TRANSCRIBE_MODEL,
        &sha256_hex(&same.wav),
        Lang::Es,
    );
    assert_eq!(
        job.request().and_then(|r| r.idempotency_key.clone()),
        Some(idempotency_key(&key_json, 3))
    );
    let outcome = job.run();
    let result = rig
        .app
        .speak_complete(job, outcome)
        .unwrap_or_else(|e| panic!("speak_complete: {e}"));
    assert_eq!(result.heard, VALID_TEXT);
    let keys: Vec<Option<String>> = client
        .sent()
        .into_iter()
        .map(|r| r.idempotency_key)
        .collect();
    assert_eq!(
        keys,
        [
            Some(idempotency_key(&key_json, 1)),
            Some(idempotency_key(&key_json, 2)),
            Some(idempotency_key(&key_json, 3)),
        ]
    );
    let conn = rusqlite::Connection::open(rig.ai_cache_path()).expect("cache/ai.sqlite3 opens");
    let attempts: i64 = conn
        .query_row("SELECT COUNT(*) FROM ai_attempts", [], |row| row.get(0))
        .unwrap_or(0);
    assert_eq!(attempts, 0, "attempts are held in memory");
}

/// Acceptance 23, with PUBLIK-INTEGRATION §6.1: a 200 whose body the client could not read at all
/// (its own `AiError::Invalid`) counts like a reply that fails the schema, so it is "The AI answer
/// could not be used." and the next check of the take carries attempt 2's key, not a replay of
/// attempt 1's.
#[test]
fn t23_an_unreadable_reply_also_makes_the_next_check_attempt_two() {
    let mut rig = Rig::new();
    rig.publik();
    let lesson = rig.speak_lesson();
    let before = rig.reviews().len();
    let same = take(8_000);
    let recording = rig.record(same.clone());
    let client = TranscribeClient::replying(&["unreadable.json", "valid.json"]);
    assert_error(
        rig.check(&lesson, &recording.id, false, access(&client))
            .err(),
        ErrorCode::AiInvalid,
        INVALID,
    );
    assert_eq!(rig.reviews().len(), before, "nothing is scored");
    let result = rig
        .check(&lesson, &recording.id, false, access(&client))
        .unwrap_or_else(|e| panic!("the second check: {e}"));
    assert_eq!(result.heard, VALID_TEXT);
    let key_json = transcribe_key_json(
        PUBLIK,
        PUBLIK_TRANSCRIBE_MODEL,
        &sha256_hex(&same.wav),
        Lang::Es,
    );
    let keys: Vec<Option<String>> = client
        .sent()
        .into_iter()
        .map(|r| r.idempotency_key)
        .collect();
    assert_eq!(
        keys,
        [
            Some(idempotency_key(&key_json, 1)),
            Some(idempotency_key(&key_json, 2)),
        ]
    );
}

/// Acceptance 23, Design notes (Transcriber): a timeout keeps its message; a local transcriber that
/// fails gives "The local speech model could not be used. Download it again in Settings >
/// Speaking." (provider_error).
#[test]
fn t23_a_timeout_and_a_failing_local_model_have_their_messages() {
    let mut rig = Rig::new();
    rig.publik();
    let lesson = rig.speak_lesson();
    let recording = rig.record(take(8_000));
    let client = TranscribeClient::replying(&["timeout.json"]);
    assert_error(
        rig.check(&lesson, &recording.id, false, access(&client))
            .err(),
        ErrorCode::Busy,
        "publik API did not answer in time.",
    );
    rig.download("base");
    rig.whisper.fails();
    assert_error(
        rig.check(&lesson, &recording.id, true, None).err(),
        ErrorCode::ProviderError,
        LOCAL_FAILED,
    );
}

/// Acceptance 23, Design notes (Transcriber): "402 → needs_credit "Can't check speaking right now.
/// {402 message}" with action {"top_up_url", "use_local"}; disconnected → "Can't check speaking
/// right now. publik API is disconnected." with {"use_local"}; 429 → its message; busy and timeout →
/// their messages; offline → ... "Speaking checks need a connection or the local model."; invalid →
/// "The AI answer could not be used."; other provider errors → "Can't check speaking right now.
/// {message}"; consent and key errors as module 06 gives them."
#[test]
fn t23_speaking_error_maps_section_9s_speaking_row() {
    let credit = AiError::NeedsCredit {
        message: BALANCE.to_string(),
        top_up_url: TOP_UP.to_string(),
    };
    for use_local in [false, true] {
        assert_eq!(
            speaking_error(&credit, use_local),
            AppError::new(ErrorCode::NeedsCredit, format!("{CANT_CHECK} {BALANCE}"))
                .with_action(json!({"top_up_url": TOP_UP, "use_local": use_local}))
        );
        assert_eq!(
            speaking_error(&AiError::Disconnected, use_local),
            AppError::new(
                ErrorCode::Disconnected,
                format!("{CANT_CHECK} publik API is disconnected.")
            )
            .with_action(json!({"use_local": use_local}))
        );
    }
    let rate = "Too many requests from this computer. Wait a minute, then try again.";
    let cases = [
        (
            AiError::RateLimited {
                message: rate.to_string(),
            },
            ErrorCode::RateLimited,
            rate.to_string(),
        ),
        (
            AiError::Busy,
            ErrorCode::Busy,
            "publik API is busy; try again in a moment".to_string(),
        ),
        (
            AiError::Timeout { provider: PUBLIK },
            ErrorCode::Busy,
            "publik API did not answer in time.".to_string(),
        ),
        (
            AiError::Offline,
            ErrorCode::Offline,
            NEED_CONNECTION.to_string(),
        ),
        (AiError::Invalid, ErrorCode::AiInvalid, INVALID.to_string()),
        (
            AiError::Provider {
                provider: PUBLIK,
                code: 500,
                message: "Internal Server Error".to_string(),
            },
            ErrorCode::ProviderError,
            format!("{CANT_CHECK} publik API 500: Internal Server Error"),
        ),
        (
            AiError::Provider {
                provider: ProviderKind::OwnKey,
                code: 400,
                message: "bad audio".to_string(),
            },
            ErrorCode::ProviderError,
            format!("{CANT_CHECK} Your provider returned 400: bad audio"),
        ),
    ];
    for (error, code, message) in cases {
        let mapped = speaking_error(&error, false);
        assert_eq!(
            (mapped.code, mapped.message.as_str()),
            (code, message.as_str()),
            "{error:?}"
        );
    }
    for error in [
        AiError::NeedsConsent,
        AiError::OwnKeyMissing,
        AiError::NoToken,
    ] {
        assert_eq!(
            speaking_error(&error, false),
            AppError::from(error.clone()),
            "{error:?} as module 06 gives it"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 24: a second take.
// ---------------------------------------------------------------------------------------------

/// Acceptance 24: "A second take on a rated exercise → rated: false, the first grade, no new
/// review_log row."
#[test]
fn t24_a_second_take_is_practice_and_keeps_the_first_grade() {
    let mut rig = Rig::new();
    rig.ready_local();
    let lesson = rig.speak_lesson();
    let first = rig.record(take(8_000));
    rig.whisper.hears("hola que");
    let graded = rig.checked(&lesson, &first.id, false, None);
    assert!(graded.rated);
    let reviews = rig.reviews().len();

    let second = rig.record(take(7_000));
    rig.whisper.hears(HOLA_HEARD);
    let again = rig.checked(&lesson, &second.id, false, None);
    assert!(!again.rated);
    assert_eq!(again.grade, graded.grade, "the first take's grade");
    close(again.scored.score, 1.0, 1e-12);
    assert!(again.scored.pass, "the take's own score");
    assert_eq!(rig.reviews().len(), reviews, "no new review_log row");
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 26: privacy and consent (I12, I13).
// ---------------------------------------------------------------------------------------------

/// Acceptance 26: "mic_start, mic_stop, recording_play and lesson_finish make no call on the
/// scripted AiClient or Net (I13)".
#[test]
fn t26_recording_replaying_and_finishing_send_nothing() {
    let mut rig = Rig::new();
    rig.publik();
    let lesson = rig.speak_lesson();
    let client = TranscribeClient::replying(&["valid.json"]);
    let recording = rig.record(take(8_000));
    rig.app
        .recording_play(&recording.id)
        .unwrap_or_else(|e| panic!("recording_play: {e}"));
    rig.app
        .lesson_finish(&lesson.session)
        .unwrap_or_else(|e| panic!("lesson_finish: {e}"));
    assert_eq!(client.requests(), 0);
    assert!(rig.net.sent().is_empty(), "the network is not used");
}

/// Acceptance 26: "with ai.provider = unset and no local model, a check gives the needs_consent
/// error and no call (I12)".
#[test]
fn t26_no_provider_and_no_local_model_asks_for_consent_and_calls_nothing() {
    let mut rig = Rig::new();
    rig.publik();
    let lesson = rig.speak_lesson();
    let recording = rig.record(take(8_000));
    rig.set(json!({"ai": {"provider": "unset"}}));
    let error = rig
        .check(&lesson, &recording.id, false, None)
        .expect_err("consent is needed");
    assert_eq!(error, AppError::from(AiError::NeedsConsent));
    assert!(rig.whisper.calls().is_empty());
    assert!(rig.net.sent().is_empty());
}

/// Acceptance 26: "with publik API chosen and consent given, the check makes exactly one call, whose
/// audio is exactly the take file's bytes (I13)."
#[test]
fn t26_a_check_sends_exactly_the_take_file_once() {
    let mut rig = Rig::new();
    rig.publik();
    let lesson = rig.speak_lesson();
    let recording = rig.record(take_of(11_000, 2_000));
    let file = fs::read(rig.recording_path(&recording.id)).expect("the take's file");
    let client = TranscribeClient::replying(&["valid.json"]);
    rig.checked(&lesson, &recording.id, false, access(&client));
    let sent = client.sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].audio == file, "exactly the take file's bytes");
    assert!(rig.net.sent().is_empty());
}

// ---------------------------------------------------------------------------------------------
// Interfaces: speak_begin, the transcriber, speak_available and the helpers.
// ---------------------------------------------------------------------------------------------

/// Interfaces, speak_begin step 1: an exercise not in a run in progress → "This exercise is no
/// longer available."; not speak → "This exercise is not a speaking exercise."; an unknown recording
/// → "This recording is no longer available. Record it again."
#[test]
fn interfaces_speak_begin_checks_the_run_the_exercise_and_the_recording() {
    let mut rig = Rig::new();
    rig.ready_local();
    let lesson = rig.speak_lesson();
    let recording = rig.record(take(8_000));
    assert_error(
        rig.app
            .speak_check_with(
                "s-0000000000000000",
                &lesson.hola.id,
                &recording.id,
                false,
                None,
            )
            .err(),
        ErrorCode::NotFound,
        NOT_AVAILABLE,
    );
    assert_error(
        rig.app
            .speak_check_with(&lesson.session, "e99", &recording.id, false, None)
            .err(),
        ErrorCode::NotFound,
        NOT_AVAILABLE,
    );
    let other = lesson
        .exercises
        .iter()
        .find(|ex| ex.kind() != ExerciseType::Speak)
        .expect("the lesson has other exercises");
    assert_error(
        rig.app
            .speak_check_with(&lesson.session, &other.id, &recording.id, false, None)
            .err(),
        ErrorCode::InvalidInput,
        NOT_SPEAKING,
    );
    assert_error(
        rig.check(&lesson, "not-a-recording", false, None).err(),
        ErrorCode::NotFound,
        RECORDING_GONE,
    );
    assert!(rig.whisper.calls().is_empty());
}

/// Interfaces, speak_begin step 2, own_key: with a transcription model the provider is asked with
/// it (the key's provider own_key); without one, the local model when downloaded, else "Add a
/// transcription model for your own key in Settings > AI, or download the local model in Settings
/// > Speaking."
#[test]
fn interfaces_an_own_key_needs_a_transcription_model() {
    let mut rig = Rig::new();
    rig.publik();
    let lesson = rig.speak_lesson();
    let same = take(8_000);
    let recording = rig.record(same.clone());

    rig.set(json!({"ai": {"provider": "own_key", "own_key": {"transcribe_model": "whisper-own"}}}));
    let client = TranscribeClient::of(ProviderKind::OwnKey, &["valid.json"]);
    let request = match rig.app.speak_begin(
        &lesson.session,
        &lesson.hola.id,
        &recording.id,
        false,
        access(&client),
    ) {
        Ok(SpeakBegin::Pending(job)) => job.request().cloned(),
        Ok(SpeakBegin::Done(_)) => panic!("a new take is transcribed"),
        Err(e) => panic!("speak_begin: {e}"),
    };
    assert_eq!(
        request,
        Some(transcribe_request(
            ProviderKind::OwnKey,
            "whisper-own",
            &same.wav,
            Lang::Es,
            1
        ))
    );

    rig.set(json!({"ai": {"own_key": {"transcribe_model": null}}}));
    let client = TranscribeClient::of(ProviderKind::OwnKey, &["valid.json"]);
    assert_error(
        rig.check(&lesson, &recording.id, false, access(&client))
            .err(),
        ErrorCode::NoProvider,
        OWN_KEY_NO_MODEL,
    );
    rig.download("base");
    let local = rig.checked(&lesson, &recording.id, false, access(&client));
    assert!(matches!(local.transcriber, Transcriber::Local));
    assert_eq!(client.requests(), 0);
}

/// Interfaces, speak_begin step 2: a failed client is speaking_error(&e, local_ready); a local chat
/// server transcribes with the local model when downloaded, else "Speaking checks need a
/// connection or the local model."; with no provider chosen the downloaded local model is used.
#[test]
fn interfaces_the_transcriber_follows_the_provider() {
    let mut rig = Rig::new();
    rig.publik();
    let lesson = rig.speak_lesson();
    let recording = rig.record(take(8_000));

    assert_error(
        rig.check(&lesson, &recording.id, false, Some(Err(AiError::NoToken)))
            .err(),
        ErrorCode::NoProvider,
        NO_TOKEN_MESSAGE,
    );
    let disconnected = rig
        .check(
            &lesson,
            &recording.id,
            false,
            Some(Err(AiError::Disconnected)),
        )
        .expect_err("publik API is disconnected");
    assert_eq!(disconnected.action, Some(json!({"use_local": false})));

    rig.set(json!({"ai": {"provider": "local"}}));
    assert_error(
        rig.check(&lesson, &recording.id, false, None).err(),
        ErrorCode::Offline,
        NEED_CONNECTION,
    );
    rig.download("base");
    let local = rig.checked(&lesson, &recording.id, false, None);
    assert!(matches!(local.transcriber, Transcriber::Local));
    assert_eq!(local.notice, None);

    rig.set(json!({"ai": {"provider": "unset"}}));
    let unset = rig.checked(&lesson, &recording.id, false, None);
    assert!(matches!(unset.transcriber, Transcriber::Local));
    assert!(unset.cached, "the local transcript of this take is held");
    assert_eq!(rig.whisper.calls().len(), 1);
}

/// Interfaces, speak_available: speaking on, not skipped, a microphone, and a transcriber that can
/// be used: with transcriber = local, local Whisper in the build; with provider, publik, an own key
/// with a transcription model, or a downloaded local model.
#[test]
fn interfaces_speak_available_needs_a_microphone_and_a_usable_transcriber() {
    let mut rig = Rig::new();
    assert!(
        !rig.app.speak_available(Lang::Es),
        "nothing to transcribe with"
    );
    rig.download("base");
    assert!(
        rig.app.speak_available(Lang::Es),
        "the local model is ready"
    );
    fs::remove_file(rig.dir.join(model_file("base"))).expect("the model is removed");
    assert!(!rig.app.speak_available(Lang::Es));
    rig.publik();
    assert!(rig.app.speak_available(Lang::Es), "publik API");
    rig.set(json!({"ai": {"provider": "own_key"}}));
    assert!(
        !rig.app.speak_available(Lang::Es),
        "an own key with no model"
    );
    rig.set(json!({"ai": {"own_key": {"transcribe_model": "whisper-own"}}}));
    assert!(rig.app.speak_available(Lang::Es), "an own key with a model");
    rig.set(json!({"ai": {"provider": "local"}}));
    assert!(!rig.app.speak_available(Lang::Es), "a chat server alone");
    rig.set(json!({"speaking": {"transcriber": "local"}}));
    assert!(
        rig.app.speak_available(Lang::Es),
        "local Whisper is in the build"
    );
    rig.set(json!({"speaking": {"enabled": false}}));
    assert!(!rig.app.speak_available(Lang::Es), "speaking is off");

    let mut deaf = Rig::build(false, true);
    deaf.publik();
    assert!(!deaf.app.speak_available(Lang::Es), "no microphone");

    let mut plain = Rig::build(true, false);
    plain.set(json!({"speaking": {"transcriber": "local"}}));
    plain.download("base");
    assert!(!plain.app.speak_available(Lang::Es), "no local Whisper");
    plain.set(json!({"speaking": {"transcriber": "provider"}}));
    assert!(
        !plain.app.speak_available(Lang::Es),
        "a file is not a build"
    );
    plain.publik();
    assert!(plain.app.speak_available(Lang::Es));
}

/// Interfaces: "publik → PUBLIK_TRANSCRIBE_MODEL; own_key → ai.own_key.transcribe_model when set;
/// else None"; a local take's model is "whisper.cpp/ggml-<name>".
#[test]
fn interfaces_transcribe_model_and_local_model_id() {
    assert_eq!(
        transcribe_model(&settings(json!({"ai": {"provider": "publik"}}))),
        Some(PUBLIK_TRANSCRIBE_MODEL.to_string())
    );
    assert_eq!(
        transcribe_model(&settings(json!({
            "ai": {"provider": "own_key", "own_key": {"transcribe_model": "whisper-own"}}
        }))),
        Some("whisper-own".to_string())
    );
    for provider in ["own_key", "local", "unset"] {
        assert_eq!(
            transcribe_model(&settings(json!({"ai": {"provider": provider}}))),
            None,
            "{provider}"
        );
    }
    assert_eq!(local_model_id("base"), "whisper.cpp/ggml-base");
    assert_eq!(local_model_id("tiny"), "whisper.cpp/ggml-tiny");
}

/// Interfaces: parse_transcript is §6.8's schema (an object whose text is a string of at most 2,000
/// characters); heard_text removes every [...] span, collapses white space and trims.
#[test]
fn interfaces_parse_transcript_and_heard_text() {
    assert_eq!(
        parse_transcript(&json!({"text": "hola"})).as_deref(),
        Some("hola")
    );
    assert_eq!(parse_transcript(&json!({"text": ""})).as_deref(), Some(""));
    assert_eq!(
        parse_transcript(&json!({"text": "hola", "language": "es"})).as_deref(),
        Some("hola"),
        "the schema allows other fields"
    );
    let longest = "a".repeat(2_000);
    assert_eq!(
        parse_transcript(&json!({ "text": longest })).as_deref(),
        Some(longest.as_str())
    );
    for refused in [
        json!({"text": "a".repeat(2_001)}),
        json!({"text": 42}),
        json!({"text": null}),
        json!({}),
        json!("hola"),
        json!(["hola"]),
    ] {
        assert_eq!(parse_transcript(&refused), None, "{refused}");
    }

    assert_eq!(heard_text("[BLANK_AUDIO] hola"), "hola");
    assert_eq!(heard_text("[BLANK_AUDIO]"), "");
    assert_eq!(heard_text("  hola   que \n tal  "), "hola que tal");
    assert_eq!(heard_text("hola [música] que [ruido]"), "hola que");
    assert_eq!(heard_text(VALID_TEXT), VALID_TEXT);
}

/// Design notes: "exercise_submit still grades typed text for a speak exercise (module 03)".
#[test]
fn interfaces_typed_text_is_still_graded_for_a_speak_exercise() {
    let mut rig = Rig::new();
    rig.ready_local();
    let lesson = rig.speak_lesson();
    let grade = rig
        .app
        .exercise_submit(
            &lesson.session,
            &lesson.hola.id,
            &Answer::Text(HOLA.to_string()),
            Some(3_000),
            false,
        )
        .unwrap_or_else(|e| panic!("exercise_submit: {e}"));
    assert_eq!(grade.verdict, Verdict::Correct);
}
