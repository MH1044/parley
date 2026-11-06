//! Module 05 tests of the `parley` binary (docs/modules/05-course-content-a1-a2.md, Acceptance
//! tests): every new command (`content scenarios`, `content report`, `freq check`, `pack
//! install`) and the extended `content validate`, with their exit codes, standard error and the
//! files they write. Each test names the bullet it covers; tests without a number cover an
//! Interfaces promise or a usage error the bullets do not name. The fixture cases of bullets 6,
//! 8 and 11 to 14 also run as golden cases under tests/cases/05-course-content-a1-a2, and the
//! library side (the real content read with plain YAML and TOML, gate G3) lives in
//! crates/parley-core/tests/m05_course_content_a1_a2.rs.
//!
//! Expected values come from the spec and from the fixture READMEs (tests/fixtures/scenarios,
//! tests/fixtures/course05, and the module 01 tests/fixtures/tatoeba-mini the fixture pack is
//! built from), never from running the code. Bullets 1 to 5 run over the real course in
//! content/; bullets 4 and 5 are skipped, with the spec's message, when the language's pack is
//! not in data-cache/packs.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use parley_core::api::PackState;
use parley_core::app::{Config, CourseFiles, Env, Parley};
use parley_core::content::{Lang, parse_syllabus};
use parley_core::env::{Clock, FailingNet, Fs, FsError, SplitMix64};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const PARLEY: &str = env!("CARGO_BIN_EXE_parley");

/// What a test over a real pack prints when the pack has not been built on this machine.
const SKIPPED: &str = "pack not found in data-cache/packs; run parley pack build";

/// The `a1_rank_le_2000_share` of each language's report over its pack, as the first run over
/// the built packs measured it on [`RECORDED_ON`]. Only raised afterwards, and never below the
/// floor of bullet 5, 0.75.
const RECORDED_A1_RANK_LE_2000_SHARE: [(&str, f64); 3] =
    [("es", 0.8771), ("fr", 0.9004), ("de", 0.8865)];
/// The date of the run that recorded [`RECORDED_A1_RANK_LE_2000_SHARE`].
const RECORDED_ON: &str = "2026-11-05";

const LANGS: [&str; 3] = ["es", "fr", "de"];
const SCENARIOS: [&str; 6] = [
    "cafe",
    "shop",
    "directions",
    "hotel",
    "doctor",
    "job_interview",
];

const SCENARIO_DIR: &str = "tests/fixtures/scenarios";
const SCENARIO_CONTENT: &str = "tests/fixtures/scenarios/content";
const COURSE: &str = "tests/fixtures/course05/course";
const RANKED: &str = "tests/fixtures/course05/ranked";
const BAD_V22: &str = "tests/fixtures/course05/bad-V22";
const BAD_CL: &str = "tests/fixtures/course05/bad-CL";
const BAD_RV: &str = "tests/fixtures/course05/bad-RV";
const META_JSONL: &str = "tests/fixtures/course05/wiktextract-meta.jsonl";
const META_SHA256: &str = "fd352e532a7ab6fad855c2b9aa40fb73e55a5dc99da6f9181f49418895f361ad";

/// 2026-11-05T00:00:00Z: 20,762 days after the epoch.
const NOW_MS: i64 = 20_762 * 86_400_000;

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

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

/// Runs the binary from the repository root, where the fixture paths of the spec are relative.
fn parley(args: &[&str]) -> Output {
    Command::new(PARLEY)
        .args(args)
        .current_dir(repo_root())
        .output()
        .unwrap_or_else(|e| panic!("could not run parley: {e}"))
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

/// A usage error: exit 2, exactly `line` on standard error, nothing on standard output.
fn assert_usage(output: &Output, line: &str) {
    assert_exit(output, 2);
    assert_eq!(text(&output.stderr), format!("{line}\n"));
    assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
}

/// A failure that is not a usage error: exit 1, exactly `line` on standard error, nothing on
/// standard output.
fn assert_failure(output: &Output, line: &str) {
    assert_exit(output, 1);
    assert_eq!(text(&output.stderr), format!("{line}\n"));
    assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A path as an argument and as messages show it: `/` as the separator.
fn path_str(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("not an array: {value}"))
        .iter()
        .map(|v| {
            v.as_str()
                .unwrap_or_else(|| panic!("not a string: {v}"))
                .to_string()
        })
        .collect()
}

fn number(value: &Value, what: &str) -> f64 {
    value
        .as_f64()
        .unwrap_or_else(|| panic!("{what} is not a number: {value}"))
}

/// Compares a report with its expected value: the two shares within 1e-9 (both are rounded to 4
/// decimal places), everything else exactly.
fn assert_report(actual: &Value, expected: &Value) {
    let mut actual = actual.clone();
    let mut expected = expected.clone();
    for key in ["attested_share", "a1_rank_le_2000_share"] {
        let (a, e) = (actual[key].take(), expected[key].take());
        if e.is_null() {
            assert!(a.is_null(), "{key}: expected null, got {a}");
        } else {
            let (a, e) = (number(&a, key), number(&e, key));
            assert!((a - e).abs() < 1e-9, "{key}: expected {e}, got {a}");
        }
    }
    assert_eq!(actual, expected);
}

/// Builds the fixture pack of tests/fixtures/course05/README.md into `dir`, from the module 01
/// fixtures, and returns its path.
fn build_fixture_pack(dir: &Path, built_at: &str) -> PathBuf {
    let out = dir.join(format!("es-{built_at}.sqlite3"));
    let out_str = path_str(&out);
    let built = parley(&[
        "pack",
        "build",
        "--lang",
        "es",
        "--tatoeba-dir",
        "tests/fixtures/tatoeba-mini",
        "--wiktextract",
        "tests/fixtures/forms-mini.jsonl",
        "--syllabus",
        "tests/fixtures/syllabus/valid-es.yaml",
        "--built-at",
        built_at,
        "--out",
        &out_str,
    ]);
    assert_exit(&built, 0);
    assert_eq!(
        stdout_json(&built)["kept"],
        22,
        "the tatoeba-mini build keeps 22"
    );
    out
}

/// The version of a language's pack in content/packs.lock.toml; every language must have one
/// (bullet 9).
fn locked_version(lang: &str) -> String {
    let lock: toml::Table = read("content/packs.lock.toml")
        .parse()
        .unwrap_or_else(|e| panic!("content/packs.lock.toml: {e}"));
    lock.get(lang)
        .and_then(|table| table.get("version"))
        .and_then(|version| version.as_str())
        .unwrap_or_else(|| panic!("content/packs.lock.toml has no version for [{lang}]"))
        .to_string()
}

/// The built pack of a language, `data-cache/packs/<lang>-<version>.sqlite3` for the lock's
/// version, as a path relative to the repository root; none, after printing the skip message,
/// when it has not been built here.
fn real_pack(lang: &str) -> Option<String> {
    let rel = format!("data-cache/packs/{lang}-{}.sqlite3", locked_version(lang));
    if repo_root().join(&rel).is_file() {
        Some(rel)
    } else {
        println!("{SKIPPED}");
        None
    }
}

/// The units of a syllabus, in syllabus order.
fn syllabus_units(path: &str) -> Vec<String> {
    let syllabus = parse_syllabus(path, &read(path)).unwrap_or_else(|errors| {
        let messages: Vec<&str> = errors.iter().map(|e| e.0.as_str()).collect();
        panic!("{path} does not parse:\n{}", messages.join("\n"))
    });
    syllabus.units.iter().map(|unit| unit.id.clone()).collect()
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value)
        .to_string()
}

