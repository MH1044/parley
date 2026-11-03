//! Module 02 tests of the `parley` binary (docs/modules/02-srs-and-learner-model.md, Acceptance
//! tests and the CLI block of Interfaces): every usage error, the not-a-log failure, the
//! syllabus that does not load, the command-line half of bullet 7, and what `--retention`,
//! `--slip`, `--seed` and `--json` change. Each test names the bullet it covers. Bullets 5, 11
//! and 12 are golden cases under tests/cases/02-srs-and-learner-model; the library side lives in
//! crates/parley-core/tests/m02_srs_and_learner_model.rs. Expected values come from the spec,
//! never from running the code.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use parley_core::content::{Forms, parse_syllabus};
use parley_core::env::{Rng, SplitMix64};
use parley_core::srs::{
    Card, DAY_MS, Facet, Rating, ReviewLog, SkillRef, SrsParams, Verdict, simulate,
};
use parley_core::store::{ReviewContext, Store, StoreError};
use serde_json::{Value, json};

const PARLEY: &str = env!("CARGO_BIN_EXE_parley");
const MINI: &str = "tests/fixtures/generated/syllabus-mini/xx/syllabus.yaml";
const REFERENCE_LOG: &str = "tests/cases/02-srs-and-learner-model/srs-replay-reference/input.json";
/// 2026-01-05T09:00:00Z.
const T0: i64 = 1_767_603_600_000;
const HOUR: i64 = 3_600_000;

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

fn parley_in(args: &[&str], cwd: &Path) -> Output {
    Command::new(PARLEY)
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|e| panic!("could not run parley: {e}"))
}

/// Runs the binary from the repository root, where the fixture paths are relative.
fn parley(args: &[&str]) -> Output {
    parley_in(args, &repo_root())
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
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

fn stdout_json(output: &Output) -> Value {
    let stdout = text(&output.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not one JSON object ({e}):\n{stdout}\nstderr:\n{}",
            text(&output.stderr)
        )
    })
}

/// A usage error: exit 2, exactly one line on stderr, nothing on stdout.
fn assert_usage_error(output: &Output, line: &str) {
    assert_exit(output, 2);
    assert_eq!(text(&output.stderr), format!("{line}\n"));
    assert!(
        output.stdout.is_empty(),
        "nothing on stdout: {}",
        text(&output.stdout)
    );
}

fn ok<T>(result: Result<T, StoreError>) -> T {
    match result {
        Ok(value) => value,
        Err(e) => panic!("store error: {e}"),
    }
}

fn mini_skills() -> Vec<SkillRef> {
    let text = fs::read_to_string(repo_root().join(MINI)).unwrap();
    let syllabus = parse_syllabus(MINI, &text).unwrap();
    syllabus
        .skills()
        .map(|(_, skill)| SkillRef {
            id: skill.id.clone(),
            cefr: skill.cefr,
        })
        .collect()
}

/// Structural equality; floats within a relative 1e-12, the most a JSON text round trip moves
/// them.
fn assert_same_json(actual: &Value, expected: &Value, path: &str) {
    match (actual, expected) {
        (Value::Object(a), Value::Object(e)) => {
            let mut a_keys: Vec<&String> = a.keys().collect();
            let mut e_keys: Vec<&String> = e.keys().collect();
            a_keys.sort();
            e_keys.sort();
            assert_eq!(a_keys, e_keys, "{path}: keys");
            for (key, value) in e {
                assert_same_json(&a[key], value, &format!("{path}.{key}"));
            }
        }
        (Value::Array(a), Value::Array(e)) => {
            assert_eq!(a.len(), e.len(), "{path}: length");
            for (i, (x, y)) in a.iter().zip(e).enumerate() {
                assert_same_json(x, y, &format!("{path}.{i}"));
            }
        }
        (Value::Number(a), Value::Number(e)) if a.is_f64() || e.is_f64() => {
            let (a, e) = (a.as_f64().unwrap(), e.as_f64().unwrap());
            assert!((a - e).abs() <= 1e-12 * e.abs(), "{path}: {a} against {e}");
        }
        _ => assert_eq!(actual, expected, "{path}"),
    }
}

