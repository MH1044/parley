//! Acceptance tests of module 04, the app shell and offline course
//! (docs/modules/04-app-shell-and-offline-course.md).
//!
//! They drive `parley_core::app::Parley` the way the desktop commands do: the course of
//! tests/fixtures/seed-course/ (the eight-skill course the app bundled when module 04 was
//! written, frozen there so the hand-worked numbers below keep their reference), the bundled pack
//! lock, a temporary data directory read and written through `DirFs`, a clock the test sets, seeded
//! SplitMix64 randomness and a network that fails every call unless a test says otherwise. No
//! test reaches the network: the pack tests answer from bytes built here from the committed
//! tatoeba-mini fixture. Each test names the acceptance test it covers. Expected values come from
//! the spec, the fixtures in tests/cases/04-app-shell-and-offline-course/ (see its README) and the
//! published rules of modules 02 and 03.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use parley_core::api::{
    COMMANDS, LessonStarted, PackProgress, PackState, language_name, megabytes, next_review_notice,
};
use parley_core::app::{Config, CourseFiles, Env, PackSentence, Parley, attributions};
use parley_core::content::items::{lemma_item_id, vocab_target};
use parley_core::content::{
    Lang, PackInfo, PackInput, Syllabus, build_pack, pack_info, parse_packs_lock, parse_syllabus,
};
use parley_core::env::{
    Clock, FailingNet, HttpRequest, HttpResponse, LocalDate, Net, NetError, SplitMix64,
};
use parley_core::error::AppError;
use parley_core::exercise::{Body, Exercise, Lesson, Origin};
use parley_core::game::{SessionKind, SessionResult, WEEKLY_RECORD, xp_for};
use parley_core::grading::{Answer, Verdict};
use parley_core::settings::{SETTINGS_NOTICE, SETTINGS_SCHEMA};
use parley_core::srs::{Facet, Placement, Rating, SkillRef, SkillState};
use serde::Serialize;
use serde_json::{Value, json};

const DAY_MS: i64 = 86_400_000;
const CASES: &str = "tests/cases/04-app-shell-and-offline-course";
/// The eight-skill course module 04's hand-worked expectations refer to (see `seed_courses`).
const SEED_COURSE: &str = "tests/fixtures/seed-course";
const PACK_URL: &str = "https://packs.example.invalid/es-2026-10-01.sqlite3";
const NOT_STARTED: &str = "This course has not been started.";
const NO_SKILL: &str = "This skill is not in the course.";
const GONE: &str = "This exercise is no longer available.";
const NOT_A_BACKUP: &str = "This is not a Parley backup.";
const CANNOT_WRITE: &str = "Parley could not write that file.";

/// The command names in the order of the spec's command table.
const SPEC_COMMANDS: [&str; 23] = [
    "app_start",
    "app_info",
    "file_pick",
    "data_delete_all",
    "cache_clear",
    "settings_get",
    "settings_set",
    "course_list",
    "course_start",
    "pack_status",
    "pack_download",
    "path_get",
    "skill_get",
    "placement_start",
    "placement_answer",
    "lesson_start",
    "review_start",
    "exercise_submit",
    "exercise_i_was_right",
    "lesson_finish",
    "stats_get",
    "backup_export",
    "backup_import",
];

// ---------------------------------------------------------------------------------------------
// The environment: a settable clock and a network fake.
// ---------------------------------------------------------------------------------------------

/// A clock the test sets; every copy shares the same time.
#[derive(Clone)]
struct TestClock {
    now: Arc<AtomicI64>,
    offset: i32,
}

impl TestClock {
    fn at(utc: &str, offset: i32) -> TestClock {
        TestClock {
            now: Arc::new(AtomicI64::new(utc_ms(utc))),
            offset,
        }
    }

    fn set(&self, utc: &str) {
        self.now.store(utc_ms(utc), Ordering::SeqCst);
    }

    fn advance(&self, ms: i64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }

    fn ms(&self) -> i64 {
        self.now.load(Ordering::SeqCst)
    }
}

impl Clock for TestClock {
    fn now_ms(&self) -> i64 {
        self.ms()
    }

    fn utc_offset_minutes(&self) -> i32 {
        self.offset
    }
}

/// A network that answers from a table of URLs (404 for any other) or fails every call, and
/// records every URL it is asked for.
#[derive(Clone, Default)]
struct FakeNet {
    routes: BTreeMap<String, (u16, Vec<u8>)>,
    failure: Option<NetError>,
    requests: Arc<Mutex<Vec<String>>>,
}

impl FakeNet {
    fn serving(url: &str, status: u16, body: Vec<u8>) -> FakeNet {
        let mut net = FakeNet::default();
        net.routes.insert(url.to_string(), (status, body));
        net
    }

    fn failing(error: NetError) -> FakeNet {
        FakeNet {
            failure: Some(error),
            ..FakeNet::default()
        }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("the request log").clone()
    }

    fn boxed(&self) -> Box<dyn Net + Send + Sync> {
        Box::new(self.clone())
    }
}

impl Net for FakeNet {
    fn send(&self, req: HttpRequest) -> Result<HttpResponse, NetError> {
        self.requests
            .lock()
            .expect("the request log")
            .push(req.url.clone());
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let (status, body) = self
            .routes
            .get(&req.url)
            .cloned()
            .unwrap_or((404, Vec::new()));
        Ok(HttpResponse {
            status,
            headers: Vec::new(),
            body,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------------------------

fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("a temporary directory")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("app/src-tauri sits two folders below the repository root")
        .to_path_buf()
}

fn case_file(name: &str) -> PathBuf {
    repo_root().join(CASES).join(name)
}

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
}

fn show<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn to_json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("a command result serialises")
}

/// Milliseconds since the epoch of `YYYY-MM-DDTHH:MM:SSZ`.
fn utc_ms(text: &str) -> i64 {
    let date = LocalDate::parse(&text[..10]).expect("a date");
    let number = |from: usize| -> i64 { text[from..from + 2].parse().expect("two digits") };
    date.days() * DAY_MS + number(11) * 3_600_000 + number(14) * 60_000 + number(17) * 1_000
}

/// `YYYY-MM-DDTHH:MM:SSZ` of milliseconds since the epoch.
fn utc_text(ms: i64) -> String {
    let date = LocalDate::from_days(ms.div_euclid(DAY_MS));
    let seconds = ms.rem_euclid(DAY_MS) / 1_000;
    format!(
        "{date}T{:02}:{:02}:{:02}Z",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    )
}

fn open(
    data_dir: &Path,
    clock: &TestClock,
    lock: &str,
    net: Box<dyn Net + Send + Sync>,
    seed: u64,
) -> Parley {
    Parley::new(
        Config {
            data_dir: data_dir.to_path_buf(),
            courses: seed_courses(),
            packs_lock: lock.to_string(),
        },
        Env {
            clock: Box::new(clock.clone()),
            rng: Box::new(SplitMix64::new(seed)),
            fs: Box::new(parley_app::env::DirFs::new(data_dir.to_path_buf())),
            net,
        },
    )
}

/// The app as the desktop gives it before module 06: the bundled lock and `FailingNet`.
fn open_offline(data_dir: &Path, clock: &TestClock) -> Parley {
    open(
        data_dir,
        clock,
        parley_app::bundled::PACKS_LOCK,
        Box::new(FailingNet),
        7,
    )
}

#[track_caller]
fn ok<T>(result: Result<T, AppError>) -> T {
    match result {
        Ok(value) => value,
        Err(e) => panic!("unexpected {}: {} ({:?})", e.code, e.message, e.action),
    }
}

#[track_caller]
fn refused<T>(result: Result<T, AppError>, code: &str, message: &str) -> AppError {
    match result {
        Ok(_) => panic!("expected {code} \"{message}\", but the command succeeded"),
        Err(e) => {
            assert_eq!((e.code.as_str(), e.message.as_str()), (code, message));
            e
        }
    }
}

#[track_caller]
fn assert_session_id(id: &str, prefix: &str) {
    let hex = id
        .strip_prefix(prefix)
        .unwrap_or_else(|| panic!("{id} does not start with {prefix}"));
    assert_eq!(hex.len(), 16, "{id} has 16 hex digits after {prefix}");
    assert!(
        hex.chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
        "{id} has lower-case hex digits after {prefix}"
    );
}

/// Compares JSON values: same object fields, same array lengths, integers, strings, booleans
/// and nulls exactly, numbers that are floats on either side within `tolerance`.
#[track_caller]
fn assert_json_close(actual: &Value, expected: &Value, tolerance: f64, path: &str) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(e)) if a.is_f64() || e.is_f64() => {
            let (a, e) = (
                a.as_f64().unwrap_or(f64::NAN),
                e.as_f64().unwrap_or(f64::NAN),
            );
            assert!(
                (a - e).abs() <= tolerance,
                "{path}: {a} is not within {tolerance} of {e}"
            );
        }
        (Value::Array(a), Value::Array(e)) => {
            assert_eq!(a.len(), e.len(), "{path}: number of entries");
            for (i, (x, y)) in a.iter().zip(e).enumerate() {
                assert_json_close(x, y, tolerance, &format!("{path}[{i}]"));
            }
        }
        (Value::Object(a), Value::Object(e)) => {
            let fields = |o: &serde_json::Map<String, Value>| -> BTreeSet<String> {
                o.keys().cloned().collect()
            };
            assert_eq!(fields(a), fields(e), "{path}: fields");
            for (key, y) in e {
                assert_json_close(&a[key], y, tolerance, &format!("{path}.{key}"));
            }
        }
        _ => assert_eq!(actual, expected, "{path}"),
    }
}

/// The course these tests run on: the eight-skill seed course of tests/fixtures/seed-course/,
/// in the shape `parley_app::bundled::courses()` gives the app. The bundled course has since
/// grown to full A1 and A2 syllabi; the placement staircase, the stats and the item counts below
/// were worked out by hand over the seed course, so it stays their reference.
fn seed_courses() -> BTreeMap<Lang, CourseFiles> {
    let dir = repo_root().join(SEED_COURSE);
    let read = |lang: Lang, file: &str| {
        let path = dir.join(lang.as_str()).join(file);
        fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()))
    };
    [Lang::Es, Lang::Fr, Lang::De]
        .into_iter()
        .map(|lang| {
            let files = CourseFiles {
                syllabus_yaml: read(lang, "syllabus.yaml"),
                forms_tsv: read(lang, "forms.tsv"),
                numbers_yaml: Some(read(lang, "numbers.yaml")),
            };
            (lang, files)
        })
        .collect()
}

fn es_syllabus() -> Syllabus {
    let courses = seed_courses();
    let files = courses.get(&Lang::Es).expect("the seed course has Spanish");
    parse_syllabus("seed-course/es/syllabus.yaml", &files.syllabus_yaml).unwrap_or_else(|errors| {
        let lines: Vec<String> = errors.into_iter().map(|e| e.0).collect();
        panic!("the seed Spanish syllabus loads: {}", lines.join("; "))
    })
}

