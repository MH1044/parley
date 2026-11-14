//! Speaking in the app service (docs/modules/10-speaking-and-pronunciation.md): `mic_start`,
//! `mic_stop`, `recording_play`, `speak_check`, `whisper_models`, `whisper_download` and
//! `speaking_status`, whether a lesson may hold speaking exercises, and what happens to the takes.
//!
//! A take is recorded by the microphone the app gives the service ([`Speaking`]), written to
//! `cache/recordings/<id>.wav`, and sent nowhere (invariant I13) until the learner's **Check**
//! asks for it to be transcribed: by the chosen provider (P6, docs/PUBLIK-INTEGRATION.md §6.8) or
//! by local Whisper, whichever the learner chose; a choice that costs money is never made for
//! them. The call is made in three steps so it holds no lock, as module 08's judge does:
//! `speak_begin` decides and returns a [`TranscribeJob`], the job's `run` makes the one call
//! (never retried, I4) or the one local transcription, and `speak_complete` checks the
//! transcript, holds it, scores it and rates the card. Transcripts are held in memory only, by
//! the key of §6.1, so the same take is never paid for twice while the app runs (I5); nothing
//! about a take is written to `cache/ai.sqlite3` but the amount it was charged.
//!
//! The local models are downloaded the same way, `whisper_download_begin`, the job's `run` over
//! the app's transport, `whisper_download_complete`, and a file takes its own name only once its
//! length and SHA-256 match the registry.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::json;

use super::audio::cannot_play;
use super::lesson::{AiAccess, open_cache, open_existing_cache, run_error, run_settings};
use super::{AI_CACHE_FILE, Parley, cannot_write, not_available, open_store, unknown_value};
use crate::ai::{
    AiClient, AiError, OFFLINE_LOCAL_NOTICE, ProviderKind, TranscribeRequest, TranscriptReply,
    heard_text, local_model_id, parse_transcript, sha256_hex, speaking_error, transcribe_cost_line,
    transcribe_key, transcribe_model, transcribe_request,
};
use crate::api::{
    ModelInfo, ModelProgress, Recording, SpeakResult, SpeakingStatus, Transcriber, megabytes,
};
use crate::audio::{CaptureError, LocalTranscriber, Platform, Speaking, mic_permission_message};
use crate::content::Lang;
use crate::env::{HttpRequest, HttpResponse, Net, NetError, Rng};
use crate::error::{AppError, ErrorCode};
use crate::exercise::Body;
use crate::speech::{
    ModelEntry, WHISPER_DIR, bundled_models, model_file, model_file_name, model_verified, score,
    speech_words,
};

/// The folder of the learner's takes, under the data directory.
pub(crate) const RECORDINGS_DIR: &str = "cache/recordings";

/// The takes recorded while the app runs: id → the take's SHA-256. Kept in memory only.
pub(crate) type Takes = HashMap<String, String>;

/// The local models being downloaded, by name, shared with each download's job, which removes its
/// name when it is dropped (completed or not).
pub(crate) type Downloads = Arc<Mutex<HashSet<String>>>;

// Messages (docs/modules/10-speaking-and-pronunciation.md, Messages).

fn no_microphone() -> AppError {
    AppError::new(
        ErrorCode::NoMicrophone,
        "No microphone was found. Connect one, or choose Can't speak now.",
    )
}

fn cannot_record() -> AppError {
    AppError::new(
        ErrorCode::NoMicrophone,
        "Parley could not record from the microphone. Try again.",
    )
}

fn too_quiet() -> AppError {
    AppError::new(
        ErrorCode::TooQuiet,
        "We didn't hear anything. Try again a little closer to the microphone.",
    )
}

fn nothing_recorded() -> AppError {
    AppError::new(ErrorCode::InvalidInput, "Nothing is being recorded.")
}

fn recording_gone() -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        "This recording is no longer available. Record it again.",
    )
}

fn not_speaking() -> AppError {
    AppError::new(
        ErrorCode::InvalidInput,
        "This exercise is not a speaking exercise.",
    )
}

fn no_local_build() -> AppError {
    AppError::new(
        ErrorCode::NoProvider,
        "Local speech checks are not available in this build. Choose your AI provider in Settings > Speaking.",
    )
}

fn own_key_without_model() -> AppError {
    AppError::new(
        ErrorCode::NoProvider,
        "Add a transcription model for your own key in Settings > AI, or download the local model in Settings > Speaking.",
    )
}

fn offline_without_model() -> AppError {
    AppError::new(
        ErrorCode::Offline,
        "Speaking checks need a connection or the local model.",
    )
}

fn local_failed() -> AppError {
    AppError::new(
        ErrorCode::ProviderError,
        "The local speech model could not be used. Download it again in Settings > Speaking.",
    )
}