fn rating_of(n: u64) -> Rating {
    match n % 4 {
        0 => Rating::Again,
        1 => Rating::Hard,
        2 => Rating::Good,
        _ => Rating::Easy,
    }
}

fn verdict_for(rating: Rating) -> Verdict {
    match rating {
        Rating::Again => Verdict::Wrong,
        Rating::Hard => Verdict::Almost,
        Rating::Good | Rating::Easy => Verdict::Correct,
    }
}

// ---------------------------------------------------------------------------------------------
// parley srs replay
// ---------------------------------------------------------------------------------------------

// Bullet 7, the command-line half: 500 seeded reviews over 40 items in a store file; the
// exported review log, replayed by `parley srs replay`, prints the store's reviewed cards.
#[test]
fn t07_cli_replay_of_the_exported_log_prints_the_store_cards() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = ok(Store::open(&dir.path().join("parley.sqlite3")));
    let text = fs::read_to_string(repo_root().join(MINI)).unwrap();
    let syllabus = parse_syllabus(MINI, &text).unwrap();
    ok(store.start_course(&syllabus, &Forms::default(), T0));
    let mut items = Vec::new();
    for (_, skill) in syllabus.skills() {
        items.extend(ok(store.item_ids(&skill.id)));
        if items.len() >= 40 {
            break;
        }
    }
    items.truncate(40);

    let p = SrsParams::default();
    let mut rng = SplitMix64::new(77);
    let mut now = T0;
    for n in 0..500u64 {
        now += (rng.next_u64() % (36 * HOUR as u64)) as i64;
        let item = &items[(rng.next_u64() % 40) as usize];
        let facet = if rng.next_u64().is_multiple_of(2) {
            Facet::Receptive
        } else {
            Facet::Productive
        };
        let rating = rating_of(rng.next_u64());
        let ctx = ReviewContext {
            exercise_type: "translate_to_en".to_string(),
            verdict: verdict_for(rating),
            response_ms: if n % 4 == 0 { None } else { Some(2_000) },
            session_id: format!("session-{}", n / 20),
        };
        ok(store.review(item, facet, rating, now, &ctx, &p));
    }

    let log = ReviewLog {
        schema: "parley.review-log/1".to_string(),
        entries: ok(store.review_log()),
    };
    assert_eq!(log.entries.len(), 500);
    let log_path = dir.path().join("log.json");
    fs::write(&log_path, serde_json::to_string(&log).unwrap()).unwrap();

    let output = parley(&["srs", "replay", &log_path.to_string_lossy(), "--json"]);
    assert_exit(&output, 0);
    let reviewed: Vec<Card> = ok(store.cards())
        .into_iter()
        .filter(|card| card.reps > 0)
        .collect();
    assert!(reviewed.len() > 60);
    let expected = json!({ "cards": serde_json::to_value(&reviewed).unwrap() });
    assert_same_json(&stdout_json(&output), &expected, "(root)");
}

