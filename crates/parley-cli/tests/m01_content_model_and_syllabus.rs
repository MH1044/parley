//! Module 01 tests of the `parley` binary (docs/modules/01-content-model-and-syllabus.md,
//! Acceptance tests): exit codes, standard error, the files commands write, and pack build and
//! info end to end. Each test names the bullet it covers. Bullets 1, 2, 8 and 13 also run as
//! golden cases under tests/cases/01-content-model-and-syllabus, the library side lives in
//! crates/parley-core/tests/m01_content_model_and_syllabus.rs, and bullet 17 is the golden
//! harness itself. Expected values come from the spec and the fixture READMEs, never from
//! running the code.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const PARLEY: &str = env!("CARGO_BIN_EXE_parley");
const VALID_ES: &str = "tests/fixtures/syllabus/valid-es.yaml";
const SKILL: &str = "es.a1.u01.s01";
const FORMS_MINI_SHA256: &str = "bdd2092585e56451edb8e59e844bfca7a0659c3cc5cf51c3716430dea3b33919";

/// Example 2 of valid-es.yaml, as the one line it occupies in the file.
const EXAMPLE_2: &str = "          - {id: 2, target: \"Buenos días.\", en: [\"Good morning.\"]}\n";
/// A fifth example that passes every rule beside the four of valid-es.yaml.
const EXAMPLE_5: &str = "          - {id: 5, target: \"Hola, Luis.\", en: [\"Hello, Luis.\"]}\n";

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

fn read(path: &str) -> String {
    let path = repo_root().join(path);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()))
}

fn parley_in(args: &[&str], cwd: &Path) -> Output {
    Command::new(PARLEY)
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|e| panic!("could not run parley: {e}"))
}

/// Runs the binary from the repository root, where the fixture paths of the spec are relative.
fn parley(args: &[&str]) -> Output {
    parley_in(args, &repo_root())
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

fn stdout_json(output: &Output) -> Value {
    let stdout = text(&output.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not one JSON object ({e}):\n{stdout}\nstderr:\n{}",
            text(&output.stderr)
        )
    })
}

