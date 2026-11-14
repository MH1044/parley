//! Listening in the app service (docs/modules/09-listening-and-voices.md): `voices_list`,
//! `tts_play`, `tts_stop`, `tts_prefetch`, `audio_status` and `session_skip`, and whether a lesson
//! may hold listening exercises.
//!
//! A sentence is spoken by the OS voice, or by a natural voice through the chosen provider (P5,
//! docs/PUBLIK-INTEGRATION.md §6.7). A natural sentence already in `cache/audio/` plays with no
//! request (invariant I5); one that is not is fetched in three steps so the call holds no lock, as
//! module 08's judge does: `tts_begin` (or `prefetch_begin`) decides and returns a [`SpeechJob`],
//! the job's `run` makes the one call (never retried, I4) and checks the audio, and the completion
//! stores, records and plays it. Audio that fails §6.7 is never stored (I2); every reply that was
//! charged counts towards the measured cost and the lesson's cost, used or not.
//!
//! One request per sentence however the calls overlap: from the moment a begin returns a job until
//! that job is completed or dropped, its sentence is being fetched. A prefetch of it then asks for
//! nothing, and a play of it waits for the fetch (with no lock held) and begins again, so it plays
//! the stored clip or decides afresh as if the fetch had ended just before it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use super::lesson::{AiAccess, open_cache, open_existing_cache};
use super::{AI_CACHE_FILE, Parley, cannot_write, open_store, remove_file, unknown_value};
use crate::ai::{
    AiClient, AiError, AudioEntry, AudioReply, NATURAL_UNAVAILABLE, ProviderKind, SpeechRequest,
    VoiceSource, audio_file, choose_source, idempotency_key, natural_voice, per_language,
    speech_cost_line, speech_key, speech_key_json, speech_request, speech_valid,
};
use crate::api::{AudioStatus, Prefetched, TtsPlayed, Voice};
use crate::audio::{Audio, AudioDecoder, Platform, no_voice_message, os_rate, voices_for};
use crate::content::Lang;
use crate::error::{AppError, ErrorCode};
use crate::exercise::Body;

/// The folder of cached natural-voice sentences, under the data directory.
pub(crate) const AUDIO_DIR: &str = "cache/audio";
/// How long **Can't listen now** (and module 10's **Can't speak now**) lasts.
const SKIP_MS: i64 = 60 * 60 * 1000;
/// The longest text read aloud, in characters.
const MAX_TEXT_CHARS: usize = 500;

/// The things `session_skip` can turn off, with when each comes back (ms since the epoch). Kept in
/// memory only: a restart brings them back.
pub(crate) type Skips = HashMap<&'static str, i64>;

/// What a play's first step gives: the play done (cached, or the OS voice), a call to make with no
/// lock held, or a fetch of the same sentence to wait for with no lock held before beginning again.
pub enum TtsBegin {
    Done(TtsPlayed),
    Pending(SpeechJob),
    Waiting(SpeechWait),
}

/// What a prefetch's first step gives: nothing to fetch, or a call to make with no lock held.
pub enum PrefetchBegin {
    Done(Prefetched),
    Pending(SpeechJob),
}

/// A P5 call to make with no lock held: the client, the request, the cache key, the decoder that
/// checks the reply, and what the completion needs to fall back or play.
pub struct SpeechJob {
    client: Arc<dyn AiClient>,
    /// Boxed, so a begin that plays at once stays small.
    request: Box<SpeechRequest>,
    key: String,
    decoder: Arc<dyn AudioDecoder>,
    /// The text as read aloud (trimmed): the duration bound counts its words.
    text: String,
    lang: Lang,
    slow: bool,
    voice: String,
    /// WSOLA's length factor for the player (slow mode the provider does not honour).
    stretch: f32,
    /// The lesson whose cost a charge is added to.
    session_id: Option<String>,
    /// The fetch of `key`, ended when the job is dropped (after its completion, or without one).
    flight: Arc<Flight>,
}

/// What a speech job's call gave: the reply or the error, and whether the reply passed §6.7.
pub struct SpeechOutcome {
    reply: Result<AudioReply, AiError>,
    valid: bool,
}

/// A natural sentence being fetched: ended once the job fetching it has been completed or
/// dropped. Plays of the same sentence wait for it instead of asking for the sentence again.
#[derive(Default)]
pub(crate) struct Flight {
    ended: Mutex<bool>,
    changed: Condvar,
}

impl Flight {
    fn end(&self) {
        *self.ended.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.changed.notify_all();
    }

    fn has_ended(&self) -> bool {
        *self.ended.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wait(&self) {
        let mut ended = self.ended.lock().unwrap_or_else(PoisonError::into_inner);
        while !*ended {
            ended = self
                .changed
                .wait(ended)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// The natural sentences being fetched, by cache key. Kept in memory only; an entry whose fetch
/// has ended counts as not being fetched and is forgotten at the next begin that looks.
pub(crate) type Flights = HashMap<String, Arc<Flight>>;

/// A play of a sentence that is being fetched: wait with no lock held, then call `tts_begin` again
/// with the same arguments.
pub struct SpeechWait {
    flight: Arc<Flight>,
}

impl SpeechWait {
    /// Blocks until the fetch has ended: its job completed (a valid clip is stored by then, and a
    /// failure recorded) or dropped without completing.
    pub fn wait(self) {
        self.flight.wait();
    }
}

impl Drop for SpeechJob {
    /// A job ends its fetch however it goes, so a call whose thread failed never leaves a play
    /// waiting. A completion drops the job only after storing the clip, so a play woken here
    /// finds the file.
    fn drop(&mut self) {
        self.flight.end();
    }
}

impl SpeechJob {
    /// What the job asks for.
    pub fn request(&self) -> &SpeechRequest {
        &self.request
    }

    /// Makes the one P5 call (never retried, I4) and checks the reply against §6.7.
    pub fn run(&self) -> SpeechOutcome {
        let reply = self.client.speech(SpeechRequest::clone(&self.request));
        let valid = reply
            .as_ref()
            .is_ok_and(|reply| speech_valid(reply, &self.text, self.request.speed, &*self.decoder));
        SpeechOutcome { reply, valid }
    }
}

fn no_text() -> AppError {
    AppError::new(ErrorCode::InvalidInput, "There is no text to read aloud.")
}

fn too_long() -> AppError {
    AppError::new(
        ErrorCode::InvalidInput,
        "This text is too long to read aloud.",
    )
}

pub(super) fn cannot_play() -> AppError {
    AppError::new(
        ErrorCode::NoVoice,
        "Parley could not play the sound. Check that a speaker or headphones are connected.",
    )
}

fn no_voice(lang: Lang) -> AppError {
    AppError::new(
        ErrorCode::NoVoice,
        no_voice_message(lang, Platform::current()),
    )
}

fn not_in_progress() -> AppError {
    AppError::new(ErrorCode::NotFound, "This lesson is no longer in progress.")
}

/// The text to read, trimmed: neither empty nor longer than 500 characters.
fn checked_text(text: &str) -> Result<&str, AppError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(no_text());
    }
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(too_long());
    }
    Ok(text)
}

/// The OS voice spoke.
fn spoken_by_os() -> TtsPlayed {
    TtsPlayed {
        source: VoiceSource::Os,
        cached: false,
        notice: None,
        ai_error: None,
    }
}

/// A natural sentence played.
fn natural(cached: bool) -> TtsPlayed {
    TtsPlayed {
        source: VoiceSource::Natural,
        cached,
        notice: None,
        ai_error: None,
    }
}

impl Parley {
    /// The speakers, OS voices and decoder the service uses (`Parley::new` starts with
    /// `Audio::default()`: no voices, a silent player and the WAV decoder).
    pub fn set_audio(&mut self, audio: Audio) {
        self.audio = audio;
    }

