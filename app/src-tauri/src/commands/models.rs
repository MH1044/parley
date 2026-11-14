//! The local Whisper models (docs/modules/10-speaking-and-pronunciation.md, Commands and events):
//! `whisper_models` and `whisper_download`, which reports its progress as `model://progress`
//! events, the size first.
//!
//! The download goes through the app's transport, `HttpNet`, never core code, and runs on a
//! blocking thread with nothing locked between its begin and its completion, so the app keeps
//! working while a model of several hundred megabytes arrives.

use parley_core::api::{MODEL_PROGRESS_EVENT, ModelInfo};
use parley_core::app::DownloadBegin;
use parley_core::error::{AppError, ErrorCode};
use parley_publik::http::HttpNet;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub fn whisper_models(state: State<'_, AppState>) -> Result<Vec<ModelInfo>, AppError> {
    Ok(state.with(|p| p.whisper_models()))
}

/// Downloads the model `name` of `models/registry.toml` and checks it; the list of models.
#[tauri::command(rename_all = "snake_case")]
pub async fn whisper_download(app: AppHandle, name: String) -> Result<Vec<ModelInfo>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        match state.with(|p| p.whisper_download_begin(&name))? {
            DownloadBegin::Done(list) => Ok(list),
            DownloadBegin::Pending(job) => {
                let outcome = job.run(&HttpNet::new(), &mut |progress| {
                    // A window that cannot hear the event still gets the result.
                    let _ = app.emit(MODEL_PROGRESS_EVENT, &progress);
                });
                state.with(|p| p.whisper_download_complete(job, outcome))
            }
        }
    })
    .await
    .map_err(|_| {
        AppError::new(
            ErrorCode::NotFound,
            "The speech model could not be downloaded. Try again later.",
        )
    })?
}
