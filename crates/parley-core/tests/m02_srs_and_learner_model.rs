//! Module 02 tests of the library (docs/modules/02-srs-and-learner-model.md, Acceptance tests).
//! Each test names the bullet it covers. Bullets 5, 11 and 12 also run as golden cases under
//! tests/cases/02-srs-and-learner-model, and the command line (its usage errors, the not-a-log
//! failure and the second half of bullet 7) is covered by
//! crates/parley-cli/tests/m02_srs_and_learner_model.rs.
//!
//! Expected values come from the spec's hand-worked numbers, docs/ARCHITECTURE.md §4, the
//! published algorithms as the Design notes state them (FSRS-6's initial stability, difficulty
//! and forgetting curve; Levitt's staircase; SplitMix64; Hinnant's civil dates), and the py-fsrs
//! 6.3.2 run recorded in tests/cases/02-srs-and-learner-model/fsrs_reference.json, never from
//! running the code. The fixture is tests/fixtures/generated/syllabus-mini/xx/syllabus.yaml
//! (`parley-fixtures --out tests/fixtures/generated`): 80 skills, each with three lemma items and
//! four example items.

use std::fs;
use std::path::{Path, PathBuf};

use parley_core::content::{Cefr, Forms, Lang, Syllabus, parse_syllabus};
use parley_core::env::{Clock, Rng, SplitMix64, local_date};
use parley_core::srs::{
    Card, CardState, DAY_MS, Facet, MAX_INTERVAL_DAYS, Outcome, Placement, PlacementResult, Rating,
    ReviewLogEntry, SkillRef, SkillState, SrsParams, Verdict, default_weights, due_queue,
    mastery_of, queue_order, rating_for, replay, retrievability, schedule, simulate, skill_mastery,
    skill_state,
};
use parley_core::store::{ReviewContext, Store, StoreError};
use rusqlite::Connection;
use serde_json::{Value, json};

const MINI: &str = "tests/fixtures/generated/syllabus-mini/xx/syllabus.yaml";
const REFERENCE: &str = "tests/cases/02-srs-and-learner-model/fsrs_reference.json";
/// 2026-01-05T09:00:00Z.
const T0: i64 = 1_767_603_600_000;
const HOUR: i64 = 3_600_000;
const SKILL_0: &str = "es.a1.u01.s01";
/// Skills × (lemma items + example items) of the mini syllabus.
const MINI_ITEMS: usize = 80 * (3 + 4);

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

fn mini() -> Syllabus {
    let text = fs::read_to_string(repo_root().join(MINI)).unwrap_or_else(|e| {
        panic!("{MINI}: {e}; generate it with parley-fixtures --out tests/fixtures/generated")
    });
    parse_syllabus(MINI, &text).unwrap_or_else(|errors| panic!("{MINI} does not parse: {errors:?}"))
}

fn mini_skills() -> Vec<SkillRef> {
    mini()
        .skills()
        .map(|(_, skill)| SkillRef {
            id: skill.id.clone(),
            cefr: skill.cefr,
        })
        .collect()
}

/// The skill id at a skill index, from the fixture's id scheme: index = 10 × (unit − 1) +
/// (skill − 1), units 1 to 4 at A1 and 5 to 8 at A2.
fn skill_id(index: usize) -> String {
    let unit = index / 10 + 1;
    let skill = index % 10 + 1;
    let level = if unit <= 4 { "a1" } else { "a2" };
    format!("es.{level}.u{unit:02}.s{skill:02}")
}

fn skill_ids(range: std::ops::Range<usize>) -> Vec<String> {
    range.map(skill_id).collect()
}

/// The seven items of the first skill in syllabus order: its lemma items in lemma order, then its
/// example items in example order (module 01's item ids).
fn skill_0_items() -> Vec<String> {
    [
        "#l:wabak", "#l:wabek", "#l:wabik", "#e:1", "#e:2", "#e:3", "#e:4",
    ]
    .iter()
    .map(|suffix| format!("{SKILL_0}{suffix}"))
    .collect()
}

fn ok<T>(result: Result<T, StoreError>) -> T {
    match result {
        Ok(value) => value,
        Err(e) => panic!("store error: {e}"),
    }
}

fn store_error<T>(result: Result<T, StoreError>) -> StoreError {
    match result {
        Ok(_) => panic!("expected a store error"),
        Err(e) => e,
    }
}

/// An in-memory store with the mini course started at T0.
fn course_store() -> Store {
    let mut store = ok(Store::open_in_memory());
    ok(store.start_course(&mini(), &Forms::default(), T0));
    store
}

/// A store file in `dir` with the mini course started at T0.
fn course_store_file(dir: &Path) -> (Store, PathBuf) {
    let path = dir.join("parley.sqlite3");
    let mut store = ok(Store::open(&path));
    ok(store.start_course(&mini(), &Forms::default(), T0));
    (store, path)
}

/// `fsrs::DEFAULT_PARAMETERS` widened to f64.
fn weights() -> [f64; 21] {
    fsrs::DEFAULT_PARAMETERS.map(f64::from)
}

/// The FSRS-6 forgetting curve in f64 (Design notes): R(t, S) = (1 + f·t/S)^(−d), d = w[20],
/// f = 0.9^(−1/d) − 1, t in days.
fn forgetting_curve(t_days: f64, stability: f64) -> f64 {
    let d = weights()[20];
    let f = 0.9f64.powf(-1.0 / d) - 1.0;
    (1.0 + f * t_days / stability).powf(-d)
}

/// The stability at which a card last reviewed `t_days` ago has retrievability `r`: the
/// forgetting curve solved for S.
fn stability_for(r: f64, t_days: f64) -> f64 {
    let d = weights()[20];
    let f = 0.9f64.powf(-1.0 / d) - 1.0;
    f * t_days / (r.powf(-1.0 / d) - 1.0)
}

/// The interval FSRS gives a stability at a desired retention: S / f × (r^(−1/d) − 1).
fn interval_at(stability: f64, desired_retention: f64) -> f64 {
    let d = weights()[20];
    let f = 0.9f64.powf(-1.0 / d) - 1.0;
    stability / f * (desired_retention.powf(-1.0 / d) - 1.0)
}

/// Whether `interval_days` is `min(365, max(1, round(x)))`. The crate computes the interval in
/// f32, so within 0.001 of a half day either neighbour is accepted.
fn interval_matches(interval_days: u32, x: f64) -> bool {
    let clamp = |v: f64| (v as i64).clamp(1, 365);
    let got = i64::from(interval_days);
    if x < 365.5 && (x - x.floor() - 0.5).abs() < 1e-3 {
        got == clamp(x.floor()) || got == clamp(x.ceil())
    } else {
        got == clamp(x.round())
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

fn rating_number(rating: Rating) -> u64 {
    match rating {
        Rating::Again => 1,
        Rating::Hard => 2,
        Rating::Good => 3,
        Rating::Easy => 4,
    }
}

fn verdict_for(rating: Rating) -> Verdict {
    match rating {
        Rating::Again => Verdict::Wrong,
        Rating::Hard => Verdict::Almost,
        Rating::Good | Rating::Easy => Verdict::Correct,
    }
}

fn context(rating: Rating, session: &str) -> ReviewContext {
    ReviewContext {
        exercise_type: "translate_to_en".to_string(),
        verdict: verdict_for(rating),
        response_ms: Some(4_000),
        session_id: session.to_string(),
    }
}

fn bits(value: Option<f64>) -> Option<u64> {
    value.map(f64::to_bits)
}

/// Field by field, floats compared bit for bit.
fn assert_same_card(actual: &Card, expected: &Card, what: &str) {
    let name = format!("{what}: {} {:?}", expected.item_id, expected.facet);
    assert_eq!(actual.item_id, expected.item_id, "{what}: item id");
    assert_eq!(actual.facet, expected.facet, "{name}: facet");
    assert_eq!(actual.state, expected.state, "{name}: state");
    assert_eq!(
        bits(actual.stability),
        bits(expected.stability),
        "{name}: stability {:?} against {:?}",
        actual.stability,
        expected.stability
    );
    assert_eq!(
        bits(actual.difficulty),
        bits(expected.difficulty),
        "{name}: difficulty {:?} against {:?}",
        actual.difficulty,
        expected.difficulty
    );
    assert_eq!(actual.due, expected.due, "{name}: due");
    assert_eq!(
        actual.last_review, expected.last_review,
        "{name}: last review"
    );
    assert_eq!(actual.reps, expected.reps, "{name}: reps");
    assert_eq!(actual.lapses, expected.lapses, "{name}: lapses");
}

fn assert_same_cards(actual: &[Card], expected: &[Card], what: &str) {
    assert_eq!(actual.len(), expected.len(), "{what}: number of cards");
    for (a, e) in actual.iter().zip(expected) {
        assert_same_card(a, e, what);
    }
}

fn assert_relative(actual: f64, expected: f64, tolerance: f64, what: &str) {
    let allowed = tolerance * expected.abs();
    assert!(
        (actual - expected).abs() <= allowed,
        "{what}: {actual} against {expected} (relative tolerance {tolerance})"
    );
}

/// A card reviewed `t_days` before `now` with stability `stability`, due `due`.
fn reviewed_card(
    item_id: &str,
    facet: Facet,
    stability: f64,
    t_days: f64,
    now: i64,
    due: i64,
) -> Card {
    Card {
        item_id: item_id.to_string(),
        facet,
        state: CardState::Review,
        stability: Some(stability),
        difficulty: Some(5.0),
        due: Some(due),
        last_review: Some(now - (t_days * DAY_MS as f64).round() as i64),
        reps: 1,
        lapses: 0,
    }
}

fn query_one<T: rusqlite::types::FromSql>(conn: &Connection, sql: &str) -> T {
    conn.query_row(sql, [], |row| row.get(0))
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

// ---------------------------------------------------------------------------------------------
// 1. Migration
// ---------------------------------------------------------------------------------------------

/// (name, declared type, NOT NULL, default, position in the primary key) of PRAGMA table_info.
type Column = (&'static str, &'static str, bool, Option<&'static str>, i64);
/// The same, as read from a file.
type ColumnInfo = (String, String, bool, Option<String>, i64);
/// (name, table, columns) of an index.
type Index = (&'static str, &'static str, Vec<&'static str>);

/// The seven tables of `0001_init`, column by column from docs/ARCHITECTURE.md §4. A TEXT or
/// INTEGER PRIMARY KEY column without NOT NULL reports notnull 0, as SQLite does.
fn architecture_tables() -> Vec<(&'static str, Vec<Column>)> {
    vec![
        (
            "meta",
            vec![
                ("key", "TEXT", false, None, 1),
                ("value", "TEXT", true, None, 0),
            ],
        ),
        (
            "courses",
            vec![
                ("lang", "TEXT", false, None, 1),
                ("syllabus_version", "INTEGER", true, None, 0),
                ("pack_version", "TEXT", false, None, 0),
                ("started_at", "INTEGER", true, None, 0),
                ("placement_index", "INTEGER", false, None, 0),
                ("cefr_estimate", "TEXT", false, None, 0),
            ],
        ),
        (
            "skills",
            vec![
                ("id", "TEXT", false, None, 1),
                ("lang", "TEXT", true, None, 0),
                ("unit_id", "TEXT", true, None, 0),
                ("ord", "INTEGER", true, None, 0),
                ("kind", "TEXT", true, None, 0),
                ("cefr", "TEXT", true, None, 0),
                ("title", "TEXT", true, None, 0),
                ("tested_out", "INTEGER", true, Some("0"), 0),
            ],
        ),
        (
            "items",
            vec![
                ("id", "TEXT", false, None, 1),
                ("lang", "TEXT", true, None, 0),
                ("skill_id", "TEXT", true, None, 0),
                ("kind", "TEXT", true, None, 0),
                ("target", "TEXT", true, None, 0),
                ("en", "TEXT", true, None, 0),
                ("lemmas", "TEXT", true, None, 0),
                ("cefr", "TEXT", true, None, 0),
                ("source_kind", "TEXT", true, None, 0),
                ("source_ref", "TEXT", false, None, 0),
                ("created_at", "INTEGER", true, None, 0),
                ("retired", "INTEGER", true, Some("0"), 0),
            ],
        ),
        (
            "accepted_answers",
            vec![
                ("item_id", "TEXT", true, None, 1),
                ("direction", "TEXT", true, None, 2),
                ("norm", "TEXT", true, None, 3),
                ("display", "TEXT", true, None, 0),
                ("source", "TEXT", true, None, 0),
                ("created_at", "INTEGER", true, None, 0),
            ],
        ),
        (
            "cards",
            vec![
                ("item_id", "TEXT", true, None, 1),
                ("facet", "TEXT", true, None, 2),
                ("state", "TEXT", true, None, 0),
                ("stability", "REAL", false, None, 0),
                ("difficulty", "REAL", false, None, 0),
                ("due", "INTEGER", false, None, 0),
                ("last_review", "INTEGER", false, None, 0),
                ("reps", "INTEGER", true, Some("0"), 0),
                ("lapses", "INTEGER", true, Some("0"), 0),
            ],
        ),
        (
            "review_log",
            vec![
                ("id", "INTEGER", false, None, 1),
                ("item_id", "TEXT", true, None, 0),
                ("facet", "TEXT", true, None, 0),
                ("reviewed_at", "INTEGER", true, None, 0),
                ("rating", "INTEGER", true, None, 0),
                ("exercise_type", "TEXT", true, None, 0),
                ("verdict", "TEXT", true, None, 0),
                ("response_ms", "INTEGER", false, None, 0),
                ("elapsed_days", "REAL", true, None, 0),
                ("stability_after", "REAL", true, None, 0),
                ("difficulty_after", "REAL", true, None, 0),
                ("session_id", "TEXT", true, None, 0),
            ],
        ),
    ]
}

/// The four indexes of §4 for those tables: (name, table, columns).
fn architecture_indexes() -> Vec<Index> {
    vec![
        ("cards_due", "cards", vec!["due"]),
        ("items_skill", "items", vec!["skill_id"]),
        (
            "review_log_item",
            "review_log",
            vec!["item_id", "reviewed_at"],
        ),
        ("skills_lang_ord", "skills", vec!["lang", "ord"]),
    ]
}

fn user_tables(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap();
    stmt.query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

fn table_info(conn: &Connection, table: &str) -> Vec<ColumnInfo> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap();
    stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)? != 0,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, i64>(5)?,
        ))
    })
    .unwrap()
    .map(|r| r.unwrap())
    .collect()
}

