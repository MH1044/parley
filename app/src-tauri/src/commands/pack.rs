//! `pack_status` and `pack_download`, which reports its progress as `pack://progress` events.

use parley_core::api::PackStatus;
use parley_core::error::{AppError, ErrorCode};
use tauri::{Emitter, Manager, State};

use crate::state::AppState;

/// The event a download reports its progress on.
pub const PROGRESS_EVENT: &str = "pack://progress";

#[tauri::command(rename_all = "snake_case")]
pub fn pack_status(state: State<'_, AppState>, lang: String) -> Result<PackStatus, AppError> {
    state.with(|p| p.pack_status(&lang))
}

/// Async, with the download itself on a blocking thread, so it never blocks the window or the
/// async runtime.
#[tauri::command(rename_all = "snake_case")]
pub async fn pack_download(app: tauri::AppHandle, lang: String) -> Result<PackStatus, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.with(|p| {
            p.pack_download(&lang, &mut |progress| {
                // A window that cannot hear the event still gets the result.
                let _ = app.emit(PROGRESS_EVENT, &progress);
            })
        })
    })
    .await
    .map_err(|_| {
        AppError::new(
            ErrorCode::PackMissing,
            "The course pack could not be downloaded. Try again later.",
        )
    })?
}