fn download_offline() -> AppError {
    AppError::new(
        ErrorCode::Offline,
        "You're offline. Download the speech model when you're connected.",
    )
}

fn download_failed() -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        "The speech model could not be downloaded. Try again later.",
    )
}

fn download_damaged() -> AppError {
    AppError::new(
        ErrorCode::ChecksumMismatch,
        "The download was damaged. Try again.",
    )
}

fn already_downloading() -> AppError {
    AppError::new(ErrorCode::Busy, "This model is already being downloaded.")
}

/// A capture error as the learner sees it.
fn capture_error(e: CaptureError) -> AppError {
    match e {
        CaptureError::NoDevice => no_microphone(),
        CaptureError::Permission => AppError::new(
            ErrorCode::MicPermission,
            mic_permission_message(Platform::current()),
        ),
        CaptureError::Backend(_) => cannot_record(),
        CaptureError::TooQuiet => too_quiet(),
        CaptureError::NotRecording => nothing_recorded(),
    }
}

/// A version-4 UUID (lowercase 8-4-4-4-12 hex) from two values of the injected generator, the
/// first the high half: version nibble 4, variant bits 10.
fn take_id(rng: &mut dyn Rng) -> String {
    let hi = (rng.next_u64() & !0xF000) | 0x4000;
    let lo = (rng.next_u64() & 0x3FFF_FFFF_FFFF_FFFF) | 0x8000_0000_0000_0000;
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        hi >> 32,
        (hi >> 16) & 0xFFFF,
        hi & 0xFFFF,
        lo >> 48,
        lo & 0xFFFF_FFFF_FFFF
    )
}

/// `cache/recordings/<id>.wav`.
fn take_file(id: &str) -> String {
    format!("{RECORDINGS_DIR}/{id}.wav")
}

/// What `speak_check`'s first step gives: the result at once (a transcript held for the take), or
/// a transcription to make with no lock held.
pub enum SpeakBegin {
    Done(SpeakResult),
    Pending(TranscribeJob),
}

/// One local transcription: the transcriber, the model file (absolute), the take, and the key its
/// transcript is held under.
struct LocalWork {
    whisper: Arc<dyn LocalTranscriber>,
    model: PathBuf,
    key: String,
    wav: Vec<u8>,
    lang: Lang,
}

impl LocalWork {
    fn run(&self, notice: Option<&str>) -> TranscribeOutcome {
        TranscribeOutcome {
            answer: Answer::Local {
                key: self.key.clone(),
                text: self.whisper.transcribe(&self.model, &self.wav, self.lang),
                notice: notice.map(str::to_string),
            },
        }
    }
}

enum Work {
    /// The P6 call, and the local transcription to make instead when the provider is offline and
    /// the local model is downloaded.
    Provider {
        client: Arc<dyn AiClient>,
        request: Box<TranscribeRequest>,
        key: String,
        fallback: Option<LocalWork>,
    },
    Local(LocalWork),
}

/// A transcription to make with no lock held: the take, its key, the language, the provider call
/// or the local model, and the local fallback; it owns everything it needs.
pub struct TranscribeJob {
    session_id: String,
    exercise_id: String,
    /// The local model is downloaded: an error offers **Use the local model**.
    use_local: bool,
    work: Work,
}

enum Answer {
    Provider {
        key: String,
        reply: Result<TranscriptReply, AiError>,
    },
    Local {
        key: String,
        text: Result<String, String>,
        notice: Option<String>,
    },
}

/// What a transcription gave: the transcript or the error, the charge (in the reply), and which
/// transcriber answered.
pub struct TranscribeOutcome {
    answer: Answer,
}

impl TranscribeJob {
    /// The P6 request; None for a local job.
    pub fn request(&self) -> Option<&TranscribeRequest> {
        match &self.work {
            Work::Provider { request, .. } => Some(request),
            Work::Local(_) => None,
        }
    }

    /// Makes the one P6 call (never retried, I4) or the one local transcription. A provider that
    /// is offline, with the local model downloaded, is answered by the local model instead, and
    /// the result says so.
    pub fn run(&self) -> TranscribeOutcome {
        match &self.work {
            Work::Provider {
                client,
                request,
                key,
                fallback,
            } => {
                let reply = client.transcribe(TranscribeRequest::clone(request));
                if let (Err(AiError::Offline), Some(local)) = (&reply, fallback) {
                    return local.run(Some(OFFLINE_LOCAL_NOTICE));
                }
                TranscribeOutcome {
                    answer: Answer::Provider {
                        key: key.clone(),
                        reply,
                    },
                }
            }
            Work::Local(local) => local.run(None),
        }
    }
}

/// What `whisper_download`'s first step gives: the list at once (the model is already here), or
/// a download to make with no lock held.
pub enum DownloadBegin {
    Done(Vec<ModelInfo>),
    Pending(ModelDownload),
}