    /// The installed OS voices of `lang`, then, only with `audio.voice_source = natural`, the
    /// natural voice the settings give.
    pub fn voices_list(&self, lang: &str) -> Result<Vec<Voice>, AppError> {
        let lang = self.lang(lang)?;
        let mut voices: Vec<Voice> = voices_for(&self.audio.os.list(), lang)
            .into_iter()
            .map(|voice| Voice {
                id: voice.id,
                name: voice.name,
                source: "os".to_string(),
                installed: true,
            })
            .collect();
        if self.settings.audio.voice_source == "natural"
            && let Some((_, voice)) = natural_voice(&self.settings, &self.audio.natural, lang)
        {
            voices.push(Voice {
                id: voice.clone(),
                name: voice,
                source: "natural".to_string(),
                installed: true,
            });
        }
        Ok(voices)
    }

    /// Speaks `text` with the OS voice: `audio.os_voice.<lang>` when that voice is installed,
    /// else the first voice of the language; none is the `no_voice` error with the platform's
    /// help. Slow mode is the backend's normal rate × the slow speed, within its range.
    fn os_play(&mut self, text: &str, lang: Lang, slow: bool) -> Result<TtsPlayed, AppError> {
        let installed = self.audio.os.list();
        let chosen = per_language(&self.settings.audio.os_voice, lang)
            .filter(|id| installed.iter().any(|voice| voice.id == *id))
            .map(str::to_string);
        let voice = match chosen {
            Some(id) => id,
            None => voices_for(&installed, lang)
                .into_iter()
                .next()
                .map(|voice| voice.id)
                .ok_or_else(|| no_voice(lang))?,
        };
        let rate = os_rate(self.audio.os.rates(), slow, self.settings.audio.slow_speed);
        // One sound at a time: a clip still playing stops before the voice starts.
        self.audio.player.stop();
        self.audio
            .os
            .speak(text, &voice, rate)
            .map_err(|_| cannot_play())?;
        Ok(spoken_by_os())
    }

    /// Plays audio bytes, stopping the OS voice first.
    fn play_bytes(&self, bytes: &[u8], stretch: f32) -> Result<(), AppError> {
        self.audio.os.stop();
        self.audio
            .player
            .play(bytes, stretch)
            .map_err(|_| cannot_play())
    }

    /// The first step of `tts_play`: checks the language and the text, chooses the source, plays
    /// a cached natural sentence or the OS voice at once, or returns the call to make.
    pub fn tts_begin(
        &mut self,
        text: &str,
        lang: &str,
        slow: bool,
        session_id: Option<&str>,
        ai: Option<AiAccess>,
    ) -> Result<TtsBegin, AppError> {
        let lang = self.lang(lang)?;
        let text = checked_text(text)?;
        let client = ai.and_then(Result::ok);
        let status = client.as_ref().map(|client| client.status());
        let source = choose_source(&self.settings, status.as_ref(), &self.audio.natural, lang);
        if let (VoiceSource::Natural, Some(client)) = (source, client)
            && let Some((model, voice)) = natural_voice(&self.settings, &self.audio.natural, lang)
        {
            let honoured = self.audio.natural.speed_honoured;
            let slow_speed = Some(self.settings.audio.slow_speed)
                .filter(|speed| speed.is_finite() && *speed > 0.0)
                .unwrap_or(1.0);
            let speed = if slow && honoured { slow_speed } else { 1.0 };
            let stretch = if slow && !honoured {
                (1.0 / slow_speed) as f32
            } else {
                1.0
            };
            let key = speech_key(client.kind(), &model, text, &voice, speed);
            if let Ok(bytes) = self.env.fs.read(Path::new(&audio_file(&key))) {
                self.play_bytes(&bytes, stretch)?;
                return Ok(TtsBegin::Done(natural(true)));
            }
            // Its prefetch, or a play a moment earlier, is fetching this very sentence: asking
            // again would buy and charge it twice.
            if let Some(flight) = self.fetching(&key) {
                return Ok(TtsBegin::Waiting(SpeechWait { flight }));
            }
            let flight = self.begin_fetch(&key);
            return Ok(TtsBegin::Pending(SpeechJob {
                request: Box::new(self.speech_attempt(
                    client.kind(),
                    &model,
                    text,
                    &voice,
                    speed,
                    &key,
                )),
                client,
                key,
                decoder: self.audio.decoder.clone(),
                text: text.to_string(),
                lang,
                slow,
                voice,
                stretch,
                session_id: session_id.map(str::to_string),
                flight,
            }));
        }
        self.os_play(text, lang, slow).map(TtsBegin::Done)
    }

    /// The fetch of `key` in progress, if any; fetches that have ended are forgotten first.
    fn fetching(&mut self, key: &str) -> Option<Arc<Flight>> {
        self.flights.retain(|_, flight| !flight.has_ended());
        self.flights.get(key).cloned()
    }

    /// Marks `key` as being fetched until the job given the returned flight is completed or
    /// dropped.
    fn begin_fetch(&mut self, key: &str) -> Arc<Flight> {
        let flight = Arc::new(Flight::default());
        self.flights.insert(key.to_string(), flight.clone());
        flight
    }

