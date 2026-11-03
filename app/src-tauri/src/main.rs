//! Parley's desktop app. Everything is in the library (`parley_app::run`), so tests can build
//! the same state without a window.

// A release build has no console window on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    parley_app::run();
}
