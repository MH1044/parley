//! Acceptance test 26 of module 06 (docs/modules/06-publik-and-providers.md): `parley publik
//! proof`, the maintainer's one real publik API call, refuses to run without `PUBLIK_PROOF=1` and
//! sends nothing. The command's environment is set on the child process only, and points at a
//! fake server with a fake key, so a request would be seen; none is expected.

#[path = "../../parley-publik/tests/fake.rs"]
mod fake;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PARLEY: &str = env!("CARGO_BIN_EXE_parley");
const REFUSAL: &str = "This makes one real publik API call. Run it with PUBLIK_PROOF=1.";

/// Runs `parley publik proof` with `proof` as PUBLIK_PROOF (removed when none), a fake key and
/// base URL, and the home folders in `home`.
fn proof(proof: Option<&str>, publik: &fake::Fake, home: &Path) -> Output {
    let mut command = Command::new(PARLEY);
    command
        .args(["publik", "proof"])
        .env_remove("PUBLIK_PROOF")
        .env("PUBLIK_API_KEY", "pk_live_a")
        .env("PUBLIK_API_BASE_URL", publik.url())
        .env("PUBLIK_APP_TOKEN", "pat_parley_test")
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home)
        .env("XDG_DATA_HOME", home)
        .env("APPDATA", home)
        .env("LOCALAPPDATA", home);
    if let Some(value) = proof {
        command.env("PUBLIK_PROOF", value);
    }
    command.output().expect("the parley binary runs")
}

#[test]
fn t26_publik_proof_without_publik_proof_1_exits_1_and_sends_nothing() {
    let publik = fake::Fake::start();
    publik.on(
        "POST",
        "/chat/completions",
        fake::Reply::chat(r#"{"ok": true}"#),
    );
    publik.on(
        "GET",
        "/models",
        fake::Reply::json(200, &serde_json::json!({"object": "list", "data": []})),
    );
    let home = tempfile::tempdir().expect("a temporary directory");

    for value in [None, Some("0"), Some("")] {
        let output = proof(value, &publik, home.path());
        assert_eq!(output.status.code(), Some(1), "PUBLIK_PROOF = {value:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr).trim(),
            REFUSAL,
            "PUBLIK_PROOF = {value:?}"
        );
        assert!(
            output.stdout.is_empty(),
            "nothing on standard output: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
    assert_eq!(publik.count(), 0, "nothing is sent");
    assert_eq!(publik.connections(), 0);
    assert!(
        fs::read_dir(home.path())
            .expect("the home folder")
            .next()
            .is_none(),
        "nothing is written"
    );
}

/// The repository root: the first ancestor of this crate that holds the workspace Cargo.toml.
fn repo_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if let Ok(text) = fs::read_to_string(dir.join("Cargo.toml"))
            && text.contains("[workspace]")
        {
            return dir;
        }
        assert!(dir.pop(), "no workspace Cargo.toml above the crate");
    }
}

#[test]
fn t26_scripts_publik_proof_runs_the_cli_command() {
    let script = repo_root().join("scripts").join("publik-proof");
    let text = fs::read_to_string(&script)
        .unwrap_or_else(|e| panic!("{} could not be read: {e}", script.display()));
    assert!(
        text.contains("cargo run -q -p parley-cli -- publik proof"),
        "scripts/publik-proof runs `cargo run -q -p parley-cli -- publik proof`"
    );
}
