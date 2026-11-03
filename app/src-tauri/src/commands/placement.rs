//! `placement_start` and `placement_answer`.

use parley_core::api::{PlacementQuestion, PlacementStep};
use parley_core::error::AppError;
use parley_core::grading::Answer;
use tauri::State;

use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub fn placement_start(
    state: State<'_, AppState>,
    lang: String,
) -> Result<PlacementQuestion, AppError> {
    state.with(|p| p.placement_start(&lang))
}

#[tauri::command(rename_all = "snake_case")]
pub fn placement_answer(
    state: State<'_, AppState>,
    session_id: String,
    answer: Answer,
) -> Result<PlacementStep, AppError> {
    state.with(|p| p.placement_answer(&session_id, &answer))
}
