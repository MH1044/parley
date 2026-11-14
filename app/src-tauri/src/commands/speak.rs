//! Speaking checks (docs/modules/10-speaking-and-pronunciation.md, Commands and events):
//! `speak_check` and `speaking_status`.
//!
//! A client is asked for only when the learner checks with their AI provider (`local` false,
//! `speaking.transcriber = provider`, and `ai.provider` publik API or the own key), so a local
//! check never reaches any provider (invariants I9 and I12). The transcription runs as module
//! 08's judge does: begun with the service locked, run on a blocking thread with nothing locked,
//! completed with the service locked again; the AI status is emitted after a call.

use parley_core::api::{SpeakResult, SpeakingStatus};
use parley_core::app::{AiAccess, SpeakBegin};
use parley_core::error::AppError;
use tauri::{AppHandle, Manager, State};

use super::ai::unfinished;
use super::lesson::emit_status;
use crate::state::AppState;

/// The chosen provider's client for a speaking check, or None when none may be asked for. The
/// settings are read first; the AI service is locked only while the client is built.
fn transcribe_access(state: &AppState, local: bool) -> Option<AiAccess> {
    let settings = state.with(|p| p.settings_get());
    let wanted = !local
        && settings.speaking.transcriber == "provider"
        && matches!(settings.ai.provider.as_str(), "publik" | "own_key");
    wanted.then(|| state.with_ai(|service| service.client(&settings.ai)))
}

/// **Check** on a take of a `speak` exercise; `local` is **Use the local model**.
#[tauri::command(rename_all = "snake_case")]
pub async fn speak_check(
    app: AppHandle,
    session_id: String,
    exercise_id: String,
    recording_id: String,
    local: bool,
) -> Result<SpeakResult, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let ai = transcribe_access(&state, local);
        let asked = ai.is_some();
        match state.with(|p| p.speak_begin(&session_id, &exercise_id, &recording_id, local, ai))? {
            SpeakBegin::Done(result) => Ok(result),
            SpeakBegin::Pending(job) => {
                let provider = job.request().is_some();
                let outcome = job.run();
                let result = state.with(|p| p.speak_complete(job, outcome));
                if asked && provider {
                    emit_status(&app, &state);
                }
                result
            }
        }
    })
    .await
    .map_err(|_| unfinished())?
}

#[tauri::command(rename_all = "snake_case")]
pub fn speaking_status(state: State<'_, AppState>) -> Result<SpeakingStatus, AppError> {
    Ok(state.with(|p| p.speaking_status()))
}
