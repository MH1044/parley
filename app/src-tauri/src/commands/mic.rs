//! The microphone (docs/modules/10-speaking-and-pronunciation.md, Commands and events):
//! `mic_start`, `mic_stop` and `recording_play`. A take is kept on this computer and sent nowhere
//! (invariant I13); only the learner's **Check** sends it.
//!
//! `mic_start`'s callbacks run on the audio thread: they only emit `mic://level` and
//! `mic://limit`, and never take a lock.

use parley_core::api::{MIC_LEVEL_EVENT, MIC_LIMIT_EVENT, MicLevel, Recording};
use parley_core::error::AppError;
use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

/// Starts a take of at most `max_s` seconds (15 for an exercise, 30 for a role-play turn).
#[tauri::command(rename_all = "snake_case")]
pub fn mic_start(app: AppHandle, state: State<'_, AppState>, max_s: u32) -> Result<(), AppError> {
    let level = app.clone();
    let limit = app;
    state.with(|p| {
        p.mic_start(
            max_s,
            Box::new(move |dbfs| {
                // A window that cannot hear the event only misses a meter reading.
                let _ = level.emit(MIC_LEVEL_EVENT, MicLevel { dbfs });
            }),
            Box::new(move || {
                let _ = limit.emit(MIC_LIMIT_EVENT, serde_json::json!({}));
            }),
        )
    })
}

/// Stops the take; it is trimmed and kept in `cache/recordings/`.
#[tauri::command(rename_all = "snake_case")]
pub fn mic_stop(state: State<'_, AppState>) -> Result<Recording, AppError> {
    state.with(|p| p.mic_stop())
}

/// **Replay my take**.
#[tauri::command(rename_all = "snake_case")]
pub fn recording_play(state: State<'_, AppState>, recording_id: String) -> Result<(), AppError> {
    state.with(|p| p.recording_play(&recording_id))
}