/// The skills of a syllabus as module 02's placement takes them.
fn skill_refs(syllabus: &Syllabus) -> Vec<SkillRef> {
    syllabus
        .skills()
        .map(|(_, skill)| SkillRef {
            id: skill.id.clone(),
            cefr: skill.cefr,
        })
        .collect()
}

/// The vocabulary items of a skill: each one's id, target and English answers.
fn vocabulary(syllabus: &Syllabus, skill_id: &str) -> Vec<(String, String, Vec<String>)> {
    let (_, skill) = syllabus
        .skills()
        .find(|(_, skill)| skill.id == skill_id)
        .unwrap_or_else(|| panic!("{skill_id} is a syllabus skill"));
    assert!(!skill.lemmas.is_empty(), "every seed skill has lemmas");
    skill
        .lemmas
        .iter()
        .map(|lemma| {
            (
                lemma_item_id(&skill.id, &lemma.lemma),
                vocab_target(syllabus.lang, lemma),
                lemma.en.clone(),
            )
        })
        .collect()
}

/// The answer module 03 grades `correct` for an exercise, read from its body.
fn right_answer(ex: &Exercise) -> Answer {
    match &ex.body {
        Body::PickMeaning { answer_index, .. } => Answer::Choice(*answer_index),
        Body::TranslateToTarget {
            accepted, target, ..
        } => Answer::Text(accepted.first().unwrap_or(target).clone()),
        Body::TranslateToEn { accepted, .. } | Body::ListenType { accepted, .. } => {
            Answer::Text(accepted.first().cloned().unwrap_or_default())
        }
        Body::Speak { text, .. } => Answer::Text(text.clone()),
        Body::FillGap { answer, .. } => Answer::Text(answer.clone()),
        Body::MatchPairs { pairs } => {
            Answer::Pairs((0..pairs.len() as u32).map(|i| [i, i]).collect())
        }
    }
}

/// An answer module 03 grades `wrong` (not `unchecked`): only for the types that have one.
fn wrong_answer(ex: &Exercise) -> Option<Answer> {
    match &ex.body {
        Body::PickMeaning {
            answer_index,
            options,
            ..
        } => Some(Answer::Choice((answer_index + 1) % options.len() as u32)),
        Body::FillGap { .. } => Some(Answer::Text("xqxqxq".to_string())),
        Body::MatchPairs { pairs } => {
            let mut matched: Vec<[u32; 2]> = (0..pairs.len() as u32).map(|i| [i, i]).collect();
            matched[0] = [0, 1];
            matched[1] = [1, 0];
            Some(Answer::Pairs(matched))
        }
        _ => None,
    }
}

/// Answers every exercise of a run, right except the first one that can be answered wrong when
/// `one_wrong` is set; returns the verdicts in order.
fn answer_all(app: &mut Parley, run: &LessonStarted, one_wrong: bool) -> Vec<Verdict> {
    let wrong_id = if one_wrong {
        let ex = run
            .exercises
            .iter()
            .find(|ex| wrong_answer(ex).is_some())
            .expect("the run has an exercise that can be answered wrong");
        Some(ex.id.clone())
    } else {
        None
    };
    let mut verdicts = Vec::new();
    for ex in &run.exercises {
        let (answer, expected) = if wrong_id.as_deref() == Some(ex.id.as_str()) {
            (wrong_answer(ex).expect("a wrong answer"), Verdict::Wrong)
        } else {
            (right_answer(ex), Verdict::Correct)
        };
        let grade = ok(app.exercise_submit(&run.session_id, &ex.id, &answer, Some(4_000), false));
        assert_eq!(grade.verdict, expected, "{}", show(ex));
        verdicts.push(grade.verdict);
    }
    verdicts
}

fn expected_xp(kind: SessionKind, verdicts: &[Verdict]) -> u32 {
    xp_for(&SessionResult {
        kind,
        verdicts: verdicts.to_vec(),
        finished: true,
    })
}

#[track_caller]
fn assert_run_shape(run: &LessonStarted, kind: &str) {
    assert_session_id(&run.session_id, "s-");
    assert_eq!(run.kind, kind);
    assert_eq!(run.origin, Origin::Template);
    assert!(
        !run.exercises.is_empty(),
        "the {kind} has exercises: {}",
        show(run)
    );
    let ids: BTreeSet<&str> = run.exercises.iter().map(|ex| ex.id.as_str()).collect();
    assert_eq!(ids.len(), run.exercises.len(), "exercise ids are distinct");
    assert!(
        run.attributions.is_empty(),
        "no pack, no attribution: {}",
        show(&run.attributions)
    );
}

/// The file names in a folder; none when the folder does not exist.
fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

/// Every file under a folder, at any depth.
fn files_below(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                found.extend(files_below(&path));
            } else {
                found.push(path);
            }
        }
    }
    found
}

/// A course pack built from the committed tatoeba-mini fixture (its README lists every sentence
/// and its fate), and the facts a lock pins about it.
struct FixturePack {
    bytes: Vec<u8>,
    info: PackInfo,
}

fn fixture_pack(scratch: &Path) -> FixturePack {
    let root = repo_root();
    let out = scratch.join("es-fixture.sqlite3");
    build_pack(
        &PackInput {
            lang: Lang::Es,
            tatoeba_dir: root.join("tests/fixtures/tatoeba-mini"),
            wiktextract: root.join("tests/fixtures/forms-mini.jsonl"),
            syllabus: root.join("tests/fixtures/syllabus/valid-es.yaml"),
            exclude: None,
            built_at: "2026-10-01".to_string(),
        },
        &out,
    )
    .expect("the tatoeba-mini pack builds");
    let info = pack_info(&out).expect("the pack describes itself");
    let bytes = fs::read(&out).expect("the pack's bytes");
    assert_eq!(bytes.len() as u64, info.bytes);
    FixturePack { bytes, info }
}

fn pack_lock(lang: &str, version: &str, url: &str, info: &PackInfo) -> String {
    format!(
        "schema = \"parley.packs/1\"\n\n[{lang}]\nversion = \"{version}\"\nurl = \"{url}\"\nsha256 = \"{}\"\nbytes = {}\ncontent_sha256 = \"{}\"\n",
        info.sha256, info.bytes, info.content_sha256
    )
}

/// `megabytes` as the spec defines it: (bytes + 50,000) / 100,000 tenths, in integers.
fn tenths_of_a_megabyte(bytes: u64) -> String {
    let tenths = (bytes + 50_000) / 100_000;
    format!("{}.{} MB", tenths / 10, tenths % 10)
}

/// `settings.json` of docs/ARCHITECTURE.md §4 with `active_course` null: the defaults.
fn default_settings() -> Value {
    json!({
        "schema": "parley.settings/1", "active_course": null, "daily_goal_xp": 20,
        "accent_leniency": "lenient", "answer_input": "typing", "desired_retention": 0.9,
        "new_items_per_lesson": 5, "lesson_length": 12,
        "audio": { "voice_source": "os", "os_voice": { "es": null, "fr": null, "de": null },
                   "natural_voice": { "es": null, "fr": null, "de": null }, "slow_speed": 0.75,
                   "autoplay": true },
        "speaking": { "enabled": true, "transcriber": "provider", "pass_threshold": 0.8,
                      "keep_recordings": false },
        "ai": { "provider": "unset", "fresh_lessons": true, "prepare_next_lesson": true,
                "stories": true, "share_device_name": false,
                "own_key": { "base_url": null, "chat_model": null, "transcribe_model": null,
                             "speech_model": null, "speech_voice": null },
                "local": { "base_url": "http://localhost:11434/v1", "chat_model": null } },
        "update_check": true
    })
}