/// A model download to make with no lock held: the registry entry. The model counts as being
/// downloaded until the job is dropped, completed or not.
pub struct ModelDownload {
    entry: ModelEntry,
    downloads: Downloads,
}

impl Drop for ModelDownload {
    fn drop(&mut self) {
        self.downloads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.entry.name);
    }
}

/// The response of a model download, or why there is none.
pub struct DownloadOutcome {
    response: Result<HttpResponse, NetError>,
}

impl ModelDownload {
    pub fn name(&self) -> &str {
        &self.entry.name
    }

    /// GETs the registry's URL through `net` (the transport follows its redirect). Reports {0,
    /// bytes} before the request, so the size shows first, and {bytes, bytes} once a 200 body has
    /// arrived.
    pub fn run(&self, net: &dyn Net, progress: &mut dyn FnMut(ModelProgress)) -> DownloadOutcome {
        let report = |done: u64| ModelProgress {
            name: self.entry.name.clone(),
            bytes_done: done,
            bytes_total: self.entry.bytes,
        };
        progress(report(0));
        let response = net.send(HttpRequest {
            method: "GET".into(),
            url: self.entry.url.clone(),
            headers: Vec::new(),
            body: Vec::new(),
        });
        if response.as_ref().is_ok_and(|r| r.status == 200) {
            progress(report(self.entry.bytes));
        }
        DownloadOutcome { response }
    }
}

/// How a check will be transcribed.
enum Chosen {
    Provider {
        client: Arc<dyn AiClient>,
        model: String,
    },
    Local,
}

impl Parley {
    /// The microphone, local Whisper and model registry the service uses (`Parley::new` starts
    /// with `Speaking::default()`: no microphone, no local Whisper, the bundled registry).
    pub fn set_speaking(&mut self, speaking: Speaking) {
        self.speaking = speaking;
    }

    /// Starts a take of at most `max_s` seconds (15 for an exercise, 30 for a role-play turn).
    /// `on_level` gets the level of every 50 ms of input and `on_limit` runs once at the limit;
    /// both run on the audio thread.
    pub fn mic_start(
        &mut self,
        max_s: u32,
        on_level: Box<dyn FnMut(f32) + Send>,
        on_limit: Box<dyn FnOnce() + Send>,
    ) -> Result<(), AppError> {
        if max_s != 15 && max_s != 30 {
            return Err(unknown_value(&max_s.to_string(), "max_s"));
        }
        self.speaking
            .capture
            .start(max_s * 1000, on_level, on_limit)
            .map_err(capture_error)
    }

    /// Stops the take and keeps it: `cache/recordings/<id>.wav`, its id and SHA-256 in memory.
    /// Nothing is sent anywhere.
    pub fn mic_stop(&mut self) -> Result<Recording, AppError> {
        let take = self.speaking.capture.stop().map_err(capture_error)?;
        let id = take_id(&mut *self.env.rng);
        self.env
            .fs
            .write(Path::new(&take_file(&id)), &take.wav)
            .map_err(|_| cannot_write())?;
        self.takes.insert(id.clone(), sha256_hex(&take.wav));
        Ok(Recording {
            id,
            duration_ms: take.duration_ms,
        })
    }

    /// The bytes of a take recorded while the app runs, when its file still reads.
    fn take_bytes(&self, recording_id: &str) -> Result<Vec<u8>, AppError> {
        if !self.takes.contains_key(recording_id) {
            return Err(recording_gone());
        }
        self.env
            .fs
            .read(Path::new(&take_file(recording_id)))
            .map_err(|_| recording_gone())
    }

    /// **Replay my take**: the take's file, as it is, after stopping the OS voice.
    pub fn recording_play(&mut self, recording_id: &str) -> Result<(), AppError> {
        let bytes = self.take_bytes(recording_id)?;
        self.audio.os.stop();
        self.audio
            .player
            .play(&bytes, 1.0)
            .map_err(|_| cannot_play())
    }

    /// Whether `speaking.whisper_model` is downloaded in a build with local Whisper.
    fn local_ready(&self) -> bool {
        self.speaking.whisper.is_some() && self.model_listed(&self.settings.speaking.whisper_model)
    }

    /// Whether the model's file is in `models/whisper/` (only a verified file ever takes its
    /// name there).
    fn model_listed(&self, name: &str) -> bool {
        self.env
            .fs
            .list(Path::new(WHISPER_DIR))
            .is_ok_and(|names| names.contains(&model_file_name(name)))
    }

