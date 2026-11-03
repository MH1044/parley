//! Module 03 tests of the `parley` binary (docs/modules/03-exercise-engine-and-grading.md,
//! Acceptance tests and the CLI block of Interfaces): the usage errors of `parley grade`,
//! `parley lesson template` and `parley streak simulate` (bullet 19), the answer that normalises
//! to nothing, the default forms table, what `--json` changes, and runs of `parley lesson
//! template` checked against the same lesson built through the library as the spec describes it
//! (bullet 10). The grading and streak results themselves are golden cases under
//! tests/cases/03-exercise-engine-and-grading; the library side lives in
//! crates/parley-core/tests/m03_exercise_engine_and_grading.rs. Expected values come from the
//! spec, never from running the code.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use parley_core::content::{
    Forms, Lang, Lemma, Syllabus, derive_items, load_numbers, parse_syllabus,
};
use parley_core::env::SplitMix64;
use parley_core::exercise::{
    Allowed, Capabilities, CheckCtx, ComposeRequest, ExerciseType, Lesson, TemplateCtx,
    check_exercise, compose, max_words, template_lesson,
};
use parley_core::grading::normalise;
use parley_core::store::{Store, StoreError};
use serde_json::{Value, json};

const PARLEY: &str = env!("CARGO_BIN_EXE_parley");
const COURSE: &str = "tests/cases/03-exercise-engine-and-grading/course-es.yaml";
const COURSE_FORMS: &str = "tests/cases/03-exercise-engine-and-grading/course-es.forms.tsv";
const STREAK_CASE: &str = "tests/cases/03-exercise-engine-and-grading/streak-16-seven-active-days-earn-a-freeze/input.json";

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
#[track_caller]
fn assert_usage_error(output: &Output, line: &str) {
    assert_exit(output, 2);
    assert_eq!(text(&output.stderr), format!("{line}\n"));
    assert!(
        output.stdout.is_empty(),
        "nothing on stdout: {}",
        text(&output.stdout)
    );
}

/// A failure that is not a usage error: exit 1, exactly this line on stderr, nothing on stdout.
#[track_caller]
fn assert_failure(output: &Output, line: &str) {
    assert_exit(output, 1);
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

/// `grade` arguments with every required flag, then `extra`.
fn grade_args<'a>(extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![
        "grade",
        "--lang",
        "es",
        "--type",
        "translate_to_target",
        "--expected",
        "Quiero un café.",
        "--answer",
        "Quiero un café",
    ];
    args.extend_from_slice(extra);
    args
}

/// `grade_args` without the flag `flag` and its value.
fn grade_args_without(flag: &str) -> Vec<&'static str> {
    let all = grade_args(&[]);
    let mut args = vec![all[0]];
    for pair in all[1..].chunks(2) {
        if pair[0] != flag {
            args.extend_from_slice(pair);
        }
    }
    args
}

// ---------------------------------------------------------------------------------------------
// parley grade
// ---------------------------------------------------------------------------------------------

// Bullet 19: every usage error of `parley grade` prints its line, in module 01's wording.
#[test]
fn t19_grade_usage_errors() {
    for flag in ["--lang", "--type", "--expected", "--answer"] {
        assert_usage_error(
            &parley(&grade_args_without(flag)),
            &format!("error: {flag} is required. Run parley --help."),
        );
    }
    let mut bad_lang = grade_args(&[]);
    bad_lang[2] = "en";
    assert_usage_error(
        &parley(&bad_lang),
        "error: --lang must be es, fr or de. Run parley --help.",
    );
    for kind in ["pick_meaning", "match_pairs", "translate", ""] {
        let mut args = grade_args(&[]);
        args[4] = kind;
        assert_usage_error(
            &parley(&args),
            "error: --type must be translate_to_target, translate_to_en, listen_type, speak or fill_gap. Run parley --help.",
        );
    }
    for accent in ["loose", "Lenient", ""] {
        assert_usage_error(
            &parley(&grade_args(&["--accent", accent])),
            "error: --accent must be lenient, strict or ignore. Run parley --help.",
        );
    }
    assert_usage_error(
        &parley(&grade_args(&["--forms", "tests/fixtures/nowhere.tsv"])),
        "error: tests/fixtures/nowhere.tsv not found",
    );
    assert_usage_error(
        &parley(&grade_args(&["--noun", "--forms", COURSE_FORMS])),
        "error: --noun needs an expected answer that starts with an article. Run parley --help.",
    );
    assert_usage_error(
        &parley(&grade_args(&["--bogus"])),
        "error: unknown argument --bogus. Run parley --help.",
    );
}