fn assert_exit(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout:\n{}\nstderr:\n{}",
        text(&output.stdout),
        text(&output.stderr)
    );
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn path_str(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn without_line(text: &str, line: &str) -> String {
    assert!(text.contains(line), "the text does not contain {line:?}");
    text.replacen(line, "", 1)
}

/// The item ids of valid-es.yaml in byte order, as `content lock` lists them.
fn valid_es_ids() -> Vec<String> {
    [
        "#e:1",
        "#e:2",
        "#e:3",
        "#e:4",
        "#l:adiós",
        "#l:bueno",
        "#l:día",
        "#l:gracias",
        "#l:hola",
        "#l:noche",
        "#l:tal",
    ]
    .iter()
    .map(|suffix| format!("{SKILL}{suffix}"))
    .collect()
}

fn lock_text(lines: &[(&str, &str)]) -> String {
    lines
        .iter()
        .map(|(id, state)| format!("{id}\t{state}\t1\n"))
        .collect()
}

fn build_args<'a>(out: &'a str, built_at: &'a str) -> Vec<&'a str> {
    vec![
        "pack",
        "build",
        "--lang",
        "es",
        "--tatoeba-dir",
        "tests/fixtures/tatoeba-mini",
        "--wiktextract",
        "tests/fixtures/forms-mini.jsonl",
        "--syllabus",
        VALID_ES,
        "--built-at",
        built_at,
        "--out",
        out,
    ]
}

fn expected_pack_report() -> Value {
    json!({
        "lang": "es",
        "kept": 22,
        "dropped": {
            "no_translation": 5, "too_short": 1, "too_long": 3,
            "excluded_lemma": 2, "unknown_word": 5, "duplicate": 2
        }
    })
}

// Bullet 1 (companion of the golden case): without --json the validator prints every error
// and warning on its own line, then `ok` when there is no error.
#[test]
fn t01_validate_plain_output() {
    let ok = parley(&["content", "validate", VALID_ES]);
    assert_exit(&ok, 0);
    assert_eq!(text(&ok.stdout), "ok\n");
    assert!(ok.stderr.is_empty());

    let bad = parley(&[
        "content",
        "validate",
        "tests/fixtures/syllabus/bad-V12.yaml",
    ]);
    assert_exit(&bad, 1);
    assert_eq!(
        text(&bad.stdout),
        "es.a1.u01.s01: example 5 uses \"tardes\", which no earlier skill teaches\n"
    );

    let warned = parley(&[
        "content",
        "validate",
        "tests/fixtures/syllabus/bad-V17.yaml",
    ]);
    assert_exit(&warned, 0);
    assert_eq!(
        text(&warned.stdout),
        "warning: hola is taught in es.a1.u01.s01 and es.a1.u01.s02\nok\n"
    );
}

// Bullet 8: pack build writes the pack and prints the report; sentence 1001 is kept with
// English 2001 by fixture_a; building again onto the same path replaces the file.
#[test]
fn t08_pack_build_writes_the_pack() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("es.sqlite3");
    let out_str = path_str(&out);
    let built = parley(&build_args(&out_str, "2026-10-01"));
    assert_exit(&built, 0);
    assert_eq!(stdout_json(&built), expected_pack_report());
    assert!(out.is_file());

    let conn = rusqlite::Connection::open(&out).unwrap();
    let sentences: i64 = conn
        .query_row("SELECT count(*) FROM sentences", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sentences, 22);
    let (text_1001, author) = conn
        .query_row(
            "SELECT text, author FROM sentences WHERE id = 1001",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .unwrap();
    assert_eq!(text_1001, "Hola, soy Ana.");
    assert_eq!(author, "fixture_a");
    let (en_id, en_text, en_author) = conn
        .query_row(
            "SELECT en_id, en_text, en_author FROM translations WHERE sentence_id = 1001",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(en_id, 2001);
    assert_eq!(en_text, "Hello, I am Ana.");
    assert_eq!(en_author, "fixture_b");
    let dropped_ids = [1023, 1024, 1028, 1029, 1030, 1033, 1035, 1040];
    for id in dropped_ids {
        let found: i64 = conn
            .query_row("SELECT count(*) FROM sentences WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(found, 0, "sentence {id} was dropped");
    }

    let meta = |key: &str| -> String {
        conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .unwrap_or_else(|e| panic!("meta {key}: {e}"))
    };
    assert_eq!(meta("schema"), "parley.pack/1");
    assert_eq!(meta("lang"), "es");
    assert_eq!(meta("built_at"), "2026-10-01");
    assert_eq!(meta("tool_version"), "0.1.0");
    assert_eq!(meta("licence"), "CC BY 2.0 FR");
    let attribution = meta("attribution");
    assert!(attribution.contains("Tatoeba"), "{attribution}");
    assert!(attribution.contains("CC BY 2.0 FR"), "{attribution}");
    assert_eq!(meta("source_sha256.forms-mini.jsonl"), FORMS_MINI_SHA256);
    drop(conn);

    let again = parley(&build_args(&out_str, "2026-10-01"));
    assert_exit(&again, 0);
    assert_eq!(stdout_json(&again), expected_pack_report());
}

// Bullet 9: pack info on two builds from the same inputs shares content_sha256 (which equals
// the stored meta value), has every key of the Interfaces format, and one source_sha256 entry
// per input file, each the SHA-256 of that file.
#[test]
fn t09_pack_info_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("es-2026-10-01.sqlite3");
    let second = dir.path().join("es-2026-10-02.sqlite3");
    assert_exit(&parley(&build_args(&path_str(&first), "2026-10-01")), 0);
    assert_exit(&parley(&build_args(&path_str(&second), "2026-10-02")), 0);

    let info_a = parley(&["pack", "info", &path_str(&first)]);
    assert_exit(&info_a, 0);
    let info_b = parley(&["pack", "info", &path_str(&second)]);
    assert_exit(&info_b, 0);
    let a = stdout_json(&info_a);
    let b = stdout_json(&info_b);
    assert_eq!(a["content_sha256"], b["content_sha256"]);
    assert_eq!(a["built_at"], "2026-10-01");
    assert_eq!(b["built_at"], "2026-10-02");

    for key in [
        "schema",
        "lang",
        "built_at",
        "tool_version",
        "licence",
        "sentences",
        "translations",
        "forms",
        "ranked_lemmas",
        "bytes",
        "sha256",
        "content_sha256",
        "source_sha256",
    ] {
        assert!(a.get(key).is_some(), "pack info lacks {key}: {a}");
    }
    assert_eq!(a["schema"], "parley.pack/1");
    assert_eq!(a["lang"], "es");
    assert_eq!(a["tool_version"], "0.1.0");
    assert_eq!(a["licence"], "CC BY 2.0 FR");
    assert_eq!(a["sentences"], 22);
    assert_eq!(a["translations"], 22);
    assert_eq!(a["forms"], 84);
    assert_eq!(a["ranked_lemmas"], 24);

    let bytes = fs::read(&first).unwrap();
    assert_eq!(a["bytes"].as_u64().unwrap(), bytes.len() as u64);
    assert_eq!(a["sha256"], sha256_hex(&bytes));

    let conn = rusqlite::Connection::open(&first).unwrap();
    let stored: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'content_sha256'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(a["content_sha256"], stored);

    let root = repo_root();
    let inputs = [
        (
            "spa_sentences_detailed.tsv",
            "tests/fixtures/tatoeba-mini/spa_sentences_detailed.tsv",
        ),
        (
            "eng_sentences_detailed.tsv",
            "tests/fixtures/tatoeba-mini/eng_sentences_detailed.tsv",
        ),
        (
            "spa-eng_links.tsv",
            "tests/fixtures/tatoeba-mini/spa-eng_links.tsv",
        ),
        ("forms-mini.jsonl", "tests/fixtures/forms-mini.jsonl"),
        ("valid-es.yaml", VALID_ES),
        ("exclude.txt", "tests/fixtures/syllabus/exclude.txt"),
    ];
    let sources = a["source_sha256"]
        .as_object()
        .expect("source_sha256 is an object");
    for (name, path) in inputs {
        let hash = sha256_hex(&fs::read(root.join(path)).unwrap());
        match sources.get(name) {
            Some(value) => assert_eq!(value, &json!(hash), "source_sha256.{name}"),
            // The exclude list came from beside the syllabus rather than --exclude.
            None => assert_eq!(
                name, "exclude.txt",
                "source_sha256 lacks {name}: {sources:?}"
            ),
        }
    }
    assert!(sources.len() >= 5 && sources.len() <= 6, "{sources:?}");
    assert_eq!(sources["forms-mini.jsonl"], FORMS_MINI_SHA256);
}

// Bullet 11: the seed content under content/ validates with no error.
#[test]
fn t11_seed_content_validates() {
    let output = parley(&["content", "validate", "content", "--json"]);
    assert_exit(&output, 0);
    let report = stdout_json(&output);
    assert_eq!(report["ok"], true);
    assert_eq!(report["errors"], json!([]));
    assert!(report["warnings"].is_array());
}

// Bullet 13: forms extract writes exactly expected-forms.tsv (whose header carries the
// fixture's SHA-256) and reports a lemma without an entry as missing.
#[test]
fn t13_forms_extract_writes_expected_forms_tsv() {
    let fixture = fs::read(repo_root().join("tests/fixtures/forms-mini.jsonl")).unwrap();
    assert_eq!(
        sha256_hex(&fixture),
        FORMS_MINI_SHA256,
        "forms-mini.jsonl as checked in"
    );
    let expected = read("tests/fixtures/syllabus/expected-forms.tsv");
    assert!(expected.starts_with(&format!(
        "# source: forms-mini.jsonl; sha256: {FORMS_MINI_SHA256}; date: 2026-10-01\n"
    )));

    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("forms.tsv");
    let out_str = path_str(&out);
    let extract = |lemmas: &[&str], out: &str| {
        let mut args = vec![
            "forms",
            "extract",
            "--lang",
            "es",
            "--wiktextract",
            "tests/fixtures/forms-mini.jsonl",
        ];
        for &lemma in lemmas {
            args.push("--lemma");
            args.push(lemma);
        }
        args.extend(["--date", "2026-10-01", "--out", out]);
        parley(&args)
    };

    let three = extract(&["bueno", "noche", "ir"], &out_str);
    assert_exit(&three, 0);
    assert_eq!(
        stdout_json(&three),
        json!({"lemmas": 3, "rows": 15, "missing": []})
    );
    assert_eq!(fs::read_to_string(&out).unwrap(), expected);
    assert_eq!(expected.lines().count(), 16, "a header and 15 rows");

    let missing = extract(&["zzz"], &out_str);
    assert_exit(&missing, 0);
    assert_eq!(
        stdout_json(&missing),
        json!({"lemmas": 0, "rows": 0, "missing": ["zzz"]})
    );
    assert_eq!(
        fs::read_to_string(&out).unwrap(),
        format!("# source: forms-mini.jsonl; sha256: {FORMS_MINI_SHA256}; date: 2026-10-01\n")
    );

    let partial = extract(&["bueno", "zzz"], &out_str);
    assert_exit(&partial, 0);
    assert_eq!(
        stdout_json(&partial),
        json!({"lemmas": 1, "rows": 4, "missing": ["zzz"]})
    );
}

// Bullet 14: the content licence and the attribution file name what ADR-0003 and the pack
// metadata promise.
#[test]
fn t14_content_licence_and_attribution() {
    let licence = read("content/LICENSE");
    assert!(
        licence.contains("CC BY-SA 4.0"),
        "content/LICENSE names CC BY-SA 4.0"
    );
    let attribution = read("content/ATTRIBUTION.md");
    for needle in ["Tatoeba", "CC BY 2.0 FR", "Wiktionary", "CC BY-SA"] {
        assert!(
            attribution.contains(needle),
            "content/ATTRIBUTION.md credits {needle}"
        );
    }
}

// Bullet 15: stable ids end to end on a copy of valid-es under <tmp>/es/syllabus.yaml with the
// forms table beside it. A skill needs four examples (V10), so a fifth example is added before
// example 2 is deleted; the lock then retires #e:2, refuses its reuse (V20) and asks for a lock
// run for a new id (V21) until locked.
#[test]
fn t15_stable_ids_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let content = path_str(dir.path());
    let es = dir.path().join("es");
    fs::create_dir_all(&es).unwrap();
    let syllabus = es.join("syllabus.yaml");
    let lock_file = es.join("ids.lock");
    let full = read(VALID_ES);
    fs::write(&syllabus, &full).unwrap();
    fs::copy(
        repo_root().join("tests/fixtures/syllabus/forms.tsv"),
        es.join("forms.tsv"),
    )
    .unwrap();

    let lock = || parley(&["content", "lock", "--lang", "es", "--content", &content]);
    let validate = || parley(&["content", "validate", &content, "--json"]);
    let errors = |output: &Output| -> Vec<String> {
        stdout_json(output)["errors"]
            .as_array()
            .expect("errors is an array")
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    };

    // A first lock creates ids.lock with every id active.
    assert!(!lock_file.exists());
    let first = lock();
    assert_exit(&first, 0);
    let ids = valid_es_ids();
    assert_eq!(stdout_json(&first), json!({"added": ids, "retired": []}));
    let active: Vec<(&str, &str)> = ids.iter().map(|id| (id.as_str(), "active")).collect();
    assert_eq!(fs::read_to_string(&lock_file).unwrap(), lock_text(&active));
    let clean = validate();
    assert_exit(&clean, 0);
    assert_eq!(
        stdout_json(&clean),
        json!({"ok": true, "errors": [], "warnings": []})
    );

    // A new example with id 5 is V21 until locked, then valid.
    let five = format!("{full}{EXAMPLE_5}");
    fs::write(&syllabus, &five).unwrap();
    let unlocked = validate();
    assert_exit(&unlocked, 1);
    assert_eq!(
        errors(&unlocked),
        vec![format!(
            "{SKILL}#e:5 is not in ids.lock; run parley content lock --lang es"
        )]
    );
    let second = lock();
    assert_exit(&second, 0);
    assert_eq!(
        stdout_json(&second),
        json!({"added": [format!("{SKILL}#e:5")], "retired": []})
    );
    assert_exit(&validate(), 0);

    // Deleting example 2 leaves the other ids alone; V21 reports #e:2 until it is retired.
    let four = without_line(&five, EXAMPLE_2);
    fs::write(&syllabus, &four).unwrap();
    let stale = validate();
    assert_exit(&stale, 1);
    assert_eq!(
        errors(&stale),
        vec![format!(
            "{SKILL}#e:2 is in ids.lock but not in the syllabus; run parley content lock --lang es to retire it"
        )]
    );
    let third = lock();
    assert_exit(&third, 0);
    assert_eq!(
        stdout_json(&third),
        json!({"added": [], "retired": [format!("{SKILL}#e:2")]})
    );
    assert_exit(&validate(), 0);
    let mut lines: Vec<(&str, &str)> = active.clone();
    lines.insert(4, ("es.a1.u01.s01#e:5", "active"));
    lines[1] = ("es.a1.u01.s01#e:2", "retired");
    assert_eq!(fs::read_to_string(&lock_file).unwrap(), lock_text(&lines));

    // A new example reusing id 2 is V20.
    let new_line = |id: u32| {
        format!(
            "          - {{id: {id}, target: \"Buenas noches, Luis.\", en: [\"Good night, Luis.\"]}}\n"
        )
    };
    fs::write(&syllabus, format!("{four}{}", new_line(2))).unwrap();
    let reused = validate();
    assert_exit(&reused, 1);
    assert_eq!(
        errors(&reused),
        vec![format!(
            "{SKILL}#e:2 was retired; give the example a new id"
        )]
    );

    // With a fresh id it is V21 until locked, then valid; the retired line stays.
    fs::write(&syllabus, format!("{four}{}", new_line(6))).unwrap();
    let fresh = validate();
    assert_exit(&fresh, 1);
    assert_eq!(
        errors(&fresh),
        vec![format!(
            "{SKILL}#e:6 is not in ids.lock; run parley content lock --lang es"
        )]
    );
    let fourth = lock();
    assert_exit(&fourth, 0);
    assert_eq!(
        stdout_json(&fourth),
        json!({"added": [format!("{SKILL}#e:6")], "retired": []})
    );
    assert_exit(&validate(), 0);
    lines.insert(5, ("es.a1.u01.s01#e:6", "active"));
    assert_eq!(fs::read_to_string(&lock_file).unwrap(), lock_text(&lines));

    // A syllabus with an error other than V20 or V21 is refused with the validate output.
    fs::write(
        &syllabus,
        four.replace(
            "        can_do: I can say hello and goodbye and ask how someone is.\n",
            "        can_do: Say hello and goodbye.\n",
        ),
    )
    .unwrap();
    let refused = lock();
    assert_exit(&refused, 1);
    assert_eq!(
        stdout_json(&refused)["errors"],
        json!(["es.a1.u01.s01: can_do must start with \"I can\" and end with a full stop"]),
        "the refusal is the validate report"
    );
    assert_eq!(
        fs::read_to_string(&lock_file).unwrap(),
        lock_text(&lines),
        "the lock is untouched"
    );
}

// Bullet 16: usage errors exit 2 with one line on stderr and nothing on stdout: a missing
// required flag, a missing Tatoeba file (named as typed, with / before the file), and a path
// that does not exist.
#[test]
fn t16_pack_build_usage_errors() {
    let without_out = parley(&[
        "pack",
        "build",
        "--lang",
        "es",
        "--tatoeba-dir",
        "tests/fixtures/tatoeba-mini",
        "--wiktextract",
        "tests/fixtures/forms-mini.jsonl",
        "--syllabus",
        VALID_ES,
    ]);
    assert_exit(&without_out, 2);
    assert_eq!(
        text(&without_out.stderr),
        "error: --out is required. Run parley --help.\n"
    );
    assert!(without_out.stdout.is_empty());

    // A copy of the fixture directory without its links file, under the same relative path, so
    // the message names the path exactly as the spec shows it.
    let dir = tempfile::tempdir().unwrap();
    let mini = dir
        .path()
        .join("tests")
        .join("fixtures")
        .join("tatoeba-mini");
    fs::create_dir_all(&mini).unwrap();
    let root = repo_root();
    for name in ["spa_sentences_detailed.tsv", "eng_sentences_detailed.tsv"] {
        fs::copy(
            root.join("tests/fixtures/tatoeba-mini").join(name),
            mini.join(name),
        )
        .unwrap();
    }
    let wiktextract = path_str(&root.join("tests/fixtures/forms-mini.jsonl"));
    let syllabus = path_str(&root.join(VALID_ES));
    let out = path_str(&dir.path().join("es.sqlite3"));
    let missing = parley_in(
        &[
            "pack",
            "build",
            "--lang",
            "es",
            "--tatoeba-dir",
            "tests/fixtures/tatoeba-mini",
            "--wiktextract",
            &wiktextract,
            "--syllabus",
            &syllabus,
            "--built-at",
            "2026-10-01",
            "--out",
            &out,
        ],
        dir.path(),
    );
    assert_exit(&missing, 2);
    assert_eq!(
        text(&missing.stderr),
        "error: tests/fixtures/tatoeba-mini/spa-eng_links.tsv not found\n"
    );
    assert!(missing.stdout.is_empty());
    assert!(!dir.path().join("es.sqlite3").exists());

    let absent = parley(&[
        "content",
        "validate",
        "tests/fixtures/syllabus/nope.yaml",
        "--json",
    ]);
    assert_exit(&absent, 2);
    assert_eq!(
        text(&absent.stderr),
        "error: tests/fixtures/syllabus/nope.yaml not found\n"
    );
    assert!(absent.stdout.is_empty());
}