    /// "The local speech model is not downloaded. …" with the size of the chosen model.
    fn model_missing(&self, name: &str) -> AppError {
        let bytes = self
            .speaking
            .models
            .get(name)
            .map(|entry| entry.bytes)
            .or_else(|| bundled_models().get(name).map(|entry| entry.bytes));
        let message = match bytes {
            Some(bytes) => format!(
                "The local speech model is not downloaded. Download it in Settings > Speaking ({}).",
                megabytes(bytes)
            ),
            None => "The local speech model is not downloaded. Download it in Settings > Speaking."
                .to_string(),
        };
        AppError::new(ErrorCode::NoProvider, message).with_action(json!({ "model": name }))
    }

    /// The local model when it is ready, else `otherwise`.
    fn local_or(&self, otherwise: AppError) -> Result<Chosen, AppError> {
        if self.local_ready() {
            Ok(Chosen::Local)
        } else {
            Err(otherwise)
        }
    }

    /// The transcriber of a check: the local model when the learner asked for it (`local`, or
    /// `speaking.transcriber = local`), else the chosen provider, falling back to the local model
    /// only where using the provider is impossible without a choice the learner has not made.
    fn transcriber(&self, local: bool, ai: Option<AiAccess>) -> Result<Chosen, AppError> {
        let name = &self.settings.speaking.whisper_model;
        if local || self.settings.speaking.transcriber == "local" {
            if self.speaking.whisper.is_none() {
                return Err(no_local_build());
            }
            if !self.model_listed(name) {
                return Err(self.model_missing(name));
            }
            return Ok(Chosen::Local);
        }
        let needs_consent = || AppError::from(AiError::NeedsConsent);
        match self.settings.ai.provider.as_str() {
            "publik" | "own_key" => {
                let Some(model) = transcribe_model(&self.settings) else {
                    return self.local_or(own_key_without_model());
                };
                match ai {
                    Some(Ok(client)) => Ok(Chosen::Provider { client, model }),
                    Some(Err(e)) => Err(speaking_error(&e, self.local_ready())),
                    None => self.local_or(needs_consent()),
                }
            }
            "local" => self.local_or(offline_without_model()),
            _ => self.local_or(needs_consent()),
        }
    }

    /// The local transcription of `wav` with the chosen model, when local Whisper is in the build.
    fn local_work(&self, wav: &[u8], sha256: &str, lang: Lang) -> Option<LocalWork> {
        let whisper = self.speaking.whisper.clone()?;
        let name = &self.settings.speaking.whisper_model;
        Some(LocalWork {
            whisper,
            model: self.config.data_dir.join(model_file(name)),
            key: transcribe_key(ProviderKind::Local, &local_model_id(name), sha256, lang),
            wav: wav.to_vec(),
            lang,
        })
    }

    /// The first step of `speak_check`: the run, the exercise and the take are checked; the
    /// transcriber is chosen; a transcript held for the take's key is scored at once (no request,
    /// I5); otherwise the transcription to make.
    pub fn speak_begin(
        &mut self,
        session_id: &str,
        exercise_id: &str,
        recording_id: &str,
        local: bool,
        ai: Option<AiAccess>,
    ) -> Result<SpeakBegin, AppError> {
        let active = self.runs.get(session_id).ok_or_else(not_available)?;
        let lang = active.lang;
        let exercise = active
            .run
            .exercises()
            .iter()
            .find(|ex| ex.id == exercise_id)
            .ok_or_else(not_available)?;
        if !matches!(exercise.body, Body::Speak { .. }) {
            return Err(not_speaking());
        }
        let wav = self.take_bytes(recording_id)?;
        let sha256 = self
            .takes
            .get(recording_id)
            .cloned()
            .unwrap_or_else(|| sha256_hex(&wav));
        let use_local = self.local_ready();
        let job = match self.transcriber(local, ai)? {
            Chosen::Local => {
                let Some(work) = self.local_work(&wav, &sha256, lang) else {
                    return Err(no_local_build());
                };
                if let Some(text) = self.transcripts.get(&work.key).cloned() {
                    return self
                        .scored(
                            session_id,
                            exercise_id,
                            &text,
                            Transcriber::Local,
                            true,
                            None,
                        )
                        .map(SpeakBegin::Done);
                }
                TranscribeJob {
                    session_id: session_id.to_string(),
                    exercise_id: exercise_id.to_string(),
                    use_local,
                    work: Work::Local(work),
                }
            }
            Chosen::Provider { client, model } => {
                let kind = client.kind();
                let key = transcribe_key(kind, &model, &sha256, lang);
                if let Some(text) = self.transcripts.get(&key).cloned() {
                    return self
                        .scored(
                            session_id,
                            exercise_id,
                            &text,
                            Transcriber::Provider,
                            true,
                            None,
                        )
                        .map(SpeakBegin::Done);
                }
                let attempt = 1 + self.failed_attempts.get(&key).copied().unwrap_or(0);
                let fallback = if use_local {
                    self.local_work(&wav, &sha256, lang)
                } else {
                    None
                };
                TranscribeJob {
                    session_id: session_id.to_string(),
                    exercise_id: exercise_id.to_string(),
                    use_local,
                    work: Work::Provider {
                        request: Box::new(transcribe_request(kind, &model, &wav, lang, attempt)),
                        client,
                        key,
                        fallback,
                    },
                }
            }
        };
        Ok(SpeakBegin::Pending(job))
    }