// Interfaces: an answer that normalises to nothing exits 1 with "Type an answer first.".
#[test]
fn t19_grade_refuses_an_empty_answer() {
    for empty in ["", "  ¿?  ", "..."] {
        let mut args = grade_args(&[]);
        args[8] = empty;
        assert_failure(&parley(&args), "error: Type an answer first.");
    }
}

// Interfaces: `--json` is accepted and changes nothing; the output has the six fields.
#[test]
fn grade_json_flag_changes_nothing() {
    let plain = parley(&grade_args(&[]));
    let json = parley(&grade_args(&["--json"]));
    assert_exit(&plain, 0);
    assert_exit(&json, 0);
    assert_eq!(plain.stdout, json.stdout);
    let out = stdout_json(&plain);
    let mut keys: Vec<&String> = out.as_object().expect("one object").keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        ["corrected", "message", "rating", "tags", "verdict", "words"]
    );
    assert_eq!(out["verdict"], "correct", "punctuation is not graded");
}

// Interfaces: --forms defaults to content/<l>/forms.tsv, and to an empty table when that file
// does not exist. With an empty table `pero` is no known word, so `el pero` for `el perro` is a
// typo; with the fixture table it is not (bullet 6).
#[test]
fn grade_forms_default_to_the_course_table_or_nothing() {
    let args = [
        "grade",
        "--lang",
        "es",
        "--type",
        "listen_type",
        "--expected",
        "el perro",
        "--answer",
        "el pero",
    ];
    let empty_dir = tempfile::tempdir().unwrap();
    let without = parley_in(&args, empty_dir.path());
    assert_exit(&without, 0);
    assert_eq!(
        stdout_json(&without),
        json!({"verdict": "typo", "tags": ["typo"], "message": "Typo: perro",
               "corrected": "el perro", "words": ["perro"], "rating": 2})
    );
    let mut with_table = args.to_vec();
    with_table.extend_from_slice(&["--forms", COURSE_FORMS]);
    let with = parley(&with_table);
    assert_exit(&with, 0);
    assert_eq!(stdout_json(&with)["verdict"], "wrong");

    // From the repository root, content/es/forms.tsv gives día its gender for --noun.
    let noun = parley(&[
        "grade",
        "--lang",
        "es",
        "--type",
        "translate_to_target",
        "--expected",
        "el día",
        "--answer",
        "la día",
        "--noun",
    ]);
    assert_exit(&noun, 0);
    assert_eq!(stdout_json(&noun)["message"], "Día is masculine: el día.");
}

// ---------------------------------------------------------------------------------------------
// parley lesson template
// ---------------------------------------------------------------------------------------------

/// One course directory as `parley lesson template --content <dir>` reads it.
struct CourseDir {
    syllabus: Syllabus,
    forms: Forms,
    numbers: Vec<String>,
}

fn course_dir(dir: &Path, lang: Lang) -> CourseDir {
    let folder = dir.join(lang.as_str());
    let syllabus_text = fs::read_to_string(folder.join("syllabus.yaml")).unwrap();
    let syllabus = parse_syllabus("syllabus.yaml", &syllabus_text)
        .unwrap_or_else(|errors| panic!("the syllabus parses: {errors:?}"));
    let forms = match fs::read_to_string(folder.join("forms.tsv")) {
        Ok(table) => Forms::load_tsv(table.as_bytes()).unwrap(),
        Err(_) => Forms::default(),
    };
    let numbers = match fs::read_to_string(folder.join("numbers.yaml")) {
        Ok(words) => load_numbers(&words).unwrap(),
        Err(_) => Vec::new(),
    };
    CourseDir {
        syllabus,
        forms,
        numbers,
    }
}

