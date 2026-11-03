//! `settings_get` and `settings_set`.

use parley_core::error::AppError;
use parley_core::settings::Settings;
use tauri::State;

use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub fn settings_get(state: State<'_, AppState>) -> Settings {
    state.with(|p| p.settings_get())
}

#[tauri::command(rename_all = "snake_case")]
pub fn settings_set(
    state: State<'_, AppState>,
    patch: serde_json::Value,
) -> Result<Settings, AppError> {
    state.with(|p| p.settings_set(&patch))
}