fn user_version(conn: &Connection) -> i64 {
    query_one(conn, "PRAGMA user_version")
}

// Bullet 1: an empty file gets the seven tables with §4's columns, their indexes and foreign
// keys. Since module 03 a new file also runs migration 0002 (five more tables), so it opens at
// user_version 2 with the seven tables among twelve; opening it again keeps version 2.
#[test]
fn t01_migration_creates_the_seven_tables_of_architecture_section_4() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parley.sqlite3");
    {
        let store = ok(Store::open(&path));
        assert_eq!(ok(store.user_version()), 2);
    }
    assert!(path.is_file(), "open creates the file");

    let conn = Connection::open(&path).unwrap();
    assert_eq!(user_version(&conn), 2);
    let expected_names: Vec<String> = architecture_tables()
        .iter()
        .map(|(name, _)| name.to_string())
        .collect();
    let tables = user_tables(&conn);
    assert_eq!(
        tables.len(),
        12,
        "the tables of 0001_init and 0002_sessions"
    );
    for name in &expected_names {
        assert!(tables.contains(name), "{name} of 0001_init is in the file");
    }

    for (table, columns) in architecture_tables() {
        let expected: Vec<ColumnInfo> = columns
            .iter()
            .map(|&(name, kind, not_null, default, pk)| {
                (
                    name.to_string(),
                    kind.to_string(),
                    not_null,
                    default.map(str::to_string),
                    pk,
                )
            })
            .collect();
        assert_eq!(
            table_info(&conn, table),
            expected,
            "PRAGMA table_info({table})"
        );
    }

    let mut stmt = conn
        .prepare("SELECT name, tbl_name FROM sqlite_master WHERE type = 'index' AND sql IS NOT NULL ORDER BY name")
        .unwrap();
    let indexes: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .filter(|(_, table): &(String, String)| expected_names.contains(table))
        .collect();
    drop(stmt);
    let expected_indexes: Vec<(String, String)> = architecture_indexes()
        .iter()
        .map(|(name, table, _)| (name.to_string(), table.to_string()))
        .collect();
    assert_eq!(indexes, expected_indexes);
    for (name, _, columns) in architecture_indexes() {
        let mut stmt = conn.prepare(&format!("PRAGMA index_info({name})")).unwrap();
        let actual: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(2))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(actual, columns, "index {name}");
    }

    for (table, column, parent) in [
        ("items", "skill_id", "skills"),
        ("accepted_answers", "item_id", "items"),
        ("cards", "item_id", "items"),
    ] {
        let mut stmt = conn
            .prepare(&format!("PRAGMA foreign_key_list({table})"))
            .unwrap();
        let keys: Vec<(String, String, String)> = stmt
            .query_map([], |row| Ok((row.get(2)?, row.get(3)?, row.get(4)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            keys,
            vec![(parent.to_string(), column.to_string(), "id".to_string())],
            "{table}.{column} references {parent}(id)"
        );
    }
    drop(conn);

    let again = ok(Store::open(&path));
    assert_eq!(
        ok(again.user_version()),
        2,
        "nothing is pending on a second open"
    );
    drop(again);
    let in_memory = ok(Store::open_in_memory());
    assert_eq!(ok(in_memory.user_version()), 2);
}

/// SQL as written, with `--` comments removed, whitespace collapsed and no space next to
/// parentheses or commas, so §4's layout and the stored text compare by content.
fn normalised_sql(sql: &str) -> String {
    let without_comments: Vec<&str> = sql
        .lines()
        .map(|line| line.split("--").next().unwrap_or(""))
        .collect();
    let collapsed = without_comments
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    collapsed
        .replace(" (", "(")
        .replace("( ", "(")
        .replace(" )", ")")
        .replace(" ,", ",")
        .replace(", ", ",")
        .trim_end_matches(';')
        .to_string()
}

/// The CREATE statements of docs/ARCHITECTURE.md §4, keyed by the object they create.
fn architecture_statements() -> Vec<(String, String)> {
    let doc = fs::read_to_string(repo_root().join("docs/ARCHITECTURE.md")).unwrap();
    let start = doc.find("```sql").expect("§4 has an sql block") + "```sql".len();
    let block = &doc[start..];
    let block = &block[..block.find("```").expect("the sql block ends")];
    let without_comments: Vec<&str> = block
        .lines()
        .map(|line| line.split("--").next().unwrap_or(""))
        .collect();
    without_comments
        .join("\n")
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|statement| {
            let words: Vec<&str> = statement.split_whitespace().collect();
            let name = if words[1] == "UNIQUE" {
                words[3]
            } else {
                words[2]
            };
            (name.to_string(), normalised_sql(statement))
        })
        .collect()
}

// Bullet 1 and Design notes: the SQL of 0001_init is §4's DDL for the seven tables and their
// indexes verbatim, so the CHECK constraints and references are those of the architecture.
#[test]
fn t01_stored_schema_is_architecture_section_4_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parley.sqlite3");
    drop(ok(Store::open(&path)));
    let conn = Connection::open(&path).unwrap();
    let statements = architecture_statements();
    let mut objects: Vec<&str> = architecture_tables()
        .iter()
        .map(|(name, _)| *name)
        .collect();
    objects.extend(architecture_indexes().iter().map(|(name, _, _)| *name));
    for object in objects {
        let expected = &statements
            .iter()
            .find(|(name, _)| name == object)
            .unwrap_or_else(|| panic!("§4 creates {object}"))
            .1;
        let stored: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = ?1",
                [object],
                |row| row.get(0),
            )
            .unwrap_or_else(|e| panic!("{object} is in the file: {e}"));
        assert_eq!(
            &normalised_sql(&stored),
            expected,
            "the stored SQL of {object}"
        );
    }
}

// Bullet 1: a file with a user_version above the newest migration is refused and left alone.
#[test]
fn t01_a_data_file_from_a_newer_parley_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parley.sqlite3");
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE later (x INTEGER); PRAGMA user_version = 99;")
            .unwrap();
    }
    assert_eq!(
        user_version(&Connection::open(&path).unwrap()),
        99,
        "set-up"
    );

    let e = store_error(Store::open(&path));
    assert!(matches!(e, StoreError::TooNew(99)), "TooNew(99), got: {e}");
    assert_eq!(
        e.to_string(),
        "This data file was written by a newer Parley. Update Parley to open it."
    );
    let conn = Connection::open(&path).unwrap();
    assert_eq!(user_version(&conn), 99);
    assert_eq!(user_tables(&conn), vec!["later".to_string()]);
}

// Bullet 1: a file that already holds one of 0001_init's tables fails migration 1 and is left
// unchanged (each migration runs in a transaction).
#[test]
fn t01_a_file_that_already_has_a_meta_table_fails_step_1() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parley.sqlite3");
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO meta VALUES ('kept', 'yes');",
        )
        .unwrap();
    }

    let e = store_error(Store::open(&path));
    assert!(
        matches!(e, StoreError::Migration(1)),
        "Migration(1), got: {e}"
    );
    assert_eq!(
        e.to_string(),
        "Parley could not update its data file (step 1). Restore a backup or report this."
    );
    let conn = Connection::open(&path).unwrap();
    assert_eq!(user_version(&conn), 0, "the version is unchanged");
    assert_eq!(
        user_tables(&conn),
        vec!["meta".to_string()],
        "no table was added"
    );
    let kept: String = query_one(&conn, "SELECT value FROM meta WHERE key = 'kept'");
    assert_eq!(kept, "yes");
}