/// The `(unit, status)` pairs of a review.yaml, in file order. The binary's tests carry no YAML
/// library, so this reads the two shapes a review file takes: a flow mapping per unit
/// (`- {unit: es.a1.u01, status: unreviewed, date: "2026-11-05"}`) or a block mapping
/// (`- unit: es.a1.u01` with `status:` and `date:` on the following lines).
fn review_marks(text: &str) -> Vec<(String, String)> {
    let mut entries: Vec<BTreeMap<String, String>> = Vec::new();
    let mut in_units = false;
    for raw in text.lines() {
        let line = match raw.find(" #") {
            Some(at) => &raw[..at],
            None => raw,
        };
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') && !line.starts_with('-') {
            in_units = line.trim_end() == "units:";
            continue;
        }
        if !in_units {
            continue;
        }
        let trimmed = line.trim();
        let body = match trimmed.strip_prefix('-') {
            Some(rest) => {
                entries.push(BTreeMap::new());
                rest.trim()
            }
            None => trimmed,
        };
        let body = body.trim_start_matches('{').trim_end_matches('}');
        for pair in body.split(',') {
            if let Some((key, value)) = pair.split_once(':')
                && let Some(entry) = entries.last_mut()
            {
                entry.insert(key.trim().to_string(), unquote(value));
            }
        }
    }
    entries
        .into_iter()
        .map(|entry| {
            let field = |name: &str| {
                entry
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| panic!("a review entry without {name}: {entry:?}"))
            };
            (field("unit"), field("status"))
        })
        .collect()
}

/// The units a review file marks unreviewed, in the syllabus's order.
fn unreviewed_in_syllabus_order(review: &str, syllabus: &str) -> Vec<String> {
    let unreviewed: BTreeSet<String> = review_marks(&read(review))
        .into_iter()
        .filter(|(_, status)| status == "unreviewed")
        .map(|(unit, _)| unit)
        .collect();
    syllabus_units(syllabus)
        .into_iter()
        .filter(|unit| unreviewed.contains(unit))
        .collect()
}

/// A fixed clock at 2026-11-05T00:00Z.
struct FixedClock;

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        NOW_MS
    }
    fn utc_offset_minutes(&self) -> i32 {
        0
    }
}

/// The data directory as the desktop app reaches it: paths relative to the directory.
struct DirFs(PathBuf);

fn fs_error(path: &Path, e: io::Error) -> FsError {
    if e.kind() == io::ErrorKind::NotFound {
        FsError::NotFound(path.to_path_buf())
    } else {
        FsError::Io(e.to_string())
    }
}

impl Fs for DirFs {
    fn read(&self, path: &Path) -> Result<Vec<u8>, FsError> {
        fs::read(self.0.join(path)).map_err(|e| fs_error(path, e))
    }
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), FsError> {
        let full = self.0.join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(|e| fs_error(path, e))?;
        }
        fs::write(full, bytes).map_err(|e| fs_error(path, e))
    }
    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        fs::rename(self.0.join(from), self.0.join(to)).map_err(|e| fs_error(from, e))
    }
    fn remove(&self, path: &Path) -> Result<(), FsError> {
        fs::remove_file(self.0.join(path)).map_err(|e| fs_error(path, e))
    }
}

/// The app's service over `data_dir`, with the module 01 example syllabus as its Spanish
/// course and `packs_lock` as its lock, offline.
fn app(data_dir: &Path, packs_lock: &str) -> Parley {
    let mut courses = BTreeMap::new();
    courses.insert(
        Lang::Es,
        CourseFiles {
            syllabus_yaml: read("tests/fixtures/syllabus/valid-es.yaml"),
            forms_tsv: read("tests/fixtures/syllabus/forms.tsv"),
            numbers_yaml: Some(read("tests/fixtures/syllabus/numbers.yaml")),
        },
    );
    Parley::new(
        Config {
            data_dir: data_dir.to_path_buf(),
            courses,
            packs_lock: packs_lock.to_string(),
        },
        Env {
            clock: Box::new(FixedClock),
            rng: Box::new(SplitMix64::new(5)),
            fs: Box::new(DirFs(data_dir.to_path_buf())),
            net: Box::new(FailingNet),
        },
    )
}

/// Starts the Spanish course in `<data_dir>/parley.sqlite3`, as onboarding does.
fn start_spanish(data_dir: &Path) {
    let mut parley = app(data_dir, "schema = \"parley.packs/1\"\n");
    parley
        .course_start("es")
        .unwrap_or_else(|e| panic!("course_start: {}", e.message));
}

/// A packs.lock.toml whose `[es]` entry pins `pack` at `version`.
fn lock_for(pack: &Path, version: &str) -> String {
    let bytes = fs::read(pack).unwrap();
    let info = parley(&["pack", "info", &path_str(pack)]);
    assert_exit(&info, 0);
    let content = stdout_json(&info)["content_sha256"]
        .as_str()
        .expect("pack info gives content_sha256")
        .to_string();
    format!(
        "schema = \"parley.packs/1\"\n\n[es]\nversion = \"{version}\"\nurl = \"https://github.com/MH1044/parley/releases/download/packs-{version}/es-{version}.sqlite3\"\nsha256 = \"{}\"\nbytes = {}\ncontent_sha256 = \"{content}\"\n",
        sha256_hex(&bytes),
        bytes.len()
    )
}

