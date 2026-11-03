//! Lessons and reviews: `lesson_start`, `review_start`, `exercise_submit`,
//! `exercise_i_was_right` and `lesson_finish`.

use parley_core::api::LessonStarted;
use parley_core::error::AppError;
use parley_core::exercise::LessonSummary;
use parley_core::grading::{Answer, Grade};
use tauri::State;

use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub fn lesson_start(
    state: State<'_, AppState>,
    skill_id: String,
    mode: String,
) -> Result<LessonStarted, AppError> {
    state.with(|p| p.lesson_start(&skill_id, &mode))
}

#[tauri::command(rename_all = "snake_case")]
pub fn review_start(
    state: State<'_, AppState>,
    lang: String,
    limit: usize,
) -> Result<LessonStarted, AppError> {
    state.with(|p| p.review_start(&lang, limit))
}

#[tauri::command(rename_all = "snake_case")]
pub fn exercise_submit(
    state: State<'_, AppState>,
    session_id: String,
    exercise_id: String,
    answer: Answer,
    response_ms: Option<i64>,
    used_hint: bool,
) -> Result<Grade, AppError> {
    state.with(|p| p.exercise_submit(&session_id, &exercise_id, &answer, response_ms, used_hint))
}

#[tauri::command(rename_all = "snake_case")]
pub fn exercise_i_was_right(
    state: State<'_, AppState>,
    session_id: String,
    exercise_id: String,
) -> Result<Grade, AppError> {
    state.with(|p| p.exercise_i_was_right(&session_id, &exercise_id))
}

#[tauri::command(rename_all = "snake_case")]
pub fn lesson_finish(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<LessonSummary, AppError> {
    state.with(|p| p.lesson_finish(&session_id))
}