    /// The P5 request of a sentence. Once replies for it have failed §6.7, the next request is
    /// attempt n + 1 with its own idempotency key (docs/PUBLIK-INTEGRATION.md §6.1), so publik API
    /// answers it afresh instead of replaying the unusable clip and its charge. The cache key never
    /// carries the attempt, so a valid clip is stored under the plain key.
    fn speech_attempt(
        &self,
        provider: ProviderKind,
        model: &str,
        text: &str,
        voice: &str,
        speed: f64,
        key: &str,
    ) -> SpeechRequest {
        let mut request = speech_request(provider, model, text, voice, speed);
        let failed = open_existing_cache(
            &self.config.data_dir.join(AI_CACHE_FILE),
            self.env.clock.now_ms(),
        )
        .map_or(0, |cache| cache.attempts(key));
        if failed > 0 {
            let key_json = speech_key_json(provider, model, text, voice, speed);
            request.idempotency_key = Some(idempotency_key(&key_json, failed + 1));
        }
        request
    }

    /// Counts a reply that failed §6.7 in `ai_attempts`, so the next request for the sentence is a
    /// new attempt. A cache that cannot be written loses only the count.
    fn count_failed_reply(&self, job: &SpeechJob) {
        let cache = open_cache(
            &self.config.data_dir.join(AI_CACHE_FILE),
            self.env.clock.now_ms(),
        );
        let _ = cache.add_attempt(&job.key);
    }

    /// Records a reply's charge: in `speech_charges` (the measured cost line) and, when the job
    /// names a lesson in progress, in that session's cost. A store that cannot be written loses
    /// only the record.
    fn record_charge(&mut self, job: &SpeechJob, charge: Option<i64>) {
        let Some(charge) = charge else {
            return;
        };
        let cache = open_cache(
            &self.config.data_dir.join(AI_CACHE_FILE),
            self.env.clock.now_ms(),
        );
        let _ = cache.add_speech_charge(&job.key, charge);
        if let Some(session_id) = &job.session_id
            && (self.runs.contains_key(session_id) || self.story_runs.contains_key(session_id))
            && let Ok(store) = open_store(&mut self.store)
        {
            let _ = store.add_session_cost(session_id, charge);
        }
    }

    /// Writes valid audio to `cache/audio/<key>.mp3` (a part file renamed into place, so a file
    /// there is always whole) and records it in `audio_cache`. A file that cannot be written loses
    /// only the saving.
    fn store_audio(&self, job: &SpeechJob, bytes: &[u8]) {
        let file = audio_file(&job.key);
        let part = format!("{file}.part");
        let fs = &*self.env.fs;
        let written = match fs.write(Path::new(&part), bytes) {
            Ok(()) => {
                fs.rename(Path::new(&part), Path::new(&file)).is_ok() || {
                    let _ = fs.remove(Path::new(&part));
                    fs.write(Path::new(&file), bytes).is_ok()
                }
            }
            Err(_) => false,
        };
        if !written {
            return;
        }
        let cache = open_cache(
            &self.config.data_dir.join(AI_CACHE_FILE),
            self.env.clock.now_ms(),
        );
        let _ = cache.put_audio(&AudioEntry {
            key: &job.key,
            provider: job.client.kind(),
            voice: &job.voice,
            speed: job.request.speed,
            file: &file,
            bytes: bytes.len() as u64,
        });
    }

    /// The OS voice in place of a natural one, with the notice or the AI error to show.
    fn fall_back(
        &mut self,
        job: &SpeechJob,
        notice: Option<&str>,
        ai_error: Option<AppError>,
    ) -> Result<TtsPlayed, AppError> {
        let mut played = self.os_play(&job.text, job.lang, job.slow)?;
        played.notice = notice.map(str::to_string);
        played.ai_error = ai_error;
        Ok(played)
    }

    /// The last step of `tts_play`. A valid reply is stored, recorded, its charge counted, and
    /// played. A reply that fails §6.7 is counted (it was charged), stored nowhere, and the OS
    /// voice speaks with "Natural voice unavailable, using the system voice."; a 402 falls back
    /// with its error for the banner and no notice; any other error falls back with the notice.
    pub fn tts_complete(
        &mut self,
        job: SpeechJob,
        outcome: SpeechOutcome,
    ) -> Result<TtsPlayed, AppError> {
        match outcome.reply {
            Ok(reply) => {
                self.record_charge(&job, reply.charge_micros);
                if outcome.valid {
                    self.store_audio(&job, &reply.bytes);
                    self.play_bytes(&reply.bytes, job.stretch)?;
                    Ok(natural(false))
                } else {
                    self.count_failed_reply(&job);
                    self.fall_back(&job, Some(NATURAL_UNAVAILABLE), None)
                }
            }
            Err(error @ AiError::NeedsCredit { .. }) => {
                self.fall_back(&job, None, Some(error.into()))
            }
            Err(_) => self.fall_back(&job, Some(NATURAL_UNAVAILABLE), None),
        }
    }

    /// `tts_play` in one go (the CLI and tests): begin, then the call and its completion when
    /// there is one, or the wait for the fetch of the same sentence and a new begin. The service
    /// is held throughout, so a wait here ends only when the job fetching the sentence is dropped
    /// elsewhere; the window's command waits with no lock held instead.
    pub fn tts_play_with(
        &mut self,
        text: &str,
        lang: &str,
        slow: bool,
        session_id: Option<&str>,
        ai: Option<AiAccess>,
    ) -> Result<TtsPlayed, AppError> {
        loop {
            match self.tts_begin(text, lang, slow, session_id, ai.clone())? {
                TtsBegin::Done(played) => return Ok(played),
                TtsBegin::Pending(job) => {
                    let outcome = job.run();
                    return self.tts_complete(job, outcome);
                }
                TtsBegin::Waiting(wait) => wait.wait(),
            }
        }
    }

    /// Stops the clip and the OS voice.
    pub fn tts_stop(&mut self) {
        self.audio.player.stop();
        self.audio.os.stop();
    }

    /// Listening exercises are off (**Can't listen now**) at this moment.
    fn listen_skipped(&self) -> bool {
        self.skips
            .get("listen")
            .is_some_and(|until| self.env.clock.now_ms() < *until)
    }