// Interfaces: every store error displays its message.
#[test]
fn store_errors_display_their_messages() {
    assert_eq!(
        StoreError::Migration(3).to_string(),
        "Parley could not update its data file (step 3). Restore a backup or report this."
    );
    assert_eq!(
        StoreError::TooNew(2).to_string(),
        "This data file was written by a newer Parley. Update Parley to open it."
    );
    assert_eq!(
        StoreError::UnknownItem("es.a1.u01.s01#l:zzz".to_string()).to_string(),
        "unknown item es.a1.u01.s01#l:zzz"
    );
    assert_eq!(
        StoreError::UnknownSkill("es.a1.u09.s01".to_string()).to_string(),
        "unknown skill es.a1.u09.s01"
    );
    assert_eq!(
        StoreError::Sqlite("database disk image is malformed".to_string()).to_string(),
        "database disk image is malformed"
    );
}

// ---------------------------------------------------------------------------------------------
// 2. Course, items and cards
// ---------------------------------------------------------------------------------------------

// Bullet 2: start_course twice gives the same item count and keeps the first call's
// timestamps; introducing an item creates exactly two new cards; a version 2 without one
// example retires that item and keeps its two cards; the example coming back reactivates it.
#[test]
fn t02_start_course_is_idempotent_and_retires_removed_items() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parley.sqlite3");
    let mut store = ok(Store::open(&path));
    let forms = Forms::default();
    let v1 = mini();

    let first = ok(store.start_course(&v1, &forms, T0));
    assert_eq!(
        (first.items, first.added, first.retired),
        (MINI_ITEMS, MINI_ITEMS, 0)
    );
    assert_eq!(ok(store.item_count(Lang::Es)), MINI_ITEMS);
    let second = ok(store.start_course(&v1, &forms, T0 + DAY_MS));
    assert_eq!(
        (second.items, second.added, second.retired),
        (MINI_ITEMS, 0, 0)
    );
    assert_eq!(ok(store.item_count(Lang::Es)), MINI_ITEMS);

    let skills = ok(store.skills(Lang::Es));
    let ids: Vec<String> = skills.iter().map(|s| s.id.clone()).collect();
    assert_eq!(ids, skill_ids(0..80), "skills in syllabus order");
    assert!(skills[..40].iter().all(|s| s.cefr == Cefr::A1));
    assert!(skills[40..].iter().all(|s| s.cefr == Cefr::A2));
    assert_eq!(ok(store.item_ids(SKILL_0)), skill_0_items());

    {
        let conn = Connection::open(&path).unwrap();
        let (version, started): (i64, i64) = conn
            .query_row(
                "SELECT syllabus_version, started_at FROM courses WHERE lang = 'es'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (version, started),
            (1, T0),
            "the course keeps its first started_at"
        );
        let (low, high, count): (i64, i64, i64) = conn
            .query_row(
                "SELECT min(created_at), max(created_at), count(*) FROM items",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (low, high, count),
            (T0, T0, MINI_ITEMS as i64),
            "items keep their first created_at"
        );
        let from_syllabus: i64 = query_one(
            &conn,
            "SELECT count(*) FROM items WHERE source_kind = 'syllabus'",
        );
        assert_eq!(from_syllabus, MINI_ITEMS as i64);
        let ord: i64 = query_one(&conn, "SELECT ord FROM skills WHERE id = 'es.a1.u03.s04'");
        assert_eq!(ord, 23, "ord is the index in syllabus order");
        let cards: i64 = query_one(&conn, "SELECT count(*) FROM cards");
        assert_eq!(cards, 0, "no card before an item is introduced");
    }

    let item = format!("{SKILL_0}#e:4");
    assert!(
        ok(store.introduce(&item, T0 + HOUR)),
        "a first introduction"
    );
    let cards = ok(store.cards());
    assert_eq!(cards.len(), 2, "exactly two cards");
    assert_same_card(&cards[0], &Card::new(&item, Facet::Receptive), "introduced");
    assert_same_card(
        &cards[1],
        &Card::new(&item, Facet::Productive),
        "introduced",
    );
    assert!(
        !ok(store.introduce(&item, T0 + 2 * HOUR)),
        "already introduced"
    );
    assert_eq!(ok(store.cards()).len(), 2);

    let p = SrsParams::default();
    ok(store.review(
        &item,
        Facet::Receptive,
        Rating::Good,
        T0 + 3 * HOUR,
        &context(Rating::Good, "lesson-1"),
        &p,
    ));

    let mut v2 = v1.clone();
    v2.version = 2;
    v2.units[0].skills[0]
        .examples
        .retain(|example| example.id != 4);
    let third = ok(store.start_course(&v2, &forms, T0 + 2 * DAY_MS));
    assert_eq!((third.added, third.retired), (0, 1));
    assert_eq!(ok(store.item_count(Lang::Es)), MINI_ITEMS - 1);
    assert!(ok(store.is_retired(&item)));
    assert!(!ok(store.is_retired(&format!("{SKILL_0}#e:3"))));
    assert_eq!(ok(store.item_ids(SKILL_0)), skill_0_items()[..6].to_vec());
    let receptive =
        ok(store.card(&item, Facet::Receptive)).expect("the retired item keeps its receptive card");
    assert_eq!((receptive.state, receptive.reps), (CardState::Review, 1));
    assert!(
        ok(store.card(&item, Facet::Productive)).is_some(),
        "and its productive card"
    );
    assert_eq!(ok(store.review_log()).len(), 1, "and its log");
    {
        let conn = Connection::open(&path).unwrap();
        let (version, started): (i64, i64) = conn
            .query_row(
                "SELECT syllabus_version, started_at FROM courses WHERE lang = 'es'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (version, started),
            (2, T0),
            "the new version, the first started_at"
        );
    }

    let mut v3 = v1.clone();
    v3.version = 3;
    let fourth = ok(store.start_course(&v3, &forms, T0 + 3 * DAY_MS));
    assert_eq!(fourth.retired, 0);
    assert!(
        !ok(store.is_retired(&item)),
        "an item that comes back is active again"
    );
    assert_eq!(ok(store.item_count(Lang::Es)), MINI_ITEMS);
    assert_eq!(ok(store.item_ids(SKILL_0)), skill_0_items());
}

// Bullet 2 (error paths): an item id that is not in `items` gives UnknownItem, for introduce
// and for review, and nothing is written.
#[test]
fn t02_an_unknown_item_is_refused() {
    let mut store = course_store();
    let unknown = format!("{SKILL_0}#l:zzz");
    let e = store_error(store.introduce(&unknown, T0));
    assert!(
        matches!(&e, StoreError::UnknownItem(id) if *id == unknown),
        "{e}"
    );
    assert_eq!(e.to_string(), format!("unknown item {unknown}"));

    let e = store_error(store.review(
        &unknown,
        Facet::Receptive,
        Rating::Good,
        T0,
        &context(Rating::Good, "lesson-1"),
        &SrsParams::default(),
    ));
    assert!(
        matches!(&e, StoreError::UnknownItem(id) if *id == unknown),
        "{e}"
    );
    assert!(ok(store.cards()).is_empty());
    assert!(ok(store.review_log()).is_empty());
}

/// (direction, norm, display, source) of an item's accepted answers, sorted.
fn accepted_answers(conn: &Connection, item_id: &str) -> Vec<(String, String, String, String)> {
    let mut stmt = conn
        .prepare(
            "SELECT direction, norm, display, source FROM accepted_answers WHERE item_id = ?1
             ORDER BY direction, norm, source",
        )
        .unwrap();
    stmt.query_map([item_id], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })
    .unwrap()
    .map(|r| r.unwrap())
    .collect()
}

fn answer(
    direction: &str,
    norm: &str,
    display: &str,
    source: &str,
) -> (String, String, String, String) {
    (
        direction.to_string(),
        norm.to_string(),
        display.to_string(),
        source.to_string(),
    )
}

// Design notes: start_course writes the `content` accepted answers of every item (norm = NFC
// lower case of display) and rewrites them on every call; `judge` and `learner` rows are kept.
#[test]
fn start_course_rewrites_content_answers_and_keeps_the_others() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, path) = course_store_file(dir.path());
    let lemma_item = format!("{SKILL_0}#l:wabak");
    let example_item = format!("{SKILL_0}#e:1");
    {
        let conn = Connection::open(&path).unwrap();
        assert_eq!(
            accepted_answers(&conn, &lemma_item),
            vec![
                answer("to_en", "word 0", "word 0", "content"),
                answer("to_target", "wabak", "wabak", "content"),
            ]
        );
        assert_eq!(
            accepted_answers(&conn, &example_item),
            vec![
                answer("to_en", "word 0 word 1.", "Word 0 word 1.", "content"),
                answer("to_target", "wabak wabek.", "Wabak wabek.", "content"),
            ]
        );
        conn.execute(
            "INSERT INTO accepted_answers (item_id, direction, norm, display, source, created_at)
             VALUES (?1, 'to_en', 'my word', 'My word', 'learner', ?2)",
            rusqlite::params![lemma_item, T0],
        )
        .unwrap();
    }

    let mut v2 = mini();
    v2.version = 2;
    v2.units[0].skills[0].lemmas[0].en = vec!["word zero".to_string()];
    ok(store.start_course(&v2, &Forms::default(), T0 + DAY_MS));
    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        accepted_answers(&conn, &lemma_item),
        vec![
            answer("to_en", "my word", "My word", "learner"),
            answer("to_en", "word zero", "word zero", "content"),
            answer("to_target", "wabak", "wabak", "content"),
        ],
        "the content rows follow the syllabus; the learner's row stays"
    );
}

// Design notes: a skill gone from the syllabus keeps its row with ord -1, and its items are
// retired.
#[test]
fn a_skill_gone_from_the_syllabus_keeps_its_row() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, path) = course_store_file(dir.path());
    let last = skill_id(79);
    let last_items = ok(store.item_ids(&last));
    assert_eq!(last_items.len(), 7);
    let mut v2 = mini();
    v2.version = 2;
    v2.units[7].skills.pop();
    let report = ok(store.start_course(&v2, &Forms::default(), T0 + DAY_MS));
    assert_eq!(report.retired, 7);
    assert_eq!(ok(store.item_count(Lang::Es)), MINI_ITEMS - 7);
    for item in &last_items {
        assert!(ok(store.is_retired(item)), "{item}");
    }
    let conn = Connection::open(&path).unwrap();
    let ord: i64 = query_one(
        &conn,
        &format!("SELECT ord FROM skills WHERE id = '{last}'"),
    );
    assert_eq!(ord, -1);
    let second_last: i64 = query_one(
        &conn,
        &format!("SELECT ord FROM skills WHERE id = '{}'", skill_id(78)),
    );
    assert_eq!(second_last, 78);
}