/// The files under `<data_dir>/packs`, by name.
fn pack_files(data_dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = match fs::read_dir(data_dir.join("packs")) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

fn install(pack: &Path, data_dir: &Path, lock: &Path) -> Output {
    parley(&[
        "pack",
        "install",
        &path_str(pack),
        "--data-dir",
        &path_str(data_dir),
        "--lock",
        &path_str(lock),
    ])
}

// ---------------------------------------------------------------------------------------------
// The real course
// ---------------------------------------------------------------------------------------------

// Bullet 1: `parley content validate content` exits 0 with no errors for es, fr and de, and the
// six scenarios of content/scenarios are part of that run.
#[test]
fn t01_the_course_validates_with_its_scenarios() {
    for lang in LANGS {
        assert!(
            repo_root()
                .join(format!("content/{lang}/syllabus.yaml"))
                .is_file(),
            "content/{lang}/syllabus.yaml exists"
        );
    }
    for id in SCENARIOS {
        assert!(
            repo_root()
                .join(format!("content/scenarios/{id}.yaml"))
                .is_file(),
            "content/scenarios/{id}.yaml exists"
        );
    }
    let output = parley(&["content", "validate", "content", "--json"]);
    assert_exit(&output, 0);
    let report = stdout_json(&output);
    assert_eq!(report["ok"], true);
    assert_eq!(report["errors"], json!([]));
    assert!(report["warnings"].is_array());
}

// Bullet 2: for each language the report shows at least 8 A1 and 8 A2 units, every unit with 3
// to 6 skills, and as unreviewed exactly the units review.yaml marks unreviewed (in syllabus
// order); without --pack both shares are null.
#[test]
fn t02_the_report_shows_the_course_shape_and_review_state() {
    for lang in LANGS {
        let output = parley(&["content", "report", "--lang", lang, "--json"]);
        assert_exit(&output, 0);
        let report = stdout_json(&output);
        assert_eq!(report["lang"], lang);
        let a1 = report["units"]["A1"].as_u64().unwrap_or(0);
        let a2 = report["units"]["A2"].as_u64().unwrap_or(0);
        assert!(a1 >= 8, "{lang}: {a1} A1 units; at least 8");
        assert!(a2 >= 8, "{lang}: {a2} A2 units; at least 8");
        let min = report["skills_per_unit"]["min"].as_u64().unwrap_or(0);
        let max = report["skills_per_unit"]["max"]
            .as_u64()
            .unwrap_or(u64::MAX);
        assert!(
            (3..=6).contains(&min) && (3..=6).contains(&max),
            "{lang}: skills per unit from {min} to {max}; a unit has 3 to 6"
        );
        assert!(report["attested_share"].is_null());
        assert!(report["a1_rank_le_2000_share"].is_null());
        let expected = unreviewed_in_syllabus_order(
            &format!("content/{lang}/review.yaml"),
            &format!("content/{lang}/syllabus.yaml"),
        );
        assert_eq!(
            strings(&report["unreviewed_units"]),
            expected,
            "{lang}: the units review.yaml marks unreviewed"
        );
    }
}

// Bullet 3 (the binary's part): no checklist entry of any language is left without skills. The
// levels and counts of the entries are checked in the library's tests.
#[test]
fn t03_every_checklist_entry_is_covered() {
    for lang in LANGS {
        assert!(
            repo_root()
                .join(format!("content/{lang}/checklist.yaml"))
                .is_file(),
            "content/{lang}/checklist.yaml exists"
        );
        let output = parley(&["content", "report", "--lang", lang, "--json"]);
        assert_exit(&output, 0);
        assert_eq!(
            stdout_json(&output)["checklist_uncovered"],
            json!([]),
            "{lang}: uncovered checklist entries"
        );
    }
}

// Bullet 4: `parley freq check` against each language's pack lists nothing in `differs` or
// `missing` (skipped without the pack).
#[test]
fn t04_ranks_match_the_pack() {
    for lang in LANGS {
        let Some(pack) = real_pack(lang) else {
            continue;
        };
        let output = parley(&["freq", "check", "--lang", lang, "--pack", &pack]);
        let report = stdout_json(&output);
        assert_eq!(report["differs"], json!([]), "{lang}: ranks that differ");
        assert_eq!(
            report["missing"],
            json!([]),
            "{lang}: lemmas the pack never ranks"
        );
        assert!(
            report["lemmas"].as_u64().unwrap_or(0) > 0,
            "{lang}: {report}"
        );
        assert_exit(&output, 0);
    }
}

// Bullet 5: with the pack, attested_share is at least 0.60 and a1_rank_le_2000_share at least
// the recorded threshold and never below 0.75 (skipped without the pack).
#[test]
fn t05_shares_over_the_pack() {
    for lang in LANGS {
        let Some(pack) = real_pack(lang) else {
            continue;
        };
        let output = parley(&[
            "content", "report", "--lang", lang, "--pack", &pack, "--json",
        ]);
        assert_exit(&output, 0);
        let report = stdout_json(&output);
        let attested = number(&report["attested_share"], "attested_share");
        assert!(
            attested >= 0.60,
            "{lang}: attested_share {attested}; at least 0.60"
        );
        let share = number(&report["a1_rank_le_2000_share"], "a1_rank_le_2000_share");
        let recorded = RECORDED_A1_RANK_LE_2000_SHARE
            .iter()
            .find(|(l, _)| *l == lang)
            .map(|(_, share)| *share)
            .expect("a recorded share per language");
        assert!(
            share >= 0.75,
            "{lang}: a1_rank_le_2000_share {share}; never below 0.75"
        );
        assert!(
            share >= recorded - 1e-9,
            "{lang}: a1_rank_le_2000_share {share}; the run of {RECORDED_ON} recorded {recorded}"
        );
    }
}

// Bullet 6 (the real scenarios): `parley content scenarios content/scenarios` passes. Their
// openings in es, fr and de are checked in the library's tests.
#[test]
fn t06_the_six_scenarios_pass() {
    let output = parley(&["content", "scenarios", "content/scenarios"]);
    assert_exit(&output, 0);
    assert_eq!(
        stdout_json(&output),
        json!({"ok": true, "errors": [], "warnings": []})
    );
}

// ---------------------------------------------------------------------------------------------
// Scenario fixtures (bullet 6)
// ---------------------------------------------------------------------------------------------

/// The messages of tests/fixtures/scenarios/README.md, per fixture.
fn scenario_messages(file: &str) -> Vec<&'static str> {
    match file {
        "bad-SC01.yaml" => vec!["tests/fixtures/scenarios/bad-SC01.yaml: /: title_en is required"],
        "bad-SC02.yaml" => {
            vec!["tests/fixtures/scenarios/bad-SC02.yaml: id must match the file name"]
        }
        "bad-SC03.yaml" => vec!["bad-SC03: 7 goals; a scenario has 3 to 6"],
        "bad-SC04.yaml" => vec!["bad-SC04: turn_cap 21 is outside 6 to 20"],
        "bad-SC05.yaml" => vec![
            "bad-SC05: es: skill es.a1.u09.s01 does not exist",
            "bad-SC05: es: skill es.a1.u01.s07 does not exist",
            "bad-SC05: fr: skill fr.a1.u03.s01 does not exist",
        ],
        "bad-SC06.yaml" => vec![
            "bad-SC06: es: opening uses \"pongo\", which the listed skills do not teach",
            "bad-SC06: es: opening uses \"Café\", which the listed skills do not teach",
        ],
        // A digit and a word of numbers.yaml in the opening are taught (SC06 counts numbers as V12).
        "valid-cafe.yaml" | "valid-numbers.yaml" => vec![],
        other => panic!("no fixture {other}"),
    }
}