/// A backup file read back, without the fields that may differ between two exports of the
/// same data.
fn backup_without(path: &Path, fields: &[&str]) -> Value {
    let mut value = read_json(path);
    let object = value.as_object_mut().expect("a backup is an object");
    for field in fields {
        object.remove(*field);
    }
    value
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 2: the path of a started course; unknown language, course not started,
// unknown skill.
// ---------------------------------------------------------------------------------------------

#[test]
fn t02_course_start_and_path_follow_the_syllabus() {
    let dir = tempdir();
    let clock = TestClock::at("2026-10-12T09:00:00Z", 0);
    let mut app = open_offline(dir.path(), &clock);
    ok(app.app_start());
    let syllabus = es_syllabus();

    let summary = ok(app.course_start("es"));
    assert_eq!(summary.lang, "es");
    assert_eq!(summary.name, "Spanish");
    assert!(summary.started);
    assert!(!summary.placement_done);
    assert_eq!(summary.cefr_estimate, None);
    assert_eq!(summary.skills, syllabus.skills().count());

    let path = ok(app.path_get("es"));
    assert_eq!(path.lang, "es");
    assert_eq!(path.units.len(), syllabus.units.len());
    for (i, (unit, view)) in syllabus.units.iter().zip(&path.units).enumerate() {
        assert_eq!(view.id, unit.id);
        assert_eq!(view.n, i + 1);
        assert_eq!(view.title, unit.title);
        assert_eq!(view.cefr, unit.cefr);
        assert_eq!(
            view.skills.len(),
            unit.skills.len(),
            "skills of {}",
            unit.id
        );
        for (skill, shown) in unit.skills.iter().zip(&view.skills) {
            assert_eq!(shown.id, skill.id);
            assert_eq!(shown.title, skill.title);
            assert_eq!(shown.can_do, skill.can_do);
            assert_eq!(shown.cefr, skill.cefr);
            assert!(shown.mastery.abs() < 1e-12, "{}", skill.id);
            assert!(!shown.tested_out, "{}", skill.id);
        }
    }
    let states: Vec<SkillState> = path
        .units
        .iter()
        .flat_map(|unit| unit.skills.iter().map(|skill| skill.state))
        .collect();
    assert_eq!(states[0], SkillState::Available, "skill 1 is available");
    assert!(
        states[1..].iter().all(|state| *state == SkillState::Locked),
        "every later skill is locked: {states:?}"
    );
    let first = &syllabus.units[0].skills[0];
    assert_eq!(path.next_skill_id.as_deref(), Some(first.id.as_str()));
    assert_eq!(
        (
            path.due,
            path.streak,
            path.freezes,
            path.today_xp,
            path.goal_xp
        ),
        (0, 0, 0, 0, 20)
    );

    // Skill details: the first skill is open; the second names the skill that opens it.
    let detail = ok(app.skill_get(&first.id));
    assert_eq!(detail.id, first.id);
    assert_eq!(detail.title, first.title);
    assert_eq!(detail.can_do, first.can_do);
    assert_eq!(detail.cefr, first.cefr);
    assert_eq!(detail.grammar_note, first.grammar_note);
    assert_eq!(detail.unit_title, syllabus.units[0].title);
    assert_eq!(detail.state, SkillState::Available);
    assert!(detail.mastery.abs() < 1e-12);
    assert_eq!(detail.words.len(), first.lemmas.len(), "one word per lemma");
    assert_eq!(detail.words[0].target, first.lemmas[0].lemma);
    assert!(
        detail.words[0].en.starts_with(&first.lemmas[0].en[0]),
        "{}",
        detail.words[0].en
    );
    assert_eq!(detail.opened_by, None);
    let second = &syllabus.units[0].skills[1];
    let locked = ok(app.skill_get(&second.id));
    assert_eq!(locked.state, SkillState::Locked);
    assert!(
        matches!(locked.opened_by.as_deref(), Some(name) if name == first.title || name == first.id),
        "the locked skill names {} as the one that opens it: {:?}",
        first.id,
        locked.opened_by
    );

    refused(
        app.course_start("it"),
        "invalid_input",
        "Unknown value it for lang.",
    );
    refused(app.path_get("fr"), "not_found", NOT_STARTED);
    refused(app.skill_get("es.a1.u09.s09"), "not_found", NO_SKILL);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 3: pack download, progress, checksum, offline, 404, a language not in the
// lock, and `megabytes`.
// ---------------------------------------------------------------------------------------------

#[test]
fn t03_pack_download_installs_a_pack_whose_checksum_matches() {
    let scratch = tempdir();
    let pack = fixture_pack(scratch.path());
    let lock = pack_lock("es", "2026-10-01", PACK_URL, &pack.info);
    let dir = tempdir();
    let clock = TestClock::at("2026-10-12T09:00:00Z", 0);
    let net = FakeNet::serving(PACK_URL, 200, pack.bytes.clone());
    let mut app = open(dir.path(), &clock, &lock, net.boxed(), 3);
    ok(app.app_start());
    ok(app.course_start("es"));

    let before = ok(app.pack_status("es"));
    assert_eq!(before.lang, "es");
    assert!(
        matches!(before.state, PackState::Available),
        "{}",
        show(&before)
    );
    assert_eq!(before.version.as_deref(), Some("2026-10-01"));
    assert_eq!(before.bytes, Some(pack.info.bytes));
    assert_eq!(
        before.size.as_deref(),
        Some(tenths_of_a_megabyte(pack.info.bytes).as_str())
    );

    let mut events: Vec<(String, u64, u64)> = Vec::new();
    let after = ok(app.pack_download("es", &mut |p: PackProgress| {
        events.push((p.lang.clone(), p.bytes_done, p.bytes_total))
    }));
    let total = pack.info.bytes;
    assert!(
        matches!(after.state, PackState::Installed),
        "{}",
        show(&after)
    );
    assert_eq!(after.version.as_deref(), Some("2026-10-01"));
    assert_eq!(net.requests(), vec![PACK_URL.to_string()], "one request");
    assert_eq!(
        events.first(),
        Some(&("es".to_string(), 0, total)),
        "0 first"
    );
    assert_eq!(
        events.last(),
        Some(&("es".to_string(), total, total)),
        "the total last"
    );
    assert!(events.iter().all(|(_, _, t)| *t == total));
    assert!(events.windows(2).all(|w| w[0].1 <= w[1].1), "{events:?}");
    let packs = dir.path().join("packs");
    assert_eq!(files_in(&packs), vec!["es-2026-10-01.sqlite3".to_string()]);
    assert_eq!(
        fs::read(packs.join("es-2026-10-01.sqlite3")).expect("the installed pack"),
        pack.bytes
    );
    assert!(matches!(
        ok(app.pack_status("es")).state,
        PackState::Installed
    ));
    drop(app);

    // A newer version replaces the course's previous pack file.
    let newer_url = "https://packs.example.invalid/es-2026-11-01.sqlite3";
    let newer_lock = pack_lock("es", "2026-11-01", newer_url, &pack.info);
    let newer_net = FakeNet::serving(newer_url, 200, pack.bytes.clone());
    let mut app = open(dir.path(), &clock, &newer_lock, newer_net.boxed(), 3);
    ok(app.app_start());
    let replaced = ok(app.pack_download("es", &mut |_| {}));
    assert!(matches!(replaced.state, PackState::Installed));
    assert_eq!(replaced.version.as_deref(), Some("2026-11-01"));
    assert_eq!(files_in(&packs), vec!["es-2026-11-01.sqlite3".to_string()]);
    drop(app);

    // Installed means the recorded file opens as a pack: without the file, it is available again.
    fs::remove_file(packs.join("es-2026-11-01.sqlite3")).expect("the pack file is removed");
    let mut app = open(
        dir.path(),
        &clock,
        &newer_lock,
        FakeNet::default().boxed(),
        3,
    );
    ok(app.app_start());
    assert!(matches!(
        ok(app.pack_status("es")).state,
        PackState::Available
    ));
}

#[test]
fn t03_a_damaged_download_leaves_nothing_in_packs() {
    let scratch = tempdir();
    let pack = fixture_pack(scratch.path());
    let lock = pack_lock("es", "2026-10-01", PACK_URL, &pack.info);
    let mut damaged = pack.bytes.clone();
    let last = damaged.len() - 1;
    damaged[last] ^= 0x01;
    let dir = tempdir();
    let clock = TestClock::at("2026-10-12T09:00:00Z", 0);
    let net = FakeNet::serving(PACK_URL, 200, damaged);
    let mut app = open(dir.path(), &clock, &lock, net.boxed(), 3);
    ok(app.app_start());
    ok(app.course_start("es"));
    refused(
        app.pack_download("es", &mut |_| {}),
        "checksum_mismatch",
        "The download was damaged. Try again.",
    );
    assert_eq!(files_in(&dir.path().join("packs")), Vec::<String>::new());
    assert!(matches!(
        ok(app.pack_status("es")).state,
        PackState::Available
    ));
}

#[test]
fn t03_no_response_is_offline_and_another_status_is_a_missing_pack() {
    let scratch = tempdir();
    let pack = fixture_pack(scratch.path());
    let lock = pack_lock("es", "2026-10-01", PACK_URL, &pack.info);
    let clock = TestClock::at("2026-10-12T09:00:00Z", 0);
    let offline = "You're offline. Parley works without the pack, with fewer example sentences.";
    let unavailable = "The course pack could not be downloaded. Try again later.";

    let cases: Vec<(Box<dyn Net + Send + Sync>, &str, &str)> = vec![
        (
            Box::new(FailingNet) as Box<dyn Net + Send + Sync>,
            "offline",
            offline,
        ),
        (
            FakeNet::failing(NetError::Offline).boxed(),
            "offline",
            offline,
        ),
        (
            FakeNet::failing(NetError::Timeout).boxed(),
            "offline",
            offline,
        ),
        (
            FakeNet::serving(PACK_URL, 500, pack.bytes.clone()).boxed(),
            "pack_missing",
            unavailable,
        ),
        (
            FakeNet::serving(PACK_URL, 503, b"unavailable".to_vec()).boxed(),
            "pack_missing",
            unavailable,
        ),
    ];
    for (net, code, message) in cases {
        let dir = tempdir();
        let mut app = open(dir.path(), &clock, &lock, net, 3);
        ok(app.app_start());
        ok(app.course_start("es"));
        refused(app.pack_download("es", &mut |_| {}), code, message);
        assert_eq!(files_in(&dir.path().join("packs")), Vec::<String>::new());
        // The app works without the pack.
        let run = ok(app.lesson_start("es.a1.u01.s01", "learn"));
        assert_run_shape(&run, "lesson");
    }
}

#[test]
fn t03_a_pack_url_that_answers_404_or_410_is_not_published() {
    let scratch = tempdir();
    let pack = fixture_pack(scratch.path());
    let lock = pack_lock("es", "2026-10-01", PACK_URL, &pack.info);
    let clock = TestClock::at("2026-10-12T09:00:00Z", 0);
    for status in [404, 410] {
        let dir = tempdir();
        let net = FakeNet::serving(PACK_URL, status, b"not found".to_vec());
        let mut app = open(dir.path(), &clock, &lock, net.boxed(), 3);
        ok(app.app_start());
        ok(app.course_start("es"));
        let outcome = ok(app.pack_download("es", &mut |_| {}));
        assert_eq!(outcome.lang, "es");
        assert!(
            matches!(outcome.state, PackState::NotPublished),
            "{status}: {}",
            show(&outcome)
        );
        assert_eq!(
            (outcome.version, outcome.bytes, outcome.size),
            (None, None, None),
            "{status}"
        );
        assert_eq!(net.requests(), vec![PACK_URL.to_string()], "{status}");
        assert_eq!(files_in(&dir.path().join("packs")), Vec::<String>::new());
        assert!(!matches!(
            ok(app.pack_status("es")).state,
            PackState::Installed
        ));
        // The app works without the pack.
        let run = ok(app.lesson_start("es.a1.u01.s01", "learn"));
        assert_run_shape(&run, "lesson");
    }
}

#[test]
fn t03_a_pack_that_cannot_be_written_is_a_storage_error_and_installs_nothing() {
    let scratch = tempdir();
    let out = tempdir();
    let pack = fixture_pack(scratch.path());
    let lock = pack_lock("es", "2026-10-01", PACK_URL, &pack.info);
    let dir = tempdir();
    let clock = TestClock::at("2026-10-12T09:00:00Z", 0);
    let net = FakeNet::serving(PACK_URL, 200, pack.bytes.clone());
    let mut app = open(dir.path(), &clock, &lock, net.boxed(), 3);
    ok(app.app_start());
    ok(app.course_start("es"));
    // A folder that is not empty where the pack file should be: the right bytes arrive and the
    // `.part` file is written, but it cannot be renamed into place.
    fs::create_dir_all(dir.path().join("packs/es-2026-10-01.sqlite3/x"))
        .expect("a folder in the pack's place");

    refused(
        app.pack_download("es", &mut |_| {}),
        "storage",
        CANNOT_WRITE,
    );
    let parts: Vec<PathBuf> = files_below(dir.path())
        .into_iter()
        .filter(|path| path.to_string_lossy().ends_with(".part"))
        .collect();
    assert!(parts.is_empty(), "no partial pack is left: {parts:?}");
    let status = ok(app.pack_status("es"));
    assert!(
        matches!(status.state, PackState::Available),
        "{}",
        show(&status)
    );
    // No pack version is recorded for the course.
    let backup = out.path().join("after.json");
    ok(app.backup_export(&backup));
    let courses = read_json(&backup)["courses"].clone();
    let es = courses
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["lang"] == "es"))
        .expect("the Spanish course");
    assert_eq!(es["pack_version"], Value::Null);
}

