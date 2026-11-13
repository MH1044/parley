//! App-level commands: `app_start`, `app_info`, `file_pick`, `data_delete_all` and
//! `cache_clear`; `session_skip` is with the audio commands, and `update_check` follows in a later
//! module.

use parley_core::api::{AppInfo, CacheCleared, Launch};
use parley_core::error::{AppError, ErrorCode};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::state::AppState;

/// The first command of a launch. It also starts the AI service in the background (one
/// `GET /wallet` while publik API is the chosen provider, nothing otherwise), whose status arrives
/// as `ai://status`.
#[tauri::command(rename_all = "snake_case")]
pub fn app_start(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<Launch, AppError> {
    let launch = state.with(|p| p.app_start())?;
    super::ai::start(app);
    Ok(launch)
}

#[tauri::command(rename_all = "snake_case")]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    state.with(|p| p.app_info())
}

/// The native file dialog for a backup or a course pack, opened from Rust so the web view needs no
/// dialog permission: a save dialog for `backup_export` and `course_pack_export`, an open dialog for
/// `backup_import` and `course_pack_import`. The chosen
/// path, or null when cancelled. Async, so the blocking dialog never runs on the main thread.
#[tauri::command(rename_all = "snake_case")]
pub async fn file_pick(app: tauri::AppHandle, kind: String) -> Result<Option<String>, AppError> {
    let backup = || app.dialog().file().add_filter("Parley backup", &["json"]);
    let pack = || {
        app.dialog()
            .file()
            .add_filter("Parley course pack", &["json"])
    };
    let picked = match kind.as_str() {
        "backup_export" => backup()
            .set_file_name("parley-backup.json")
            .blocking_save_file(),
        "backup_import" => backup().blocking_pick_file(),
        "course_pack_export" => {
            let lang = app
                .state::<AppState>()
                .with(|p| p.settings_get().active_course)
                .unwrap_or_else(|| "course".into());
            pack()
                .set_file_name(format!("parley-course-{lang}.json"))
                .blocking_save_file()
        }
        "course_pack_import" => pack().blocking_pick_file(),
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

/// Deletes the learner's data, then forgets the AI status (`publik-status.json` and what is held in
/// memory). The publik credential, outside the data directory, and the own key in the keychain are
/// kept. Async, with the work on a blocking thread, so an AI request still running never holds up
/// the window; the two locks are taken one after the other, never together.
#[tauri::command(rename_all = "snake_case")]
pub async fn data_delete_all(app: tauri::AppHandle, confirm_text: String) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.with(|p| p.data_delete_all(&confirm_text))?;
        super::ai::forget(&state);
        Ok(())
    })
    .await
    .map_err(|_| AppError::new(ErrorCode::Storage, "Parley could not write that file."))?
}

#[tauri::command(rename_all = "snake_case")]
pub fn cache_clear(state: State<'_, AppState>, kind: String) -> Result<CacheCleared, AppError> {
    state.with(|p| p.cache_clear(&kind))
}