// Interfaces: a card prints its nine fields in the Interfaces order, null for an empty Option.
#[test]
fn srs_replay_prints_card_fields_in_order() {
    let output = parley(&["srs", "replay", REFERENCE_LOG]);
    assert_exit(&output, 0);
    let stdout = text(&output.stdout);
    let first = &stdout[stdout.find('{').unwrap() + 1..];
    let first = &first[first.find('{').expect("a first card")..first.find('}').unwrap()];
    let keys = [
        "\"item_id\"",
        "\"facet\"",
        "\"state\"",
        "\"stability\"",
        "\"difficulty\"",
        "\"due\"",
        "\"last_review\"",
        "\"reps\"",
        "\"lapses\"",
    ];
    let positions: Vec<usize> = keys
        .iter()
        .map(|key| {
            first
                .find(key)
                .unwrap_or_else(|| panic!("{key} in {first}"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|w| w[0] < w[1]),
        "field order: {first}"
    );
    assert!(stdout.trim_start().starts_with('{') && stdout.trim_end().ends_with('}'));
}

// Interfaces: `--json` is accepted and changes nothing.
#[test]
fn srs_replay_json_flag_changes_nothing() {
    let plain = parley(&["srs", "replay", REFERENCE_LOG]);
    let json = parley(&["srs", "replay", REFERENCE_LOG, "--json"]);
    assert_exit(&plain, 0);
    assert_exit(&json, 0);
    assert_eq!(plain.stdout, json.stdout);
}

// Interfaces: elapsed_days, stability_after and difficulty_after in a log entry are optional
// and ignored (recomputed); response_ms may be null or absent.
#[test]
fn srs_replay_recomputes_recorded_values() {
    let reference = parley(&["srs", "replay", REFERENCE_LOG]);
    assert_exit(&reference, 0);

    let mut log: Value =
        serde_json::from_str(&fs::read_to_string(repo_root().join(REFERENCE_LOG)).unwrap())
            .unwrap();
    for entry in log["entries"].as_array_mut().unwrap() {
        entry["elapsed_days"] = json!(123.0);
        entry["stability_after"] = json!(9_999.5);
        entry["difficulty_after"] = json!(0.25);
        entry["response_ms"] = json!(1_500);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recorded.json");
    fs::write(&path, serde_json::to_string_pretty(&log).unwrap()).unwrap();
    let recorded = parley(&["srs", "replay", &path.to_string_lossy()]);
    assert_exit(&recorded, 0);
    assert_eq!(stdout_json(&recorded), stdout_json(&reference));
}

/// The interval FSRS gives a stability at a desired retention, S / f × (r^(−1/d) − 1), with
/// d = w[20] of the crate's default parameters and f = 0.9^(−1/d) − 1.
fn interval_at(stability: f64, desired_retention: f64) -> f64 {
    let d = parley_core::srs::default_weights()[20];
    let f = 0.9f64.powf(-1.0 / d) - 1.0;
    stability / f * (desired_retention.powf(-1.0 / d) - 1.0)
}

// Interfaces and Design notes: `--retention` changes due dates only; at 0.80 each due is the
// last review plus min(365, max(1, round(interval at 0.80))) days.
#[test]
fn srs_replay_retention_moves_due_dates_only() {
    let base = parley(&["srs", "replay", REFERENCE_LOG]);
    let low = parley(&["srs", "replay", REFERENCE_LOG, "--retention", "0.80"]);
    assert_exit(&base, 0);
    assert_exit(&low, 0);
    let (base, low) = (stdout_json(&base), stdout_json(&low));
    let base_cards = base["cards"].as_array().unwrap();
    let low_cards = low["cards"].as_array().unwrap();
    assert_eq!(base_cards.len(), low_cards.len());
    let mut moved = 0;
    for (b, l) in base_cards.iter().zip(low_cards) {
        for field in [
            "item_id",
            "facet",
            "state",
            "stability",
            "difficulty",
            "last_review",
            "reps",
            "lapses",
        ] {
            assert_eq!(b[field], l[field], "{}: {field}", b["item_id"]);
        }
        let stability = l["stability"].as_f64().unwrap();
        let x = interval_at(stability, 0.80);
        let days = (l["due"].as_i64().unwrap() - l["last_review"].as_i64().unwrap()) / DAY_MS;
        let clamp = |v: f64| (v as i64).clamp(1, 365);
        let fits = if x < 365.5 && (x - x.floor() - 0.5).abs() < 1e-3 {
            days == clamp(x.floor()) || days == clamp(x.ceil())
        } else {
            days == clamp(x.round())
        };
        assert!(
            fits,
            "{}: {days} days for S = {stability} at 0.80",
            l["item_id"]
        );
        if b["due"] != l["due"] {
            moved += 1;
        }
    }
    assert!(moved > 0);

    for edge in ["0.80", "0.95", "0.9"] {
        let output = parley(&["srs", "replay", REFERENCE_LOG, "--retention", edge]);
        assert_exit(&output, 0);
    }
}

// Bullet 17: every usage error of `parley srs replay` prints its line.
#[test]
fn t17_srs_replay_usage_errors() {
    assert_usage_error(
        &parley(&["srs", "replay"]),
        "error: <log.json> is required. Run parley --help.",
    );
    assert_usage_error(
        &parley(&["srs", "replay", "--json"]),
        "error: <log.json> is required. Run parley --help.",
    );
    assert_usage_error(
        &parley(&["srs", "replay", "tests/fixtures/nope.json"]),
        "error: tests/fixtures/nope.json not found",
    );
    for bad in ["0.5", "0.99", "0.79", "0.951", "1", "abc", "-0.9", ""] {
        assert_usage_error(
            &parley(&["srs", "replay", REFERENCE_LOG, "--retention", bad]),
            "error: --retention must be a number from 0.80 to 0.95. Run parley --help.",
        );
    }
}

// Bullet 17: a file that is not a review log exits 1 with `error: {path}: {parser message}`
// on one line of stderr.
#[test]
fn t17_srs_replay_refuses_a_file_that_is_not_a_review_log() {
    let dir = tempfile::tempdir().unwrap();
    for (name, contents) in [
        ("not-json.json", "this is not JSON\n"),
        ("no-entries.json", "{\"schema\": \"parley.review-log/1\"}\n"),
        (
            "bad-entry.json",
            "{\"schema\": \"parley.review-log/1\", \"entries\": [{\"item_id\": \"a\"}]}\n",
        ),
        (
            "bad-rating.json",
            "{\"schema\": \"parley.review-log/1\", \"entries\": [{\"item_id\": \"a\", \"facet\": \"receptive\", \"reviewed_at\": 0, \"rating\": 7, \"exercise_type\": \"x\", \"verdict\": \"correct\", \"session_id\": \"s\"}]}\n",
        ),
    ] {
        fs::write(dir.path().join(name), contents).unwrap();
        let output = parley_in(&["srs", "replay", name, "--json"], dir.path());
        assert_exit(&output, 1);
        let stderr = text(&output.stderr);
        let prefix = format!("error: {name}: ");
        assert!(stderr.starts_with(&prefix), "{name}: {stderr:?}");
        assert!(
            stderr.len() > prefix.len() + 1,
            "{name}: the parser message follows"
        );
        assert_eq!(stderr.lines().count(), 1, "{name}: one line: {stderr:?}");
        assert!(stderr.ends_with('\n'));
        assert!(output.stdout.is_empty(), "{name}: {}", text(&output.stdout));
    }
}

// A log that is not UTF-8 text fails with its own line (Design notes, CLI decisions).
#[test]
fn srs_replay_refuses_a_file_that_is_not_utf8() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("bin.json"), [0xFF, 0xFE, b'{', b'}']).unwrap();
    let output = parley_in(&["srs", "replay", "bin.json", "--json"], dir.path());
    assert_exit(&output, 1);
    assert_eq!(text(&output.stderr), "error: bin.json: not valid UTF-8\n");
    assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
}

// A log saved with a UTF-8 byte-order mark reads like one without (Design notes, ReviewLog::parse).
#[test]
fn srs_replay_reads_a_log_with_a_byte_order_mark() {
    let log = fs::read(repo_root().join(REFERENCE_LOG)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut with_bom = vec![0xEF, 0xBB, 0xBF];
    with_bom.extend_from_slice(&log);
    fs::write(dir.path().join("plain.json"), &log).unwrap();
    fs::write(dir.path().join("bom.json"), &with_bom).unwrap();
    let plain = parley_in(&["srs", "replay", "plain.json", "--json"], dir.path());
    let bom = parley_in(&["srs", "replay", "bom.json", "--json"], dir.path());
    assert_exit(&plain, 0);
    assert_exit(&bom, 0);
    assert_eq!(stdout_json(&bom), stdout_json(&plain));
}

// ---------------------------------------------------------------------------------------------
// parley placement simulate
// ---------------------------------------------------------------------------------------------

// Bullet 17: every usage error of `parley placement simulate` prints its line.
#[test]
fn t17_placement_simulate_usage_errors() {
    assert_usage_error(
        &parley(&["placement", "simulate", "--boundary", "5"]),
        "error: --syllabus is required. Run parley --help.",
    );
    assert_usage_error(
        &parley(&["placement", "simulate", "--syllabus", MINI]),
        "error: --boundary is required. Run parley --help.",
    );
    assert_usage_error(
        &parley(&[
            "placement",
            "simulate",
            "--syllabus",
            "tests/fixtures/nope.yaml",
            "--boundary",
            "5",
        ]),
        "error: tests/fixtures/nope.yaml not found",
    );
    for bad in ["abc", "-1", "2.5", ""] {
        assert_usage_error(
            &parley(&[
                "placement",
                "simulate",
                "--syllabus",
                MINI,
                "--boundary",
                bad,
            ]),
            "error: --boundary must be a whole number. Run parley --help.",
        );
    }
    for bad in ["1.5", "-0.1", "abc", "2", ""] {
        assert_usage_error(
            &parley(&[
                "placement",
                "simulate",
                "--syllabus",
                MINI,
                "--boundary",
                "5",
                "--slip",
                bad,
            ]),
            "error: --slip must be a number from 0 to 1. Run parley --help.",
        );
    }
    for bad in ["abc", "-1", "1.5", ""] {
        assert_usage_error(
            &parley(&[
                "placement",
                "simulate",
                "--syllabus",
                MINI,
                "--boundary",
                "5",
                "--seed",
                bad,
            ]),
            "error: --seed must be a whole number. Run parley --help.",
        );
    }
}

// Interfaces: a syllabus that does not load exits 1 with the load errors as `content validate
// --json` prints them. (The golden case placement-syllabus-does-not-load covers a schema error.)
#[test]
fn placement_simulate_on_a_file_that_is_not_yaml() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("broken.yaml"),
        "units:\n  - id: x\n   title: y\n",
    )
    .unwrap();
    let output = parley_in(
        &[
            "placement",
            "simulate",
            "--syllabus",
            "broken.yaml",
            "--boundary",
            "5",
        ],
        dir.path(),
    );
    assert_exit(&output, 1);
    let report = stdout_json(&output);
    assert_eq!(report["ok"], false);
    assert_eq!(report["warnings"], json!([]));
    let errors = report["errors"].as_array().expect("errors is an array");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].as_str().unwrap().starts_with("broken.yaml: "),
        "{errors:?}"
    );
}

