//! Tauri commands, one file per area (docs/ARCHITECTURE.md §5). Each command marshals arguments
//! into `parley_core::app::Parley` and its result or `AppError` back out, and nothing more. The
//! web view passes argument names in snake case, as the command table writes them.

pub mod ai;
pub mod app;
pub mod audio;
pub mod backup;
pub mod course;
pub mod lesson;
pub mod mic;
pub mod models;
pub mod pack;
pub mod path;
pub mod placement;
pub mod settings;
pub mod speak;
pub mod stats;