    /// Records a take's charge in `transcribe_charges` (the cost line) and, while the run is in
    /// progress, in the session's cost. A store that cannot be written loses only the record.
    fn record_take_charge(&mut self, session_id: &str, charge: Option<i64>) {
        let Some(charge) = charge else {
            return;
        };
        let cache = open_cache(
            &self.config.data_dir.join(AI_CACHE_FILE),
            self.env.clock.now_ms(),
        );
        let _ = cache.add_transcribe_charge(charge);
        if self.runs.contains_key(session_id)
            && let Ok(store) = open_store(&mut self.store)
        {
            let _ = store.add_session_cost(session_id, charge);
        }
    }

    /// Scores a transcript against the exercise's sentence and reviews the card with the first
    /// scored take (`LessonRun::spoken`). No words heard is "We didn't hear anything. …" and
    /// rates nothing; a run that ended is "This exercise is no longer available.".
    fn scored(
        &mut self,
        session_id: &str,
        exercise_id: &str,
        transcript: &str,
        transcriber: Transcriber,
        cached: bool,
        notice: Option<String>,
    ) -> Result<SpeakResult, AppError> {
        let active = self.runs.get(session_id).ok_or_else(not_available)?;
        let lang = active.lang;
        let sentence = match active
            .run
            .exercises()
            .iter()
            .find(|ex| ex.id == exercise_id)
            .map(|ex| &ex.body)
        {
            Some(Body::Speak { text, .. }) => text.clone(),
            Some(_) => return Err(not_speaking()),
            None => return Err(not_available()),
        };
        let course = self.content.get(&lang).ok_or_else(not_available)?.clone();
        let heard = heard_text(transcript);
        if speech_words(lang, &heard, &course.number_words, false).is_empty() {
            return Err(too_quiet());
        }
        let scored = score(
            lang,
            &sentence,
            &heard,
            &course.number_words,
            self.settings.speaking.pass_threshold,
        );
        let store = open_store(&mut self.store)?;
        let active = self.runs.get_mut(session_id).ok_or_else(not_available)?;
        let (grade, rated) = active
            .run
            .spoken(
                store,
                &run_settings(&self.settings),
                exercise_id,
                &heard,
                &scored,
                &*self.env.clock,
            )
            .map_err(run_error)?;
        Ok(SpeakResult {
            scored,
            heard,
            grade,
            rated,
            transcriber,
            cached,
            notice,
        })
    }

    /// The last step of `speak_check`. A provider reply's charge is recorded, usable or not; a
    /// transcript that fails the schema counts a failed attempt for the take (so the next check
    /// sends attempt n + 1's idempotency key) and is "The AI answer could not be used."; a valid
    /// one is held and scored. A local failure asks for the model to be downloaded again.
    pub fn speak_complete(
        &mut self,
        job: TranscribeJob,
        outcome: TranscribeOutcome,
    ) -> Result<SpeakResult, AppError> {
        let (text, transcriber, notice) = match outcome.answer {
            Answer::Provider { key, reply } => {
                // A 200 the client could not read at all (its own `Invalid`) counts like a
                // transcript that fails the schema: the next check must send attempt n + 1's
                // idempotency key, or the provider would replay the same unusable answer.
                if matches!(reply, Err(AiError::Invalid)) {
                    *self.failed_attempts.entry(key.clone()).or_insert(0) += 1;
                }
                let reply = reply.map_err(|e| speaking_error(&e, job.use_local))?;
                self.record_take_charge(&job.session_id, reply.charge_micros);
                let Some(text) = parse_transcript(&reply.body) else {
                    *self.failed_attempts.entry(key).or_insert(0) += 1;
                    return Err(speaking_error(&AiError::Invalid, job.use_local));
                };
                self.transcripts.insert(key, text.clone());
                (text, Transcriber::Provider, None)
            }
            Answer::Local { key, text, notice } => {
                let text = text.map_err(|_| local_failed())?;
                self.transcripts.insert(key, text.clone());
                (text, Transcriber::Local, notice)
            }
        };
        self.scored(
            &job.session_id,
            &job.exercise_id,
            &text,
            transcriber,
            false,
            notice,
        )
    }

    /// `speak_check` in one go (the CLI and tests): begin, the transcription when there is one,
    /// complete.
    pub fn speak_check_with(
        &mut self,
        session_id: &str,
        exercise_id: &str,
        recording_id: &str,
        local: bool,
        ai: Option<AiAccess>,
    ) -> Result<SpeakResult, AppError> {
        match self.speak_begin(session_id, exercise_id, recording_id, local, ai)? {
            SpeakBegin::Done(result) => Ok(result),
            SpeakBegin::Pending(job) => {
                let outcome = job.run();
                self.speak_complete(job, outcome)
            }
        }
    }