// Interfaces: the simulated learner draws from SplitMix64(<seed>), default seed 0, and default
// slip 0; the command prints what `simulate` gives for the same inputs; `--json` changes
// nothing.
#[test]
fn placement_simulate_seed_slip_and_json() {
    let run = |extra: &[&str]| {
        let mut args = vec![
            "placement",
            "simulate",
            "--syllabus",
            MINI,
            "--boundary",
            "40",
        ];
        args.extend_from_slice(extra);
        let output = parley(&args);
        assert_exit(&output, 0);
        output
    };
    let seeded = run(&["--slip", "0.1", "--seed", "7"]);
    let again = run(&["--slip", "0.1", "--seed", "7", "--json"]);
    assert_eq!(seeded.stdout, again.stdout, "the same seed, the same run");

    let expected = simulate(mini_skills(), 40, 0.1, &mut SplitMix64::new(7));
    let expected_json = json!({
        "skills": 80,
        "questions": expected.questions,
        "answers": expected.answers,
        "reversals": expected.reversals,
        "estimate": expected.estimate,
        "label": expected.label,
        "tested_out": expected.tested_out,
    });
    assert_same_json(&stdout_json(&seeded), &expected_json, "(root)");

    let default_seed = run(&["--slip", "0.1"]);
    let seed_0 = run(&["--slip", "0.1", "--seed", "0"]);
    assert_eq!(default_seed.stdout, seed_0.stdout, "the default seed is 0");
    let default_slip = run(&[]);
    let slip_0 = run(&["--slip", "0"]);
    assert_eq!(default_slip.stdout, slip_0.stdout, "the default slip is 0");

    // Slip 1 flips every answer: a learner who knows everything answers like one who knows
    // nothing.
    let flipped = run(&["--slip", "1"]);
    let report = stdout_json(&flipped);
    assert_eq!(report["questions"], json!([20, 0, 0, 0, 0]));
    assert_eq!(report["estimate"], json!(0));
}