    /// The first step of `tts_prefetch`: the sentence of the exercise after `exercise_id` in the
    /// run's order (a `listen_type`'s text, or with autoplay on a `pick_meaning` or
    /// `translate_to_en` prompt), fetched at normal speed when the source is natural and the
    /// sentence is neither cached nor being fetched; nothing otherwise. A run that is not in
    /// progress is "This lesson is no longer in progress."
    pub fn prefetch_begin(
        &mut self,
        session_id: &str,
        exercise_id: &str,
        ai: Option<AiAccess>,
    ) -> Result<PrefetchBegin, AppError> {
        let nothing = PrefetchBegin::Done(Prefetched { prefetched: false });
        let active = self.runs.get(session_id).ok_or_else(not_in_progress)?;
        let lang = active.lang;
        let exercises = active.run.exercises();
        let autoplay = self.settings.audio.autoplay;
        let listening = !self.listen_skipped();
        let sentence = exercises
            .iter()
            .position(|ex| ex.id == exercise_id)
            .and_then(|at| exercises.get(at + 1))
            .and_then(|next| match &next.body {
                Body::ListenType { text, .. } if listening => Some(text.clone()),
                Body::PickMeaning { prompt_target, .. }
                | Body::TranslateToEn { prompt_target, .. }
                    if autoplay =>
                {
                    Some(prompt_target.clone())
                }
                _ => None,
            });
        let Some(sentence) = sentence else {
            return Ok(nothing);
        };
        let Ok(text) = checked_text(&sentence) else {
            return Ok(nothing);
        };
        // An automatic call: never without a ready client, never while needs-credit is set.
        let Some(Ok(client)) = ai else {
            return Ok(nothing);
        };
        let status = client.status();
        if choose_source(&self.settings, Some(&status), &self.audio.natural, lang)
            != VoiceSource::Natural
        {
            return Ok(nothing);
        }
        let Some((model, voice)) = natural_voice(&self.settings, &self.audio.natural, lang) else {
            return Ok(nothing);
        };
        let speed = 1.0;
        let key = speech_key(client.kind(), &model, text, &voice, speed);
        if self.env.fs.read(Path::new(&audio_file(&key))).is_ok() || self.fetching(&key).is_some() {
            return Ok(nothing);
        }
        let flight = self.begin_fetch(&key);
        Ok(PrefetchBegin::Pending(SpeechJob {
            request: Box::new(self.speech_attempt(
                client.kind(),
                &model,
                text,
                &voice,
                speed,
                &key,
            )),
            client,
            key,
            decoder: self.audio.decoder.clone(),
            text: text.to_string(),
            lang,
            slow: false,
            voice,
            stretch: 1.0,
            session_id: Some(session_id.to_string()),
            flight,
        }))
    }

    /// The last step of `tts_prefetch`: a valid reply is stored and recorded exactly as
    /// `tts_complete` stores it, and nothing plays; a failure stores nothing and is not shown
    /// (its charge still counts).
    pub fn prefetch_complete(
        &mut self,
        job: SpeechJob,
        outcome: SpeechOutcome,
    ) -> Result<Prefetched, AppError> {
        let prefetched = match outcome.reply {
            Ok(reply) => {
                self.record_charge(&job, reply.charge_micros);
                if outcome.valid {
                    self.store_audio(&job, &reply.bytes);
                } else {
                    self.count_failed_reply(&job);
                }
                outcome.valid
            }
            Err(_) => false,
        };
        Ok(Prefetched { prefetched })
    }

    /// `tts_prefetch` in one go: begin, the call when there is one, complete.
    pub fn tts_prefetch_with(
        &mut self,
        session_id: &str,
        exercise_id: &str,
        ai: Option<AiAccess>,
    ) -> Result<Prefetched, AppError> {
        match self.prefetch_begin(session_id, exercise_id, ai)? {
            PrefetchBegin::Done(done) => Ok(done),
            PrefetchBegin::Pending(job) => {
                let outcome = job.run();
                self.prefetch_complete(job, outcome)
            }
        }
    }

    /// **Can't listen now** (`listen`): listening exercises are off for 60 minutes of the
    /// injected clock; `speak` records the same for the speaking checks of module 10.
    pub fn session_skip(&mut self, kind: &str) -> Result<(), AppError> {
        let until = self.env.clock.now_ms().saturating_add(SKIP_MS);
        let kind = match kind {
            "listen" => "listen",
            "speak" => "speak",
            other => return Err(unknown_value(other, "kind")),
        };
        self.skips.insert(kind, until);
        Ok(())
    }

    /// The measured cost line (the mean of this install's recorded charges, or the setup run's)
    /// and, while listening is off, until when. Reading never creates the cache file.
    pub fn audio_status(&self) -> AudioStatus {
        let now = self.env.clock.now_ms();
        let charges = open_existing_cache(&self.config.data_dir.join(AI_CACHE_FILE), now)
            .map(|cache| cache.speech_charges())
            .unwrap_or_default();
        AudioStatus {
            cost_line: speech_cost_line(&charges),
            listen_off_until: self
                .skips
                .get("listen")
                .copied()
                .filter(|until| now < *until),
        }
    }

    /// Not skipped, and an OS voice for `lang` exists or `audio.voice_source = natural` with a
    /// natural voice configured for `lang` (`natural_voice` is Some).
    pub fn listen_available(&self, lang: Lang) -> bool {
        if self.listen_skipped() {
            return false;
        }
        !voices_for(&self.audio.os.list(), lang).is_empty()
            || (self.settings.audio.voice_source == "natural"
                && natural_voice(&self.settings, &self.audio.natural, lang).is_some())
    }

