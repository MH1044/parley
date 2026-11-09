//! Parley's desktop app. Everything is in the library (`parley_app::run`), so tests can build
//! the same state without a window.
//!
//! The first lines of `main` are the built-app self-test (docs/PUBLIK-CONTRACT.md §3): started with
//! `PUBLIK_SELF_TEST` set, the app prints one `publik self-test:` line and exits before logging,
//! the database, the app state or any window exists. Normal start-up never runs it.

// A release build has no console window on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Kept exactly as docs/modules/06-publik-and-providers.md shows it.
#[rustfmt::skip]
fn main() {
    if let Some(line) = parley_publik::startup_self_test(
        std::env::var_os("PUBLIK_SELF_TEST").as_deref(), env!("CARGO_PKG_VERSION")) {
        attach_parent_console();
        println!("{line}");
        return;
    }
    parley_app::run();
}

/// A release build on Windows uses the GUI subsystem, so a console that starts it does not see its
/// output. On the self-test path only, when standard output has no handle, attach to the console
/// of the process that started the app, so the line appears there (PowerShell and cmd alike).
#[cfg(windows)]
fn attach_parent_console() {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
    };
    // SAFETY: both calls take plain values and only read or set this process's console state.
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}
