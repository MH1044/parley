//! `course_list` and `course_start`.

use parley_core::api::CourseSummary;
use parley_core::error::AppError;
use tauri::State;

use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub fn course_list(state: State<'_, AppState>) -> Result<Vec<CourseSummary>, AppError> {
    state.with(|p| p.course_list())
}

#[tauri::command(rename_all = "snake_case")]
pub fn course_start(state: State<'_, AppState>, lang: String) -> Result<CourseSummary, AppError> {
    state.with(|p| p.course_start(&lang))
}