// Interfaces (error path): a skill id the course does not have gives UnknownSkill.
#[test]
fn an_unknown_skill_is_refused() {
    let store = course_store();
    let e = store_error(skill_state(&store, "es.a1.u09.s01", T0));
    assert!(
        matches!(&e, StoreError::UnknownSkill(id) if id == "es.a1.u09.s01"),
        "{e}"
    );
    assert_eq!(e.to_string(), "unknown skill es.a1.u09.s01");
    let e = store_error(skill_mastery(&store, "es.a1.u09.s01", T0));
    assert!(
        matches!(&e, StoreError::UnknownSkill(id) if id == "es.a1.u09.s01"),
        "{e}"
    );
}

// ---------------------------------------------------------------------------------------------
// 3 to 5. Scheduling
// ---------------------------------------------------------------------------------------------

// Bullet 3: a new card's first review sets stability to w[0] to w[3] for Again to Easy (FSRS's
// initial stability) and difficulty to D0(G) = w[4] − e^(w[5]·(G − 1)) + 1 clamped to 1..10.
#[test]
fn t03_first_review_sets_the_initial_stability() {
    let w = weights();
    let p = SrsParams::default();
    let expected_intervals = [1u32, 1, 2, 8];
    for (i, rating) in [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy]
        .into_iter()
        .enumerate()
    {
        let out = schedule(&Card::new("x", Facet::Receptive), rating, T0, &p);
        let stability = out.card.stability.expect("a reviewed card has a stability");
        assert!(
            (stability - w[i]).abs() < 1e-9,
            "{rating:?}: stability {stability}, w[{i}] = {}",
            w[i]
        );
        let g = (i + 1) as f64;
        let d0 = (w[4] - (w[5] * (g - 1.0)).exp() + 1.0).clamp(1.0, 10.0);
        let difficulty = out
            .card
            .difficulty
            .expect("a reviewed card has a difficulty");
        assert!(
            (difficulty - d0).abs() < 1e-5,
            "{rating:?}: difficulty {difficulty}, D0 {d0}"
        );
        assert_eq!(out.elapsed_days, 0.0, "a new card has 0 days elapsed");
        assert_eq!(
            out.interval_days, expected_intervals[i],
            "max(1, round(w[{i}]))"
        );
        assert_eq!(
            out.card.due,
            Some(T0 + i64::from(expected_intervals[i]) * DAY_MS)
        );
        assert_eq!(out.card.last_review, Some(T0));
        assert_eq!((out.card.reps, out.card.lapses), (1, 0));
        let state = if rating == Rating::Again {
            CardState::Relearning
        } else {
            CardState::Review
        };
        assert_eq!(out.card.state, state);
    }
    assert_eq!(
        default_weights(),
        w,
        "default_weights is the crate's vector widened to f64"
    );
}

// Bullet 4: at retention 0.90 and no fuzz every interval is max(1, round(S)) days, capped at
// 365, and due = now + interval days (1,000 seeded rating sequences; reviews from 36 hours early
// to 72 hours late, so same-day reviews occur too).
#[test]
fn t04_intervals_equal_rounded_stability_at_retention_0_90() {
    let p = SrsParams::default();
    assert_eq!(p.desired_retention, 0.90);
    let mut reviews = 0;
    let mut capped = 0;
    for seed in 0..1_000u64 {
        let mut rng = SplitMix64::new(seed);
        let mut card = Card::new(&format!("seq-{seed}"), Facet::Receptive);
        let mut now = T0;
        let length = 1 + rng.next_u64() % 12;
        for _ in 0..length {
            let rating = rating_of(rng.next_u64());
            if let (Some(due), Some(last)) = (card.due, card.last_review) {
                let offset_hours = (rng.next_u64() % 109) as i64 - 36;
                now = (due + offset_hours * HOUR).max(last);
            }
            let elapsed = card
                .last_review
                .map_or(0, |last| (now - last).div_euclid(DAY_MS));
            let out = schedule(&card, rating, now, &p);
            let stability = out.card.stability.expect("stability after a review");
            assert!(
                interval_matches(out.interval_days, stability),
                "seed {seed}: interval {} for S = {stability}",
                out.interval_days
            );
            assert!((1..=MAX_INTERVAL_DAYS).contains(&out.interval_days));
            assert_eq!(
                out.card.due,
                Some(now + i64::from(out.interval_days) * DAY_MS),
                "seed {seed}"
            );
            assert_eq!(
                out.elapsed_days, elapsed as f64,
                "seed {seed}: whole days elapsed"
            );
            if out.interval_days == MAX_INTERVAL_DAYS {
                capped += 1;
            }
            card = out.card;
            reviews += 1;
        }
    }
    assert!(reviews >= 5_000, "{reviews} reviews checked");
    assert!(capped > 0, "some sequence reaches the 365-day cap");
    assert_eq!(MAX_INTERVAL_DAYS, 365);
    assert_eq!(DAY_MS, 86_400_000);
}

fn read_reference() -> Value {
    let text = fs::read_to_string(repo_root().join(REFERENCE))
        .unwrap_or_else(|e| panic!("{REFERENCE}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{REFERENCE} is not JSON: {e}"))
}

fn rating_from_number(n: u64) -> Rating {
    match n {
        1 => Rating::Again,
        2 => Rating::Hard,
        3 => Rating::Good,
        4 => Rating::Easy,
        other => panic!("rating {other} is not 1 to 4"),
    }
}

// Bullet 5: the five py-fsrs scenarios: stability and difficulty after each review within
// relative tolerance 1e-4 (the crate works in f32, py-fsrs in f64), and the same whole days
// elapsed, interval and due time.
#[test]
fn t05_reference_scenarios_match_py_fsrs() {
    let reference = read_reference();
    assert_eq!(reference["py_fsrs_version"], "6.3.2");
    assert_eq!(reference["fsrs_rs_version"], "6.6.2");
    let recorded = reference["parameters"].as_array().expect("parameters");
    assert_eq!(recorded.len(), 21);
    for (i, (value, crate_value)) in recorded
        .iter()
        .zip(fsrs::DEFAULT_PARAMETERS.iter())
        .enumerate()
    {
        assert_eq!(
            value.as_f64().unwrap() as f32,
            *crate_value,
            "parameter w[{i}]"
        );
    }
    assert_eq!(reference["scheduler"]["desired_retention"], json!(0.9));
    assert_eq!(reference["scheduler"]["maximum_interval"], json!(365));
    assert_eq!(reference["scheduler"]["enable_fuzzing"], json!(false));

    let p = SrsParams::default();
    let scenarios = reference["scenarios"].as_array().expect("scenarios");
    assert_eq!(scenarios.len(), 5);
    let mut reviews = 0;
    for scenario in scenarios {
        let name = scenario["name"].as_str().unwrap();
        for recorded_card in scenario["cards"].as_array().unwrap() {
            let item_id = recorded_card["item_id"].as_str().unwrap();
            let mut card = Card::new(item_id, Facet::Receptive);
            for (n, review) in recorded_card["reviews"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
            {
                let what = format!("{name} / {item_id} review {}", n + 1);
                let at = review["reviewed_at"].as_i64().unwrap();
                let rating = rating_from_number(review["rating"].as_u64().unwrap());
                let out = schedule(&card, rating, at, &p);
                assert_relative(
                    out.card.stability.unwrap(),
                    review["stability"].as_f64().unwrap(),
                    1e-4,
                    &format!("{what}: stability"),
                );
                assert_relative(
                    out.card.difficulty.unwrap(),
                    review["difficulty"].as_f64().unwrap(),
                    1e-4,
                    &format!("{what}: difficulty"),
                );
                assert_eq!(
                    out.elapsed_days,
                    review["elapsed_days"].as_f64().unwrap(),
                    "{what}: days elapsed"
                );
                assert_eq!(
                    i64::from(out.interval_days),
                    review["interval_days"].as_i64().unwrap(),
                    "{what}: interval"
                );
                assert_eq!(out.card.due, review["due"].as_i64(), "{what}: due");
                assert_eq!(out.card.last_review, Some(at), "{what}: last review");
                card = out.card;
                reviews += 1;
            }
        }
    }
    assert_eq!(reviews, 36, "every recorded review was replayed");
}

// Design notes: stability, difficulty, state, counts and last review depend on the log alone;
// the desired retention moves due dates only, by FSRS's interval S / f × (r^(−1/d) − 1).
#[test]
fn desired_retention_moves_due_dates_only() {
    let base = SrsParams::default();
    let low = SrsParams::with_retention(0.80);
    let mut a = Card::new("r", Facet::Productive);
    let mut b = Card::new("r", Facet::Productive);
    let times = [
        T0,
        T0 + 3 * DAY_MS,
        T0 + 10 * DAY_MS + 5 * HOUR,
        T0 + 40 * DAY_MS,
        T0 + 41 * DAY_MS,
    ];
    let ratings = [
        Rating::Good,
        Rating::Good,
        Rating::Hard,
        Rating::Again,
        Rating::Easy,
    ];
    let mut moved = 0;
    for (&at, &rating) in times.iter().zip(&ratings) {
        let out_a = schedule(&a, rating, at, &base);
        let out_b = schedule(&b, rating, at, &low);
        assert_eq!(bits(out_a.card.stability), bits(out_b.card.stability));
        assert_eq!(bits(out_a.card.difficulty), bits(out_b.card.difficulty));
        assert_eq!(out_a.card.state, out_b.card.state);
        assert_eq!(
            (out_a.card.reps, out_a.card.lapses),
            (out_b.card.reps, out_b.card.lapses)
        );
        assert_eq!(out_a.card.last_review, out_b.card.last_review);
        let stability = out_b.card.stability.unwrap();
        assert!(
            interval_matches(out_b.interval_days, interval_at(stability, 0.80)),
            "interval {} at 0.80 for S = {stability}",
            out_b.interval_days
        );
        assert_eq!(
            out_b.card.due,
            Some(at + i64::from(out_b.interval_days) * DAY_MS)
        );
        if out_a.interval_days != out_b.interval_days {
            moved += 1;
        }
        a = out_a.card;
        b = out_b.card;
    }
    assert!(moved > 0, "a lower retention gives longer intervals");
}

// ---------------------------------------------------------------------------------------------
// 6. Rating mapping
// ---------------------------------------------------------------------------------------------

fn outcome(
    verdict: Verdict,
    used_hint: bool,
    exercise_type: &str,
    response_ms: Option<i64>,
    learner_override: bool,
) -> Outcome {
    Outcome {
        verdict,
        used_hint,
        exercise_type: exercise_type.to_string(),
        response_ms,
        learner_override,
    }
}

// Bullet 6: each row of the Design notes table, first matching row wins.
#[test]
fn t06_rating_for_returns_each_row_of_the_table() {
    use Verdict::{Almost, Correct, Wrong};
    let rows = [
        (
            "override",
            outcome(Wrong, true, "translate_to_target", Some(9_000), true),
            Rating::Good,
        ),
        (
            "wrong",
            outcome(Wrong, false, "pick_meaning", Some(1_000), false),
            Rating::Again,
        ),
        (
            "almost",
            outcome(Almost, false, "pick_meaning", Some(1_000), false),
            Rating::Hard,
        ),
        (
            "correct with a hint",
            outcome(Correct, true, "pick_meaning", Some(1_000), false),
            Rating::Hard,
        ),
        (
            "fast pick_meaning",
            outcome(Correct, false, "pick_meaning", Some(2_500), false),
            Rating::Easy,
        ),
        (
            "fast match_pairs",
            outcome(Correct, false, "match_pairs", Some(800), false),
            Rating::Easy,
        ),
        (
            "correct",
            outcome(Correct, false, "translate_to_en", Some(1_000), false),
            Rating::Good,
        ),
    ];
    for (name, outcome, expected) in &rows {
        assert_eq!(rating_for(outcome), *expected, "{name}");
    }

    let more = [
        (
            "override beats almost",
            outcome(Almost, false, "pick_meaning", Some(1_000), true),
            Rating::Good,
        ),
        (
            "override beats a hint",
            outcome(Correct, true, "pick_meaning", Some(1_000), true),
            Rating::Good,
        ),
        (
            "2,501 ms is not fast",
            outcome(Correct, false, "pick_meaning", Some(2_501), false),
            Rating::Good,
        ),
        (
            "no response time never gives Easy",
            outcome(Correct, false, "pick_meaning", None, false),
            Rating::Good,
        ),
        (
            "no response time, match_pairs",
            outcome(Correct, false, "match_pairs", None, false),
            Rating::Good,
        ),
        (
            "a wrong answer stays Again when fast",
            outcome(Wrong, false, "match_pairs", Some(500), false),
            Rating::Again,
        ),
    ];
    for (name, outcome, expected) in &more {
        assert_eq!(rating_for(outcome), *expected, "{name}");
    }
}

// ---------------------------------------------------------------------------------------------
// 7. Review log and replay
// ---------------------------------------------------------------------------------------------

/// The first 40 item ids of the mini course in syllabus order.
fn forty_items(store: &Store) -> Vec<String> {
    let mut ids = Vec::new();
    for skill in skill_ids(0..80) {
        ids.extend(ok(store.item_ids(&skill)));
        if ids.len() >= 40 {
            break;
        }
    }
    ids.truncate(40);
    ids
}

// Bullet 7: after 500 seeded reviews over 40 items, replay(review_log) equals the reviewed cards
// of the cards table exactly; the log only ever grows (invariant I6) and each row carries the
// review's context and the card's new stability and difficulty.
#[test]
fn t07_replay_of_the_review_log_equals_the_cards_table() {
    let mut store = course_store();
    let p = SrsParams::default();
    let items = forty_items(&store);
    assert_eq!(items.len(), 40);
    let mut rng = SplitMix64::new(2026);
    let mut now = T0;
    let mut first_half = Vec::new();
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
            exercise_type: if n.is_multiple_of(2) {
                "pick_meaning"
            } else {
                "translate_to_target"
            }
            .to_string(),
            verdict: verdict_for(rating),
            response_ms: if n % 3 == 0 {
                None
            } else {
                Some(1_000 + n as i64)
            },
            session_id: format!("session-{}", n / 25),
        };
        let returned = ok(store.review(item, facet, rating, now, &ctx, &p));
        let stored = ok(store.card(item, facet)).expect("a reviewed card is stored");
        assert_same_card(&returned, &stored, "review returns the stored card");
        let log = ok(store.review_log());
        assert_eq!(log.len() as u64, n + 1, "one row per review");
        let last = log.last().unwrap();
        assert_eq!(
            (last.item_id.as_str(), last.facet, last.reviewed_at),
            (item.as_str(), facet, now)
        );
        assert_eq!(last.rating, rating);
        assert_eq!(last.exercise_type, ctx.exercise_type);
        assert_eq!(last.verdict, ctx.verdict);
        assert_eq!(last.response_ms, ctx.response_ms);
        assert_eq!(last.session_id, ctx.session_id);
        assert_eq!(
            last.stability_after.to_bits(),
            stored.stability.unwrap().to_bits()
        );
        assert_eq!(
            last.difficulty_after.to_bits(),
            stored.difficulty.unwrap().to_bits()
        );
        assert_eq!(last.elapsed_days, last.elapsed_days.floor(), "whole days");
        if n == 249 {
            first_half = log;
        }
    }

    let log = ok(store.review_log());
    assert_eq!(log.len(), 500);
    for (early, late) in first_half.iter().zip(&log) {
        assert_eq!(
            (
                early.item_id.as_str(),
                early.facet,
                early.reviewed_at,
                early.rating
            ),
            (
                late.item_id.as_str(),
                late.facet,
                late.reviewed_at,
                late.rating
            ),
            "earlier rows are never changed"
        );
        assert_eq!(
            early.stability_after.to_bits(),
            late.stability_after.to_bits()
        );
    }
    assert!(
        log.windows(2).all(|w| w[0].reviewed_at <= w[1].reviewed_at),
        "log order"
    );

    let reviewed: Vec<Card> = ok(store.cards())
        .into_iter()
        .filter(|c| c.reps > 0)
        .collect();
    assert!(reviewed.len() > 60, "most of the 80 cards were reviewed");
    let replayed = replay(&log, &p);
    assert_same_cards(&replayed, &reviewed, "replay against the cards table");
    let total: u32 = replayed.iter().map(|c| c.reps).sum();
    assert_eq!(total, 500);
}

