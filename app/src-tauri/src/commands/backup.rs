//! `backup_export` and `backup_import`, with a path the learner picked through `file_pick`.

use std::path::Path;

use parley_core::api::{ExportSummary, ImportReport};
use parley_core::error::AppError;
use tauri::State;

use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub fn backup_export(
    state: State<'_, AppState>,
    dest_path: String,
) -> Result<ExportSummary, AppError> {
    state.with(|p| p.backup_export(Path::new(&dest_path)))
}

#[tauri::command(rename_all = "snake_case")]
pub fn backup_import(
    state: State<'_, AppState>,
    src_path: String,
) -> Result<ImportReport, AppError> {
    state.with(|p| p.backup_import(Path::new(&src_path)))
}