/// The lesson `parley lesson template` describes, built through the library: an in-memory
/// store started from the course at time 0, the skill's first (seq − 1) × 5 items introduced
/// at time 0, compose at time 0 with lesson_length 12, new_items_per_lesson 5 and no
/// capabilities, then template_lesson with Allowed::for_skill, no pack sentences, the plan's
/// item ids and SplitMix64(seed).
fn library_lesson(course: &CourseDir, skill: &str, seq: u32, seed: u64) -> Lesson {
    let mut store = ok(Store::open_in_memory());
    ok(store.start_course(&course.syllabus, &course.forms, 0));
    for item in ok(store.item_ids(skill))
        .iter()
        .take(((seq - 1) * 5) as usize)
    {
        ok(store.introduce(item, 0));
    }
    let request = ComposeRequest {
        lang: course.syllabus.lang,
        skill_id: skill.to_string(),
        lesson_length: 12,
        new_items_per_lesson: 5,
        capabilities: Capabilities {
            audio_out: false,
            mic: false,
        },
    };
    let plan = ok(compose(&request, &store, 0));
    let items = derive_items(&course.syllabus, &course.forms);
    let allowed = Allowed::for_skill(&course.syllabus, &course.numbers, skill);
    let plan_items: Vec<String> = plan.entries.iter().map(|e| e.item_id.clone()).collect();
    let ctx = TemplateCtx {
        syllabus: &course.syllabus,
        forms: &course.forms,
        items: &items,
        allowed: &allowed,
        sentences: &[],
        skill_id: skill,
        plan_items: &plan_items,
    };
    template_lesson(&plan, &ctx, seq, &mut SplitMix64::new(seed))
}

/// Every exercise of the lesson that fails check_exercise (focus the item's lemmas, seen the
/// earlier exercises, max_words of the skill's level).
fn failing_exercises(course: &CourseDir, lesson: &Lesson) -> Vec<String> {
    let syllabus = &course.syllabus;
    let skill = lesson.skill_id.as_str();
    let cefr = syllabus
        .skills()
        .find(|(_, s)| s.id == skill)
        .map(|(_, s)| s.cefr)
        .expect("the lesson's skill");
    let items = derive_items(syllabus, &course.forms);
    let allowed = Allowed::for_skill(syllabus, &course.numbers, skill);
    let mut seen: Vec<(ExerciseType, String)> = Vec::new();
    let mut failing = Vec::new();
    for ex in &lesson.exercises {
        let focus: Vec<Lemma> = items
            .iter()
            .find(|item| item.id == ex.item_id)
            .map(|item| {
                item.lemmas
                    .iter()
                    .filter_map(|name| {
                        syllabus
                            .skills()
                            .flat_map(|(_, s)| s.lemmas.iter())
                            .find(|lemma| lemma.lemma == *name)
                            .cloned()
                    })
                    .collect()
            })
            .unwrap_or_default();
        let ctx = CheckCtx {
            lang: lesson.lang,
            forms: &course.forms,
            allowed: &allowed,
            max_words: max_words(cefr),
            focus: &focus,
            seen: &seen,
        };
        if let Err(failure) = check_exercise(ex, &ctx) {
            failing.push(format!("{} fails {}", ex.id, failure.rule()));
        }
        let first = ex.targets().first().copied().unwrap_or("").to_string();
        seen.push((ex.kind(), normalise(lesson.lang, &first)));
    }
    failing
}

// Bullet 10, the command line: `parley lesson template` prints the lesson the library builds as
// the spec describes, for skills of each language and seq 1 and 2; every exercise passes the
// checks; the lesson header names the language, skill, seq and template origin.
#[test]
fn t10_lesson_template_prints_the_library_lesson() {
    let content = repo_root().join("content");
    for (lang, skill, seq, seed) in [
        (Lang::Es, "es.a1.u02.s02", 1, 1),
        (Lang::Es, "es.a1.u01.s01", 1, 7),
        (Lang::Es, "es.a1.u02.s02", 2, 5),
        (Lang::Fr, "fr.a1.u01.s01", 1, 3),
        (Lang::De, "de.a1.u01.s02", 1, 11),
    ] {
        let seq_text = seq.to_string();
        let seed_text = seed.to_string();
        let output = parley(&[
            "lesson",
            "template",
            "--lang",
            lang.as_str(),
            "--skill",
            skill,
            "--seq",
            &seq_text,
            "--seed",
            &seed_text,
        ]);
        assert_exit(&output, 0);
        let printed = stdout_json(&output);
        let course = course_dir(&content, lang);
        let lesson = library_lesson(&course, skill, seq, seed);
        assert_eq!(
            printed,
            serde_json::to_value(&lesson).unwrap(),
            "{skill} seq {seq} seed {seed}"
        );
        assert_eq!(printed["schema"], "parley.lesson/1");
        assert_eq!(printed["lang"], lang.as_str());
        assert_eq!(printed["skill_id"], skill);
        assert_eq!(printed["seq"], seq);
        assert_eq!(printed["origin"], "template");
        assert!(!lesson.exercises.is_empty(), "{skill}: exercises");
        let failing = failing_exercises(&course, &lesson);
        assert!(failing.is_empty(), "{skill} seed {seed}: {failing:?}");
    }
}