fn log_entry(item_id: &str, facet: Facet, reviewed_at: i64, rating: Rating) -> ReviewLogEntry {
    ReviewLogEntry {
        item_id: item_id.to_string(),
        facet,
        reviewed_at,
        rating,
        exercise_type: "translate_to_en".to_string(),
        verdict: verdict_for(rating),
        response_ms: None,
        // Recorded values are ignored: replay recomputes them.
        elapsed_days: 99.0,
        stability_after: 999.0,
        difficulty_after: 999.0,
        session_id: "replay".to_string(),
    }
}

// Design notes: replay starts every (item, facet) from Card::new, applies schedule in log
// order, and returns one card per (item, facet) by item id in byte order, receptive first.
#[test]
fn replay_recomputes_every_card_and_orders_by_item_id_bytes() {
    let p = SrsParams::default();
    let log = vec![
        log_entry("b", Facet::Productive, T0, Rating::Good),
        log_entry("é", Facet::Receptive, T0, Rating::Easy),
        log_entry("a", Facet::Productive, T0 + HOUR, Rating::Hard),
        log_entry("Z", Facet::Receptive, T0 + HOUR, Rating::Again),
        log_entry("a", Facet::Receptive, T0 + 2 * HOUR, Rating::Good),
        log_entry("b", Facet::Productive, T0 + 3 * DAY_MS, Rating::Again),
        log_entry("a", Facet::Productive, T0 + 4 * DAY_MS, Rating::Good),
    ];
    let cards = replay(&log, &p);
    let order: Vec<(&str, Facet)> = cards
        .iter()
        .map(|c| (c.item_id.as_str(), c.facet))
        .collect();
    assert_eq!(
        order,
        vec![
            ("Z", Facet::Receptive),
            ("a", Facet::Receptive),
            ("a", Facet::Productive),
            ("b", Facet::Productive),
            ("é", Facet::Receptive),
        ]
    );
    for card in &cards {
        let mut expected = Card::new(&card.item_id, card.facet);
        for entry in log
            .iter()
            .filter(|e| e.item_id == card.item_id && e.facet == card.facet)
        {
            expected = schedule(&expected, entry.rating, entry.reviewed_at, &p).card;
        }
        assert_same_card(card, &expected, "replay");
    }
    let b = &cards[3];
    assert_eq!(
        (b.state, b.reps, b.lapses),
        (CardState::Relearning, 2, 1),
        "Again on a reviewed card is a lapse"
    );
    let z = &cards[0];
    assert_eq!(
        (z.state, z.reps, z.lapses),
        (CardState::Relearning, 1, 0),
        "Again on a new card is not"
    );
    assert!(replay(&[], &p).is_empty());
}

// Design notes: Again leaves a card relearning and counts a lapse only on a card in state
// review; every review counts a rep.
#[test]
fn card_state_follows_the_rating() {
    let p = SrsParams::default();
    let steps = [
        (Rating::Again, CardState::Relearning, 0),
        (Rating::Again, CardState::Relearning, 0),
        (Rating::Good, CardState::Review, 0),
        (Rating::Again, CardState::Relearning, 1),
        (Rating::Hard, CardState::Review, 1),
        (Rating::Again, CardState::Relearning, 2),
        (Rating::Easy, CardState::Review, 2),
    ];
    let mut card = Card::new("s", Facet::Receptive);
    let mut now = T0;
    for (n, (rating, state, lapses)) in steps.into_iter().enumerate() {
        card = schedule(&card, rating, now, &p).card;
        assert_eq!(card.state, state, "step {n}");
        assert_eq!(card.lapses, lapses, "step {n}");
        assert_eq!(card.reps as usize, n + 1, "step {n}");
        assert_eq!(card.last_review, Some(now));
        now = card.due.unwrap();
    }
}

// ---------------------------------------------------------------------------------------------
// 8 and 9. Retrievability, queue and mastery
// ---------------------------------------------------------------------------------------------

// Design notes: retrievability is the forgetting curve over fractional days; a card without
// stability has 0; a review time after now counts as 0 days.
#[test]
fn t08_retrievability_follows_the_forgetting_curve() {
    let now = T0 + 100 * DAY_MS;
    assert_eq!(retrievability(&Card::new("n", Facet::Receptive), now), 0.0);
    let at_s = reviewed_card("a", Facet::Receptive, 10.0, 10.0, now, now);
    assert!(
        (retrievability(&at_s, now) - 0.9).abs() < 1e-12,
        "R(S, S) = 0.9"
    );
    let fractional = reviewed_card("b", Facet::Receptive, 3.0, 1.5, now, now);
    assert!((retrievability(&fractional, now) - forgetting_curve(1.5, 3.0)).abs() < 1e-12);
    let future = reviewed_card("c", Facet::Receptive, 3.0, -2.0, now, now);
    assert!(
        (retrievability(&future, now) - 1.0).abs() < 1e-12,
        "t never below 0"
    );
}