const SCENARIO_FILES: [&str; 8] = [
    "bad-SC01.yaml",
    "bad-SC02.yaml",
    "bad-SC03.yaml",
    "bad-SC04.yaml",
    "bad-SC05.yaml",
    "bad-SC06.yaml",
    "valid-cafe.yaml",
    "valid-numbers.yaml",
];

// Bullet 6 (SC01 to SC06): each fixture, checked with `--content tests/fixtures/scenarios/content`,
// gives exactly its messages and exit 1; valid-cafe.yaml (and valid-numbers.yaml, whose opening
// uses a digit and a word of numbers.yaml) gives {"ok":true,...} and exit 0. Nothing goes to
// standard error.
#[test]
fn t06_scenario_fixtures_give_exactly_their_messages() {
    for file in SCENARIO_FILES {
        let path = format!("{SCENARIO_DIR}/{file}");
        let output = parley(&["content", "scenarios", &path, "--content", SCENARIO_CONTENT]);
        let messages = scenario_messages(file);
        assert_exit(&output, if messages.is_empty() { 0 } else { 1 });
        assert_eq!(
            stdout_json(&output),
            json!({"ok": messages.is_empty(), "errors": messages, "warnings": []}),
            "{file}"
        );
        assert!(output.stderr.is_empty(), "{file}: {}", text(&output.stderr));
    }
}

// Bullet 6, Interfaces (`content scenarios <dir>`): a directory means every *.yaml in it in byte
// order of file name, so the whole fixture folder gives every message above in file order.
#[test]
fn t06_a_scenario_directory_is_checked_in_file_name_order() {
    let output = parley(&[
        "content",
        "scenarios",
        SCENARIO_DIR,
        "--content",
        SCENARIO_CONTENT,
    ]);
    assert_exit(&output, 1);
    let all: Vec<&str> = SCENARIO_FILES
        .into_iter()
        .flat_map(scenario_messages)
        .collect();
    assert_eq!(all.len(), 9);
    assert_eq!(
        stdout_json(&output),
        json!({"ok": false, "errors": all, "warnings": []})
    );
}

// Interfaces (`content validate <dir>`, extended): a course folder's scenarios/ is checked after
// its syllabi, against them.
#[test]
fn validate_checks_the_scenarios_of_a_course_folder() {
    let output = parley(&["content", "validate", SCENARIO_CONTENT, "--json"]);
    assert_exit(&output, 1);
    assert_eq!(
        stdout_json(&output),
        json!({
            "ok": false,
            "errors": [
                "shop: 2 goals; a scenario has 3 to 6",
                "shop: turn_cap 5 is outside 6 to 20"
            ],
            "warnings": []
        })
    );
}

// Interfaces (`content scenarios`): a missing path is a usage error, and so is no path at all.
#[test]
fn content_scenarios_usage_errors() {
    let missing = parley(&[
        "content",
        "scenarios",
        "tests/fixtures/scenarios/nope.yaml",
        "--content",
        SCENARIO_CONTENT,
    ]);
    assert_usage(
        &missing,
        "error: tests/fixtures/scenarios/nope.yaml not found",
    );

    let none = parley(&["content", "scenarios", "--content", SCENARIO_CONTENT]);
    assert_exit(&none, 2);
    assert!(none.stdout.is_empty());
    let stderr = text(&none.stderr);
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(
        stderr.starts_with("error: ") && stderr.ends_with(" is required. Run parley --help.\n"),
        "{stderr}"
    );
}

// ---------------------------------------------------------------------------------------------
// Forms (bullet 8)
// ---------------------------------------------------------------------------------------------

/// `parley forms extract --lang <lang> --wiktextract wiktextract-meta.jsonl --lemma ...` into a
/// fresh file; the output and the file's text.
fn extract_meta(lang: &str, lemmas: &[&str]) -> (Output, String) {
    let dir = tempfile::tempdir().unwrap();
    let out = path_str(&dir.path().join("forms.tsv"));
    let mut args = vec![
        "forms",
        "extract",
        "--lang",
        lang,
        "--wiktextract",
        META_JSONL,
    ];
    for lemma in lemmas {
        args.extend(["--lemma", lemma]);
    }
    args.extend(["--date", "2026-11-04", "--out", &out]);
    let output = parley(&args);
    let written = fs::read_to_string(&out).unwrap_or_default();
    (output, written)
}

// Bullet 8 (the binary's part): `forms extract` gives no row for wiktextract's table metadata,
// forms tagged `table-tags`, `inflection-template`, `class` or `auxiliary` (Spanish `irregular`,
// `es-conj`, `es-noun`, `regular`; German `strong`, `de-conj`, `5 strong`, `haben`), and writes
// exactly the tables of tests/fixtures/course05/README.md.
#[test]
fn t08_forms_extract_skips_table_metadata() {
    let fixture = fs::read(repo_root().join(META_JSONL)).unwrap();
    assert_eq!(
        sha256_hex(&fixture),
        META_SHA256,
        "{META_JSONL} as checked in"
    );
    let header =
        format!("# source: wiktextract-meta.jsonl; sha256: {META_SHA256}; date: 2026-11-04\n");

    let (spanish, written) = extract_meta("es", &["hablar", "mesa"]);
    assert_exit(&spanish, 0);
    assert_eq!(
        stdout_json(&spanish),
        json!({"lemmas": 2, "rows": 5, "missing": []})
    );
    assert_eq!(
        written,
        format!(
            "{header}\
             habla\thablar\tverb\t\tindicative present singular third-person\n\
             hablar\thablar\tverb\t\tcanonical\n\
             hablo\thablar\tverb\t\tfirst-person indicative present singular\n\
             mesa\tmesa\tnoun\tf\tcanonical\n\
             mesas\tmesa\tnoun\tf\tplural\n"
        )
    );

    let (german, written) = extract_meta("de", &["sehen"]);
    assert_exit(&german, 0);
    assert_eq!(
        stdout_json(&german),
        json!({"lemmas": 1, "rows": 4, "missing": []})
    );
    assert_eq!(
        written,
        format!(
            "{header}\
             gesehen\tsehen\tverb\t\tparticiple past\n\
             sah\tsehen\tverb\t\tfirst-person preterite singular\n\
             sehen\tsehen\tverb\t\tcanonical\n\
             sieht\tsehen\tverb\t\tpresent singular third-person\n"
        )
    );
}

// ---------------------------------------------------------------------------------------------
// Pack install (bullet 10)
// ---------------------------------------------------------------------------------------------

