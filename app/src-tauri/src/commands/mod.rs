//! Tauri commands, one file per area (docs/ARCHITECTURE.md §5). Each command marshals arguments
//! into the core and its result or `AppError` back out, and nothing more.

pub mod app;