    /// Removes every file in `cache/audio/` and the `audio_cache` rows; the number of files
    /// removed. Every removal is attempted; the first failure is reported.
    pub(crate) fn clear_audio_files(&mut self) -> Result<u32, AppError> {
        let fs = &*self.env.fs;
        let names = fs.list(Path::new(AUDIO_DIR)).map_err(|_| cannot_write())?;
        let mut removed = 0;
        let mut failure = None;
        for name in names {
            match remove_file(fs, &format!("{AUDIO_DIR}/{name}")) {
                Ok(true) => removed += 1,
                Ok(false) => {}
                Err(e) => {
                    failure.get_or_insert(e);
                }
            }
        }
        if let Some(cache) = open_existing_cache(
            &self.config.data_dir.join(AI_CACHE_FILE),
            self.env.clock.now_ms(),
        ) {
            let _ = cache.clear_audio();
        }
        match failure {
            Some(e) => Err(e),
            None => Ok(removed),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::{Arc, Mutex};

    use super::super::ActiveRun;
    use super::super::test_support::{MONDAY_NOON, NO_PACKS, parley_at};
    use super::*;
    use crate::ai::{
        AiStatus, ChatReply, ChatRequest, ProviderKind, TranscribeRequest, TranscriptReply,
    };
    use crate::audio::test_support::wav_ms;
    use crate::audio::{AudioError, OsVoice, OsVoices, Player, RateRange, WavDecoder};
    use crate::env::FailingNet;
    use crate::exercise::{Exercise, ExerciseType, Lesson, LessonRun, Origin};
    use crate::grading::Answer;

    const SESSION: &str = "s-00000000000000aa";
    const SKILL: &str = "es.a1.u01.s01";
    const COFFEE: &str = "Quiero un café con leche.";

    /// OS voices that record what they were asked to say.
    struct Voices {
        voices: Vec<OsVoice>,
        rates: RateRange,
        spoken: Mutex<Vec<(String, String, f32)>>,
    }

    impl OsVoices for Voices {
        fn list(&self) -> Vec<OsVoice> {
            self.voices.clone()
        }
        fn rates(&self) -> RateRange {
            self.rates
        }
        fn speak(&self, text: &str, voice: &str, rate: f32) -> Result<(), AudioError> {
            self.spoken
                .lock()
                .unwrap()
                .push((text.into(), voice.into(), rate));
            Ok(())
        }
        fn stop(&self) {}
    }

    /// A player that records what it played, or fails every play.
    struct Speakers {
        played: Mutex<Vec<(Vec<u8>, f32)>>,
        broken: bool,
    }

    impl Player for Speakers {
        fn play(&self, audio: &[u8], stretch: f32) -> Result<(), AudioError> {
            if self.broken {
                return Err(AudioError::NoDevice);
            }
            self.played.lock().unwrap().push((audio.to_vec(), stretch));
            Ok(())
        }
        fn stop(&self) {}
    }

    /// A publik API client that answers speech from a list; a 402 sets needs-credit, as the
    /// real client's status does.
    struct Speech {
        replies: Mutex<Vec<Result<AudioReply, AiError>>>,
        seen: Mutex<Vec<SpeechRequest>>,
        status: Mutex<AiStatus>,
    }

    impl Speech {
        fn new(replies: Vec<Result<AudioReply, AiError>>) -> Arc<Speech> {
            Arc::new(Speech {
                replies: Mutex::new(replies),
                seen: Mutex::new(Vec::new()),
                status: Mutex::new(AiStatus {
                    provider: "publik".into(),
                    connected: true,
                    ..AiStatus::default()
                }),
            })
        }
        fn requests(&self) -> usize {
            self.seen.lock().unwrap().len()
        }
    }

    impl AiClient for Speech {
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
            self.seen.lock().unwrap().push(req);
            let mut replies = self.replies.lock().unwrap();
            let reply = if replies.is_empty() {
                Err(AiError::Offline)
            } else {
                replies.remove(0)
            };
            if matches!(reply, Err(AiError::NeedsCredit { .. })) {
                self.status.lock().unwrap().needs_credit = true;
            }
            reply
        }
        fn kind(&self) -> ProviderKind {
            ProviderKind::Publik
        }
        fn status(&self) -> AiStatus {
            self.status.lock().unwrap().clone()
        }
        fn chat_model(&self, tier: &str) -> String {
            tier.to_string()
        }
    }

    fn access(client: &Arc<Speech>) -> Option<AiAccess> {
        let client: Arc<dyn AiClient> = client.clone();
        Some(Ok(client))
    }

    fn wav(ms: u32, charge: Option<i64>) -> Result<AudioReply, AiError> {
        Ok(AudioReply {
            bytes: wav_ms(ms),
            content_type: "audio/wav".into(),
            charge_micros: charge,
        })
    }

    fn voice(id: &str, tag: &str) -> OsVoice {
        OsVoice {
            id: id.into(),
            name: format!("Voice {id}"),
            lang_tag: tag.into(),
        }
    }

    struct Rig {
        _dir: tempfile::TempDir,
        dir: std::path::PathBuf,
        app: Parley,
        now: Arc<AtomicI64>,
        voices: Arc<Voices>,
        speakers: Arc<Speakers>,
    }

    fn rig_with(voices: Vec<OsVoice>, broken: bool) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let now = Arc::new(AtomicI64::new(MONDAY_NOON));
        let mut app = parley_at(dir.path(), NO_PACKS, Box::new(FailingNet), now.clone());
        let voices = Arc::new(Voices {
            voices,
            rates: RateRange {
                min: 0.5,
                normal: 1.0,
                max: 6.0,
            },
            spoken: Mutex::new(Vec::new()),
        });
        let speakers = Arc::new(Speakers {
            played: Mutex::new(Vec::new()),
            broken,
        });
        app.set_audio(Audio {
            os: voices.clone(),
            player: speakers.clone(),
            decoder: Arc::new(WavDecoder),
            natural: crate::ai::bundled_voices(),
        });
        Rig {
            dir: dir.path().to_path_buf(),
            _dir: dir,
            app,
            now,
            voices,
            speakers,
        }
    }

    fn rig() -> Rig {
        rig_with(
            vec![
                voice("es1", "es-ES"),
                voice("es2", "es-MX"),
                voice("fr1", "fr-FR"),
            ],
            false,
        )
    }

    fn natural_on(app: &mut Parley) {
        let patch =
            serde_json::json!({"audio": {"voice_source": "natural"}, "ai": {"provider": "publik"}});
        app.settings_set(&patch).unwrap();
    }

    fn cost_of(app: &Parley, session: &str) -> i64 {
        let tables = app.store.as_ref().unwrap().dump().unwrap();
        tables
            .sessions
            .iter()
            .find(|s| s.id == session)
            .map_or(0, |s| s.cost_micros)
    }