// Bullet 10: a file one byte different from the lock is refused with nothing written; the
// matching file with the course not started asks for the course; once it is started the pack is
// installed: the JSON of the Interfaces, the file in place, no .part file, and pack_status
// installed at the lock's version.
#[test]
fn t10_pack_install_checks_the_lock_and_the_course() {
    let scratch = tempfile::tempdir().unwrap();
    let pack = build_fixture_pack(scratch.path(), "2026-11-05");
    let lock_text = lock_for(&pack, "2026-11-05");
    let lock = scratch.path().join("packs.lock.toml");
    fs::write(&lock, &lock_text).unwrap();
    let original = fs::read(&pack).unwrap();

    // One byte different: the last byte of the SQLite header's application id (offset 71),
    // which leaves the file a readable pack with the same tables.
    let mut changed = original.clone();
    changed[71] ^= 1;
    let damaged = scratch.path().join("damaged").join("es-2026-11-05.sqlite3");
    fs::create_dir_all(damaged.parent().unwrap()).unwrap();
    fs::write(&damaged, &changed).unwrap();
    let started = tempfile::tempdir().unwrap();
    start_spanish(started.path());
    let refused = install(&damaged, started.path(), &lock);
    assert_failure(&refused, "error: This pack does not match packs.lock.toml.");
    assert_eq!(
        pack_files(started.path()),
        Vec::<String>::new(),
        "nothing written"
    );

    // The matching file, but no course started in the data directory.
    let fresh = tempfile::tempdir().unwrap();
    let not_started = install(&pack, fresh.path(), &lock);
    assert_failure(
        &not_started,
        "error: Start the Spanish course in Parley first.",
    );
    assert_eq!(
        pack_files(fresh.path()),
        Vec::<String>::new(),
        "nothing written"
    );
    assert!(
        !fresh.path().join("parley.sqlite3").exists(),
        "a data directory without parley.sqlite3 is not started, and no data file is created"
    );

    // The matching file and the course started.
    let installed = install(&pack, started.path(), &lock);
    assert_exit(&installed, 0);
    assert_eq!(
        stdout_json(&installed),
        json!({"lang": "es", "version": "2026-11-05", "path": "packs/es-2026-11-05.sqlite3"})
    );
    assert_eq!(pack_files(started.path()), vec!["es-2026-11-05.sqlite3"]);
    assert_eq!(
        fs::read(started.path().join("packs/es-2026-11-05.sqlite3")).unwrap(),
        original,
        "the pack is written as it is"
    );
    let mut parley = app(started.path(), &lock_text);
    let status = parley
        .pack_status("es")
        .unwrap_or_else(|e| panic!("pack_status: {}", e.message));
    assert_eq!(status.state, PackState::Installed);
    assert_eq!(status.version.as_deref(), Some("2026-11-05"));
    assert_eq!(status.bytes, Some(original.len() as u64));
}

// Bullet 10, Interfaces (`pack install`): it writes through the same path as `pack_download`, so
// installing a newer pack removes the course's previous one and records the new version.
#[test]
fn t10_pack_install_replaces_the_previous_pack() {
    let scratch = tempfile::tempdir().unwrap();
    let first = build_fixture_pack(scratch.path(), "2026-11-05");
    let second = build_fixture_pack(scratch.path(), "2026-11-06");
    let first_lock = scratch.path().join("first.lock.toml");
    fs::write(&first_lock, lock_for(&first, "2026-11-05")).unwrap();
    let second_lock_text = lock_for(&second, "2026-11-06");
    let second_lock = scratch.path().join("second.lock.toml");
    fs::write(&second_lock, &second_lock_text).unwrap();

    let data = tempfile::tempdir().unwrap();
    start_spanish(data.path());
    assert_exit(&install(&first, data.path(), &first_lock), 0);
    assert_eq!(pack_files(data.path()), vec!["es-2026-11-05.sqlite3"]);
    let newer = install(&second, data.path(), &second_lock);
    assert_exit(&newer, 0);
    assert_eq!(
        stdout_json(&newer),
        json!({"lang": "es", "version": "2026-11-06", "path": "packs/es-2026-11-06.sqlite3"})
    );
    assert_eq!(pack_files(data.path()), vec!["es-2026-11-06.sqlite3"]);
    let mut parley = app(data.path(), &second_lock_text);
    let status = parley
        .pack_status("es")
        .unwrap_or_else(|e| panic!("pack_status: {}", e.message));
    assert_eq!(status.state, PackState::Installed);
    assert_eq!(status.version.as_deref(), Some("2026-11-06"));
}

// Interfaces (`pack install`): a file that is not a pack gives the message `pack info` gives for
// it, exit 1; a missing file, data directory or lock and a missing --data-dir are usage errors.
#[test]
fn pack_install_refusals_and_usage_errors() {
    let scratch = tempfile::tempdir().unwrap();
    let pack = build_fixture_pack(scratch.path(), "2026-11-05");
    let lock = scratch.path().join("packs.lock.toml");
    fs::write(&lock, lock_for(&pack, "2026-11-05")).unwrap();
    let data = tempfile::tempdir().unwrap();
    start_spanish(data.path());

    let not_a_pack = scratch.path().join("notes.sqlite3");
    fs::write(&not_a_pack, "These are notes, not a sentence pack.\n").unwrap();
    let info = parley(&["pack", "info", &path_str(&not_a_pack)]);
    assert_exit(&info, 1);
    let info_stderr = text(&info.stderr);
    assert!(info_stderr.starts_with("error: "), "{info_stderr}");
    let refused = install(&not_a_pack, data.path(), &lock);
    assert_failure(&refused, info_stderr.trim_end());
    assert_eq!(pack_files(data.path()), Vec::<String>::new());

    let without_dir = parley(&[
        "pack",
        "install",
        &path_str(&pack),
        "--lock",
        &path_str(&lock),
    ]);
    assert_usage(
        &without_dir,
        "error: --data-dir is required. Run parley --help.",
    );

    let missing_file = parley(&[
        "pack",
        "install",
        "tests/fixtures/course05/missing.sqlite3",
        "--data-dir",
        &path_str(data.path()),
        "--lock",
        &path_str(&lock),
    ]);
    assert_usage(
        &missing_file,
        "error: tests/fixtures/course05/missing.sqlite3 not found",
    );

    let missing_dir = parley(&[
        "pack",
        "install",
        &path_str(&pack),
        "--data-dir",
        "tests/fixtures/course05/nowhere",
        "--lock",
        &path_str(&lock),
    ]);
    assert_usage(
        &missing_dir,
        "error: tests/fixtures/course05/nowhere not found",
    );

    let missing_lock = parley(&[
        "pack",
        "install",
        &path_str(&pack),
        "--data-dir",
        &path_str(data.path()),
        "--lock",
        "tests/fixtures/course05/missing.lock.toml",
    ]);
    assert_usage(
        &missing_lock,
        "error: tests/fixtures/course05/missing.lock.toml not found",
    );
    assert_eq!(pack_files(data.path()), Vec::<String>::new());
}