// Bullet 10: seeds 1 and 2 give different lessons for a skill; a seed gives the same lesson
// every time; --seq defaults to 1 and --seed to 0; --json changes nothing.
#[test]
fn t10_lesson_template_seeds_and_defaults() {
    let run = |extra: &[&str]| {
        let mut args = vec![
            "lesson",
            "template",
            "--lang",
            "es",
            "--skill",
            "es.a1.u02.s02",
        ];
        args.extend_from_slice(extra);
        let output = parley(&args);
        assert_exit(&output, 0);
        output.stdout
    };
    let one = run(&["--seed", "1"]);
    assert_eq!(one, run(&["--seed", "1"]), "the same seed, the same lesson");
    assert_ne!(one, run(&["--seed", "2"]), "seeds 1 and 2 differ");
    assert_eq!(run(&[]), run(&["--seq", "1", "--seed", "0"]));
    assert_eq!(run(&["--seed", "4"]), run(&["--seed", "4", "--json"]));
}

// Interfaces: --content <dir> reads <dir>/<l>/syllabus.yaml and the forms.tsv beside it (no
// numbers.yaml here): the fixture course copied into a temporary directory.
#[test]
fn lesson_template_reads_another_content_directory() {
    let root = repo_root();
    let dir = tempfile::tempdir().unwrap();
    let course_path = dir.path().join("course");
    let es = course_path.join("es");
    fs::create_dir_all(&es).unwrap();
    fs::copy(root.join(COURSE), es.join("syllabus.yaml")).unwrap();
    fs::copy(root.join(COURSE_FORMS), es.join("forms.tsv")).unwrap();
    let run = |content: &str| {
        parley_in(
            &[
                "lesson",
                "template",
                "--lang",
                "es",
                "--skill",
                "es.a1.u01.s02",
                "--seed",
                "4",
                "--content",
                content,
            ],
            dir.path(),
        )
    };
    assert_usage_error(
        &run("elsewhere"),
        "error: elsewhere/es/syllabus.yaml not found",
    );
    let output = run("course");
    assert_exit(&output, 0);
    let course = course_dir(&course_path, Lang::Es);
    let lesson = library_lesson(&course, "es.a1.u01.s02", 1, 4);
    assert_eq!(stdout_json(&output), serde_json::to_value(&lesson).unwrap());
}

// Bullet 19: every usage error of `parley lesson template` prints its line.
#[test]
fn t19_lesson_template_usage_errors() {
    let base = [
        "lesson",
        "template",
        "--lang",
        "es",
        "--skill",
        "es.a1.u01.s01",
    ];
    assert_usage_error(
        &parley(&["lesson", "template", "--skill", "es.a1.u01.s01"]),
        "error: --lang is required. Run parley --help.",
    );
    assert_usage_error(
        &parley(&["lesson", "template", "--lang", "es"]),
        "error: --skill is required. Run parley --help.",
    );
    assert_usage_error(
        &parley(&[
            "lesson",
            "template",
            "--lang",
            "xx",
            "--skill",
            "es.a1.u01.s01",
        ]),
        "error: --lang must be es, fr or de. Run parley --help.",
    );
    for seq in ["0", "-1", "abc", "1.5", ""] {
        let mut args = base.to_vec();
        args.extend_from_slice(&["--seq", seq]);
        assert_usage_error(
            &parley(&args),
            "error: --seq must be a whole number from 1. Run parley --help.",
        );
    }
    for seed in ["-1", "abc", "1.5", ""] {
        let mut args = base.to_vec();
        args.extend_from_slice(&["--seed", seed]);
        assert_usage_error(
            &parley(&args),
            "error: --seed must be a whole number. Run parley --help.",
        );
    }
    let mut args = base.to_vec();
    args.extend_from_slice(&["--content", "tests/fixtures/nowhere"]);
    assert_usage_error(
        &parley(&args),
        "error: tests/fixtures/nowhere/es/syllabus.yaml not found",
    );
    let mut args = base.to_vec();
    args.push("--bogus");
    assert_usage_error(
        &parley(&args),
        "error: unknown argument --bogus. Run parley --help.",
    );
}

// Interfaces: a skill the syllabus does not have exits 1 with `error: unknown skill {id}`.
#[test]
fn t19_lesson_template_unknown_skill() {
    assert_failure(
        &parley(&[
            "lesson",
            "template",
            "--lang",
            "es",
            "--skill",
            "es.a1.u09.s09",
        ]),
        "error: unknown skill es.a1.u09.s09",
    );
}

