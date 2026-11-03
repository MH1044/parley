//! `path_get` and `skill_get`.

use parley_core::api::{PathView, SkillDetail};
use parley_core::error::AppError;
use tauri::State;

use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub fn path_get(state: State<'_, AppState>, lang: String) -> Result<PathView, AppError> {
    state.with(|p| p.path_get(&lang))
}

#[tauri::command(rename_all = "snake_case")]
pub fn skill_get(state: State<'_, AppState>, skill_id: String) -> Result<SkillDetail, AppError> {
    state.with(|p| p.skill_get(&skill_id))
}
