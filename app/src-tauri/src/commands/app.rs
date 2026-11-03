//! App-level commands: `app_start`, `app_info`, `file_pick`, `data_delete_all` and
//! `cache_clear`; `session_skip` and `update_check` follow in later modules.

use parley_core::api::{AppInfo, CacheCleared, Launch};
use parley_core::error::{AppError, ErrorCode};
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub fn app_start(state: State<'_, AppState>) -> Result<Launch, AppError> {
    state.with(|p| p.app_start())
}

#[tauri::command(rename_all = "snake_case")]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    state.with(|p| p.app_info())
}

/// The native file dialog for a backup, opened from Rust so the web view needs no dialog
/// permission: a save dialog for `backup_export`, an open dialog for `backup_import`. The chosen
/// path, or null when cancelled. Async, so the blocking dialog never runs on the main thread.
#[tauri::command(rename_all = "snake_case")]
pub async fn file_pick(app: tauri::AppHandle, kind: String) -> Result<Option<String>, AppError> {
    let dialog = app.dialog().file().add_filter("Parley backup", &["json"]);
    let picked = match kind.as_str() {
        "backup_export" => dialog
            .set_file_name("parley-backup.json")
            .blocking_save_file(),
        "backup_import" => dialog.blocking_pick_file(),
        other => {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                format!("Unknown value {other} for kind."),
            ));
        }
    };
    Ok(picked
        .and_then(|path| path.into_path().ok())
        .map(|path| path.display().to_string()))
}

#[tauri::command(rename_all = "snake_case")]
pub fn data_delete_all(state: State<'_, AppState>, confirm_text: String) -> Result<(), AppError> {
    state.with(|p| p.data_delete_all(&confirm_text))
}

#[tauri::command(rename_all = "snake_case")]
pub fn cache_clear(state: State<'_, AppState>, kind: String) -> Result<CacheCleared, AppError> {
    state.with(|p| p.cache_clear(&kind))
}