// ---------------------------------------------------------------------------------------------
// Review, ranks and checklist fixtures (bullets 11 to 13)
// ---------------------------------------------------------------------------------------------

fn validate_dir(dir: &str) -> Output {
    parley(&["content", "validate", dir, "--json"])
}

// Bullet 11 (the fixtures): bad-RV gives RV02, RV03 and RV04 exactly, exit 1.
#[test]
fn t11_review_fixtures_give_rv02_to_rv04() {
    let output = validate_dir(BAD_RV);
    assert_exit(&output, 1);
    let file = format!("{BAD_RV}/es/review.yaml");
    assert_eq!(
        stdout_json(&output),
        json!({
            "ok": false,
            "errors": [
                format!("{file}: unit es.a1.u01 is listed twice"),
                format!("{file}: unit es.a1.u05 is not in the syllabus"),
                format!("{file}: unit es.a2.u01 is missing"),
            ],
            "warnings": []
        })
    );
}

// Bullet 12: an A1 lemma with rank 3,500 gives the V22 warning (after the V17 warning); a
// single-word lemma without a rank gives the V22 error, at A1 and at A2; a multi-word lemma
// without a rank and an A2 lemma ranked 3,500 give nothing; without review.yaml beside the
// syllabus neither V22 message appears.
#[test]
fn t12_rank_rule_v22() {
    let output = validate_dir(BAD_V22);
    assert_exit(&output, 1);
    assert_eq!(
        stdout_json(&output),
        json!({
            "ok": false,
            "errors": [
                "es.a1.u01.s01: gato needs a rank",
                "es.a2.u01.s01: pueblo needs a rank"
            ],
            "warnings": [
                "warning: ser is taught in es.a1.u01.s01 and es.a2.u01.s01",
                "warning: mesa has rank 3500; check that it belongs at A1"
            ]
        })
    );

    let dir = tempfile::tempdir().unwrap();
    let es = dir.path().join("es");
    fs::create_dir_all(&es).unwrap();
    for name in ["syllabus.yaml", "forms.tsv"] {
        fs::copy(
            repo_root().join(BAD_V22).join("es").join(name),
            es.join(name),
        )
        .unwrap();
    }
    let without_review = validate_dir(&path_str(dir.path()));
    assert_exit(&without_review, 0);
    assert_eq!(
        stdout_json(&without_review),
        json!({
            "ok": true,
            "errors": [],
            "warnings": ["warning: ser is taught in es.a1.u01.s01 and es.a2.u01.s01"]
        })
    );
}

// Bullet 13: bad-CL gives CL02, CL03 and CL04 exactly, exit 1.
#[test]
fn t13_checklist_fixtures_give_cl02_to_cl04() {
    let output = validate_dir(BAD_CL);
    assert_exit(&output, 1);
    let file = format!("{BAD_CL}/es/checklist.yaml");
    assert_eq!(
        stdout_json(&output),
        json!({
            "ok": false,
            "errors": [
                format!("{file}: duplicate entry a1.f.greet"),
                format!("{file}: a1.g.ser_present: skill es.a1.u03.s01 does not exist"),
                format!("{file}: a1.f.directions: skill es.a2.u01.s01 is A2, but the entry is A1"),
            ],
            "warnings": []
        })
    );
}

// Interfaces (`content validate <dir>`): the fixture courses with a correct checklist and review
// file validate, with only the warnings their READMEs state.
#[test]
fn fixture_courses_validate() {
    let course = validate_dir(COURSE);
    assert_exit(&course, 0);
    assert_eq!(
        stdout_json(&course),
        json!({
            "ok": true,
            "errors": [],
            "warnings": ["warning: casa is taught in es.a1.u01.s02 and es.a2.u01.s01"]
        })
    );
    let ranked = validate_dir(RANKED);
    assert_exit(&ranked, 0);
    assert_eq!(
        stdout_json(&ranked),
        json!({"ok": true, "errors": [], "warnings": []})
    );
}

// ---------------------------------------------------------------------------------------------
// Report and freq check on the fixture course (bullets 14 and 15)
// ---------------------------------------------------------------------------------------------

/// The report of tests/fixtures/course05/README.md for the `course` folder.
fn course_report(attested: Value, a1_share: Value) -> Value {
    json!({
        "lang": "es",
        "units": {"A1": 2, "A2": 1},
        "skills": 4,
        "skills_per_unit": {"min": 1, "max": 2},
        "lemmas": {"A1": 18, "A2": 5},
        "examples": 16,
        "attested_share": attested,
        "a1_rank_le_2000_share": a1_share,
        "checklist_uncovered": ["a1.g.plural_nouns", "a2.f.directions"],
        "unreviewed_units": ["es.a1.u02", "es.a2.u01"]
    })
}

// Bullet 14: the report on the fixture course with the fixture pack gives the counts and the
// shares of the README (9 of 16 examples attested, 14 of 17 A1 lemmas ranked within 2,000);
// without --pack both shares are null; --json is the only output form.
#[test]
fn t14_report_on_the_fixture_course() {
    let scratch = tempfile::tempdir().unwrap();
    let pack = path_str(&build_fixture_pack(scratch.path(), "2026-11-05"));
    let with_pack = parley(&[
        "content",
        "report",
        "--lang",
        "es",
        "--content",
        COURSE,
        "--pack",
        &pack,
        "--json",
    ]);
    assert_exit(&with_pack, 0);
    assert_report(
        &stdout_json(&with_pack),
        &course_report(json!(0.5625), json!(0.8235)),
    );

    let without_pack = parley(&[
        "content",
        "report",
        "--lang",
        "es",
        "--content",
        COURSE,
        "--json",
    ]);
    assert_exit(&without_pack, 0);
    assert_report(
        &stdout_json(&without_pack),
        &course_report(Value::Null, Value::Null),
    );

    let plain = parley(&["content", "report", "--lang", "es", "--content", COURSE]);
    assert_exit(&plain, 0);
    assert_report(
        &stdout_json(&plain),
        &course_report(Value::Null, Value::Null),
    );
}

// Interfaces (`content report`): a syllabus that does not validate gives exit 1 and the validate
// output instead of a report.
#[test]
fn report_refuses_a_course_that_does_not_validate() {
    let output = parley(&[
        "content",
        "report",
        "--lang",
        "es",
        "--content",
        BAD_V22,
        "--json",
    ]);
    assert_exit(&output, 1);
    assert_eq!(
        stdout_json(&output),
        json!({
            "ok": false,
            "errors": [
                "es.a1.u01.s01: gato needs a rank",
                "es.a2.u01.s01: pueblo needs a rank"
            ],
            "warnings": [
                "warning: ser is taught in es.a1.u01.s01 and es.a2.u01.s01",
                "warning: mesa has rank 3500; check that it belongs at A1"
            ]
        })
    );
}