// Interfaces: a syllabus that does not load exits 1 and prints the load errors as `content
// validate --json` prints them.
#[test]
fn lesson_template_reports_a_syllabus_that_does_not_load() {
    let dir = tempfile::tempdir().unwrap();
    let es = dir.path().join("c").join("es");
    fs::create_dir_all(&es).unwrap();
    let valid = fs::read_to_string(repo_root().join(COURSE)).unwrap();
    assert!(valid.contains("\nversion: 1\n"));
    fs::write(
        es.join("syllabus.yaml"),
        valid.replace("\nversion: 1\n", "\nversion: one\n"),
    )
    .unwrap();
    let output = parley_in(
        &[
            "lesson",
            "template",
            "--lang",
            "es",
            "--skill",
            "es.a1.u01.s02",
            "--content",
            "c",
        ],
        dir.path(),
    );
    assert_exit(&output, 1);
    assert_eq!(
        stdout_json(&output),
        json!({"ok": false, "errors": ["c/es/syllabus.yaml: /version: expected an integer"],
               "warnings": []})
    );
}

// ---------------------------------------------------------------------------------------------
// parley streak simulate
// ---------------------------------------------------------------------------------------------

// Bullet 19: the usage errors of `parley streak simulate`.
#[test]
fn t19_streak_simulate_usage_errors() {
    assert_usage_error(
        &parley(&["streak", "simulate"]),
        "error: <days.json> is required. Run parley --help.",
    );
    assert_usage_error(
        &parley(&["streak", "simulate", "--json"]),
        "error: <days.json> is required. Run parley --help.",
    );
    assert_usage_error(
        &parley(&["streak", "simulate", "tests/fixtures/nowhere.json"]),
        "error: tests/fixtures/nowhere.json not found",
    );
    assert_usage_error(
        &parley(&["streak", "simulate", STREAK_CASE, "--bogus"]),
        "error: unknown argument --bogus. Run parley --help.",
    );
}

// Interfaces: a file that is not a days file exits 1 with `error: {path}: {parser message}` on
// one line.
#[test]
fn streak_simulate_refuses_a_file_that_is_not_a_days_file() {
    let dir = tempfile::tempdir().unwrap();
    for (name, contents) in [
        ("not-json.json", "this is not JSON\n"),
        ("no-today.json", "{\"sessions\": []}\n"),
        (
            "not-a-date.json",
            "{\"today\": \"2026-02-30\", \"sessions\": []}\n",
        ),
        (
            "sessions-not-a-list.json",
            "{\"today\": \"2026-01-05\", \"sessions\": 3}\n",
        ),
        (
            "negative-xp.json",
            "{\"today\": \"2026-01-05\", \"sessions\": [{\"started_at\": 0, \"xp\": -5}]}\n",
        ),
    ] {
        fs::write(dir.path().join(name), contents).unwrap();
        let output = parley_in(&["streak", "simulate", name], dir.path());
        assert_exit(&output, 1);
        let stderr = text(&output.stderr);
        let prefix = format!("error: {name}: ");
        assert!(stderr.starts_with(&prefix), "{name}: {stderr:?}");
        assert!(
            stderr.len() > prefix.len() + 1,
            "{name}: the parser message follows"
        );
        assert_eq!(stderr.lines().count(), 1, "{name}: one line: {stderr:?}");
        assert!(output.stdout.is_empty(), "{name}: {}", text(&output.stdout));
    }
}

// Interfaces: `--json` is accepted and changes nothing; utc_offset_minutes and goal_xp are
// optional.
#[test]
fn streak_simulate_json_flag_and_defaults() {
    let plain = parley(&["streak", "simulate", STREAK_CASE]);
    let json = parley(&["streak", "simulate", STREAK_CASE, "--json"]);
    assert_exit(&plain, 0);
    assert_exit(&json, 0);
    assert_eq!(plain.stdout, json.stdout);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("days.json");
    // 2026-01-05T12:00:00Z, a Monday, with every optional field given.
    fs::write(
        &path,
        "{\"today\": \"2026-01-05\", \"utc_offset_minutes\": 0, \"goal_xp\": 30, \"sessions\": [{\"started_at\": 1767614400000, \"xp\": 12}]}\n",
    )
    .unwrap();
    let output = parley(&["streak", "simulate", &path.to_string_lossy()]);
    assert_exit(&output, 0);
    assert_eq!(
        stdout_json(&output),
        json!({"length": 1, "freezes": 0, "active_today": true, "frozen": [], "events": [],
               "days": [{"local_date": "2026-01-05", "xp": 12, "sessions": 1}], "records": []})
    );
}