    /// One entry per model of the registry: its size, whether it is downloaded, and whether it is
    /// the chosen one.
    pub fn whisper_models(&self) -> Vec<ModelInfo> {
        let listed = self.env.fs.list(Path::new(WHISPER_DIR)).unwrap_or_default();
        self.speaking
            .models
            .whisper
            .iter()
            .map(|entry| ModelInfo {
                name: entry.name.clone(),
                bytes: entry.bytes,
                size: megabytes(entry.bytes),
                downloaded: listed.contains(&model_file_name(&entry.name)),
                chosen: entry.name == self.settings.speaking.whisper_model,
            })
            .collect()
    }

    /// The first step of `whisper_download`: an unknown name is refused; a model already here
    /// needs no request; a model being downloaded is busy; otherwise the download to make.
    pub fn whisper_download_begin(&mut self, name: &str) -> Result<DownloadBegin, AppError> {
        let Some(entry) = self.speaking.models.get(name).cloned() else {
            return Err(unknown_value(name, "name"));
        };
        if self.model_listed(name) {
            return Ok(DownloadBegin::Done(self.whisper_models()));
        }
        let mut downloading = self
            .downloads
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !downloading.insert(entry.name.clone()) {
            return Err(already_downloading());
        }
        drop(downloading);
        Ok(DownloadBegin::Pending(ModelDownload {
            entry,
            downloads: self.downloads.clone(),
        }))
    }

    /// The last step of `whisper_download`: the body is written to
    /// `models/whisper/ggml-<name>.bin.part`, checked against the registry's length and SHA-256,
    /// and renamed to `ggml-<name>.bin` only when it matches; a damaged download is removed.
    /// Returns the list of models.
    pub fn whisper_download_complete(
        &mut self,
        job: ModelDownload,
        outcome: DownloadOutcome,
    ) -> Result<Vec<ModelInfo>, AppError> {
        let response = outcome.response.map_err(|_| download_offline())?;
        if response.status != 200 {
            return Err(download_failed());
        }
        let file = model_file(&job.entry.name);
        let part = format!("{file}.part");
        let fs = &*self.env.fs;
        if fs.write(Path::new(&part), &response.body).is_err() {
            let _ = fs.remove(Path::new(&part));
            return Err(cannot_write());
        }
        if !model_verified(&job.entry, &response.body) {
            let _ = fs.remove(Path::new(&part));
            return Err(download_damaged());
        }
        if fs.rename(Path::new(&part), Path::new(&file)).is_err() {
            let _ = fs.remove(Path::new(&part));
            return Err(cannot_write());
        }
        drop(job);
        Ok(self.whisper_models())
    }

    /// `whisper_download` in one go: begin, the request through the injected network, complete.
    pub fn whisper_download_with(
        &mut self,
        name: &str,
        progress: &mut dyn FnMut(ModelProgress),
    ) -> Result<Vec<ModelInfo>, AppError> {
        match self.whisper_download_begin(name)? {
            DownloadBegin::Done(list) => Ok(list),
            DownloadBegin::Pending(job) => {
                let outcome = job.run(&*self.env.net, progress);
                self.whisper_download_complete(job, outcome)
            }
        }
    }

    /// Speaking exercises are off (**Can't speak now**) at this moment.
    fn speak_skipped(&self) -> bool {
        self.skips
            .get("speak")
            .is_some_and(|until| self.env.clock.now_ms() < *until)
    }

    /// The measured cost line (the mean of this install's take charges, or the setup run's), until
    /// when speaking is off while it is, whether local Whisper is in the build, and whether a
    /// microphone exists. Reading never creates the cache file.
    pub fn speaking_status(&self) -> SpeakingStatus {
        let now = self.env.clock.now_ms();
        let charges = open_existing_cache(&self.config.data_dir.join(AI_CACHE_FILE), now)
            .map(|cache| cache.transcribe_charges())
            .unwrap_or_default();
        SpeakingStatus {
            cost_line: transcribe_cost_line(&charges),
            speak_off_until: self
                .skips
                .get("speak")
                .copied()
                .filter(|until| now < *until),
            local_available: self.speaking.whisper.is_some(),
            microphone: self.speaking.capture.has_device(),
        }
    }

