//! Command argument and result types (docs/ARCHITECTURE.md §5).
//!
//! `app/src/api/commands.ts` mirrors these by hand. [`COMMANDS`] lists every command name in the
//! order the UI wrapper lists them; a test compares the two lists.

use std::path::Path;

use serde::Serialize;

/// Every Tauri command the core backs, in the order `app/src/api/commands.ts` lists them.
pub const COMMANDS: &[&str] = &["app_info"];

/// The version shared by every crate and the app (the workspace version in Cargo.toml).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Result of `app_info`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppInfo {
    pub version: String,
    /// The app data directory, shown on the About page so a learner can find their data.
    pub data_dir: String,
}

/// The running version and the data directory the app was given.
pub fn app_info(data_dir: &Path) -> AppInfo {
    AppInfo {
        version: VERSION.to_string(),
        data_dir: data_dir.display().to_string(),
    }
}