#[test]
fn t03_a_language_missing_from_the_lock_is_not_published() {
    let scratch = tempdir();
    let pack = fixture_pack(scratch.path());
    // The lock has French only.
    let lock = pack_lock(
        "fr",
        "2026-10-01",
        "https://packs.example.invalid/fr-2026-10-01.sqlite3",
        &pack.info,
    );
    // The bundled lock publishes every language's pack since the course content was pinned,
    // so only the French-only lock leaves Spanish unpublished.
    {
        let lock = lock.as_str();
        let dir = tempdir();
        let clock = TestClock::at("2026-10-12T09:00:00Z", 0);
        let net = FakeNet::default();
        let mut app = open(dir.path(), &clock, lock, net.boxed(), 3);
        ok(app.app_start());
        ok(app.course_start("es"));
        let status = ok(app.pack_status("es"));
        assert_eq!(status.lang, "es");
        assert!(
            matches!(status.state, PackState::NotPublished),
            "{}",
            show(&status)
        );
        assert_eq!(
            (status.version, status.bytes, status.size),
            (None, None, None)
        );
        refused(
            app.pack_download("es", &mut |_| {}),
            "pack_missing",
            "No course pack is published for Spanish yet.",
        );
        assert!(net.requests().is_empty(), "nothing to download, no request");
    }
}

