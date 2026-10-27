//! Parley's desktop shell: a thin Tauri layer that owns the app state, turns commands into calls
//! into `parley-core`, and emits events. All learning logic lives in the core (docs/ARCHITECTURE.md
//! §3). The web view never holds a key and never touches the filesystem on its own; it only
//! sends intents and renders results.

// A release build has no console window on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![commands::app::app_info])
        .run(tauri::generate_context!())
        .expect("Parley could not open its window");
}
