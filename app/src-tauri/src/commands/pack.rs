//! `pack_status` and `pack_download`, which reports its progress as `pack://progress` events.

use parley_core::api::PackStatus;
use parley_core::error::AppError;
use tauri::{Emitter, State};

use crate::state::AppState;

/// The event a download reports its progress on.
pub const PROGRESS_EVENT: &str = "pack://progress";

#[tauri::command(rename_all = "snake_case")]
pub fn pack_status(state: State<'_, AppState>, lang: String) -> Result<PackStatus, AppError> {
    state.with(|p| p.pack_status(&lang))
}

/// Async, so the download never blocks the window.
#[tauri::command(rename_all = "snake_case")]
pub async fn pack_download(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    lang: String,
) -> Result<PackStatus, AppError> {
    state.with(|p| {
        p.pack_download(&lang, &mut |progress| {
            // A window that cannot hear the event still gets the result.
            let _ = app.emit(PROGRESS_EVENT, &progress);
        })
    })
}
