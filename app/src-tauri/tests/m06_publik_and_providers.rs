//! Acceptance tests of module 06 that need the desktop crate (docs/modules/06-publik-and-providers.md):
//! the desktop binary's built-app self-test (test 32, contract test 18 of
//! docs/PUBLIK-CONTRACT.md §11), the AI commands in the one list of commands, and the web view's
//! one way to open a link (test 19).
//!
//! The binary is started with its environment set on the child process only; the test process's
//! own environment is never changed. A fake server stands where publik API would be, so a request
//! would be seen, and none is expected.

#[path = "../../../crates/parley-publik/tests/fake.rs"]
mod fake;

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use parley_core::api::{AI_STATUS_EVENT, COMMANDS};
use serde_json::{Value, json};

/// The commands module 06 appends after `backup_import`, in the spec's order.
const AI_COMMANDS: [&str; 11] = [
    "ai_status",
    "ai_consent_accept",
    "ai_consent_decline",
    "ai_set_own_key",
    "ai_clear_own_key",
    "ai_local_probe",
    "ai_set_local",
    "ai_check_connection",
    "ai_disconnect_publik",
    "ai_reconnect_publik",
    "ai_open_link",
];

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn app_dir() -> PathBuf {
    crate_dir()
        .parent()
        .expect("app/src-tauri sits in the app folder")
        .to_path_buf()
}

fn read_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()))
}

/// Every file and folder left under `dir`.
fn entries_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(entries_under(&path));
            }
            found.push(path);
        }
    }
    found
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 32 (desktop half): the built-app self-test.
// ---------------------------------------------------------------------------------------------

#[test]
fn t32_the_desktop_binary_prints_the_self_test_line_and_exits_before_anything_starts() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let publik = fake::Fake::start();
    let mut child = Command::new(env!("CARGO_BIN_EXE_parley-app"))
        .env("PUBLIK_SELF_TEST", "1")
        .env("PUBLIK_APP_TOKEN", "none")
        .env("PUBLIK_API_BASE_URL", publik.url())
        .env_remove("PUBLIK_API_KEY")
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("XDG_DATA_HOME", home.path())
        .env("APPDATA", home.path())
        .env("LOCALAPPDATA", home.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the desktop binary starts");

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("the child's state") {
            break status;
        }
        if started.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the self-test did not exit within 10 s");
        }
        thread::sleep(Duration::from_millis(50));
    };
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .expect("standard output")
        .read_to_string(&mut stdout)
        .expect("standard output is text");

    assert!(
        status.success(),
        "the self-test exits with code 0: {status}"
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        [
            "publik self-test: failed: This build has no publik API app token. Use your own key for now."
        ],
        "exactly one line"
    );
    assert_eq!(
        publik.count(),
        0,
        "the token check fails before any request"
    );
    assert_eq!(publik.connections(), 0);
    let left = entries_under(home.path());
    assert!(
        left.is_empty(),
        "no window, database, log or credential file: {left:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// The command list: the AI commands follow backup_import in the core, the UI and the desktop.
// ---------------------------------------------------------------------------------------------

/// The quoted names of `export const COMMANDS = [...]` in a TypeScript file.
fn ts_commands(text: &str) -> Vec<String> {
    let start = text
        .find("export const COMMANDS = [")
        .expect("commands.ts exports COMMANDS as an array");
    let rest = &text[start + "export const COMMANDS = [".len()..];
    let body = &rest[..rest.find(']').expect("the array closes")];
    let mut names = Vec::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c == '\'' || c == '"' {
            let name: String = chars.by_ref().take_while(|&d| d != c).collect();
            names.push(name);
        }
    }
    names
}

fn contains_word(text: &str, word: &str) -> bool {
    let is_name = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(is_name) && !after.is_some_and(is_name)
    })
}

#[test]
fn the_ai_commands_follow_backup_import_in_the_core_the_ui_and_the_desktop() {
    let at = COMMANDS
        .iter()
        .position(|name| *name == "backup_import")
        .expect("backup_import is a command");
    let after = &COMMANDS[at + 1..];
    assert!(after.len() >= AI_COMMANDS.len(), "{COMMANDS:?}");
    assert_eq!(
        &after[..AI_COMMANDS.len()],
        AI_COMMANDS.as_slice(),
        "appended after backup_import, in the spec's order"
    );

    let ts = read_text(&app_dir().join("src").join("api").join("commands.ts"));
    assert_eq!(
        ts_commands(&ts),
        COMMANDS,
        "commands.ts lists the same names in order"
    );
    let lib = read_text(&crate_dir().join("src").join("lib.rs"));
    for name in AI_COMMANDS {
        assert!(
            contains_word(&lib, name),
            "{name} is registered in app/src-tauri/src/lib.rs"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// The event ai://status: one name in the core and the UI.
// ---------------------------------------------------------------------------------------------

#[test]
fn the_ai_status_event_has_one_name_in_the_core_and_the_ui() {
    assert_eq!(AI_STATUS_EVENT, "ai://status");
    let ts = read_text(&app_dir().join("src").join("api").join("commands.ts"));
    let exported = ts.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("export const")
            && ['\'', '"', '`']
                .iter()
                .any(|quote| line.contains(&format!("{quote}{AI_STATUS_EVENT}{quote}")))
    });
    assert!(
        exported,
        "app/src/api/commands.ts exports the event name {AI_STATUS_EVENT}"
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 19 (desktop half): no command opens a URL the web view made up.
// ---------------------------------------------------------------------------------------------

#[test]
fn t19_ai_open_link_is_the_only_command_that_opens_a_link_and_the_web_view_has_core_permissions_only()
 {
    let openers: Vec<&str> = COMMANDS
        .iter()
        .copied()
        .filter(|name| name.contains("open") || name.contains("url") || name.contains("link"))
        .collect();
    assert_eq!(openers, ["ai_open_link"]);

    let capabilities: Value = serde_json::from_str(&read_text(
        &crate_dir().join("capabilities").join("default.json"),
    ))
    .expect("the capabilities file is JSON");
    assert_eq!(
        capabilities["permissions"],
        json!(["core:default"]),
        "no opener, shell or HTTP permission for the web view"
    );
}