#[test]
fn t03_megabytes_and_language_names() {
    assert_eq!(megabytes(8_200_000), "8.2 MB");
    for bytes in [
        0,
        49_999,
        50_000,
        1_000_000,
        8_249_999,
        8_250_000,
        123_456_789,
    ] {
        assert_eq!(megabytes(bytes), tenths_of_a_megabyte(bytes), "{bytes}");
    }
    assert_eq!(megabytes(49_999), "0.0 MB");
    assert_eq!(megabytes(8_250_000), "8.3 MB");
    assert_eq!(megabytes(123_456_789), "123.5 MB");
    assert_eq!(language_name(Lang::Es), "Spanish");
    assert_eq!(language_name(Lang::Fr), "French");
    assert_eq!(language_name(Lang::De), "German");
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 4: the whole offline run (invariant I3).
// ---------------------------------------------------------------------------------------------

#[test]
fn t04_a_full_run_works_with_a_network_that_fails_every_call() {
    let dir = tempdir();
    // Monday 2026-10-12, ISO week 42.
    let clock = TestClock::at("2026-10-12T09:00:00Z", 0);
    let net = FakeNet::failing(NetError::Disabled);
    let mut app = open(
        dir.path(),
        &clock,
        parley_app::bundled::PACKS_LOCK,
        net.boxed(),
        4,
    );
    let syllabus = es_syllabus();
    let skills = skill_refs(&syllabus);

    let launch = ok(app.app_start());
    assert!(!launch.onboarded);
    assert_eq!(launch.active_course, None);
    assert_eq!(launch.notice, None);

    let courses = ok(app.course_list());
    let names: Vec<(&str, &str, bool)> = courses
        .iter()
        .map(|c| (c.lang.as_str(), c.name.as_str(), c.started))
        .collect();
    for expected in [
        ("es", "Spanish", false),
        ("fr", "French", false),
        ("de", "German", false),
    ] {
        assert!(names.contains(&expected), "{names:?}");
    }

    let es = ok(app.course_start("es"));
    assert!(es.started);
    // Onboarding calls only course_start and settings_set({"daily_goal_xp": n}), and a launch
    // with an onboarded learner opens the path of the active course: so the course started
    // becomes the active one.
    assert!(es.active);
    // The bundled lock publishes the Spanish pack: it is available to download, and asking for
    // its status makes no network call.
    let status = ok(app.pack_status("es"));
    assert!(
        matches!(status.state, PackState::Available),
        "{}",
        show(&status)
    );
    let locked = parse_packs_lock(parley_app::bundled::PACKS_LOCK)
        .expect("the bundled lock parses")
        .langs
        .remove("es")
        .expect("the bundled lock publishes Spanish");
    assert_eq!(status.version.as_deref(), Some(locked.version.as_str()));
    assert_eq!(status.bytes, Some(locked.bytes));
    let settings = ok(app.settings_set(&json!({"daily_goal_xp": 30})));
    assert_eq!(to_json(&settings)["daily_goal_xp"], json!(30));

    // Placement: a learner who knows exactly the first three skills, answered from each
    // question's own exercise; module 02's staircase is the reference. By hand, with N = 8:
    // questions at 2, 4, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2; reversals 4, 2, 3, 2, 3, 2, 3, 2, 3, 2;
    // estimate (2·20 + 8) / 16 = 3; label "around A1"; skills 0 to 2 tested out.
    // Every visit to a skill asks one of its vocabulary items not asked before; no skill is
    // visited more often than it has words (index 2 six times of 8, index 3 five of 14, index 4
    // once of 8), so every question translates into English.
    const KNOWN: usize = 3;
    let mut oracle = Placement::new(skills.clone());
    let mut question = ok(app.placement_start("es"));
    let placement_session = question.session_id.clone();
    assert_session_id(&placement_session, "p-");
    let mut asked = 0;
    let mut asked_items: Vec<String> = Vec::new();
    let outcome = loop {
        asked += 1;
        let expected = oracle
            .next_question()
            .expect("module 02's staircase asks another question");
        assert_eq!(question.session_id, placement_session);
        assert_eq!(question.n, asked);
        assert_eq!(question.skill_id, expected.id, "question {asked}");
        let ex = &question.exercise;
        let words = vocabulary(&syllabus, &question.skill_id);
        let (item_id, target, english) = words
            .iter()
            .find(|(id, _, _)| *id == ex.item_id)
            .unwrap_or_else(|| panic!("a vocabulary item of the skill: {}", show(ex)));
        assert!(
            !asked_items.contains(item_id),
            "question {asked} asks {item_id} again"
        );
        asked_items.push(item_id.clone());
        assert_eq!(ex.id, format!("p{asked}"));
        assert_eq!(ex.facet, Facet::Receptive);
        assert_eq!(ex.origin, Origin::Template);
        let Body::TranslateToEn {
            prompt_target,
            accepted,
        } = &ex.body
        else {
            panic!("a placement question is translate_to_en: {}", show(ex));
        };
        assert_eq!(prompt_target, target);
        assert_eq!(accepted, english);
        let index = skills
            .iter()
            .position(|skill| skill.id == question.skill_id)
            .expect("a course skill");
        let knows = index < KNOWN;
        // "I don't know" sends an empty text, which counts as wrong.
        let answer = Answer::Text(if knows {
            accepted[0].clone()
        } else {
            String::new()
        });
        oracle.answer(knows);
        let step = ok(app.placement_answer(&placement_session, &answer));
        match (step.next, step.result) {
            (Some(next), None) => question = next,
            (None, Some(result)) => break result,
            _ => panic!("a placement step carries exactly one of next and result"),
        }
    };
    let reference = oracle.result().expect("module 02's staircase has finished");
    assert_eq!(
        reference.questions,
        vec![2, 4, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2]
    );
    assert_eq!(asked, reference.questions.len());
    assert_eq!(outcome.estimate, 3);
    assert_eq!(outcome.label, "around A1");
    assert_eq!(outcome.tested_out, 3);
    assert_eq!(outcome.start_skill_id, "es.a1.u01.s04");
    assert_eq!(outcome.start_unit, 1);
    assert_eq!(outcome.start_unit_title, "Hello");
    let es = ok(app.course_list())
        .into_iter()
        .find(|c| c.lang == "es")
        .expect("Spanish is listed");
    assert!(es.placement_done);
    assert_eq!(es.cefr_estimate.as_deref(), Some("around A1"));
    let launch = ok(app.app_start());
    assert!(launch.onboarded);
    assert_eq!(launch.active_course.as_deref(), Some("es"));

    // Tested-out skills: every card reviewed Good just now, so retrievability 1 and mastery 1.
    let path = ok(app.path_get("es"));
    let shown: Vec<_> = path.units.iter().flat_map(|u| u.skills.iter()).collect();
    for skill in &shown[..3] {
        assert!(skill.tested_out, "{} is tested out", skill.id);
        assert_eq!(skill.state, SkillState::Mastered, "{}", skill.id);
        assert!((skill.mastery - 1.0).abs() < 1e-9, "{}", skill.mastery);
    }
    assert!(!shown[3].tested_out);
    assert_eq!(shown[3].state, SkillState::Available);
    assert_eq!(path.next_skill_id.as_deref(), Some("es.a1.u01.s04"));

    // A lesson: one wrong answer, the rest right.
    let lesson_day = LocalDate::parse("2026-10-12").expect("a date");
    let lesson = ok(app.lesson_start("es.a1.u01.s04", "learn"));
    assert_run_shape(&lesson, "lesson");
    assert_eq!(lesson.skill_id.as_deref(), Some("es.a1.u01.s04"));
    assert!(lesson.exercises.len() <= 12, "lesson_length 12");
    assert_eq!(lesson.notice, None);
    let lesson_verdicts = answer_all(&mut app, &lesson, true);
    let summary = ok(app.lesson_finish(&lesson.session_id));
    let lesson_xp = expected_xp(SessionKind::Lesson, &lesson_verdicts);
    assert_eq!(summary.xp, lesson_xp);
    assert_eq!(summary.correct as usize, lesson_verdicts.len() - 1);
    assert_eq!(summary.total as usize, lesson.exercises.len());
    assert_eq!((summary.streak, summary.freezes), (1, 0));
    assert!(!summary.record_week);
    assert_eq!((summary.daily_xp, summary.goal_xp), (lesson_xp, 30));
    let path = ok(app.path_get("es"));
    assert_eq!(
        (path.today_xp, path.goal_xp, path.streak, path.due),
        (lesson_xp, 30, 1, 0)
    );

    // Three days on, the cards of the placement and the lesson are due (every first interval is
    // at most w[2] = 2.3 days rounded); a review session takes up to 20.
    clock.advance(3 * DAY_MS);
    let review = ok(app.review_start("es", 20));
    assert_run_shape(&review, "review");
    assert!(review.exercises.len() <= 20);
    assert_eq!(review.notice, None);
    let review_verdicts = answer_all(&mut app, &review, false);
    let review_summary = ok(app.lesson_finish(&review.session_id));
    let review_xp = expected_xp(SessionKind::Review, &review_verdicts);
    assert_eq!(review_summary.xp, review_xp);
    // Two missed days and no freeze reset the streak; today counts 1.
    assert_eq!((review_summary.streak, review_summary.freezes), (1, 0));
    assert!(!review_summary.record_week);

    // Stats. Every answer was graded, so every one wrote a review; the placement's do not
    // count. Words known: retrievability ≥ 0.9 means t ≤ S; productive cards reviewed three days
    // ago have S ≤ 2.31, so only those reviewed in this session (t = 0) count.
    let stats = ok(app.stats_get("es"));
    let reviews = lesson_verdicts.len() + review_verdicts.len();
    let known: BTreeSet<&str> = review
        .exercises
        .iter()
        .filter(|ex| ex.facet == Facet::Productive && ex.item_id.contains("#l:"))
        .map(|ex| ex.item_id.as_str())
        .collect();
    let today = LocalDate::parse("2026-10-15").expect("a date");
    let days: Vec<Value> = (0..30)
        .rev()
        .map(|back| {
            let date = LocalDate::from_days(today.days() - back);
            let xp = if date == today {
                review_xp
            } else if date == lesson_day {
                lesson_xp
            } else {
                0
            };
            json!({"local_date": date.to_string(), "xp": xp, "active": xp > 0, "frozen": false})
        })
        .collect();
    assert_eq!(stats.lang, "es");
    assert!(!stats.empty);
    assert_eq!((stats.streak, stats.freezes), (1, 0));
    assert_json_close(&to_json(&stats.days), &Value::Array(days), 0.0, "days");
    assert_eq!(stats.week_xp, lesson_xp + review_xp, "same ISO week");
    assert_eq!(stats.record_week_xp, 0, "no earlier week");
    assert_eq!(stats.words_known as usize, known.len());
    assert_eq!(
        stats.vocab_items, 76,
        "the lemmas of the seed Spanish syllabus"
    );
    assert_eq!(stats.cefr_estimate.as_deref(), Some("around A1"));
    assert_eq!(stats.reviews_30 as usize, reviews);
    let rate = stats.recall_rate.expect("reviews in the last 30 days");
    assert!(
        (rate - (reviews - 1) as f64 / reviews as f64).abs() < 1e-9,
        "{rate}"
    );
    let ids: Vec<&str> = stats.skills.iter().map(|s| s.id.as_str()).collect();
    let expected_ids: Vec<&str> = skills.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, expected_ids);
    assert_eq!(stats.skills[3].title, "Numbers and age");
    assert_eq!(
        stats.skills[3].state,
        SkillState::Learning,
        "five of its items introduced"
    );
    assert_eq!(
        stats.skills[4].state,
        SkillState::Locked,
        "it requires the skill being learnt"
    );

    assert!(
        net.requests().is_empty(),
        "the offline run asked for nothing: {:?}",
        net.requests()
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 5: submit and finish by module 03's rules.
// ---------------------------------------------------------------------------------------------

#[test]
fn t05_submit_grades_and_finish_rewards_by_module_03() {
    let dir = tempdir();
    // Monday 2026-10-05, ISO week 41.
    let clock = TestClock::at("2026-10-05T09:00:00Z", 0);
    let mut app = open_offline(dir.path(), &clock);
    ok(app.app_start());
    ok(app.course_start("es"));

    let run = ok(app.lesson_start("es.a1.u01.s01", "learn"));
    assert_run_shape(&run, "lesson");
    assert_eq!(run.skill_id.as_deref(), Some("es.a1.u01.s01"));
    let session = run.session_id.clone();
    let pick = run
        .exercises
        .iter()
        .find(|ex| matches!(ex.body, Body::PickMeaning { .. }))
        .expect("a first lesson introduces its words with pick_meaning");

    refused(
        app.exercise_submit(&session, &pick.id, &Answer::Text("  ".into()), None, false),
        "invalid_input",
        "Type an answer first.",
    );
    refused(
        app.exercise_i_was_right(&session, &pick.id),
        "invalid_input",
        "Type an answer first.",
    );
    // A right choice in 1.5 s with no hint: correct, rated Easy (module 02's table).
    let grade =
        ok(app.exercise_submit(&session, &pick.id, &right_answer(pick), Some(1_500), false));
    assert_eq!(grade.verdict, Verdict::Correct);
    assert_eq!(grade.rating, Some(Rating::Easy));
    refused(
        app.exercise_submit(&session, &pick.id, &right_answer(pick), Some(1_500), false),
        "invalid_input",
        "This exercise was already checked.",
    );
    refused(
        app.exercise_i_was_right(&session, &pick.id),
        "invalid_input",
        "This exercise was already checked.",
    );
    refused(
        app.exercise_submit(
            "s-0000000000000000",
            &pick.id,
            &right_answer(pick),
            None,
            false,
        ),
        "not_found",
        GONE,
    );
    refused(
        app.exercise_submit(&session, "e99", &Answer::Choice(0), None, false),
        "not_found",
        GONE,
    );

    let mut verdicts = vec![grade.verdict];
    for ex in run.exercises.iter().filter(|ex| ex.id != pick.id) {
        // A right answer with a hint rates Hard; without one, in 4 s, Good.
        let hint = matches!(ex.body, Body::FillGap { .. });
        let grade = ok(app.exercise_submit(&session, &ex.id, &right_answer(ex), Some(4_000), hint));
        assert_eq!(grade.verdict, Verdict::Correct, "{}", show(ex));
        let rating = if hint { Rating::Hard } else { Rating::Good };
        assert_eq!(grade.rating, Some(rating), "{}", show(ex));
        verdicts.push(grade.verdict);
    }
    let first = ok(app.lesson_finish(&session));
    assert_eq!(first.xp, expected_xp(SessionKind::Lesson, &verdicts));
    assert_eq!(first.correct as usize, verdicts.len());
    assert_eq!(first.total as usize, run.exercises.len());
    assert_eq!((first.streak, first.freezes), (1, 0));
    assert!(!first.record_week, "no earlier week");
    assert_eq!(first.message, None);
    assert_eq!((first.daily_xp, first.goal_xp), (first.xp, 20));
    refused(app.lesson_finish(&session), "not_found", GONE);

    // The next week: the weekly record comes once, when this week's XP first exceeds the best
    // earlier week's (module 03). Six missed days with no freeze reset the streak to 1.
    clock.set("2026-10-12T09:00:00Z");
    let best_earlier = first.xp;
    let mut week = 0;
    let mut records = 0;
    for _ in 0..4 {
        let run = ok(app.lesson_start("es.a1.u01.s01", "learn"));
        assert_run_shape(&run, "lesson");
        let verdicts = answer_all(&mut app, &run, false);
        let summary = ok(app.lesson_finish(&run.session_id));
        assert_eq!(summary.xp, expected_xp(SessionKind::Lesson, &verdicts));
        let record = best_earlier > 0 && week <= best_earlier && week + summary.xp > best_earlier;
        assert_eq!(summary.record_week, record, "after {week} XP this week");
        assert_eq!(
            summary.message.as_deref(),
            if record { Some(WEEKLY_RECORD) } else { None }
        );
        assert_eq!(summary.streak, 1);
        week += summary.xp;
        assert_eq!(summary.daily_xp, week);
        records += usize::from(summary.record_week);
    }
    assert_eq!(records, 1, "New weekly record! appears exactly once");
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 6: an empty review session tells the next review.
// ---------------------------------------------------------------------------------------------

#[test]
fn t06_nothing_due_names_the_next_review() {
    let dir = tempdir();
    // 20:30 local at UTC+2.
    let clock = TestClock::at("2026-10-14T18:30:00Z", 120);
    let mut app = open_offline(dir.path(), &clock);
    ok(app.app_start());
    ok(app.backup_import(&case_file("next-review-backup.json")));

    let es = ok(app.review_start("es", 20));
    assert_eq!(es.session_id, "");
    assert!(es.exercises.is_empty());
    assert!(es.attributions.is_empty());
    assert_eq!(
        es.notice.as_deref(),
        Some("Nothing to review. Next review: tomorrow at 09:00.")
    );
    let fr = ok(app.review_start("fr", 20));
    assert!(fr.exercises.is_empty());
    assert_eq!(
        fr.notice.as_deref(),
        Some("Nothing to review. Next review: today at 22:00.")
    );
}

#[test]
fn t06_with_no_reviewed_card_the_notice_asks_for_a_lesson() {
    let dir = tempdir();
    let clock = TestClock::at("2026-10-14T18:30:00Z", 120);
    let mut app = open_offline(dir.path(), &clock);
    ok(app.app_start());
    ok(app.course_start("es"));
    let empty = ok(app.review_start("es", 20));
    assert_eq!(empty.session_id, "");
    assert!(empty.exercises.is_empty());
    assert_eq!(
        empty.notice.as_deref(),
        Some("Nothing to review yet. Finish a lesson to start your reviews.")
    );
}

#[test]
fn t06_next_review_notice_forms() {
    // The function may give the whole notice or only the part after "Next review: ".
    #[track_caller]
    fn says(due: &str, now: &str, offset: i32, when: &str) {
        let notice = next_review_notice(utc_ms(due), utc_ms(now), offset);
        let full = format!("Nothing to review. Next review: {when}.");
        assert!(
            notice == when || notice == full,
            "{notice:?} is not {full:?}"
        );
    }
    says(
        "2026-10-14T19:30:00Z",
        "2026-10-14T18:30:00Z",
        120,
        "today at 21:30",
    );
    says(
        "2026-10-14T09:05:00Z",
        "2026-10-14T05:00:00Z",
        0,
        "today at 09:05",
    );
    says(
        "2026-10-15T07:00:00Z",
        "2026-10-14T18:30:00Z",
        120,
        "tomorrow at 09:00",
    );
    // 21:00 local on 13 October at UTC−5; the due time is 08:05 local on the 14th.
    says(
        "2026-10-14T13:05:00Z",
        "2026-10-14T02:00:00Z",
        -300,
        "tomorrow at 08:05",
    );
    says(
        "2026-10-14T09:00:00Z",
        "2026-10-10T10:00:00Z",
        0,
        "on 14 October",
    );
    says(
        "2026-11-03T08:05:00Z",
        "2026-10-30T10:00:00Z",
        0,
        "on 3 November",
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 7: stats of the hand-written backup.
// ---------------------------------------------------------------------------------------------

#[test]
fn t07_stats_of_the_hand_written_backup_match_the_hand_worked_numbers() {
    let expected = read_json(&case_file("expected-stats.json"));
    let now = expected["clock"]["now"].as_str().expect("clock.now");
    let offset = expected["clock"]["utc_offset_minutes"]
        .as_i64()
        .expect("clock.utc_offset_minutes");
    assert_eq!(
        utc_ms(now),
        expected["clock"]["now_ms"].as_i64().unwrap_or(0)
    );
    let tolerance = expected["tolerance"].as_f64().expect("tolerance");
    let dir = tempdir();
    let clock = TestClock::at(now, offset as i32);
    let mut app = open_offline(dir.path(), &clock);
    ok(app.app_start());

    let report = ok(app.backup_import(&case_file("stats-backup.json")));
    let file = read_json(&case_file("stats-backup.json"));
    let count = |table: &str| file[table].as_array().map_or(0, Vec::len);
    assert_eq!(report.courses, count("courses"));
    assert_eq!(report.cards, count("cards"));
    assert_eq!(report.reviews, count("review_log"));
    assert_eq!(report.skipped_cards, 0);

    let launch = ok(app.app_start());
    assert!(launch.onboarded);
    assert_eq!(launch.active_course.as_deref(), Some("es"));

    let stats = ok(app.stats_get("es"));
    assert_json_close(&to_json(&stats), &expected["stats"], tolerance, "stats");
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 8: backup export and import.
// ---------------------------------------------------------------------------------------------

/// The top-level keys of a backup, in the spec's order.
const BACKUP_KEYS: [&str; 14] = [
    "schema",
    "app_version",
    "exported_at",
    "settings",
    "courses",
    "items",
    "accepted_answers",
    "cards",
    "review_log",
    "sessions",
    "lessons",
    "judgements",
    "daily",
    "freeze_events",
];

#[track_caller]
fn assert_backup_layout(path: &Path) {
    let text = fs::read_to_string(path).expect("the exported backup");
    let text = text.replace("\r\n", "\n");
    assert!(
        text.starts_with("{\n  \"schema\": \"parley.backup/1\",\n"),
        "pretty-printed with two spaces, schema first"
    );
    let positions: Vec<usize> = BACKUP_KEYS
        .iter()
        .map(|key| {
            text.find(&format!("\n  \"{key}\":"))
                .unwrap_or_else(|| panic!("the backup has {key} at the top level"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|w| w[0] < w[1]),
        "keys in the spec's order"
    );
    let value = read_json(path);
    let columns: [(&str, &[&str]); 9] = [
        (
            "courses",
            &[
                "lang",
                "syllabus_version",
                "pack_version",
                "started_at",
                "placement_index",
                "cefr_estimate",
                "tested_out",
            ],
        ),
        (
            "items",
            &[
                "id",
                "lang",
                "skill_id",
                "kind",
                "target",
                "en",
                "lemmas",
                "cefr",
                "source_kind",
                "source_ref",
                "created_at",
                "retired",
            ],
        ),
        (
            "accepted_answers",
            &[
                "item_id",
                "direction",
                "norm",
                "display",
                "source",
                "created_at",
            ],
        ),
        (
            "cards",
            &[
                "item_id",
                "facet",
                "state",
                "stability",
                "difficulty",
                "due",
                "last_review",
                "reps",
                "lapses",
            ],
        ),
        (
            "review_log",
            &[
                "id",
                "item_id",
                "facet",
                "reviewed_at",
                "rating",
                "exercise_type",
                "verdict",
                "response_ms",
                "elapsed_days",
                "stability_after",
                "difficulty_after",
                "session_id",
            ],
        ),
        (
            "sessions",
            &[
                "id",
                "lang",
                "kind",
                "skill_id",
                "started_at",
                "ended_at",
                "local_date",
                "xp",
                "exercises",
                "correct",
                "cost_micros",
            ],
        ),
        (
            "lessons",
            &[
                "id",
                "lang",
                "skill_id",
                "seq",
                "origin",
                "prompt_id",
                "content",
                "created_at",
            ],
        ),
        (
            "daily",
            &["local_date", "xp", "sessions", "goal_xp", "frozen"],
        ),
        ("freeze_events", &["id", "local_date", "kind", "streak_len"]),
    ];
    for (table, names) in columns {
        let expected: BTreeSet<&str> = names.iter().copied().collect();
        for row in value[table].as_array().expect("an array per table") {
            let fields: BTreeSet<&str> = row
                .as_object()
                .expect("a row is an object")
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(fields, expected, "{table} rows are the table's columns");
        }
    }
    for row in value["lessons"].as_array().into_iter().flatten() {
        assert!(row["content"].is_object(), "lessons.content as JSON");
    }
    for row in value["items"].as_array().into_iter().flatten() {
        assert_ne!(
            row["source_kind"], "syllabus",
            "syllabus items are rebuilt, not exported"
        );
    }
    for row in value["accepted_answers"].as_array().into_iter().flatten() {
        assert_ne!(
            row["source"], "content",
            "content answers are rebuilt, not exported"
        );
    }
}

#[test]
fn t08_export_delete_import_export_gives_the_same_backup() {
    let dir = tempdir();
    let out = tempdir();
    let clock = TestClock::at("2026-10-15T12:00:00Z", 0);
    let mut app = open_offline(dir.path(), &clock);
    ok(app.app_start());
    ok(app.backup_import(&case_file("stats-backup.json")));

    // A lesson after the import adds a lesson row, sessions, reviews, daily XP and, through
    // "I was right", a learner's accepted answer. With 10 new items, the next skill's first 10
    // items are its 8 words and examples 1 and 2, whose introductions are translate_to_en.
    ok(app.settings_set(&json!({"new_items_per_lesson": 10, "lesson_length": 20})));
    let run = ok(app.lesson_start("es.a1.u01.s03", "learn"));
    assert_run_shape(&run, "lesson");
    let free = run
        .exercises
        .iter()
        .find(|ex| {
            matches!(
                ex.body,
                Body::TranslateToEn { .. } | Body::TranslateToTarget { .. }
            )
        })
        .expect("a translation the offline grader leaves unchecked");
    let mut verdicts = Vec::new();
    for ex in &run.exercises {
        if ex.id == free.id {
            let answer = Answer::Text("something else entirely said in other words".into());
            let grade =
                ok(app.exercise_submit(&run.session_id, &ex.id, &answer, Some(5_000), false));
            assert_eq!(grade.verdict, Verdict::Unchecked);
            assert_eq!(grade.rating, None);
            let grade = ok(app.exercise_i_was_right(&run.session_id, &ex.id));
            assert_eq!(grade.verdict, Verdict::Correct);
            assert_eq!(grade.rating, Some(Rating::Good));
            verdicts.push(grade.verdict);
        } else {
            let grade = ok(app.exercise_submit(
                &run.session_id,
                &ex.id,
                &right_answer(ex),
                Some(5_000),
                false,
            ));
            verdicts.push(grade.verdict);
        }
    }
    assert!(verdicts.iter().all(|v| *v == Verdict::Correct));
    ok(app.lesson_finish(&run.session_id));

    clock.set("2026-10-15T13:00:00Z");
    let first = out.path().join("first.json");
    let summary = ok(app.backup_export(&first));
    assert_backup_layout(&first);
    let exported = read_json(&first);
    assert_eq!(exported["schema"], "parley.backup/1");
    assert_eq!(exported["exported_at"], utc_text(clock.ms()));
    assert_eq!(exported["settings"], to_json(&app.settings_get()));
    let count = |table: &str| exported[table].as_array().map_or(0, Vec::len);
    assert_eq!(
        summary.bytes,
        fs::metadata(&first).expect("the backup file").len()
    );
    assert_eq!(
        (summary.courses, summary.cards, summary.reviews),
        (count("courses"), count("cards"), count("review_log"))
    );
    assert!(count("lessons") >= 1, "the lesson is exported");
    assert!(
        exported["accepted_answers"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|row| row["source"] == "learner"),
        "the answer accepted by I was right is exported"
    );

    refused(
        app.data_delete_all("no"),
        "invalid_input",
        "Type delete to confirm.",
    );
    ok(app.data_delete_all("delete"));
    assert!(!ok(app.app_start()).onboarded);

    let report = ok(app.backup_import(&first));
    assert_eq!(
        (
            report.courses,
            report.cards,
            report.reviews,
            report.skipped_cards
        ),
        (count("courses"), count("cards"), count("review_log"), 0)
    );
    assert!(
        !dir.path().join("parley.sqlite3.import").exists(),
        "the new store took the old one's place"
    );
    let launch = ok(app.app_start());
    assert!(launch.onboarded);
    assert_eq!(launch.active_course.as_deref(), Some("es"));

    clock.set("2026-10-15T14:00:00Z");
    let second = out.path().join("second.json");
    ok(app.backup_export(&second));
    assert_eq!(read_json(&second)["exported_at"], utc_text(clock.ms()));
    assert_eq!(
        backup_without(&second, &["exported_at"]),
        backup_without(&first, &["exported_at"]),
        "identical apart from exported_at"
    );
}

#[test]
fn t08_refused_backups_change_nothing() {
    let dir = tempdir();
    let out = tempdir();
    let clock = TestClock::at("2026-10-15T12:00:00Z", 0);
    let mut app = open_offline(dir.path(), &clock);
    ok(app.app_start());
    ok(app.backup_import(&case_file("stats-backup.json")));

    // Not JSON, not the schema's shape, another schema, a newer version.
    let not_json = out.path().join("not-json.json");
    fs::write(&not_json, b"this is not a backup").expect("a file");
    refused(app.backup_import(&not_json), "invalid_input", NOT_A_BACKUP);
    let mut no_cards = read_json(&case_file("stats-backup.json"));
    no_cards.as_object_mut().expect("an object").remove("cards");
    let no_cards_path = out.path().join("no-cards.json");
    fs::write(&no_cards_path, no_cards.to_string()).expect("a file");
    refused(
        app.backup_import(&no_cards_path),
        "invalid_input",
        NOT_A_BACKUP,
    );
    refused(
        app.backup_import(&case_file("backup-other-schema.json")),
        "invalid_input",
        NOT_A_BACKUP,
    );
    refused(
        app.backup_import(&case_file("backup-v2.json")),
        "invalid_input",
        "This backup was made by a newer Parley. Update Parley to import it.",
    );
    refused(
        app.backup_import(&out.path().join("missing.json")),
        "not_found",
        "Parley could not read that file.",
    );

    // A destination under a file cannot be written.
    let blocker = out.path().join("a-file");
    fs::write(&blocker, b"in the way").expect("a file");
    refused(
        app.backup_export(&blocker.join("backup.json")),
        "storage",
        CANNOT_WRITE,
    );

    // The learner's data is untouched: an export gives the fixture back, apart from the time of
    // export and the version that wrote it. The fixture predates stories (module 07), which an
    // export now always writes, here as an empty array.
    let after = out.path().join("after.json");
    ok(app.backup_export(&after));
    assert_backup_layout(&after);
    let mut expected = backup_without(
        &case_file("stats-backup.json"),
        &["exported_at", "app_version"],
    );
    expected["stories"] = serde_json::json!([]);
    assert_json_close(
        &backup_without(&after, &["exported_at", "app_version"]),
        &expected,
        1e-12,
        "backup",
    );
}

#[test]
fn t08_a_card_whose_item_is_missing_is_skipped_and_counted() {
    let dir = tempdir();
    let out = tempdir();
    let clock = TestClock::at("2026-10-15T12:00:00Z", 0);
    let mut app = open_offline(dir.path(), &clock);
    ok(app.app_start());
    let report = ok(app.backup_import(&case_file("backup-missing-item.json")));
    assert_eq!(report.courses, 1);
    assert_eq!(report.skipped_cards, 1);
    assert_eq!(report.cards, 2, "the two cards whose items exist");

    let exported = out.path().join("after.json");
    ok(app.backup_export(&exported));
    let value = read_json(&exported);
    let cards: Vec<&str> = value["cards"]
        .as_array()
        .expect("cards")
        .iter()
        .filter_map(|card| card["item_id"].as_str())
        .collect();
    assert_eq!(cards, vec!["es.a1.u01.s01#l:hola", "es:conv:6f1c:1"]);
    let items: Vec<&str> = value["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter_map(|item| item["id"].as_str())
        .collect();
    assert_eq!(
        items,
        vec!["es:conv:6f1c:1"],
        "the conversation item is kept"
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 9: settings.
// ---------------------------------------------------------------------------------------------

#[test]
fn t09_a_missing_settings_file_gives_the_defaults_without_a_notice() {
    let dir = tempdir();
    let clock = TestClock::at("2026-10-15T12:00:00Z", 0);
    let mut app = open_offline(dir.path(), &clock);
    assert_eq!(ok(app.app_start()).notice, None);
    assert_json_close(
        &to_json(&app.settings_get()),
        &default_settings(),
        0.0,
        "settings",
    );
    assert_eq!(SETTINGS_SCHEMA, "parley.settings/1");
    assert_eq!(
        SETTINGS_NOTICE,
        "Your settings file could not be read, so Parley started with default settings."
    );
}

#[test]
fn t09_a_bad_settings_file_gives_the_defaults_and_the_notice() {
    let mut unknown_key = default_settings();
    unknown_key["colour"] = json!("blue");
    let mut other_schema = default_settings();
    other_schema["schema"] = json!("parley.settings/2");
    let mut wrong_type = default_settings();
    wrong_type["daily_goal_xp"] = json!("twenty");
    let files: Vec<(&str, Vec<u8>)> = vec![
        ("not JSON", b"{not json".to_vec()),
        ("not UTF-8", vec![0xff, 0xfe, 0x7b, 0x7d]),
        ("another schema", other_schema.to_string().into_bytes()),
        ("an unknown key", unknown_key.to_string().into_bytes()),
        ("a wrong type", wrong_type.to_string().into_bytes()),
    ];
    for (what, bytes) in files {
        let dir = tempdir();
        fs::write(dir.path().join("settings.json"), bytes).expect("settings.json");
        let clock = TestClock::at("2026-10-15T12:00:00Z", 0);
        let mut app = open_offline(dir.path(), &clock);
        let launch = ok(app.app_start());
        assert_eq!(launch.notice.as_deref(), Some(SETTINGS_NOTICE), "{what}");
        assert_json_close(
            &to_json(&app.settings_get()),
            &default_settings(),
            0.0,
            what,
        );

        // The next save overwrites the bad file, and a new launch reads it without a notice.
        ok(app.settings_set(&json!({"daily_goal_xp": 10})));
        drop(app);
        let mut app = open_offline(dir.path(), &clock);
        assert_eq!(ok(app.app_start()).notice, None, "{what}");
        assert_eq!(to_json(&app.settings_get())["daily_goal_xp"], json!(10));
    }
}

#[test]
fn t09_settings_set_refuses_bad_values_and_keeps_good_ones() {
    let dir = tempdir();
    let clock = TestClock::at("2026-10-15T12:00:00Z", 0);
    let mut app = open_offline(dir.path(), &clock);
    ok(app.app_start());
    let before = to_json(&app.settings_get());

    let refusals = [
        (
            json!({"accent_leniency": "loose"}),
            "Unknown value loose for accent_leniency.",
        ),
        (
            json!({"lesson_length": 30}),
            "Invalid value for lesson_length.",
        ),
        (json!({"colour": "blue"}), "Unknown setting colour."),
        (
            json!({"daily_goal_xp": 25}),
            "Invalid value for daily_goal_xp.",
        ),
        (
            json!({"answer_input": "voice"}),
            "Unknown value voice for answer_input.",
        ),
        (
            json!({"answer_input": 3}),
            "Invalid value for answer_input.",
        ),
        (
            json!({"desired_retention": 0.99}),
            "Invalid value for desired_retention.",
        ),
        (
            json!({"new_items_per_lesson": 0}),
            "Invalid value for new_items_per_lesson.",
        ),
        (
            json!({"lesson_length": 7}),
            "Invalid value for lesson_length.",
        ),
        (
            json!({"active_course": "it"}),
            "Unknown value it for active_course.",
        ),
        (
            json!({"audio": {"voice_source": "robot"}}),
            "Unknown value robot for audio.voice_source.",
        ),
        (
            json!({"audio": {"autoplay": "yes"}}),
            "Invalid value for audio.autoplay.",
        ),
        (
            json!({"audio": {"pitch": 1}}),
            "Unknown setting audio.pitch.",
        ),
        (
            json!({"ai": {"provider": "magic"}}),
            "Unknown value magic for ai.provider.",
        ),
    ];
    for (patch, message) in refusals {
        refused(app.settings_set(&patch), "invalid_input", message);
        assert_eq!(
            to_json(&app.settings_get()),
            before,
            "a refused patch changes nothing: {patch}"
        );
    }

    // Nested objects merge key by key, null sets null, schema is ignored.
    ok(app.settings_set(&json!({"audio": {"os_voice": {"es": "voice-1"}}})));
    let saved = ok(app.settings_set(&json!({
        "schema": "something else",
        "daily_goal_xp": 50,
        "accent_leniency": "strict",
        "answer_input": "word_bank",
        "lesson_length": 20,
        "audio": {"autoplay": false, "os_voice": {"fr": "voice-2"}},
        "speaking": {"keep_recordings": true}
    })));
    let mut expected = default_settings();
    expected["daily_goal_xp"] = json!(50);
    expected["accent_leniency"] = json!("strict");
    expected["answer_input"] = json!("word_bank");
    expected["lesson_length"] = json!(20);
    expected["audio"]["autoplay"] = json!(false);
    expected["audio"]["os_voice"]["es"] = json!("voice-1");
    expected["audio"]["os_voice"]["fr"] = json!("voice-2");
    expected["speaking"]["keep_recordings"] = json!(true);
    assert_json_close(&to_json(&saved), &expected, 0.0, "saved");
    ok(app.settings_set(&json!({"audio": {"os_voice": {"es": null}}})));
    expected["audio"]["os_voice"]["es"] = Value::Null;
    drop(app);

    // A new Parley on the same directory reads the saved settings back.
    let mut app = open_offline(dir.path(), &clock);
    assert_eq!(ok(app.app_start()).notice, None);
    assert_json_close(&to_json(&app.settings_get()), &expected, 0.0, "read back");
}

#[test]
fn t09_a_settings_file_that_cannot_be_written_is_a_storage_error() {
    let dir = tempdir();
    // A folder where the settings file should be.
    fs::create_dir_all(dir.path().join("settings.json")).expect("a folder");
    let clock = TestClock::at("2026-10-15T12:00:00Z", 0);
    let mut app = open_offline(dir.path(), &clock);
    let _ = app.app_start();
    refused(
        app.settings_set(&json!({"daily_goal_xp": 30})),
        "storage",
        CANNOT_WRITE,
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 10: Delete everything and the cache.
// ---------------------------------------------------------------------------------------------

#[test]
fn t10_delete_everything_needs_the_word_and_leaves_a_fresh_start() {
    let scratch = tempdir();
    let pack = fixture_pack(scratch.path());
    let lock = pack_lock("es", "2026-10-01", PACK_URL, &pack.info);
    let dir = tempdir();
    let clock = TestClock::at("2026-10-12T09:00:00Z", 0);
    let net = FakeNet::serving(PACK_URL, 200, pack.bytes.clone());
    let mut app = open(dir.path(), &clock, &lock, net.boxed(), 5);
    ok(app.app_start());
    ok(app.course_start("es"));
    ok(app.settings_set(&json!({"daily_goal_xp": 50})));
    ok(app.pack_download("es", &mut |_| {}));
    let run = ok(app.lesson_start("es.a1.u01.s01", "learn"));
    answer_all(&mut app, &run, false);
    ok(app.lesson_finish(&run.session_id));
    fs::create_dir_all(dir.path().join("cache")).expect("the cache folder");
    fs::write(dir.path().join("cache/ai.sqlite3"), b"cached answers").expect("a cache file");

    for text in ["", "Delete", "delete everything", "yes"] {
        refused(
            app.data_delete_all(text),
            "invalid_input",
            "Type delete to confirm.",
        );
    }
    assert!(ok(app.app_start()).onboarded, "a refusal deletes nothing");
    assert!(ok(app.path_get("es")).today_xp > 0);

    ok(app.data_delete_all("  delete \n"));
    // Removed at once: the settings file, the AI cache and the pack files.
    assert!(!dir.path().join("settings.json").exists());
    assert!(!dir.path().join("cache/ai.sqlite3").exists());
    assert_eq!(files_in(&dir.path().join("packs")), Vec::<String>::new());
    let launch = ok(app.app_start());
    assert!(!launch.onboarded);
    assert_eq!(launch.active_course, None);
    assert_eq!(launch.notice, None);
    assert!(ok(app.course_list()).iter().all(|course| !course.started));
    assert_json_close(
        &to_json(&app.settings_get()),
        &default_settings(),
        0.0,
        "settings",
    );
    refused(app.path_get("es"), "not_found", NOT_STARTED);
    assert!(matches!(
        ok(app.pack_status("es")).state,
        PackState::Available
    ));
}

#[test]
fn t10_delete_everything_that_cannot_remove_a_file_reports_it_and_leaves_a_usable_store() {
    let dir = tempdir();
    let clock = TestClock::at("2026-10-12T09:00:00Z", 0);
    let mut app = open_offline(dir.path(), &clock);
    ok(app.app_start());
    ok(app.course_start("es"));
    // A folder where the AI cache file should be: removing it as a file fails.
    fs::create_dir_all(dir.path().join("cache/ai.sqlite3")).expect("a folder");

    refused(app.data_delete_all("delete"), "storage", CANNOT_WRITE);
    ok(app.app_start());
    let courses = ok(app.course_list());
    assert!(
        courses.iter().any(|course| course.lang == "es"),
        "the store answers again: {}",
        show(&courses)
    );
}

#[test]
fn t10_cache_clear_counts_the_files_it_removes() {
    let dir = tempdir();
    let clock = TestClock::at("2026-10-12T09:00:00Z", 0);
    let mut app = open_offline(dir.path(), &clock);
    ok(app.app_start());
    assert_eq!(ok(app.cache_clear("ai")).files_removed, 0);
    assert_eq!(ok(app.cache_clear("audio")).files_removed, 0);
    assert_eq!(ok(app.cache_clear("all")).files_removed, 0);
    refused(
        app.cache_clear("other"),
        "invalid_input",
        "Unknown value other for kind.",
    );

    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).expect("the cache folder");
    for kind in ["ai", "all"] {
        fs::write(cache.join("ai.sqlite3"), b"cached answers").expect("a cache file");
        assert_eq!(ok(app.cache_clear(kind)).files_removed, 1, "{kind}");
        assert!(!cache.join("ai.sqlite3").exists(), "{kind}");
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 11: one list of commands.
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
fn t11_commands_are_one_list_in_core_ui_and_desktop() {
    assert_eq!(
        &COMMANDS[..SPEC_COMMANDS.len()],
        SPEC_COMMANDS.as_slice(),
        "the spec's table order, before the commands later modules append"
    );
    let app_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the app folder");
    let ts = fs::read_to_string(app_dir.join("src/api/commands.ts")).expect("commands.ts");
    assert_eq!(
        ts_commands(&ts),
        COMMANDS,
        "commands.ts lists the same names in order"
    );
    let lib = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"))
        .expect("app/src-tauri/src/lib.rs");
    for name in COMMANDS {
        assert!(
            contains_word(&lib, name),
            "{name} is registered in app/src-tauri/src/lib.rs"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 13: attribution of pack sentences.
// ---------------------------------------------------------------------------------------------

fn fill_gap(id: &str, item_id: &str, sentence: &str, answer: &str, full: &str) -> Exercise {
    Exercise {
        id: id.to_string(),
        item_id: item_id.to_string(),
        facet: Facet::Productive,
        origin: Origin::Template,
        body: Body::FillGap {
            sentence: sentence.to_string(),
            answer: answer.to_string(),
            full_sentence: full.to_string(),
            hint_en: "hello".to_string(),
        },
    }
}

fn sorted_attributions(value: Value) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = value
        .as_array()
        .expect("a list of attributions")
        .iter()
        .map(|a| {
            (
                a["exercise_id"].as_str().unwrap_or_default().to_string(),
                a["author"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    pairs.sort();
    pairs
}

#[test]
fn t13_attributions_name_pack_authors_and_not_syllabus_examples() {
    let syllabus = es_syllabus();
    let lesson = Lesson {
        schema: "parley.lesson/1".to_string(),
        lang: Lang::Es,
        skill_id: "es.a1.u01.s01".to_string(),
        seq: 1,
        origin: Origin::Template,
        exercises: vec![
            // fixture_a's pack sentence (tatoeba-mini 1001).
            fill_gap(
                "e1",
                "es.a1.u01.s01#l:hola",
                "___, soy Ana.",
                "Hola",
                "Hola, soy Ana.",
            ),
            // Example 2 of es.a1.u01.s01, which a pack sentence also says.
            fill_gap(
                "e2",
                "es.a1.u01.s01#l:día",
                "Buenos ___.",
                "días",
                "Buenos días.",
            ),
            Exercise {
                id: "e3".to_string(),
                item_id: "es.a1.u01.s01#l:hola".to_string(),
                facet: Facet::Receptive,
                origin: Origin::Template,
                body: Body::PickMeaning {
                    prompt_target: "hola".to_string(),
                    options: vec![
                        "hello".into(),
                        "goodbye".into(),
                        "night".into(),
                        "good".into(),
                    ],
                    answer_index: 0,
                },
            },
            // A pack sentence with no author (tatoeba-mini 1010).
            fill_gap(
                "e4",
                "es.a1.u01.s01#l:hola",
                "Tenemos agua y ___.",
                "café",
                "Tenemos agua y café.",
            ),
        ],
        ai: None,
    };
    let pack = vec![
        PackSentence {
            id: 1001,
            text: "Hola, soy Ana.".to_string(),
            en: vec!["Hello, I am Ana.".to_string()],
            author: "fixture_a".to_string(),
        },
        PackSentence {
            id: 1099,
            text: "Buenos días.".to_string(),
            en: vec!["Good morning.".to_string()],
            author: "fixture_b".to_string(),
        },
        PackSentence {
            id: 1010,
            text: "Tenemos agua y café.".to_string(),
            en: vec!["We have water and coffee.".to_string()],
            author: String::new(),
        },
    ];
    let named = attributions(&lesson, &pack, &syllabus);
    assert_eq!(
        sorted_attributions(to_json(&named)),
        vec![
            ("e1".to_string(), "fixture_a".to_string()),
            ("e4".to_string(), String::new()),
        ]
    );
    assert!(sorted_attributions(to_json(&attributions(&lesson, &[], &syllabus))).is_empty());
}

#[test]
fn t13_lessons_attribute_exactly_the_pack_sentences_they_use() {
    // tatoeba-mini's Spanish sentences and their authors (`\N` is no author).
    let tsv = fs::read_to_string(
        repo_root().join("tests/fixtures/tatoeba-mini/spa_sentences_detailed.tsv"),
    )
    .expect("tatoeba-mini");
    let authors: BTreeMap<String, String> = tsv
        .lines()
        .filter_map(|line| {
            let columns: Vec<&str> = line.split('\t').collect();
            (columns.len() >= 4).then(|| {
                let author = if columns[3] == "\\N" { "" } else { columns[3] };
                (columns[2].to_string(), author.to_string())
            })
        })
        .collect();
    let syllabus = es_syllabus();
    let examples: BTreeSet<String> = syllabus
        .skills()
        .flat_map(|(_, skill)| skill.examples.iter().map(|e| e.target.clone()))
        .collect();

    let scratch = tempdir();
    let pack = fixture_pack(scratch.path());
    let lock = pack_lock("es", "2026-10-01", PACK_URL, &pack.info);
    let dir = tempdir();
    let clock = TestClock::at("2026-10-12T09:00:00Z", 0);
    let net = FakeNet::serving(PACK_URL, 200, pack.bytes.clone());
    let mut app = open(dir.path(), &clock, &lock, net.boxed(), 11);
    ok(app.app_start());
    ok(app.course_start("es"));
    ok(app.pack_download("es", &mut |_| {}));

    let mut from_pack = 0;
    for _ in 0..6 {
        let run = ok(app.lesson_start("es.a1.u01.s01", "learn"));
        let named: BTreeMap<String, String> = sorted_attributions(to_json(&run.attributions))
            .into_iter()
            .collect();
        let mut expected = BTreeMap::new();
        for ex in &run.exercises {
            if let Body::FillGap { full_sentence, .. } = &ex.body
                && !examples.contains(full_sentence)
            {
                let author = authors.get(full_sentence).unwrap_or_else(|| {
                    panic!("{full_sentence:?} is neither a syllabus example nor a pack sentence")
                });
                expected.insert(ex.id.clone(), author.clone());
            }
        }
        assert_eq!(named, expected, "{}", show(&run.exercises));
        from_pack += expected.len();
        answer_all(&mut app, &run, false);
        ok(app.lesson_finish(&run.session_id));
    }
    // Which sentence a gap takes is the seeded generator's choice; the count is reported only.
    eprintln!("{from_pack} exercises on pack sentences");
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 15: a data file that cannot be opened.
// ---------------------------------------------------------------------------------------------

#[test]
fn t15_a_corrupt_data_file_is_reported_and_a_backup_restores_it() {
    let dir = tempdir();
    let garbage: Vec<u8> = b"this is not a Parley data file "
        .iter()
        .copied()
        .cycle()
        .take(4_096)
        .collect();
    fs::write(dir.path().join("parley.sqlite3"), garbage).expect("a corrupt data file");
    let clock = TestClock::at("2026-10-15T12:00:00Z", 0);
    let mut app = open_offline(dir.path(), &clock);

    let error = refused(
        app.app_start(),
        "storage",
        "Parley could not open its data file.",
    );
    let details = error
        .action
        .as_ref()
        .and_then(|action| action["details"].as_str())
        .unwrap_or_else(|| panic!("the error carries details: {:?}", error.action));
    assert!(!details.trim().is_empty());
    refused(
        app.course_list(),
        "storage",
        "Parley could not open its data file.",
    );

    let report = ok(app.backup_import(&case_file("stats-backup.json")));
    assert_eq!(report.skipped_cards, 0);
    let launch = ok(app.app_start());
    assert!(launch.onboarded);
    assert_eq!(launch.active_course.as_deref(), Some("es"));
    ok(app.path_get("es"));
}

// ---------------------------------------------------------------------------------------------
// The messages table: every refusal not covered above.
// ---------------------------------------------------------------------------------------------

#[test]
fn messages_for_unknown_values_courses_skills_and_sessions() {
    let dir = tempdir();
    let clock = TestClock::at("2026-10-12T09:00:00Z", 0);
    // The lock publishes French, so only the course not being started stands in the way.
    let lock = "schema = \"parley.packs/1\"\n\n[fr]\nversion = \"2026-10-01\"\nurl = \"https://packs.example.invalid/fr-2026-10-01.sqlite3\"\nsha256 = \"0000000000000000000000000000000000000000000000000000000000000000\"\nbytes = 1000\ncontent_sha256 = \"0000000000000000000000000000000000000000000000000000000000000000\"\n";
    let net = FakeNet::default();
    let mut app = open(dir.path(), &clock, lock, net.boxed(), 9);
    ok(app.app_start());

    refused(
        app.course_start("xx"),
        "invalid_input",
        "Unknown value xx for lang.",
    );
    refused(
        app.pack_status("xx"),
        "invalid_input",
        "Unknown value xx for lang.",
    );
    refused(
        app.review_start("xx", 20),
        "invalid_input",
        "Unknown value xx for lang.",
    );
    refused(app.path_get("fr"), "not_found", NOT_STARTED);
    refused(app.placement_start("fr"), "not_found", NOT_STARTED);
    refused(app.review_start("fr", 20), "not_found", NOT_STARTED);
    refused(app.stats_get("fr"), "not_found", NOT_STARTED);
    refused(
        app.pack_download("fr", &mut |_| {}),
        "not_found",
        NOT_STARTED,
    );
    assert!(net.requests().is_empty());

    ok(app.course_start("es"));
    refused(app.skill_get("es.a1.u01.s99"), "not_found", NO_SKILL);
    refused(
        app.lesson_start("es.a1.u09.s09", "learn"),
        "not_found",
        NO_SKILL,
    );
    refused(
        app.lesson_start("es.a1.u01.s01", "quiz"),
        "invalid_input",
        "Unknown value quiz for mode.",
    );
    refused(
        app.placement_answer("p-0000000000000000", &Answer::Text("hello".into())),
        "not_found",
        GONE,
    );
    refused(
        app.exercise_submit("s-0000000000000000", "e1", &Answer::Choice(0), None, false),
        "not_found",
        GONE,
    );
    refused(
        app.exercise_i_was_right("s-0000000000000000", "e1"),
        "not_found",
        GONE,
    );
    refused(app.lesson_finish("s-0000000000000000"), "not_found", GONE);

    // A started course with nothing finished has empty stats.
    let stats = ok(app.stats_get("es"));
    assert!(stats.empty);
    assert_eq!(stats.reviews_30, 0);
    assert_eq!(stats.recall_rate, None);
    assert_eq!(stats.days.len(), 30);
}
