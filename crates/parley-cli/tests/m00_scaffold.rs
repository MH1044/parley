//! Module 00 tests (docs/modules/00-scaffold.md, Acceptance tests). Each test names the bullet it
//! covers. Bullet 1 is the golden case `tests/cases/00-scaffold/version`, bullet 3 is the CI run,
//! bullet 4 is `crates/parley-core/tests/hygiene.rs` and bullet 10 is `app/src/App.test.tsx`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use parley_core::env::{FailingNet, HttpRequest, Net, NetError};
use parley_core::error::{AppError, ErrorCode};

const PARLEY: &str = env!("CARGO_BIN_EXE_parley");
const PARLEY_FIXTURES: &str = env!("CARGO_BIN_EXE_parley-fixtures");

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

fn read(path: impl AsRef<Path>) -> String {
    let path = repo_root().join(path);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()))
}

fn run(program: &str, args: &[&str], cwd: &Path) -> Output {
    Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|e| panic!("could not run {program}: {e}"))
}

/// A POSIX shell for the scripts: `sh` where it exists, else `bash` (Git for Windows ships both).
fn shell() -> &'static str {
    for candidate in ["sh", "bash"] {
        if Command::new(candidate)
            .arg("-c")
            .arg("true")
            .output()
            .is_ok()
        {
            return candidate;
        }
    }
    panic!("neither sh nor bash is available to run the scripts");
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

/// The workspace version from the root Cargo.toml.
fn workspace_version() -> String {
    let manifest: toml::Table = read("Cargo.toml").parse().expect("Cargo.toml parses");
    manifest["workspace"]["package"]["version"]
        .as_str()
        .expect("workspace.package.version is a string")
        .to_string()
}

/// Reads the samples of a 16 kHz, mono, 16-bit PCM WAV file, checking its header.
fn pcm16_samples(wav: &[u8]) -> Vec<i16> {
    assert!(wav.len() >= 44, "a WAV file has a 44-byte header");
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(&wav[12..16], b"fmt ");
    assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 1, "PCM");
    assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1, "mono");
    assert_eq!(
        u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
        16_000,
        "16 kHz"
    );
    assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16, "16-bit");
    assert_eq!(&wav[36..40], b"data");
    let data_len = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]) as usize;
    assert_eq!(wav.len(), 44 + data_len);
    let (pairs, rest) = wav[44..].as_chunks::<2>();
    assert!(rest.is_empty(), "a whole number of 16-bit samples");
    pairs.iter().map(|&pair| i16::from_le_bytes(pair)).collect()
}

// Bullet 2: `parley --bogus` exits 2 with the usage error on stderr.
#[test]
fn t02_unknown_argument_exits_2_with_the_message() {
    let output = run(PARLEY, &["--bogus"], &repo_root());
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        text(&output.stderr),
        "error: unknown argument --bogus. Run parley --help.\n"
    );
    assert!(output.stdout.is_empty());

    let help = run(PARLEY, &["--help"], &repo_root());
    assert_eq!(help.status.code(), Some(0));
    assert!(text(&help.stdout).starts_with("Usage: parley"));
}

// Bullet 1 (in-process companion of the golden case): the version line, plain and as JSON, equals
// the one workspace version.
#[test]
fn t01_version_is_the_workspace_version() {
    let version = workspace_version();
    let plain = run(PARLEY, &["--version"], &repo_root());
    assert_eq!(text(&plain.stdout), format!("parley {version}\n"));
    let json = run(PARLEY, &["--version", "--json"], &repo_root());
    assert_eq!(
        text(&json.stdout),
        format!("{{\"name\":\"parley\",\"version\":\"{version}\"}}\n")
    );
}

// Scope: one shared version. The Tauri configuration and the front-end package carry the same
// number as the workspace, so a release bumps one number in three named places (RELEASING.md).
#[test]
fn version_is_shared_by_the_app_and_the_front_end() {
    let version = workspace_version();
    let tauri_conf: serde_json::Value =
        serde_json::from_str(&read("app/src-tauri/tauri.conf.json"))
            .expect("tauri.conf.json parses");
    assert_eq!(
        tauri_conf["version"], version,
        "app/src-tauri/tauri.conf.json"
    );
    let package: serde_json::Value =
        serde_json::from_str(&read("app/package.json")).expect("package.json parses");
    assert_eq!(package["version"], version, "app/package.json");
}