// Interfaces (`content report`): usage errors as module 01.
#[test]
fn report_usage_errors() {
    assert_usage(
        &parley(&["content", "report", "--content", COURSE, "--json"]),
        "error: --lang is required. Run parley --help.",
    );
    assert_usage(
        &parley(&[
            "content",
            "report",
            "--lang",
            "it",
            "--content",
            COURSE,
            "--json",
        ]),
        "error: --lang must be es, fr or de. Run parley --help.",
    );
    assert_usage(
        &parley(&[
            "content",
            "report",
            "--lang",
            "fr",
            "--content",
            COURSE,
            "--json",
        ]),
        "error: tests/fixtures/course05/course/fr/syllabus.yaml not found",
    );
    assert_usage(
        &parley(&[
            "content",
            "report",
            "--lang",
            "es",
            "--content",
            COURSE,
            "--pack",
            "tests/fixtures/course05/missing.sqlite3",
            "--json",
        ]),
        "error: tests/fixtures/course05/missing.sqlite3 not found",
    );
}

// Bullet 15: `freq check` on the fixture course lists the README's `differs` and `missing`, in
// document order, and exits 1; on bad-V22 a null rank differs too; on `ranked`, whose ranks all
// match, both lists are empty and the exit code is 0.
#[test]
fn t15_freq_check_on_the_fixture_courses() {
    let scratch = tempfile::tempdir().unwrap();
    let pack = path_str(&build_fixture_pack(scratch.path(), "2026-11-05"));
    let check = |content: &str| {
        parley(&[
            "freq",
            "check",
            "--lang",
            "es",
            "--pack",
            &pack,
            "--content",
            content,
        ])
    };

    let course = check(COURSE);
    assert_exit(&course, 1);
    assert_eq!(
        stdout_json(&course),
        json!({
            "lang": "es",
            "lemmas": 23,
            "differs": [
                {"skill": "es.a1.u01.s02", "lemma": "casa", "rank": 9, "pack_rank": 7},
                {"skill": "es.a2.u01.s01", "lemma": "amigo", "rank": 30, "pack_rank": 9}
            ],
            "missing": [
                {"skill": "es.a1.u01.s01", "lemma": "adiós"},
                {"skill": "es.a1.u01.s02", "lemma": "perro"},
                {"skill": "es.a1.u02.s01", "lemma": "leche"}
            ]
        })
    );

    let unranked = check(BAD_V22);
    assert_exit(&unranked, 1);
    assert_eq!(
        stdout_json(&unranked),
        json!({
            "lang": "es",
            "lemmas": 10,
            "differs": [
                {"skill": "es.a1.u01.s01", "lemma": "gato", "rank": null, "pack_rank": 4},
                {"skill": "es.a1.u01.s01", "lemma": "mesa", "rank": 3500, "pack_rank": 14},
                {"skill": "es.a2.u01.s01", "lemma": "mercado", "rank": 3500, "pack_rank": 13},
                {"skill": "es.a2.u01.s01", "lemma": "pueblo", "rank": null, "pack_rank": 17}
            ],
            "missing": []
        })
    );

    let ranked = check(RANKED);
    assert_exit(&ranked, 0);
    assert_eq!(
        stdout_json(&ranked),
        json!({"lang": "es", "lemmas": 7, "differs": [], "missing": []})
    );
}

// Interfaces (`freq check`): missing flags, a missing pack and a missing content file are usage
// errors; --json is accepted and changes nothing (Design notes).
#[test]
fn freq_check_usage_errors() {
    let scratch = tempfile::tempdir().unwrap();
    let pack = path_str(&build_fixture_pack(scratch.path(), "2026-11-05"));
    // --json is accepted and changes nothing.
    let with_json = parley(&[
        "freq",
        "check",
        "--lang",
        "es",
        "--pack",
        &pack,
        "--content",
        RANKED,
        "--json",
    ]);
    assert_exit(&with_json, 0);
    assert_eq!(
        stdout_json(&with_json),
        json!({"lang": "es", "lemmas": 7, "differs": [], "missing": []})
    );
    assert_usage(
        &parley(&["freq", "check", "--pack", &pack, "--content", COURSE]),
        "error: --lang is required. Run parley --help.",
    );
    assert_usage(
        &parley(&["freq", "check", "--lang", "es", "--content", COURSE]),
        "error: --pack is required. Run parley --help.",
    );
    assert_usage(
        &parley(&[
            "freq",
            "check",
            "--lang",
            "es",
            "--pack",
            "tests/fixtures/course05/missing.sqlite3",
            "--content",
            COURSE,
        ]),
        "error: tests/fixtures/course05/missing.sqlite3 not found",
    );
    assert_usage(
        &parley(&[
            "freq",
            "check",
            "--lang",
            "fr",
            "--pack",
            &pack,
            "--content",
            COURSE,
        ]),
        "error: tests/fixtures/course05/course/fr/syllabus.yaml not found",
    );
}

// Design notes (command groups): `content`, `pack` and `freq` without a subcommand name every
// choice, now including the new commands.
#[test]
fn command_groups_name_their_subcommands() {
    assert_usage(
        &parley(&["content"]),
        "error: content validate, content lock, content scenarios or content report is required. Run parley --help.",
    );
    assert_usage(
        &parley(&["pack"]),
        "error: pack build, pack info or pack install is required. Run parley --help.",
    );
    assert_usage(
        &parley(&["freq"]),
        "error: freq check is required. Run parley --help.",
    );
}

// Design notes (`content scenarios`): a --content directory that does not exist is a usage
// error; --json is accepted and changes nothing.
#[test]
fn content_scenarios_content_directory_and_json() {
    assert_usage(
        &parley(&[
            "content",
            "scenarios",
            "tests/fixtures/scenarios/valid-cafe.yaml",
            "--content",
            "tests/fixtures/scenarios/nowhere",
        ]),
        "error: tests/fixtures/scenarios/nowhere not found",
    );
    let output = parley(&[
        "content",
        "scenarios",
        "tests/fixtures/scenarios/bad-SC04.yaml",
        "--content",
        SCENARIO_CONTENT,
        "--json",
    ]);
    assert_exit(&output, 1);
    assert_eq!(
        stdout_json(&output),
        json!({"ok": false, "errors": ["bad-SC04: turn_cap 21 is outside 6 to 20"], "warnings": []})
    );
}

