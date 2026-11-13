//! Listening (docs/modules/09-listening-and-voices.md, Commands and events): `voices_list`,
//! `tts_play`, `tts_stop`, `tts_prefetch`, `audio_status` and `session_skip`; the end of a clip or
//! of the OS voice is the `audio://ended` event, emitted by the audio objects the setup hook gives
//! the service.
//!
//! A natural voice is asked for only when the learner chose it (`audio.voice_source = natural`)
//! and the chosen provider can speak (`publik` or `own_key`); no client is built otherwise, so the
//! OS voice never reaches any provider (invariants I9 and I12). A natural sentence that is not
//! cached is fetched as module 08's judge calls: begun with the service locked, the call made on
//! a blocking thread with nothing locked, completed with the service locked again; the AI status
//! is emitted after the call.

use parley_core::api::{AudioStatus, Prefetched, TtsPlayed, Voice};
use parley_core::app::{AiAccess, PrefetchBegin, TtsBegin};
use parley_core::error::AppError;
use tauri::{AppHandle, Manager, State};

use super::ai::unfinished;
use super::lesson::emit_status;
use crate::state::AppState;

/// The chosen provider's client for a natural voice, or None when none may be asked for: the
/// system voice is chosen, or the provider cannot speak (unset or local). The settings are read
/// first; the AI service is locked only while the client is built.
fn speech_access(state: &AppState) -> Option<AiAccess> {
    let settings = state.with(|p| p.settings_get());
    let wanted = settings.audio.voice_source == "natural"
        && matches!(settings.ai.provider.as_str(), "publik" | "own_key");
    wanted.then(|| state.with_ai(|service| service.client(&settings.ai)))
}

#[tauri::command(rename_all = "snake_case")]
pub fn voices_list(state: State<'_, AppState>, lang: String) -> Result<Vec<Voice>, AppError> {
    state.with(|p| p.voices_list(&lang))
}

/// Reads `text` aloud; returns once it has started. `session_id` names the lesson a natural
/// voice's charge is added to (null outside a lesson).
#[tauri::command(rename_all = "snake_case")]
pub async fn tts_play(
    app: AppHandle,
    text: String,
    lang: String,
    slow: bool,
    session_id: Option<String>,
) -> Result<TtsPlayed, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let ai = speech_access(&state);
        match state.with(|p| p.tts_begin(&text, &lang, slow, session_id.as_deref(), ai))? {
            TtsBegin::Done(played) => Ok(played),
            TtsBegin::Pending(job) => {
                let outcome = job.run();
                let played = state.with(|p| p.tts_complete(job, outcome));
                emit_status(&app, &state);
                played
            }
        }
    })
    .await
    .map_err(|_| unfinished())?
}

#[tauri::command(rename_all = "snake_case")]
pub fn tts_stop(state: State<'_, AppState>) -> Result<(), AppError> {
    state.with(|p| p.tts_stop());
    Ok(())
}

/// Fetches the sentence of the exercise after `exercise_id` into the audio cache, when natural
/// voices are on and it is not cached yet; nothing plays.
#[tauri::command(rename_all = "snake_case")]
pub async fn tts_prefetch(
    app: AppHandle,
    session_id: String,
    exercise_id: String,
) -> Result<Prefetched, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let ai = speech_access(&state);
        match state.with(|p| p.prefetch_begin(&session_id, &exercise_id, ai))? {
            PrefetchBegin::Done(done) => Ok(done),
            PrefetchBegin::Pending(job) => {
                let outcome = job.run();
                let done = state.with(|p| p.prefetch_complete(job, outcome));
                emit_status(&app, &state);
                done
            }
        }
    })
    .await
    .map_err(|_| unfinished())?
}

#[tauri::command(rename_all = "snake_case")]
pub fn audio_status(state: State<'_, AppState>) -> Result<AudioStatus, AppError> {
    Ok(state.with(|p| p.audio_status()))
}

/// **Can't listen now** (`listen`) and module 10's **Can't speak now** (`speak`): 60 minutes.
#[tauri::command(rename_all = "snake_case")]
pub fn session_skip(state: State<'_, AppState>, kind: String) -> Result<(), AppError> {
    state.with(|p| p.session_skip(&kind))
}