    /// Speaking is on and not skipped, a microphone exists, and a transcriber can be used: with
    /// `speaking.transcriber = local`, local Whisper is in the build; with `provider`, the
    /// provider is publik API, or the own key with a transcription model, or the local model is
    /// downloaded. The language does not change the answer: every course language is spoken.
    pub fn speak_available(&self, lang: Lang) -> bool {
        let _ = lang;
        let speaking = &self.settings.speaking;
        if !speaking.enabled || self.speak_skipped() || !self.speaking.capture.has_device() {
            return false;
        }
        if speaking.transcriber == "local" {
            return self.speaking.whisper.is_some();
        }
        match self.settings.ai.provider.as_str() {
            "publik" => true,
            "own_key" if transcribe_model(&self.settings).is_some() => true,
            _ => self.local_ready(),
        }
    }

    /// Removes every take in `cache/recordings/` and forgets them. Every removal is attempted.
    pub(crate) fn remove_takes(&mut self) {
        let fs = &*self.env.fs;
        if let Ok(names) = fs.list(Path::new(RECORDINGS_DIR)) {
            for name in names {
                let _ = fs.remove(Path::new(&format!("{RECORDINGS_DIR}/{name}")));
            }
        }
        self.takes.clear();
    }

    /// Removes the takes unless **Keep my recordings** is on (`lesson_finish`, quitting and
    /// launching).
    pub(crate) fn remove_takes_unless_kept(&mut self) {
        if !self.settings.speaking.keep_recordings {
            self.remove_takes();
        }
    }

    /// Quitting: stops a capture in progress (its take is dropped) and removes the takes unless
    /// `speaking.keep_recordings` is on.
    pub fn on_exit(&mut self) {
        let _ = self.speaking.capture.stop();
        self.remove_takes_unless_kept();
    }