// Bullet 8: five due cards with known stability and elapsed days come out in ascending
// retrievability computed from the FSRS-6 forgetting curve at the crate's parameters.
#[test]
fn t08_due_cards_are_queued_most_at_risk_first() {
    let now = T0 + 500 * DAY_MS;
    // (item, stability, days since the last review, interval days): every card is due.
    let fixture = [
        ("q.a", 2.0, 3.0, 2),
        ("q.b", 10.0, 10.0, 10),
        ("q.c", 30.0, 45.5, 30),
        ("q.d", 1.2, 1.5, 1),
        ("q.e", 100.0, 400.0, 100),
    ];
    let cards: Vec<Card> = fixture
        .iter()
        .map(|&(id, s, t, interval)| {
            let last = now - (t * DAY_MS as f64).round() as i64;
            Card {
                due: Some(last + interval * DAY_MS),
                ..reviewed_card(id, Facet::Receptive, s, t, now, now)
            }
        })
        .collect();
    let mut expected: Vec<(&str, f64)> = fixture
        .iter()
        .map(|&(id, s, t, _)| (id, forgetting_curve(t, s)))
        .collect();
    expected.sort_by(|a, b| a.1.total_cmp(&b.1));
    let expected_ids: Vec<&str> = expected.iter().map(|(id, _)| *id).collect();
    // By hand from the curve: R ≈ 0.782, 0.869, 0.870, 0.884, 0.900.
    assert_eq!(expected_ids, ["q.e", "q.c", "q.a", "q.d", "q.b"]);

    let queue = queue_order(&cards, now, 10);
    let ids: Vec<&str> = queue.iter().map(|c| c.item_id.as_str()).collect();
    assert_eq!(ids, expected_ids);
    for (entry, (id, r)) in queue.iter().zip(&expected) {
        assert!(
            (entry.retrievability - r).abs() < 1e-9,
            "{id}: {} against {r}",
            entry.retrievability
        );
        let card = cards.iter().find(|c| c.item_id == *id).unwrap();
        assert_eq!(Some(entry.due), card.due);
        assert_eq!(entry.facet, Facet::Receptive);
    }
    let top = queue_order(&cards, now, 3);
    let top_ids: Vec<&str> = top.iter().map(|c| c.item_id.as_str()).collect();
    assert_eq!(top_ids, expected_ids[..3].to_vec(), "at most limit cards");
}

// Design notes: equal retrievability is ordered by due, then item id, then receptive first.
#[test]
fn t08_queue_ties_break_by_due_then_item_then_facet() {
    let now = T0 + 50 * DAY_MS;
    let card = |id: &str, facet: Facet, due: i64| reviewed_card(id, facet, 5.0, 6.0, now, due);
    let cards = vec![
        card("t.b", Facet::Receptive, now - DAY_MS),
        card("t.a", Facet::Productive, now - DAY_MS),
        card("t.c", Facet::Receptive, now - 2 * DAY_MS),
        card("t.a", Facet::Receptive, now - DAY_MS),
    ];
    let queue = queue_order(&cards, now, 10);
    let order: Vec<(&str, Facet)> = queue
        .iter()
        .map(|c| (c.item_id.as_str(), c.facet))
        .collect();
    assert_eq!(
        order,
        vec![
            ("t.c", Facet::Receptive),
            ("t.a", Facet::Receptive),
            ("t.a", Facet::Productive),
            ("t.b", Facet::Receptive),
        ]
    );
}

// Design notes: the due queue holds cards due by now, of items not retired, in the course's
// language, at most `limit`, most at risk first.
#[test]
fn t08_due_queue_from_the_store() {
    let mut store = course_store();
    let p = SrsParams::default();
    let items = skill_0_items();
    let ratings = [
        Rating::Again,
        Rating::Hard,
        Rating::Good,
        Rating::Easy,
        Rating::Good,
        Rating::Hard,
        Rating::Again,
    ];
    for (n, (item, rating)) in items.iter().zip(ratings).enumerate() {
        let at = T0 + n as i64 * HOUR;
        ok(store.review(
            item,
            Facet::Receptive,
            rating,
            at,
            &context(rating, "lesson-1"),
            &p,
        ));
        ok(store.review(
            item,
            Facet::Productive,
            Rating::Good,
            at,
            &context(Rating::Good, "lesson-1"),
            &p,
        ));
    }
    // Retire the last example, which was rated Again and is due.
    let retired = &items[6];
    let mut v2 = mini();
    v2.version = 2;
    v2.units[0].skills[0]
        .examples
        .retain(|example| example.id != 4);
    ok(store.start_course(&v2, &Forms::default(), T0 + 2 * HOUR));
    assert!(ok(store.is_retired(retired)));

    let now = T0 + 2 * DAY_MS + 3 * HOUR;
    let mut expected: Vec<(String, Facet, i64, f64)> = ok(store.cards())
        .into_iter()
        .filter(|c| c.due.is_some_and(|due| due <= now) && c.item_id != *retired)
        .map(|c| {
            let t = (now - c.last_review.unwrap()) as f64 / DAY_MS as f64;
            let r = forgetting_curve(t, c.stability.unwrap());
            (c.item_id, c.facet, c.due.unwrap(), r)
        })
        .collect();
    let facet_rank = |f: Facet| if f == Facet::Receptive { 0 } else { 1 };
    expected.sort_by(|a, b| {
        a.3.total_cmp(&b.3)
            .then(a.2.cmp(&b.2))
            .then(a.0.as_bytes().cmp(b.0.as_bytes()))
            .then(facet_rank(a.1).cmp(&facet_rank(b.1)))
    });
    assert!(
        expected.len() >= 8,
        "most cards are due: {}",
        expected.len()
    );
    assert!(expected.len() < 14, "some are not");

    let queue = ok(due_queue(&store, Lang::Es, now, 100));
    let actual: Vec<(&str, Facet)> = queue
        .iter()
        .map(|c| (c.item_id.as_str(), c.facet))
        .collect();
    let wanted: Vec<(&str, Facet)> = expected.iter().map(|e| (e.0.as_str(), e.1)).collect();
    assert_eq!(actual, wanted);
    for (entry, e) in queue.iter().zip(&expected) {
        assert!((entry.retrievability - e.3).abs() < 1e-9);
        assert_eq!(entry.due, e.2);
    }
    assert!(
        queue.iter().all(|c| c.item_id != *retired),
        "retired items are never queued"
    );
    assert_eq!(ok(due_queue(&store, Lang::Es, now, 3)).len(), 3);
    assert!(
        ok(due_queue(&store, Lang::Fr, now, 100)).is_empty(),
        "another language has no cards"
    );
}

// Bullet 9: a skill of 4 items where 2 items have cards of retrievability 0.9, 0.8, 0.7 and 0.6
// has mastery (0.9 + 0.8 + 0.7 + 0.6) / 8 = 0.375; a new card counts 0.
#[test]
fn t09_mastery_is_mean_retrievability_over_twice_the_items() {
    let now = T0 + 30 * DAY_MS;
    let t = 10.0;
    let cards: Vec<Card> = [
        ("m.1", Facet::Receptive, 0.9),
        ("m.1", Facet::Productive, 0.8),
        ("m.2", Facet::Receptive, 0.7),
        ("m.2", Facet::Productive, 0.6),
    ]
    .iter()
    .map(|&(id, facet, r)| reviewed_card(id, facet, stability_for(r, t), t, now, now))
    .chain([
        Card::new("m.3", Facet::Receptive),
        Card::new("m.3", Facet::Productive),
    ])
    .collect();
    for (card, r) in cards.iter().zip([0.9, 0.8, 0.7, 0.6, 0.0, 0.0]) {
        assert!(
            (retrievability(card, now) - r).abs() < 1e-12,
            "{} {:?}",
            card.item_id,
            card.facet
        );
    }
    let mastery = mastery_of(&cards, 4, now);
    assert!((mastery - 0.375).abs() < 1e-9, "mastery {mastery}");
}

// Design notes: skill_mastery sums the retrievability of the skill's cards over twice its items.
#[test]
fn t09_skill_mastery_from_the_store() {
    let mut store = course_store();
    let p = SrsParams::default();
    for item in skill_0_items() {
        ok(store.review(
            &item,
            Facet::Receptive,
            Rating::Good,
            T0,
            &context(Rating::Good, "lesson-1"),
            &p,
        ));
    }
    // A review introduces an item that has no cards yet: its other facet is a new card.
    let untouched = ok(store.card(&skill_0_items()[0], Facet::Productive))
        .expect("review introduced the item's productive card");
    assert_same_card(
        &untouched,
        &Card::new(&skill_0_items()[0], Facet::Productive),
        "introduced by review",
    );
    let now = T0 + 5 * DAY_MS;
    // Each card has S = w[2] (a first Good), reviewed 5 days ago; productive cards count 0.
    let expected = 7.0 * forgetting_curve(5.0, weights()[2]) / 14.0;
    let mastery = ok(skill_mastery(&store, SKILL_0, now));
    assert!(
        (mastery - expected).abs() < 1e-9,
        "mastery {mastery}, expected {expected}"
    );
    assert_eq!(ok(skill_mastery(&store, &skill_id(1), now)), 0.0);
}

// ---------------------------------------------------------------------------------------------
// 10. Skill states
// ---------------------------------------------------------------------------------------------

// Bullet 10: on syllabus-mini, skill 0 starts available and skill 1 locked; one reviewed item
// makes skill 0 learning; all its items, practised, and skill 1 available. Then: every card
// reviewed is mastered while retrievability is high, and practised again once it decays; an
// introduced item makes a skill learning.
#[test]
fn t10_skill_states_on_the_mini_syllabus() {
    let mut store = course_store();
    let p = SrsParams::default();
    let state =
        |store: &Store, index: usize, now: i64| ok(skill_state(store, &skill_id(index), now));
    assert_eq!(
        state(&store, 0, T0),
        SkillState::Available,
        "the first skill is always available"
    );
    assert_eq!(state(&store, 1, T0), SkillState::Locked);
    assert_eq!(state(&store, 2, T0), SkillState::Locked);

    let items = skill_0_items();
    let now = T0 + HOUR;
    ok(store.review(
        &items[0],
        Facet::Receptive,
        Rating::Good,
        now,
        &context(Rating::Good, "lesson-1"),
        &p,
    ));
    assert_eq!(state(&store, 0, now), SkillState::Learning);
    assert_eq!(state(&store, 1, now), SkillState::Locked);

    for item in &items[1..] {
        ok(store.review(
            item,
            Facet::Receptive,
            Rating::Good,
            now,
            &context(Rating::Good, "lesson-1"),
            &p,
        ));
    }
    // Mastery is 7 / 14 = 0.5 now: practised, not mastered.
    assert_eq!(state(&store, 0, now), SkillState::Practised);
    assert_eq!(state(&store, 1, now), SkillState::Available);
    assert_eq!(state(&store, 2, now), SkillState::Locked);

    for item in &items {
        ok(store.review(
            item,
            Facet::Productive,
            Rating::Good,
            now,
            &context(Rating::Good, "lesson-1"),
            &p,
        ));
    }
    assert_eq!(
        state(&store, 0, now),
        SkillState::Mastered,
        "mastery 1 at the review time"
    );
    let later = now + 400 * DAY_MS;
    assert!(forgetting_curve(400.0, weights()[2]) < 0.85);
    assert_eq!(
        state(&store, 0, later),
        SkillState::Practised,
        "mastery has decayed below 0.85"
    );
    assert_eq!(state(&store, 1, later), SkillState::Available);

    let item_5 = ok(store.item_ids(&skill_id(5)))[0].clone();
    assert!(ok(store.introduce(&item_5, now)));
    assert_eq!(state(&store, 5, now), SkillState::Learning);
}

// ---------------------------------------------------------------------------------------------
// 11 to 14. Placement
// ---------------------------------------------------------------------------------------------

