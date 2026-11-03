//! `stats_get`.

use parley_core::api::Stats;
use parley_core::error::AppError;
use tauri::State;

use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub fn stats_get(state: State<'_, AppState>, lang: String) -> Result<Stats, AppError> {
    state.with(|p| p.stats_get(&lang))
}
