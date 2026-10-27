//! App-level commands: `app_info` (module 00); `data_delete_all`, `cache_clear`, `session_skip`
//! and `update_check` follow in later modules.

use parley_core::api::{self, AppInfo};
use parley_core::error::{AppError, ErrorCode};
use tauri::Manager;

/// The running version and the app data directory.
#[tauri::command]
pub fn app_info(app: tauri::AppHandle) -> Result<AppInfo, AppError> {
    let data_dir = app.path().app_data_dir().map_err(|_| {
        AppError::new(
            ErrorCode::Storage,
            "Parley could not find its data directory.",
        )
    })?;
    Ok(api::app_info(&data_dir))
}