// Bullet 5: the fixture generator is bit-reproducible, matches the committed manifest, and the
// tone has the shape stated in the Design notes.
#[test]
fn t05_fixtures_are_bit_identical_and_match_the_committed_manifest() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let manifest_a = parley_cli::fixtures::write(a.path()).unwrap();
    let manifest_b = parley_cli::fixtures::write(b.path()).unwrap();
    assert_eq!(manifest_a, manifest_b);

    let json_a = fs::read_to_string(a.path().join("manifest.json")).unwrap();
    let json_b = fs::read_to_string(b.path().join("manifest.json")).unwrap();
    assert_eq!(json_a, json_b);
    let committed = read("tests/fixtures/generated/manifest.json");
    assert_eq!(
        json_a, committed,
        "the committed manifest is the specification: regenerate it only for a change the spec and commit body describe"
    );

    let wav = fs::read(a.path().join("audio").join("tone_gaps_a.wav")).unwrap();
    let samples = pcm16_samples(&wav);
    assert_eq!(samples.len(), 40_000);
    assert_eq!(samples[0], 16_000);
    assert_eq!(samples[20], -16_000);
    assert!(samples[16_000..24_000].iter().all(|&s| s == 0));
    assert_eq!(samples[24_000], 16_000);

    // The binary: --out twice gives identical manifests; --check passes on the committed
    // directory and names a tampered file.
    let c = tempfile::tempdir().unwrap();
    let d = tempfile::tempdir().unwrap();
    for dir in [c.path(), d.path()] {
        let out = run(
            PARLEY_FIXTURES,
            &["--out", &dir.to_string_lossy()],
            &repo_root(),
        );
        assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    }
    assert_eq!(
        fs::read(c.path().join("manifest.json")).unwrap(),
        fs::read(d.path().join("manifest.json")).unwrap()
    );
    let committed_dir = repo_root().join("tests").join("fixtures").join("generated");
    let check = run(
        PARLEY_FIXTURES,
        &["--check", &committed_dir.to_string_lossy()],
        &repo_root(),
    );
    assert_eq!(check.status.code(), Some(0), "{}", text(&check.stderr));

    let tampered = c.path().join("audio").join("tone_gaps_a.wav");
    let mut bytes = fs::read(&tampered).unwrap();
    bytes[100] ^= 0x01;
    fs::write(&tampered, bytes).unwrap();
    let check = run(
        PARLEY_FIXTURES,
        &["--check", &c.path().to_string_lossy()],
        &repo_root(),
    );
    assert_eq!(check.status.code(), Some(1));
    assert_eq!(
        text(&check.stderr),
        "fixture audio/tone_gaps_a.wav differs from manifest.json\n"
    );
}

// Bullet 6: the licence gate's allow list contains MIT and Apache-2.0 and nothing copyleft.
#[test]
fn t06_deny_toml_allows_only_permissive_licences() {
    let deny: toml::Table = read("deny.toml").parse().expect("deny.toml parses");
    let allow: Vec<String> = deny["licenses"]["allow"]
        .as_array()
        .expect("licenses.allow is an array")
        .iter()
        .map(|v| v.as_str().expect("licence ids are strings").to_string())
        .collect();
    assert!(allow.iter().any(|id| id == "MIT"));
    assert!(allow.iter().any(|id| id == "Apache-2.0"));
    for id in &allow {
        let upper = id.to_uppercase();
        for forbidden in ["GPL", "AGPL", "SSPL", "BUSL"] {
            assert!(!upper.contains(forbidden), "{id} is in the allow list");
        }
    }
}

// Bullet 7: the npm licence gate fails on the GPL fixture with the exact message and passes on
// the real lockfile.
#[test]
fn t07_npm_licence_gate() {
    let root = repo_root();
    let gpl = run(
        "node",
        &[
            "scripts/check-npm-licences.mjs",
            "tests/fixtures/npm/gpl-lock.json",
        ],
        &root,
    );
    assert_eq!(gpl.status.code(), Some(1));
    assert!(
        text(&gpl.stderr).contains(
            "left-pad-gpl@1.0.0 is GPL-3.0; the licence gate allows only permissive licences"
        ),
        "stderr was:\n{}",
        text(&gpl.stderr)
    );

    let real = run(
        "node",
        &["scripts/check-npm-licences.mjs", "app/package-lock.json"],
        &root,
    );
    assert_eq!(real.status.code(), Some(0), "{}", text(&real.stderr));
}