    /// Every file in `models/whisper/`, relative to the data directory (Delete everything).
    pub(crate) fn model_files(&self) -> Vec<String> {
        self.env
            .fs
            .list(Path::new(WHISPER_DIR))
            .unwrap_or_default()
            .into_iter()
            .map(|name| format!("{WHISPER_DIR}/{name}"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::SplitMix64;

    #[test]
    fn take_ids_are_version_4_uuids_from_two_values() {
        let mut rng = SplitMix64::new(7);
        let id = take_id(&mut rng);
        assert_eq!(id.len(), 36);
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12]
        );
        assert!(parts[2].starts_with('4'));
        assert!(matches!(
            parts[3].chars().next(),
            Some('8' | '9' | 'a' | 'b')
        ));
        assert!(
            id.chars()
                .all(|c| c == '-' || c.is_ascii_digit() || ('a'..='f').contains(&c))
        );
        struct Fixed(Vec<u64>);
        impl Rng for Fixed {
            fn next_u64(&mut self) -> u64 {
                self.0.remove(0)
            }
        }
        assert_eq!(
            take_id(&mut Fixed(vec![u64::MAX, 0])),
            "ffffffff-ffff-4fff-8000-000000000000"
        );
        assert_eq!(
            take_id(&mut Fixed(vec![0, u64::MAX])),
            "00000000-0000-4000-bfff-ffffffffffff"
        );
    }

    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::super::ActiveRun;
    use super::super::test_support::{NO_PACKS, ScriptedNet, parley, parley_with};
    use crate::audio::test_support::wav_ms;
    use crate::audio::{Capture, Take};
    use crate::env::HttpResponse;
    use crate::exercise::{Exercise, Lesson, LessonRun, Origin};
    use crate::grading::Verdict;
    use crate::speech::ModelRegistry;

    const SESSION: &str = "s-00000000000000ab";

    /// A microphone whose every take is the same WAV.
    struct OneTake(Vec<u8>);

    impl Capture for OneTake {
        fn has_device(&self) -> bool {
            true
        }
        fn start(
            &self,
            _max_ms: u32,
            _on_level: Box<dyn FnMut(f32) + Send>,
            _on_limit: Box<dyn FnOnce() + Send>,
        ) -> Result<(), CaptureError> {
            Ok(())
        }
        fn stop(&self) -> Result<Take, CaptureError> {
            Ok(Take {
                wav: self.0.clone(),
                duration_ms: 1_000,
            })
        }
    }

    /// Local Whisper that always hears the same text, and counts its transcriptions.
    struct Hears(&'static str, AtomicUsize);

    impl LocalTranscriber for Hears {
        fn transcribe(&self, _model: &Path, _wav: &[u8], _lang: Lang) -> Result<String, String> {
            self.1.fetch_add(1, Ordering::SeqCst);
            Ok(self.0.to_string())
        }
    }

    /// A lesson of one `speak` exercise in progress under SESSION.
    fn speaking_run(app: &mut Parley) {
        let body = Body::Speak {
            text: "Buenos días.".into(),
            prompt_en: "Good morning.".into(),
        };
        let lesson = Lesson {
            schema: Lesson::SCHEMA.into(),
            lang: Lang::Es,
            skill_id: "es.a1.u01.s01".into(),
            seq: 1,
            origin: Origin::Template,
            exercises: vec![Exercise {
                id: "e1".into(),
                item_id: "es.a1.u01.s01#e:2".into(),
                facet: body.kind().facet(),
                origin: Origin::Template,
                body,
            }],
            ai: None,
        };
        let store = super::super::open_store(&mut app.store).unwrap();
        let run = LessonRun::start_lesson(store, &lesson, SESSION, &*app.env.clock).unwrap();
        app.runs.insert(
            SESSION.into(),
            ActiveRun {
                lang: Lang::Es,
                run,
                skill_id: None,
                judging_stopped: false,
            },
        );
    }

    #[test]
    fn a_take_is_kept_checked_locally_once_and_removed_at_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = parley(dir.path());
        app.course_start("es").unwrap();
        let heard = Arc::new(Hears("buenos dias", AtomicUsize::new(0)));
        app.set_speaking(Speaking {
            capture: Arc::new(OneTake(wav_ms(1_000))),
            whisper: Some(heard.clone()),
            models: bundled_models(),
        });
        app.settings_set(&json!({"speaking": {"transcriber": "local"}}))
            .unwrap();
        speaking_run(&mut app);
        assert!(app.speak_available(Lang::Es));
        assert_eq!(
            app.mic_start(20, Box::new(|_| {}), Box::new(|| {}))
                .unwrap_err()
                .message,
            "Unknown value 20 for max_s."
        );
        app.mic_start(15, Box::new(|_| {}), Box::new(|| {}))
            .unwrap();
        let take = app.mic_stop().unwrap();
        assert!(dir.path().join(take_file(&take.id)).is_file());

        let missing = app
            .speak_check_with(SESSION, "e1", &take.id, false, None)
            .unwrap_err();
        assert_eq!(
            missing.message,
            "The local speech model is not downloaded. Download it in Settings > Speaking (148.0 MB)."
        );
        assert_eq!(missing.action, Some(json!({"model": "base"})));

        std::fs::create_dir_all(dir.path().join(WHISPER_DIR)).unwrap();
        std::fs::write(dir.path().join(model_file("base")), b"model").unwrap();
        let first = app
            .speak_check_with(SESSION, "e1", &take.id, false, None)
            .unwrap();
        assert_eq!(
            (first.scored.pass, first.rated, first.cached),
            (true, true, false)
        );
        assert_eq!(
            (first.transcriber, first.grade.verdict),
            (Transcriber::Local, Verdict::Correct)
        );
        let again = app
            .speak_check_with(SESSION, "e1", &take.id, true, None)
            .unwrap();
        assert_eq!((again.rated, again.cached), (false, true));
        assert_eq!(heard.1.load(Ordering::SeqCst), 1, "the transcript is held");
        let status = app.speaking_status();
        assert!(status.local_available && status.microphone);
        assert!(app.whisper_models()[1].downloaded && app.whisper_models()[1].chosen);

        app.lesson_finish(SESSION).unwrap();
        assert_eq!(
            app.env.fs.list(Path::new(RECORDINGS_DIR)).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            app.recording_play(&take.id).unwrap_err().message,
            "This recording is no longer available. Record it again."
        );
    }

    #[test]
    fn a_model_is_kept_only_when_it_matches() {
        let entry = ModelEntry {
            name: "tiny".into(),
            url: "https://example.invalid/ggml-tiny.bin".into(),
            bytes: 3,
            // SHA-256 of "abc" (FIPS 180-2, appendix B.1).
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
        };
        let dir = tempfile::tempdir().unwrap();
        let serving = |body: &[u8]| {
            Box::new(ScriptedNet {
                response: Ok(HttpResponse {
                    status: 200,
                    headers: Vec::new(),
                    body: body.to_vec(),
                }),
                requests: Arc::new(Mutex::new(Vec::new())),
            })
        };
        for (body, kept) in [(&b"abd"[..], false), (&b"abc"[..], true)] {
            let mut app = parley_with(dir.path(), NO_PACKS, serving(body));
            app.set_speaking(Speaking {
                models: ModelRegistry {
                    whisper: vec![entry.clone()],
                },
                ..Speaking::default()
            });
            let mut reports = Vec::new();
            let result = app.whisper_download_with("tiny", &mut |p| reports.push(p.bytes_done));
            assert_eq!(reports.first(), Some(&0), "the size first");
            if kept {
                assert!(result.unwrap()[0].downloaded);
            } else {
                let error = result.unwrap_err();
                assert_eq!(error.message, "The download was damaged. Try again.");
                assert!(app.env.fs.list(Path::new(WHISPER_DIR)).unwrap().is_empty());
            }
        }
        let mut app = parley(dir.path());
        assert_eq!(
            app.whisper_download_begin("huge").err().map(|e| e.message),
            Some("Unknown value huge for name.".to_string())
        );
    }
}