/// A staircase run as the reference computes it.
#[derive(Debug, PartialEq)]
struct Staircase {
    questions: Vec<usize>,
    answers: Vec<bool>,
    reversals: Vec<usize>,
    estimate: usize,
}

/// `(2·sum + count) / (2·count)`: a mean rounded half up in integer arithmetic.
fn mean_half_up(values: &[usize]) -> usize {
    let sum: usize = values.iter().sum();
    let count = values.len();
    (2 * sum + count) / (2 * count)
}

/// The placement staircase exactly as the Design notes state it, written independently of the
/// core: start at N / 4 with step max(1, N / 4); after an answer whose correctness differs from
/// the previous one, a reversal at the current index and step = max(1, step / 2); then up a step
/// if correct, down if wrong; stop at 20 questions, at 12 or more with 8 reversals, or after 4
/// wrong answers in a row at index 0.
fn reference_staircase(n: usize, mut correct_at: impl FnMut(usize) -> bool) -> Staircase {
    let mut run = Staircase {
        questions: Vec::new(),
        answers: Vec::new(),
        reversals: Vec::new(),
        estimate: 0,
    };
    if n == 0 {
        return run;
    }
    let mut i = n / 4;
    let mut step = (n / 4).max(1);
    loop {
        let correct = correct_at(i);
        if run
            .answers
            .last()
            .is_some_and(|&previous| previous != correct)
        {
            run.reversals.push(i);
            step = (step / 2).max(1);
        }
        run.questions.push(i);
        run.answers.push(correct);
        i = if correct {
            (i + step).min(n - 1)
        } else {
            i.saturating_sub(step)
        };
        let wrong_at_zero = run
            .questions
            .iter()
            .zip(&run.answers)
            .rev()
            .take_while(|&(&q, &a)| q == 0 && !a)
            .count();
        let asked = run.questions.len();
        if asked >= 20 || (asked >= 12 && run.reversals.len() >= 8) || wrong_at_zero >= 4 {
            break;
        }
    }
    run.estimate = if run.reversals.len() >= 3 {
        mean_half_up(&run.reversals[2..])
    } else {
        mean_half_up(&run.questions[run.questions.len().saturating_sub(4)..])
    };
    run
}

/// A learner correct exactly at indices below `boundary`.
fn below(boundary: usize) -> impl Fn(usize) -> bool {
    move |index| index < boundary
}

/// One draw per question: u = (next_u64 >> 11) / 2^53, the answer `index < boundary` flipped
/// when u < slip.
fn simulated_learner(
    boundary: usize,
    slip: f64,
    rng: &mut SplitMix64,
) -> impl FnMut(usize) -> bool + '_ {
    move |index| {
        let u = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        (index < boundary) != (u < slip)
    }
}

/// The CEFR label of an estimate: below 3 "Pre-A1", else "around " and the skill's level.
fn label_of(estimate: usize, skills: &[SkillRef]) -> String {
    if estimate < 3 {
        "Pre-A1".to_string()
    } else {
        format!("around {}", skills[estimate].cefr.as_str())
    }
}

fn assert_result(result: &PlacementResult, expected: &Staircase, label: &str, what: &str) {
    assert_eq!(result.questions, expected.questions, "{what}: questions");
    assert_eq!(result.answers, expected.answers, "{what}: answers");
    assert_eq!(result.reversals, expected.reversals, "{what}: reversals");
    assert_eq!(result.estimate, expected.estimate, "{what}: estimate");
    assert_eq!(result.label, label, "{what}: label");
    assert_eq!(
        result.tested_out,
        skill_ids(0..expected.estimate),
        "{what}: skills below the estimate are tested out"
    );
}

/// Runs a Placement question by question against a learner correct exactly below `boundary`.
fn ask_all(boundary: usize) -> (Vec<usize>, PlacementResult) {
    let skills = mini_skills();
    let mut placement = Placement::new(mini_skills());
    let mut asked = Vec::new();
    while let Some(question) = placement.next_question() {
        assert!(
            placement.result().is_none(),
            "no result before the last answer"
        );
        let index = skills
            .iter()
            .position(|s| s.id == question.id)
            .unwrap_or_else(|| panic!("{} is a skill of the course", question.id));
        assert_eq!(question.cefr, skills[index].cefr);
        asked.push(index);
        assert!(asked.len() <= 20, "at most 20 questions");
        placement.answer(index < boundary);
    }
    let result = placement
        .result()
        .expect("a result once no question is left");
    (asked, result)
}

// Bullet 11: N = 80, a learner correct exactly below index 23: the spec's hand-worked run.
#[test]
fn t11_placement_with_the_boundary_at_23() {
    let hand_worked = Staircase {
        questions: vec![20, 40, 30, 20, 25, 23, 21, 22, 23, 22, 23, 22],
        answers: [20, 40, 30, 20, 25, 23, 21, 22, 23, 22, 23, 22]
            .iter()
            .map(|&i| i < 23)
            .collect(),
        reversals: vec![40, 20, 25, 21, 23, 22, 23, 22],
        estimate: 23,
    };
    assert_eq!(
        mean_half_up(&[25, 21, 23, 22, 23, 22]),
        23,
        "round(136 / 6)"
    );
    assert_eq!(
        reference_staircase(80, below(23)),
        hand_worked,
        "the reference reproduces the spec"
    );

    let (asked, result) = ask_all(23);
    assert_eq!(asked, hand_worked.questions);
    assert_result(&result, &hand_worked, "around A1", "question by question");
    let simulated = simulate(mini_skills(), 23, 0.0, &mut SplitMix64::new(0));
    assert_result(&simulated, &hand_worked, "around A1", "simulate");
    assert_eq!(simulated.questions.len(), 12);
}

// Bullet 12: boundary 0 stops early after 5 questions at estimate 0, "Pre-A1"; boundary 80 asks
// 20 questions ending at 79, estimate 79, "around A2"; an empty course finishes at once.
#[test]
fn t12_placement_at_the_extremes() {
    let zero = Staircase {
        questions: vec![20, 0, 0, 0, 0],
        answers: vec![false; 5],
        reversals: vec![],
        estimate: 0,
    };
    assert_eq!(reference_staircase(80, below(0)), zero);
    let (asked, result) = ask_all(0);
    assert_eq!(asked, zero.questions);
    assert_result(&result, &zero, "Pre-A1", "boundary 0");
    assert_result(
        &simulate(mini_skills(), 0, 0.0, &mut SplitMix64::new(0)),
        &zero,
        "Pre-A1",
        "simulate, boundary 0",
    );

    let mut top_questions = vec![20, 40, 60];
    top_questions.extend([79; 17]);
    let top = Staircase {
        questions: top_questions,
        answers: vec![true; 20],
        reversals: vec![],
        estimate: 79,
    };
    assert_eq!(reference_staircase(80, below(80)), top);
    let (asked, result) = ask_all(80);
    assert_eq!(asked, top.questions);
    assert_eq!(*asked.last().unwrap(), 79);
    assert_result(&result, &top, "around A2", "boundary 80");
    assert_result(
        &simulate(mini_skills(), 80, 0.0, &mut SplitMix64::new(0)),
        &top,
        "around A2",
        "simulate, boundary 80",
    );

    let empty = Placement::new(Vec::new());
    assert!(empty.next_question().is_none(), "N = 0 asks nothing");
    let result = empty.result().expect("N = 0 finishes at once");
    assert!(
        result.questions.is_empty() && result.answers.is_empty() && result.reversals.is_empty()
    );
    assert_eq!(result.estimate, 0);
    assert_eq!(result.label, "Pre-A1");
    assert!(result.tested_out.is_empty());
    assert_eq!(
        simulate(Vec::new(), 5, 0.1, &mut SplitMix64::new(1)).estimate,
        0
    );
}

// Bullet 13: 200 seeded learners with slip 0.1 and boundaries uniform over 0 to 80: at least 85%
// of estimates within ±4 of the boundary; every run asks 12 to 20 questions or exits early at
// index 0. Each run also equals the reference staircase with the Design notes' learner.
#[test]
fn t13_noisy_learners_are_placed_near_their_boundary() {
    let skills = mini_skills();
    let mut boundaries = SplitMix64::new(13);
    let mut within = 0;
    for learner in 0..200u64 {
        let boundary = (boundaries.next_u64() % 81) as usize;
        let seed = 1_000 + learner;
        let result = simulate(mini_skills(), boundary, 0.1, &mut SplitMix64::new(seed));
        let mut rng = SplitMix64::new(seed);
        let expected = reference_staircase(80, simulated_learner(boundary, 0.1, &mut rng));
        let what = format!("learner {learner}, boundary {boundary}");
        assert_result(
            &result,
            &expected,
            &label_of(expected.estimate, &skills),
            &what,
        );

        let asked = result.questions.len();
        let early_exit = (5..12).contains(&asked)
            && result.questions[asked - 4..].iter().all(|&q| q == 0)
            && result.answers[asked - 4..].iter().all(|&a| !a);
        assert!(
            (12..=20).contains(&asked) || early_exit,
            "{what}: {asked} questions"
        );
        if result.estimate.abs_diff(boundary) <= 4 {
            within += 1;
        }
    }
    assert!(
        within >= 170,
        "{within} of 200 estimates within ±4 of the boundary"
    );
}