// Bullet 8: the release-notes check refuses a version without docs/releases/<version>.md.
#[test]
fn t08_check_release_notes() {
    let script = repo_root().join("scripts").join("check-release-notes");
    let script = script.to_string_lossy().into_owned();
    let dir = tempfile::tempdir().unwrap();
    let sh = shell();

    let missing = run(sh, &[&script, "0.1.0"], dir.path());
    assert_eq!(missing.status.code(), Some(1));
    assert_eq!(
        text(&missing.stderr),
        "docs/releases/0.1.0.md is missing; write the release notes first.\n"
    );

    let notes = dir.path().join("docs").join("releases");
    fs::create_dir_all(&notes).unwrap();
    fs::write(notes.join("0.1.0.md"), "# 0.1.0\n").unwrap();
    let present = run(sh, &[&script, "0.1.0"], dir.path());
    assert_eq!(present.status.code(), Some(0), "{}", text(&present.stderr));
}

// Bullet 9: app_info reports the shared version.
#[test]
fn t09_app_info_returns_the_version() {
    let info = parley_core::api::app_info(Path::new("data"));
    assert_eq!(info.version, workspace_version());
    assert_eq!(info.version, "0.1.0");
    assert_eq!(info.data_dir, "data");
}

// Bullet 11: the failing network fake refuses any request.
#[test]
fn t11_failing_net_returns_disabled() {
    let req = HttpRequest {
        method: "POST".into(),
        url: "https://publikhq.example/api/v1/chat/completions".into(),
        headers: vec![("content-type".into(), "application/json".into())],
        body: b"{}".to_vec(),
    };
    assert_eq!(FailingNet.send(req), Err(NetError::Disabled));
}

// Bullet 12: twelve ADRs, indexed, each with a Status line; 0009 and 0010 Proposed, the rest
// Accepted (0003 was Proposed at module 00 and was accepted with CC BY-SA 4.0 before module 01);
// every document the Scope names exists.
#[test]
fn t12_adrs_and_documents_exist() {
    let root = repo_root();
    let index = read("docs/adr/README.md");
    let mut adr_files: Vec<PathBuf> = fs::read_dir(root.join("docs").join("adr"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            name.len() > 5 && name[..4].chars().all(|c| c.is_ascii_digit()) && name.ends_with(".md")
        })
        .collect();
    adr_files.sort();
    assert_eq!(adr_files.len(), 12, "ADR files: {adr_files:?}");

    for (i, path) in adr_files.iter().enumerate() {
        let number = i + 1;
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with(&format!("{number:04}-")),
            "{name} is out of sequence"
        );
        assert!(
            index.contains(&name),
            "docs/adr/README.md does not list {name}"
        );
        let body = fs::read_to_string(path).unwrap();
        let status_line = body
            .lines()
            .find(|line| line.contains("Status:"))
            .unwrap_or_else(|| panic!("{name} has no Status: line"));
        let expected = if [9, 10].contains(&number) {
            "Proposed"
        } else {
            "Accepted"
        };
        assert!(
            status_line.contains(expected),
            "{name}: expected {expected}, found {status_line:?}"
        );
    }
    assert!(root.join("docs").join("adr").join("template.md").is_file());

    for doc in [
        "README.md",
        "CONTRIBUTING.md",
        "SECURITY.md",
        "RELEASING.md",
        "THIRD_PARTY.md",
        "LICENSE-MIT",
        "LICENSE-APACHE",
        "docs/HANDBOOK.md",
        "docs/PUBLIK-CONTRACT.md",
    ] {
        assert!(root.join(doc).is_file(), "{doc} is missing");
    }
    assert!(read("CONTRIBUTING.md").contains("## publik API"));
}

// Bullet 13: AppError serialises exactly as docs/ARCHITECTURE.md §5 shows.
#[test]
fn t13_app_error_serialises_with_a_null_action() {
    let err = AppError {
        code: ErrorCode::NotFound,
        message: "x".to_string(),
        action: None,
    };
    assert_eq!(
        serde_json::to_string(&err).unwrap(),
        r#"{"code":"not_found","message":"x","action":null}"#
    );
}