// Design notes (V22): the rule runs whenever review.yaml lies beside the syllabus, in the file
// form as in the directory form, so the file form of bad-V22 gives the same report.
#[test]
fn v22_runs_in_the_file_form_too() {
    let output = parley(&[
        "content",
        "validate",
        "tests/fixtures/course05/bad-V22/es/syllabus.yaml",
        "--json",
    ]);
    assert_exit(&output, 1);
    assert_eq!(
        stdout_json(&output),
        json!({
            "ok": false,
            "errors": [
                "es.a1.u01.s01: gato needs a rank",
                "es.a2.u01.s01: pueblo needs a rank"
            ],
            "warnings": [
                "warning: ser is taught in es.a1.u01.s01 and es.a2.u01.s01",
                "warning: mesa has rank 3500; check that it belongs at A1"
            ]
        })
    );
}

const BAD_V01: &str = "tests/fixtures/course05/bad-V01";

/// The validate output of bad-V01: V01 for the syllabus, then CL02 and RV02, which need no
/// syllabus; CL03, RV03 and RV04, which do, are left out.
fn bad_v01_report() -> Value {
    json!({
        "ok": false,
        "errors": [
            format!("{BAD_V01}/es/syllabus.yaml: /: lang is required"),
            format!("{BAD_V01}/es/checklist.yaml: duplicate entry a1.f.greet"),
            format!("{BAD_V01}/es/review.yaml: unit es.a1.u01 is listed twice"),
        ],
        "warnings": []
    })
}

// Bullets 11 and 13, Design notes (a syllabus that does not load): the checklist and review file
// beside a syllabus failing V01 still get CL02 and RV02, but no rule that needs the syllabus; the
// report refuses with that same output.
#[test]
fn a_syllabus_that_does_not_load_still_gets_cl02_and_rv02() {
    let output = validate_dir(BAD_V01);
    assert_exit(&output, 1);
    assert_eq!(stdout_json(&output), bad_v01_report());

    let report = parley(&[
        "content",
        "report",
        "--lang",
        "es",
        "--content",
        BAD_V01,
        "--json",
    ]);
    assert_exit(&report, 1);
    assert_eq!(stdout_json(&report), bad_v01_report());
}

// Bullets 11 and 13, rules CL01 and RV01: a checklist entry without text_en and a review entry
// naming a reviewer break their schemas, each reported as V01 words it, and each stops the other
// rules of its file (the repeated entry and unit are not reported).
#[test]
fn checklist_and_review_schema_faults_are_cl01_and_rv01() {
    let dir = "tests/fixtures/course05/bad-CL01-RV01";
    let output = validate_dir(dir);
    assert_exit(&output, 1);
    assert_eq!(
        stdout_json(&output),
        json!({
            "ok": false,
            "errors": [
                format!("{dir}/es/checklist.yaml: /entries/0: text_en is required"),
                format!("{dir}/es/review.yaml: /units/0: unknown field reviewer"),
            ],
            "warnings": []
        })
    );
}

// Design notes (`freq check`): it needs only V01 to pass, and a syllabus that does not load gives
// the V01 report, exit 1.
#[test]
fn freq_check_on_a_syllabus_that_does_not_load() {
    let scratch = tempfile::tempdir().unwrap();
    let pack = path_str(&build_fixture_pack(scratch.path(), "2026-11-05"));
    let output = parley(&[
        "freq",
        "check",
        "--lang",
        "es",
        "--pack",
        &pack,
        "--content",
        BAD_V01,
    ]);
    assert_exit(&output, 1);
    assert_eq!(
        stdout_json(&output),
        json!({
            "ok": false,
            "errors": [format!("{BAD_V01}/es/syllabus.yaml: /: lang is required")],
            "warnings": []
        })
    );
}

// Design notes (`content report`, `freq check`): a --pack file that exists but is not a pack is
// one `error: {message}` line on standard error, exit 1, nothing on standard output.
#[test]
fn report_and_freq_check_refuse_a_file_that_is_not_a_pack() {
    let scratch = tempfile::tempdir().unwrap();
    let notes = path_str(&scratch.path().join("notes.sqlite3"));
    fs::write(&notes, "These are notes, not a sentence pack.\n").unwrap();
    let report = parley(&[
        "content",
        "report",
        "--lang",
        "es",
        "--content",
        COURSE,
        "--pack",
        &notes,
        "--json",
    ]);
    let check = parley(&[
        "freq",
        "check",
        "--lang",
        "es",
        "--pack",
        &notes,
        "--content",
        COURSE,
    ]);
    for (name, output) in [("content report", report), ("freq check", check)] {
        assert_exit(&output, 1);
        assert!(output.stdout.is_empty(), "{name}: {}", text(&output.stdout));
        let stderr = text(&output.stderr);
        assert_eq!(stderr.lines().count(), 1, "{name}: {stderr}");
        assert!(stderr.starts_with("error: "), "{name}: {stderr}");
    }
}

// Design notes (`pack install`): a lock that does not parse gives the lock's own message, exit 1,
// and nothing is written.
#[test]
fn pack_install_refuses_a_lock_that_does_not_parse() {
    let scratch = tempfile::tempdir().unwrap();
    let pack = build_fixture_pack(scratch.path(), "2026-11-05");
    let lock = scratch.path().join("packs.lock.toml");
    let full = lock_for(&pack, "2026-11-05");
    let without_sha: String = full
        .lines()
        .filter(|line| !line.starts_with("sha256 = "))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_ne!(full, without_sha, "the sha256 line is gone");
    fs::write(&lock, without_sha).unwrap();
    let data = tempfile::tempdir().unwrap();
    start_spanish(data.path());
    let refused = install(&pack, data.path(), &lock);
    assert_failure(&refused, "error: packs.lock.toml: [es] needs sha256");
    assert_eq!(
        pack_files(data.path()),
        Vec::<String>::new(),
        "nothing written"
    );
}

// The review reader above reads both shapes of a review file the same way.
#[test]
fn the_review_reader_reads_flow_and_block_entries() {
    let flow = "schema: parley.review/1\nlang: es\nunits:\n  - {unit: es.a1.u01, status: unreviewed, date: \"2026-11-05\"}\n  - {unit: es.a1.u02, status: reviewed, date: \"2026-11-05\"}\n";
    let block = "schema: parley.review/1\nlang: es\nunits:\n  # first unit\n  - unit: es.a1.u01\n    status: unreviewed\n    date: '2026-11-05'\n  - date: \"2026-11-05\"\n    unit: es.a1.u02\n    status: reviewed\n";
    let expected = vec![
        ("es.a1.u01".to_string(), "unreviewed".to_string()),
        ("es.a1.u02".to_string(), "reviewed".to_string()),
    ];
    assert_eq!(review_marks(flow), expected);
    assert_eq!(review_marks(block), expected);
    assert_eq!(
        unreviewed_in_syllabus_order(
            &format!("{COURSE}/es/review.yaml"),
            &format!("{COURSE}/es/syllabus.yaml")
        ),
        vec!["es.a1.u02", "es.a2.u01"]
    );
}