// Bullet 14: apply_placement marks the tested-out skills, introduces their items, gives every
// card still new one Good review at the placement time (exercise type "placement", verdict
// correct, no response time, the session id) due round(w[2]) days later, and records the
// estimate and label; a card reviewed before keeps its own history.
#[test]
fn t14_tested_out_cards_get_one_placement_review() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, path) = course_store_file(dir.path());
    let p = SrsParams::default();
    let earlier = format!("{SKILL_0}#l:wabak");
    ok(store.review(
        &earlier,
        Facet::Receptive,
        Rating::Again,
        T0,
        &context(Rating::Again, "lesson-1"),
        &p,
    ));

    let questions = vec![20, 40, 30, 20, 25, 23, 21, 22, 23, 22, 23, 22];
    let result = PlacementResult {
        answers: questions.iter().map(|&i| i < 23).collect(),
        questions,
        reversals: vec![40, 20, 25, 21, 23, 22, 23, 22],
        estimate: 23,
        label: "around A1".to_string(),
        tested_out: skill_ids(0..23),
    };
    let placed_at = T0 + HOUR;
    ok(store.apply_placement(Lang::Es, &result, placed_at, "placement-1", &p));

    let w2 = weights()[2];
    let interval = w2.round() as i64;
    assert_eq!(interval, 2, "round(w[2]) = round(2.3065)");
    let log = ok(store.review_log());
    let mut placement_reviews = 0;
    for skill in skill_ids(0..23) {
        let items = ok(store.item_ids(&skill));
        assert_eq!(items.len(), 7, "{skill}");
        for item in items {
            for facet in [Facet::Receptive, Facet::Productive] {
                let card = ok(store.card(&item, facet))
                    .unwrap_or_else(|| panic!("{item} {facet:?} was introduced"));
                let entries: Vec<&ReviewLogEntry> = log
                    .iter()
                    .filter(|e| e.item_id == item && e.facet == facet)
                    .collect();
                assert_eq!(entries.len(), 1, "{item} {facet:?}: one log entry");
                let entry = entries[0];
                if item == earlier && facet == Facet::Receptive {
                    assert_eq!(
                        entry.exercise_type, "translate_to_en",
                        "the earlier review is kept"
                    );
                    assert_eq!(card.state, CardState::Relearning);
                    continue;
                }
                assert_eq!(entry.exercise_type, "placement", "{item} {facet:?}");
                assert_eq!(entry.rating, Rating::Good);
                assert_eq!(entry.verdict, Verdict::Correct);
                assert_eq!(entry.response_ms, None);
                assert_eq!(entry.session_id, "placement-1");
                assert_eq!(entry.reviewed_at, placed_at);
                assert_eq!(
                    (card.state, card.reps, card.lapses),
                    (CardState::Review, 1, 0)
                );
                assert_eq!(card.last_review, Some(placed_at));
                assert_eq!(
                    card.due,
                    Some(placed_at + interval * DAY_MS),
                    "{item} {facet:?}: due"
                );
                assert!((card.stability.unwrap() - w2).abs() < 1e-9);
                placement_reviews += 1;
            }
        }
    }
    assert_eq!(placement_reviews, 23 * 7 * 2 - 1);
    assert_eq!(log.len(), 23 * 7 * 2, "no other review was logged");
    for item in ok(store.item_ids(&skill_id(23))) {
        assert!(
            ok(store.card(&item, Facet::Receptive)).is_none(),
            "{item} is not tested out"
        );
    }

    assert_eq!(
        ok(skill_state(&store, &skill_id(22), placed_at)),
        SkillState::Mastered
    );
    assert_eq!(
        ok(skill_state(&store, &skill_id(23), placed_at)),
        SkillState::Available
    );
    assert_eq!(
        ok(skill_state(&store, &skill_id(24), placed_at)),
        SkillState::Locked
    );

    let conn = Connection::open(&path).unwrap();
    let mut stmt = conn
        .prepare("SELECT id FROM skills WHERE tested_out = 1 ORDER BY ord")
        .unwrap();
    let tested_out: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(tested_out, skill_ids(0..23));
    let (index, cefr): (i64, String) = conn
        .query_row(
            "SELECT placement_index, cefr_estimate FROM courses WHERE lang = 'es'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((index, cefr.as_str()), (23, "around A1"));
}

// Design notes: a skill keeps its tested_out flag when start_course runs again.
#[test]
fn t14_tested_out_survives_a_new_syllabus_version() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, path) = course_store_file(dir.path());
    let p = SrsParams::default();
    let result = simulate(mini_skills(), 10, 0.0, &mut SplitMix64::new(0));
    ok(store.apply_placement(Lang::Es, &result, T0 + HOUR, "placement-1", &p));
    let mut v2 = mini();
    v2.version = 2;
    ok(store.start_course(&v2, &Forms::default(), T0 + DAY_MS));
    let conn = Connection::open(&path).unwrap();
    let flagged: i64 = query_one(&conn, "SELECT count(*) FROM skills WHERE tested_out = 1");
    assert_eq!(flagged, result.tested_out.len() as i64);
    assert!(flagged > 0);
}

// ---------------------------------------------------------------------------------------------
// 15 to 17. Environment, invariant I6, SplitMix64 and settings
// ---------------------------------------------------------------------------------------------

struct FixedClock {
    ms: i64,
    offset_minutes: i32,
}

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.ms
    }
    fn utc_offset_minutes(&self) -> i32 {
        self.offset_minutes
    }
}

fn date_at(ms: i64, offset_minutes: i32) -> (i32, u32, u32, String) {
    let date = local_date(&FixedClock { ms, offset_minutes });
    (date.year, date.month, date.day, date.to_string())
}

// Bullet 15: the local date is the clock's milliseconds plus its UTC offset, floored to days.
// The further dates are civil-calendar facts: leap days, a century that is not a leap year, a
// day before the epoch, and year ends in both directions.
#[test]
fn t15_local_date_uses_the_clock_offset() {
    // 2026-03-29T00:30:00Z
    let ms = 1_774_744_200_000;
    assert_eq!(date_at(ms, 60), (2026, 3, 29, "2026-03-29".to_string()));
    assert_eq!(date_at(ms, -300), (2026, 3, 28, "2026-03-28".to_string()));

    // 2024-02-29T23:59:00Z
    assert_eq!(date_at(1_709_251_140_000, 0).3, "2024-02-29");
    assert_eq!(date_at(1_709_251_140_000, 1).3, "2024-03-01");
    // 1970-01-01T00:00:00Z: one minute west is the day before, floored, not truncated.
    assert_eq!(date_at(0, 0).3, "1970-01-01");
    assert_eq!(date_at(0, -1).3, "1969-12-31");
    // 2000-03-01T00:00:00Z and 2100-03-01T00:00:00Z
    assert_eq!(date_at(951_868_800_000, -1).3, "2000-02-29");
    assert_eq!(date_at(4_107_542_400_000, -1).3, "2100-02-28");
    // 2026-01-01T02:00:00Z and 2026-12-31T22:30:00Z
    assert_eq!(date_at(1_767_232_800_000, -180).3, "2025-12-31");
    assert_eq!(
        date_at(1_798_756_200_000, 90),
        (2027, 1, 1, "2027-01-01".to_string())
    );
}

/// Every file under `dir`, recursively, skipping build output.
fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "target" && name != ".git" && name != "node_modules" {
                files_under(&path, out);
            }
        } else {
            out.push(path);
        }
    }
}

// Bullet 16 (invariant I6, the review log is append-only): no source file under crates/ holds
// an update or a delete statement on the review_log table, in any letter case or spacing. The
// two statements are assembled from pieces so this file does not match itself.
#[test]
fn t16_no_source_updates_or_deletes_review_log_rows() {
    let forbidden = [
        concat!("UPD", "ATE REVIEW", "_LOG"),
        concat!("DEL", "ETE FROM REVIEW", "_LOG"),
    ];
    let mut files = Vec::new();
    files_under(&repo_root().join("crates"), &mut files);
    let mut scanned = 0;
    let mut saw_the_table = false;
    let mut offenders = Vec::new();
    for path in &files {
        let Ok(bytes) = fs::read(path) else { continue };
        let text = String::from_utf8_lossy(&bytes).to_uppercase();
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        scanned += 1;
        saw_the_table |= text.contains(concat!("CREATE TABLE REVIEW", "_LOG"));
        for statement in forbidden {
            if text.contains(statement) {
                offenders.push(format!("{}: {statement}", path.display()));
            }
        }
    }
    assert!(scanned >= 20, "the walk found {scanned} files");
    assert!(
        saw_the_table,
        "the walk reaches the migration that creates review_log"
    );
    assert!(
        offenders.is_empty(),
        "the review log is append-only:\n{}",
        offenders.join("\n")
    );
}

// Bullet 17: SplitMix64 with seed 0 gives the reference C implementation's first outputs.
#[test]
fn t17_splitmix64_matches_the_reference_outputs() {
    let mut rng = SplitMix64::new(0);
    assert_eq!(rng.next_u64(), 0xE220_A839_7B1D_CDAF);
    assert_eq!(rng.next_u64(), 0x6E78_9E6A_A1B9_65F4);
    assert_eq!(rng.next_u64(), 0x06C4_5D18_8009_454F);
    let mut a = SplitMix64::new(42);
    let mut b = SplitMix64::new(42);
    for _ in 0..100 {
        assert_eq!(a.next_u64(), b.next_u64(), "a seed fixes the sequence");
    }
}

// Bullet 17: the desired retention is clamped to 0.80..=0.95; the default is 0.90.
#[test]
fn t17_with_retention_clamps_to_the_allowed_range() {
    assert_eq!(SrsParams::with_retention(0.5).desired_retention, 0.80);
    assert_eq!(SrsParams::with_retention(0.99).desired_retention, 0.95);
    assert_eq!(SrsParams::with_retention(0.85).desired_retention, 0.85);
    assert_eq!(SrsParams::with_retention(0.80).desired_retention, 0.80);
    assert_eq!(SrsParams::with_retention(0.95).desired_retention, 0.95);
    assert_eq!(SrsParams::default().desired_retention, 0.90);
}

// Interfaces: the serde forms of the enums and of a card (nine fields in order, null for an
// empty Option).
#[test]
fn enums_and_cards_serialise_as_the_interfaces_say() {
    for (rating, n) in [
        (Rating::Again, 1),
        (Rating::Hard, 2),
        (Rating::Good, 3),
        (Rating::Easy, 4),
    ] {
        assert_eq!(serde_json::to_value(rating).unwrap(), json!(n));
        assert_eq!(serde_json::from_value::<Rating>(json!(n)).unwrap(), rating);
        assert_eq!(rating_number(rating), n);
    }
    for (facet, text) in [
        (Facet::Receptive, "receptive"),
        (Facet::Productive, "productive"),
    ] {
        assert_eq!(serde_json::to_value(facet).unwrap(), json!(text));
        assert_eq!(serde_json::from_value::<Facet>(json!(text)).unwrap(), facet);
    }
    for (state, text) in [
        (CardState::New, "new"),
        (CardState::Review, "review"),
        (CardState::Relearning, "relearning"),
    ] {
        assert_eq!(serde_json::to_value(state).unwrap(), json!(text));
    }
    for (verdict, text) in [
        (Verdict::Correct, "correct"),
        (Verdict::Almost, "almost"),
        (Verdict::Wrong, "wrong"),
    ] {
        assert_eq!(serde_json::to_value(verdict).unwrap(), json!(text));
    }
    for (state, text) in [
        (SkillState::Locked, "locked"),
        (SkillState::Available, "available"),
        (SkillState::Learning, "learning"),
        (SkillState::Practised, "practised"),
        (SkillState::Mastered, "mastered"),
    ] {
        assert_eq!(serde_json::to_value(state).unwrap(), json!(text));
    }

    let card = Card::new("es.a1.u01.s01#l:wabak", Facet::Productive);
    assert_eq!(
        serde_json::to_string(&card).unwrap(),
        r#"{"item_id":"es.a1.u01.s01#l:wabak","facet":"productive","state":"new","stability":null,"difficulty":null,"due":null,"last_review":null,"reps":0,"lapses":0}"#
    );
    let reviewed = schedule(&card, Rating::Good, T0, &SrsParams::default()).card;
    let text = serde_json::to_string(&reviewed).unwrap();
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
        .map(|k| text.find(k).unwrap_or_else(|| panic!("{k} in {text}")))
        .collect();
    assert!(
        positions.windows(2).all(|w| w[0] < w[1]),
        "field order: {text}"
    );
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["state"], "review");
    assert_eq!(value["due"], json!(T0 + 2 * DAY_MS));
    assert_eq!(value["last_review"], json!(T0));
    assert_eq!(value["reps"], json!(1));
}