    /// A run of three exercises (pick, listen, translate) in progress under SESSION.
    fn with_run(app: &mut Parley) {
        app.course_start("es").unwrap();
        let ex = |id: &str, body: Body| Exercise {
            id: id.into(),
            item_id: format!("{SKILL}#l:hola"),
            facet: body.kind().facet(),
            origin: Origin::Template,
            body,
        };
        let lesson = Lesson {
            schema: Lesson::SCHEMA.into(),
            lang: Lang::Es,
            skill_id: SKILL.into(),
            seq: 1,
            origin: Origin::Template,
            exercises: vec![
                ex(
                    "e1",
                    Body::PickMeaning {
                        prompt_target: "hola".into(),
                        options: vec!["hello".into(), "bye".into()],
                        answer_index: 0,
                    },
                ),
                ex(
                    "e2",
                    Body::ListenType {
                        text: COFFEE.into(),
                        accepted: vec![COFFEE.into()],
                    },
                ),
                ex(
                    "e3",
                    Body::TranslateToEn {
                        prompt_target: "Buenos días.".into(),
                        accepted: vec!["Good morning.".into()],
                    },
                ),
            ],
            ai: None,
        };
        let store = open_store(&mut app.store).unwrap();
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
    fn voices_are_listed_per_language_with_the_natural_one_when_chosen() {
        let mut r = rig();
        let es = r.app.voices_list("es").unwrap();
        assert_eq!(
            es.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(),
            ["es1", "es2"]
        );
        assert!(es.iter().all(|v| v.source == "os" && v.installed));
        assert_eq!(
            r.app.voices_list("it").unwrap_err().message,
            "Unknown value it for lang."
        );
        natural_on(&mut r.app);
        let es = r.app.voices_list("es").unwrap();
        assert_eq!(
            es.last().unwrap(),
            &Voice {
                id: "alloy".into(),
                name: "alloy".into(),
                source: "natural".into(),
                installed: true
            }
        );
    }

    #[test]
    fn the_os_voice_speaks_at_the_normal_or_slow_rate() {
        let mut r = rig();
        let played = r
            .app
            .tts_play_with(&format!("  {COFFEE} "), "es", true, None, None)
            .unwrap();
        assert_eq!(played, spoken_by_os());
        r.app
            .tts_play_with(COFFEE, "es", false, None, None)
            .unwrap();
        r.app
            .settings_set(&serde_json::json!({"audio": {"os_voice": {"es": "es2"}}}))
            .unwrap();
        r.app
            .tts_play_with(COFFEE, "es", false, None, None)
            .unwrap();
        r.app
            .settings_set(&serde_json::json!({"audio": {"os_voice": {"es": "gone"}}}))
            .unwrap();
        r.app
            .tts_play_with(COFFEE, "es", false, None, None)
            .unwrap();
        let spoken = r.voices.spoken.lock().unwrap().clone();
        assert_eq!(spoken.len(), 4);
        assert_eq!(spoken[0].0, COFFEE, "the text is trimmed");
        assert!((spoken[0].2 - 0.75).abs() < 1e-6);
        assert_eq!(spoken[1].2, 1.0);
        assert_eq!(
            spoken.iter().map(|s| s.1.as_str()).collect::<Vec<_>>(),
            ["es1", "es1", "es2", "es1"]
        );
        assert!(r.speakers.played.lock().unwrap().is_empty());
    }

    #[test]
    fn refusals_have_their_messages() {
        let mut r = rig_with(vec![voice("fr1", "fr-FR")], false);
        let message = |e: AppError| (e.code, e.message);
        assert_eq!(
            message(
                r.app
                    .tts_play_with(" \n ", "es", false, None, None)
                    .unwrap_err()
            ),
            (
                ErrorCode::InvalidInput,
                "There is no text to read aloud.".to_string()
            )
        );
        let long = "a".repeat(501);
        assert_eq!(
            message(
                r.app
                    .tts_play_with(&long, "es", false, None, None)
                    .unwrap_err()
            ),
            (
                ErrorCode::InvalidInput,
                "This text is too long to read aloud.".to_string()
            )
        );
        assert_eq!(
            message(
                r.app
                    .tts_play_with(COFFEE, "es", false, None, None)
                    .unwrap_err()
            ),
            (
                ErrorCode::NoVoice,
                no_voice_message(Lang::Es, Platform::current())
            )
        );
        assert_eq!(
            r.app
                .tts_play_with(COFFEE, "xx", false, None, None)
                .unwrap_err()
                .message,
            "Unknown value xx for lang."
        );
        assert!(!r.app.listen_available(Lang::Es));
        assert_eq!(
            r.app.session_skip("sing").unwrap_err().message,
            "Unknown value sing for kind."
        );
        let gone = r.app.tts_prefetch_with("s-1", "e1", None).unwrap_err();
        assert_eq!(
            message(gone),
            (
                ErrorCode::NotFound,
                "This lesson is no longer in progress.".to_string()
            )
        );
    }

    #[test]
    fn a_player_that_fails_gives_the_speaker_message() {
        let mut r = rig_with(vec![voice("es1", "es-ES")], true);
        natural_on(&mut r.app);
        let client = Speech::new(vec![wav(2_000, None)]);
        let error = r
            .app
            .tts_play_with(COFFEE, "es", false, None, access(&client))
            .unwrap_err();
        assert_eq!(
            (error.code, error.message.as_str()),
            (
                ErrorCode::NoVoice,
                "Parley could not play the sound. Check that a speaker or headphones are connected."
            )
        );
    }

    #[test]
    fn a_natural_sentence_is_fetched_once_cached_and_charged_to_the_lesson() {
        let mut r = rig();
        natural_on(&mut r.app);
        with_run(&mut r.app);
        let client = Speech::new(vec![wav(2_000, Some(600))]);
        let TtsBegin::Pending(job) = r
            .app
            .tts_begin(COFFEE, "es", true, Some(SESSION), access(&client))
            .unwrap()
        else {
            panic!("a call")
        };
        assert_eq!(
            job.request().body(),
            serde_json::json!({"model": "tts-1", "input": COFFEE, "voice": "alloy", "response_format": "mp3", "speed": 0.75})
        );
        let outcome = job.run();
        let played = r.app.tts_complete(job, outcome).unwrap();
        assert_eq!(played, natural(false));
        let again = r
            .app
            .tts_play_with(COFFEE, "es", true, Some(SESSION), access(&client))
            .unwrap();
        assert_eq!(again, natural(true));
        assert_eq!(client.requests(), 1, "the second play makes no request");
        let plays = r.speakers.played.lock().unwrap().clone();
        assert_eq!(plays.len(), 2);
        assert_eq!(plays[0].0, wav_ms(2_000));
        assert_eq!(
            plays[0].1, 1.0,
            "speed is honoured, so nothing is stretched"
        );
        assert_eq!(cost_of(&r.app, SESSION), 600);
        let key = speech_key(ProviderKind::Publik, "tts-1", COFFEE, "alloy", 0.75);
        assert!(r.dir.join(audio_file(&key)).is_file());
        assert!(!r.dir.join(format!("{}.part", audio_file(&key))).exists());
        let cache = open_existing_cache(&r.dir.join(AI_CACHE_FILE), 0).unwrap();
        assert_eq!((cache.audio_len(), cache.speech_charges()), (1, vec![600]));
        // Offline later: the cached sentence still plays; another one falls back.
        let offline = Speech::new(vec![]);
        let cached = r
            .app
            .tts_play_with(COFFEE, "es", true, None, access(&offline))
            .unwrap();
        assert_eq!((cached.cached, offline.requests()), (true, 0));
        let uncached = r
            .app
            .tts_play_with("Hola.", "es", false, None, access(&offline))
            .unwrap();
        assert_eq!(uncached.source, VoiceSource::Os);
        assert_eq!(uncached.notice.as_deref(), Some(NATURAL_UNAVAILABLE));
    }

    #[test]
    fn unusable_audio_falls_back_and_still_counts_its_charge() {
        let mut r = rig();
        natural_on(&mut r.app);
        with_run(&mut r.app);
        let html = Ok(AudioReply {
            bytes: wav_ms(2_000),
            content_type: "text/html".into(),
            charge_micros: Some(640),
        });
        let client = Speech::new(vec![html, wav(50, None)]);
        for _ in 0..2 {
            let played = r
                .app
                .tts_play_with(COFFEE, "es", false, Some(SESSION), access(&client))
                .unwrap();
            assert_eq!(played.source, VoiceSource::Os);
            assert_eq!(played.notice.as_deref(), Some(NATURAL_UNAVAILABLE));
            assert_eq!(played.ai_error, None);
        }
        assert_eq!(client.requests(), 2);
        assert_eq!(cost_of(&r.app, SESSION), 640);
        assert!(r.speakers.played.lock().unwrap().is_empty());
        let cache = open_existing_cache(&r.dir.join(AI_CACHE_FILE), 0).unwrap();
        assert_eq!((cache.audio_len(), cache.speech_charges()), (0, vec![640]));
        assert_eq!(r.voices.spoken.lock().unwrap().len(), 2);
        assert_eq!(
            r.app.audio_status().cost_line,
            "Natural voices cost about $0.00064 a sentence (measured)."
        );
    }

    #[test]
    fn a_402_shows_once_and_then_the_os_voice_needs_no_request() {
        let mut r = rig();
        natural_on(&mut r.app);
        with_run(&mut r.app);
        let credit = AiError::NeedsCredit {
            message: "Add credit to keep going.".into(),
            top_up_url: "https://example.invalid/top-up".into(),
        };
        let client = Speech::new(vec![Err(credit)]);
        let first = r
            .app
            .tts_play_with(COFFEE, "es", false, Some(SESSION), access(&client))
            .unwrap();
        assert_eq!(
            (first.source, first.notice.as_deref()),
            (VoiceSource::Os, None)
        );
        assert_eq!(first.ai_error.map(|e| e.code), Some(ErrorCode::NeedsCredit));
        let second = r
            .app
            .tts_play_with("Hola.", "es", false, Some(SESSION), access(&client))
            .unwrap();
        assert_eq!(second, spoken_by_os());
        let prefetch = r
            .app
            .tts_prefetch_with(SESSION, "e1", access(&client))
            .unwrap();
        assert!(!prefetch.prefetched);
        assert_eq!(client.requests(), 1);
    }

    #[test]
    fn slow_mode_stretches_locally_when_speed_is_not_honoured() {
        let mut r = rig();
        let mut audio = r.app.audio.clone();
        audio.natural.speed_honoured = false;
        r.app.set_audio(audio);
        natural_on(&mut r.app);
        let client = Speech::new(vec![wav(2_000, None)]);
        let slow = r
            .app
            .tts_play_with(COFFEE, "es", true, None, access(&client))
            .unwrap();
        assert_eq!(slow, natural(false));
        assert_eq!(client.seen.lock().unwrap()[0].speed, 1.0);
        let normal = r
            .app
            .tts_play_with(COFFEE, "es", false, None, access(&client))
            .unwrap();
        assert_eq!(normal, natural(true), "normal and slow share one file");
        let plays = r.speakers.played.lock().unwrap().clone();
        assert!((plays[0].1 - 1.0 / 0.75).abs() < 1e-6);
        assert_eq!(plays[1].1, 1.0);
    }

    #[test]
    fn prefetch_fetches_the_next_sentence_once() {
        let mut r = rig();
        natural_on(&mut r.app);
        with_run(&mut r.app);
        let client = Speech::new(vec![wav(2_000, Some(540)), wav(1_000, None)]);
        let prefetch = |r: &mut Rig, id: &str| {
            r.app
                .tts_prefetch_with(SESSION, id, access(&client))
                .unwrap()
                .prefetched
        };
        assert!(prefetch(&mut r, "e1"), "e2 is a listen_type");
        assert!(!prefetch(&mut r, "e1"), "already cached");
        assert!(prefetch(&mut r, "e2"), "autoplay is on: e3's prompt");
        assert!(!prefetch(&mut r, "e3"), "nothing after the last");
        assert!(!prefetch(&mut r, "e9"));
        assert_eq!(client.requests(), 2);
        assert_eq!(client.seen.lock().unwrap()[0].input, COFFEE);
        assert_eq!(client.seen.lock().unwrap()[0].speed, 1.0);
        assert_eq!(cost_of(&r.app, SESSION), 540);
        assert!(
            r.speakers.played.lock().unwrap().is_empty(),
            "nothing plays"
        );
        let played = r
            .app
            .tts_play_with(COFFEE, "es", false, Some(SESSION), access(&client))
            .unwrap();
        assert!(played.cached);
        // With autoplay off a prompt is not prefetched; with the OS source nothing is.
        let patch = serde_json::json!({"audio": {"autoplay": false, "voice_source": "os"}});
        r.app.settings_set(&patch).unwrap();
        assert!(!prefetch(&mut r, "e2"));
        assert!(
            !r.app
                .tts_prefetch_with(SESSION, "e1", None)
                .unwrap()
                .prefetched
        );
        assert_eq!(client.requests(), 2);
    }

    #[test]
    fn a_fetch_ends_with_its_job_completed_or_dropped_and_is_then_forgotten() {
        let mut r = rig();
        natural_on(&mut r.app);
        let client = Speech::new(vec![wav(2_000, None), wav(1_000, None)]);
        let begin = |app: &mut Parley, text: &str| {
            app.tts_begin(text, "es", false, None, access(&client))
                .unwrap()
        };
        let TtsBegin::Pending(coffee) = begin(&mut r.app, COFFEE) else {
            panic!("a call")
        };
        let TtsBegin::Pending(hola) = begin(&mut r.app, "Hola.") else {
            panic!("a call")
        };
        let completed = r.app.flights[&coffee.key].clone();
        let dropped = r.app.flights[&hola.key].clone();
        assert!(!completed.has_ended() && !dropped.has_ended());
        let TtsBegin::Waiting(_) = begin(&mut r.app, COFFEE) else {
            panic!("the sentence is being fetched")
        };

        let outcome = coffee.run();
        r.app.tts_complete(coffee, outcome).unwrap();
        assert!(completed.has_ended(), "a completed job ends its fetch");
        completed.wait();
        drop(hola);
        assert!(dropped.has_ended(), "so does a job dropped unfinished");
        assert_eq!(r.app.flights.len(), 2, "kept until a begin looks");

        // The next begin that looks forgets both and fetches the dropped sentence afresh.
        let TtsBegin::Pending(again) = begin(&mut r.app, "Hola.") else {
            panic!("a call")
        };
        assert_eq!(
            r.app.flights.keys().collect::<Vec<_>>(),
            [&again.key],
            "only the new fetch is kept"
        );
        let TtsBegin::Done(played) = begin(&mut r.app, COFFEE) else {
            panic!("the stored clip plays")
        };
        assert_eq!(played, natural(true));
        assert_eq!(
            client.requests(),
            1,
            "dropped and waiting begins asked for nothing"
        );
    }

    /// A lesson's kinds, every answer right, so the next lesson moves on.
    fn lesson_kinds(app: &mut Parley) -> Vec<ExerciseType> {
        let started = app.lesson_start(SKILL, "learn").unwrap();
        for ex in &started.exercises {
            let answer = match &ex.body {
                Body::PickMeaning { answer_index, .. } => Answer::Choice(*answer_index),
                Body::MatchPairs { pairs } => {
                    Answer::Pairs((0..pairs.len() as u32).map(|i| [i, i]).collect())
                }
                Body::FillGap { answer, .. } => Answer::Text(answer.clone()),
                Body::TranslateToTarget { target, .. } => Answer::Text(target.clone()),
                Body::TranslateToEn { accepted, .. } => Answer::Text(accepted[0].clone()),
                Body::ListenType { text, .. } | Body::Speak { text, .. } => {
                    Answer::Text(text.clone())
                }
            };
            app.exercise_submit(&started.session_id, &ex.id, &answer, Some(5_000), false)
                .unwrap();
        }
        app.lesson_finish(&started.session_id).unwrap();
        started.exercises.iter().map(Exercise::kind).collect()
    }

    #[test]
    fn cant_listen_now_turns_listening_off_for_an_hour() {
        let mut on = rig();
        let mut off = rig();
        on.app.course_start("es").unwrap();
        off.app.course_start("es").unwrap();
        assert!(off.app.listen_available(Lang::Es));
        assert!(!off.app.listen_available(Lang::De));
        off.app.session_skip("listen").unwrap();
        off.app.session_skip("speak").unwrap();
        assert!(!off.app.listen_available(Lang::Es));
        assert!(on.app.listen_available(Lang::Es));
        assert_eq!(
            off.app.audio_status().listen_off_until,
            Some(MONDAY_NOON + SKIP_MS)
        );
        // The same lessons (the same seed), with translate_to_en where listen_type was.
        let heard: Vec<ExerciseType> = (0..3).flat_map(|_| lesson_kinds(&mut on.app)).collect();
        let read: Vec<ExerciseType> = (0..3).flat_map(|_| lesson_kinds(&mut off.app)).collect();
        assert!(heard.contains(&ExerciseType::ListenType), "{heard:?}");
        assert!(!read.contains(&ExerciseType::ListenType), "{read:?}");
        assert_eq!(heard.len(), read.len());
        for (with, without) in heard.iter().zip(&read) {
            if *with == ExerciseType::ListenType {
                assert_eq!(*without, ExerciseType::TranslateToEn);
            } else {
                assert_eq!(with, without);
            }
        }
        off.now.fetch_add(SKIP_MS - 1, Ordering::SeqCst);
        assert!(!off.app.listen_available(Lang::Es));
        off.now.fetch_add(1, Ordering::SeqCst);
        assert!(off.app.listen_available(Lang::Es));
        assert_eq!(off.app.audio_status().listen_off_until, None);
        // Natural voices make listening available without an OS voice.
        let mut none = rig_with(vec![], false);
        assert!(!none.app.listen_available(Lang::Es));
        natural_on(&mut none.app);
        assert!(none.app.listen_available(Lang::Es));
    }

    #[test]
    fn clearing_the_audio_cache_removes_files_and_rows_and_keeps_progress() {
        let mut r = rig();
        natural_on(&mut r.app);
        with_run(&mut r.app);
        let client = Speech::new(vec![wav(2_000, Some(600)), wav(1_000, Some(600))]);
        r.app
            .tts_play_with(COFFEE, "es", false, Some(SESSION), access(&client))
            .unwrap();
        r.app
            .tts_play_with("Hola.", "es", false, Some(SESSION), access(&client))
            .unwrap();
        let before = r.app.store.as_ref().unwrap().dump().unwrap();
        assert_eq!(r.app.cache_clear("audio").unwrap().files_removed, 2);
        let cache = open_existing_cache(&r.dir.join(AI_CACHE_FILE), 0).unwrap();
        assert_eq!(cache.audio_len(), 0);
        assert_eq!(
            r.app.store.as_ref().unwrap().dump().unwrap(),
            before,
            "progress is never touched"
        );
        assert_eq!(r.app.cache_clear("audio").unwrap().files_removed, 0);
        let again = Speech::new(vec![wav(2_000, None)]);
        r.app
            .tts_play_with(COFFEE, "es", false, Some(SESSION), access(&again))
            .unwrap();
        assert_eq!(r.app.cache_clear("all").unwrap().files_removed, 2);
        assert!(!r.dir.join(AI_CACHE_FILE).exists());
        assert_eq!(
            r.app.audio_status().cost_line,
            "Natural voices cost about $0.00054 a sentence (measured)."
        );
        assert!(
            !r.dir.join(AI_CACHE_FILE).exists(),
            "reading the cost creates no file"
        );
    }
}
