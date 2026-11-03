//! The SQLite side of the store: migrations, the course and its items, cards and the review log.
//!
//! The review log is append-only (invariant I6): this file only ever inserts into it and reads
//! it. Every write that touches more than one row runs in one transaction, so a failure leaves
//! the file as it was.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use super::{CourseReport, ReviewContext, StoreError};
use crate::content::{Cefr, Forms, ItemKind, Lang, Syllabus, derive_items, nfc_lower};
use crate::srs::{
    Card, CardState, Facet, PlacementResult, Rating, ReviewLogEntry, SkillRef, SrsParams, Verdict,
    schedule,
};

/// Every migration in order; migration `n` is `MIGRATIONS[n - 1]` and sets `user_version` to n.
const MIGRATIONS: [&str; 1] = [include_str!("migrations/0001_init.sql")];

/// The columns of a card, in the order [`card_from_row`] reads them.
const CARD_COLUMNS: &str =
    "item_id, facet, state, stability, difficulty, due, last_review, reps, lapses";

/// Cards in the order the store lists them: by item id in byte order, receptive first.
const CARD_ORDER: &str = "item_id, CASE facet WHEN 'receptive' THEN 0 ELSE 1 END";

/// What the syllabus says about a skill that the `skills` table has no column for, kept in
/// `meta` under `skill:<id>`: its `requires` list and its items in syllabus order.
#[derive(Debug, Default, Serialize, Deserialize)]
struct SkillMeta {
    requires: Vec<String>,
    items: Vec<String>,
}

fn skill_meta_key(skill_id: &str) -> String {
    format!("skill:{skill_id}")
}

/// A row of `skills` with its meta entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillRow {
    pub lang: String,
    pub ord: i64,
    pub tested_out: bool,
    pub requires: Vec<String>,
}

fn text_of<'a>(value: ValueRef<'a>) -> Result<&'a str, FromSqlError> {
    value.as_str()
}

fn bad_value(text: &str) -> FromSqlError {
    FromSqlError::Other(format!("unexpected value {text}").into())
}

impl FromSql for Facet {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = text_of(value)?;
        text.parse().map_err(|_| bad_value(text))
    }
}

impl ToSql for Facet {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for CardState {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = text_of(value)?;
        text.parse().map_err(|_| bad_value(text))
    }
}

impl ToSql for CardState {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for Verdict {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = text_of(value)?;
        text.parse().map_err(|_| bad_value(text))
    }
}

impl ToSql for Verdict {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for Rating {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let number = value.as_i64()?;
        Rating::try_from(number).map_err(|e| FromSqlError::Other(e.into()))
    }
}

impl ToSql for Rating {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(i64::from(*self)))
    }
}

impl FromSql for Cefr {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match text_of(value)? {
            "A1" => Ok(Cefr::A1),
            "A2" => Ok(Cefr::A2),
            other => Err(bad_value(other)),
        }
    }
}

fn item_kind(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Vocab => "vocab",
        ItemKind::Phrase => "phrase",
        ItemKind::Pattern => "pattern",
    }
}

fn card_from_row(row: &Row<'_>) -> rusqlite::Result<Card> {
    Ok(Card {
        item_id: row.get(0)?,
        facet: row.get(1)?,
        state: row.get(2)?,
        stability: row.get(3)?,
        difficulty: row.get(4)?,
        due: row.get(5)?,
        last_review: row.get(6)?,
        reps: row.get(7)?,
        lapses: row.get(8)?,
    })
}

fn user_version_of(conn: &Connection) -> Result<u32, StoreError> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    // A negative version was not written by Parley; it is as unknown as a newer one.
    Ok(u32::try_from(version).unwrap_or(u32::MAX))
}

/// Runs the pending migrations, each in its own transaction. A failed step is rolled back, so
/// the file is left as it was, and reported as `Migration(step)`.
fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    let version = user_version_of(conn)?;
    let newest = MIGRATIONS.len() as u32;
    if version > newest {
        return Err(StoreError::TooNew(version));
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let step = index as u32 + 1;
        let failed = |_| StoreError::Migration(step);
        let tx = conn.transaction().map_err(failed)?;
        tx.execute_batch(sql).map_err(failed)?;
        tx.pragma_update(None, "user_version", step)
            .map_err(failed)?;
        tx.commit().map_err(failed)?;
    }
    Ok(())
}

fn item_exists(conn: &Connection, item_id: &str) -> Result<bool, StoreError> {
    Ok(conn
        .query_row("SELECT 1 FROM items WHERE id = ?1", [item_id], |_| Ok(()))
        .optional()?
        .is_some())
}

fn card_in(conn: &Connection, item_id: &str, facet: Facet) -> Result<Option<Card>, StoreError> {
    Ok(conn
        .query_row(
            &format!("SELECT {CARD_COLUMNS} FROM cards WHERE item_id = ?1 AND facet = ?2"),
            params![item_id, facet],
            card_from_row,
        )
        .optional()?)
}

/// Creates the two `new` cards of an item that has none; false when it already had them.
fn introduce_in(conn: &Connection, item_id: &str) -> Result<bool, StoreError> {
    if !item_exists(conn, item_id)? {
        return Err(StoreError::UnknownItem(item_id.to_string()));
    }
    let mut inserted = 0;
    for facet in Facet::ALL {
        inserted += conn.execute(
            "INSERT INTO cards (item_id, facet, state) VALUES (?1, ?2, ?3)
             ON CONFLICT (item_id, facet) DO NOTHING",
            params![item_id, facet, CardState::New],
        )?;
    }
    Ok(inserted > 0)
}

/// Schedules one review, writes the card and appends the log row (inside the caller's
/// transaction).
fn review_in(
    conn: &Connection,
    item_id: &str,
    facet: Facet,
    rating: Rating,
    now_ms: i64,
    ctx: &ReviewContext,
    p: &SrsParams,
) -> Result<Card, StoreError> {
    introduce_in(conn, item_id)?;
    let card = card_in(conn, item_id, facet)?
        .ok_or_else(|| StoreError::UnknownItem(item_id.to_string()))?;
    let scheduled = schedule(&card, rating, now_ms, p);
    let next = &scheduled.card;
    conn.execute(
        "UPDATE cards SET state = ?3, stability = ?4, difficulty = ?5, due = ?6, last_review = ?7,
           reps = ?8, lapses = ?9
         WHERE item_id = ?1 AND facet = ?2",
        params![
            item_id,
            facet,
            next.state,
            next.stability,
            next.difficulty,
            next.due,
            next.last_review,
            next.reps,
            next.lapses
        ],
    )?;
    conn.execute(
        "INSERT INTO review_log (item_id, facet, reviewed_at, rating, exercise_type, verdict,
           response_ms, elapsed_days, stability_after, difficulty_after, session_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            item_id,
            facet,
            now_ms,
            rating,
            ctx.exercise_type,
            ctx.verdict,
            ctx.response_ms,
            scheduled.elapsed_days,
            next.stability,
            next.difficulty,
            ctx.session_id
        ],
    )?;
    Ok(scheduled.card)
}

fn skill_meta(conn: &Connection, skill_id: &str) -> Result<SkillMeta, StoreError> {
    let text: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [skill_meta_key(skill_id)],
            |row| row.get(0),
        )
        .optional()?;
    match text {
        Some(text) => serde_json::from_str(&text)
            .map_err(|e| StoreError::Sqlite(format!("meta {}: {e}", skill_meta_key(skill_id)))),
        None => Ok(SkillMeta::default()),
    }
}

fn skill_row_in(conn: &Connection, skill_id: &str) -> Result<SkillRow, StoreError> {
    let row = conn
        .query_row(
            "SELECT lang, ord, tested_out FROM skills WHERE id = ?1",
            [skill_id],
            |row| {
                Ok(SkillRow {
                    lang: row.get(0)?,
                    ord: row.get(1)?,
                    tested_out: row.get::<_, i64>(2)? != 0,
                    requires: Vec::new(),
                })
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::UnknownSkill(skill_id.to_string()))?;
    Ok(SkillRow {
        requires: skill_meta(conn, skill_id)?.requires,
        ..row
    })
}

/// The skill's items that are not retired, in syllabus order (items the syllabus does not list
/// follow in the order they were added).
fn item_ids_in(conn: &Connection, skill_id: &str) -> Result<Vec<String>, StoreError> {
    skill_row_in(conn, skill_id)?;
    let order = skill_meta(conn, skill_id)?.items;
    let position: HashMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    let mut statement =
        conn.prepare("SELECT id FROM items WHERE skill_id = ?1 AND retired = 0 ORDER BY rowid")?;
    let mut ids: Vec<String> = statement
        .query_map([skill_id], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    ids.sort_by_key(|id| position.get(id.as_str()).copied().unwrap_or(usize::MAX));
    Ok(ids)
}

fn query_cards(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<Card>, StoreError> {
    let mut statement = conn.prepare(sql)?;
    let cards = statement
        .query_map(params, card_from_row)?
        .collect::<Result<_, _>>()?;
    Ok(cards)
}

/// The learner's data file.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (creating it when absent) the data file at `path` and runs pending migrations.
    pub fn open(path: &Path) -> Result<Store, StoreError> {
        Store::with_connection(Connection::open(path)?)
    }

    /// A store that lives in memory only, for tests and simulations.
    pub fn open_in_memory() -> Result<Store, StoreError> {
        Store::with_connection(Connection::open_in_memory()?)
    }

    fn with_connection(mut conn: Connection) -> Result<Store, StoreError> {
        conn.pragma_update(None, "foreign_keys", true)?;
        migrate(&mut conn)?;
        Ok(Store { conn })
    }

    /// The newest migration applied to the file.
    pub fn user_version(&self) -> Result<u32, StoreError> {
        user_version_of(&self.conn)
    }

    /// Brings the course of the syllabus's language up to date with the syllabus, idempotently:
    /// the course row keeps its first `started_at`; skills are upserted in syllabus order with
    /// their `tested_out` flag kept, and a skill gone from the syllabus keeps its row with `ord`
    /// −1; items come from `derive_items`, an item gone from the syllabus is retired (its cards
    /// and log are kept) and one that comes back is active again; the `content` accepted answers
    /// are made to match the syllabus, `judge` and `learner` answers are kept. Rows that do not
    /// change keep their timestamps.
    pub fn start_course(
        &mut self,
        syllabus: &Syllabus,
        forms: &Forms,
        now_ms: i64,
    ) -> Result<CourseReport, StoreError> {
        let lang = syllabus.lang.as_str();
        let items = derive_items(syllabus, forms);
        let tx = self.conn.transaction()?;

        tx.execute(
            "INSERT INTO courses (lang, syllabus_version, started_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (lang) DO UPDATE SET syllabus_version = excluded.syllabus_version",
            params![lang, syllabus.version, now_ms],
        )?;

        let mut present: HashSet<&str> = HashSet::new();
        for (ord, (unit, skill)) in syllabus.skills().enumerate() {
            tx.execute(
                "INSERT INTO skills (id, lang, unit_id, ord, kind, cefr, title)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (id) DO UPDATE SET lang = excluded.lang, unit_id = excluded.unit_id,
                   ord = excluded.ord, kind = excluded.kind, cefr = excluded.cefr,
                   title = excluded.title",
                params![
                    skill.id,
                    lang,
                    unit.id,
                    ord as i64,
                    skill.kind.as_str(),
                    skill.cefr.as_str(),
                    skill.title
                ],
            )?;
            present.insert(skill.id.as_str());
        }
        let listed: Vec<String> = {
            let mut statement = tx.prepare("SELECT id FROM skills WHERE lang = ?1 AND ord >= 0")?;
            statement
                .query_map([lang], |row| row.get(0))?
                .collect::<Result<_, _>>()?
        };
        for id in listed.iter().filter(|id| !present.contains(id.as_str())) {
            tx.execute("UPDATE skills SET ord = -1 WHERE id = ?1", [id])?;
        }

        let active_before: HashSet<String> = {
            let mut statement = tx.prepare(
                "SELECT id FROM items WHERE lang = ?1 AND source_kind = 'syllabus' AND retired = 0",
            )?;
            statement
                .query_map([lang], |row| row.get(0))?
                .collect::<Result<_, _>>()?
        };
        let mut added = 0;
        let mut by_skill: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for item in &items {
            if !active_before.contains(&item.id) {
                added += 1;
            }
            let en = serde_json::to_string(&item.en).expect("strings serialise");
            let lemmas = serde_json::to_string(&item.lemmas).expect("strings serialise");
            tx.execute(
                "INSERT INTO items (id, lang, skill_id, kind, target, en, lemmas, cefr, source_kind,
                   created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'syllabus', ?9)
                 ON CONFLICT (id) DO UPDATE SET lang = excluded.lang, skill_id = excluded.skill_id,
                   kind = excluded.kind, target = excluded.target, en = excluded.en,
                   lemmas = excluded.lemmas, cefr = excluded.cefr, retired = 0",
                params![
                    item.id,
                    lang,
                    item.skill,
                    item_kind(item.kind),
                    item.target,
                    en,
                    lemmas,
                    item.cefr.as_str(),
                    now_ms
                ],
            )?;
            by_skill
                .entry(item.skill.as_str())
                .or_default()
                .push(item.id.clone());
        }
        let current: HashSet<&str> = items.iter().map(|item| item.id.as_str()).collect();
        let mut retired = 0;
        for id in active_before
            .iter()
            .filter(|id| !current.contains(id.as_str()))
        {
            tx.execute("UPDATE items SET retired = 1 WHERE id = ?1", [id])?;
            retired += 1;
        }

        for (_, skill) in syllabus.skills() {
            let meta = SkillMeta {
                requires: skill.requires.clone(),
                items: by_skill.remove(skill.id.as_str()).unwrap_or_default(),
            };
            tx.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![
                    skill_meta_key(&skill.id),
                    serde_json::to_string(&meta).expect("skill meta serialises")
                ],
            )?;
        }

        // The content answers: the syllabus's accepted answers, keyed by their normal form.
        let mut wanted: BTreeMap<(String, String, String), String> = BTreeMap::new();
        for item in &items {
            for (direction, answers) in [
                ("to_target", &item.accepted.to_target),
                ("to_en", &item.accepted.to_en),
            ] {
                for display in answers {
                    wanted
                        .entry((item.id.clone(), direction.to_string(), nfc_lower(display)))
                        .or_insert_with(|| display.clone());
                }
            }
        }
        let existing: Vec<(String, String, String, String)> = {
            let mut statement = tx.prepare(
                "SELECT a.item_id, a.direction, a.norm, a.display FROM accepted_answers a
                 JOIN items i ON i.id = a.item_id
                 WHERE i.lang = ?1 AND i.source_kind = 'syllabus' AND a.source = 'content'",
            )?;
            statement
                .query_map([lang], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })?
                .collect::<Result<_, _>>()?
        };
        let mut kept: HashMap<(String, String, String), String> = HashMap::new();
        for (item_id, direction, norm, display) in existing {
            let key = (item_id, direction, norm);
            if wanted.contains_key(&key) {
                kept.insert(key, display);
            } else {
                let (item_id, direction, norm) = key;
                tx.execute(
                    "DELETE FROM accepted_answers
                     WHERE item_id = ?1 AND direction = ?2 AND norm = ?3 AND source = 'content'",
                    params![item_id, direction, norm],
                )?;
            }
        }
        for (key, display) in &wanted {
            let (item_id, direction, norm) = key;
            match kept.get(key) {
                Some(old) if old == display => {}
                Some(_) => {
                    tx.execute(
                        "UPDATE accepted_answers SET display = ?4
                         WHERE item_id = ?1 AND direction = ?2 AND norm = ?3 AND source = 'content'",
                        params![item_id, direction, norm, display],
                    )?;
                }
                None => {
                    tx.execute(
                        "INSERT INTO accepted_answers (item_id, direction, norm, display, source,
                           created_at)
                         VALUES (?1, ?2, ?3, ?4, 'content', ?5)
                         ON CONFLICT (item_id, direction, norm) DO NOTHING",
                        params![item_id, direction, norm, display, now_ms],
                    )?;
                }
            }
        }

        tx.commit()?;
        Ok(CourseReport {
            items: items.len(),
            added,
            retired,
        })
    }

    /// The course's skills in syllabus order (skills gone from the syllabus are left out).
    pub fn skills(&self, lang: Lang) -> Result<Vec<SkillRef>, StoreError> {
        let mut statement = self
            .conn
            .prepare("SELECT id, cefr FROM skills WHERE lang = ?1 AND ord >= 0 ORDER BY ord")?;
        let skills = statement
            .query_map([lang.as_str()], |row| {
                Ok(SkillRef {
                    id: row.get(0)?,
                    cefr: row.get(1)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(skills)
    }

    /// The ids of the skill's items that are not retired, in syllabus order.
    pub fn item_ids(&self, skill_id: &str) -> Result<Vec<String>, StoreError> {
        item_ids_in(&self.conn, skill_id)
    }

    /// How many items of the course are not retired.
    pub fn item_count(&self, lang: Lang) -> Result<usize, StoreError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM items WHERE lang = ?1 AND retired = 0",
            [lang.as_str()],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Whether the item has left the syllabus.
    pub fn is_retired(&self, item_id: &str) -> Result<bool, StoreError> {
        let retired: Option<i64> = self
            .conn
            .query_row(
                "SELECT retired FROM items WHERE id = ?1",
                [item_id],
                |row| row.get(0),
            )
            .optional()?;
        retired
            .map(|flag| flag != 0)
            .ok_or_else(|| StoreError::UnknownItem(item_id.to_string()))
    }

    /// Creates the item's two `new` cards; false when it was already introduced. `now_ms` is the
    /// time of introduction; the 0001 tables have no column for it, so it is not stored.
    pub fn introduce(&mut self, item_id: &str, now_ms: i64) -> Result<bool, StoreError> {
        let _ = now_ms;
        let tx = self.conn.transaction()?;
        let created = introduce_in(&tx, item_id)?;
        tx.commit()?;
        Ok(created)
    }

    /// One card, if the item has been introduced.
    pub fn card(&self, item_id: &str, facet: Facet) -> Result<Option<Card>, StoreError> {
        card_in(&self.conn, item_id, facet)
    }

    /// Every card, by item id in byte order, receptive before productive.
    pub fn cards(&self) -> Result<Vec<Card>, StoreError> {
        query_cards(
            &self.conn,
            &format!("SELECT {CARD_COLUMNS} FROM cards ORDER BY {CARD_ORDER}"),
            [],
        )
    }

    /// Reviews one card: introduces the item if it has no cards yet, schedules the review,
    /// updates the card and appends the log row, all in one transaction.
    pub fn review(
        &mut self,
        item_id: &str,
        facet: Facet,
        rating: Rating,
        now_ms: i64,
        ctx: &ReviewContext,
        p: &SrsParams,
    ) -> Result<Card, StoreError> {
        let tx = self.conn.transaction()?;
        let card = review_in(&tx, item_id, facet, rating, now_ms, ctx, p)?;
        tx.commit()?;
        Ok(card)
    }

    /// The whole review log, in the order it was written.
    pub fn review_log(&self) -> Result<Vec<ReviewLogEntry>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT item_id, facet, reviewed_at, rating, exercise_type, verdict, response_ms,
               elapsed_days, stability_after, difficulty_after, session_id
             FROM review_log ORDER BY id",
        )?;
        let entries = statement
            .query_map([], |row| {
                Ok(ReviewLogEntry {
                    item_id: row.get(0)?,
                    facet: row.get(1)?,
                    reviewed_at: row.get(2)?,
                    rating: row.get(3)?,
                    exercise_type: row.get(4)?,
                    verdict: row.get(5)?,
                    response_ms: row.get(6)?,
                    elapsed_days: row.get(7)?,
                    stability_after: row.get(8)?,
                    difficulty_after: row.get(9)?,
                    session_id: row.get(10)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(entries)
    }

    /// Records a finished placement test, in one transaction: each tested-out skill is flagged,
    /// each of its items not retired is introduced, and each of its cards still `new` gets one
    /// review rated Good at `now_ms` (exercise type `placement`, verdict `correct`, no response
    /// time); the course keeps the estimate and its label.
    pub fn apply_placement(
        &mut self,
        lang: Lang,
        result: &PlacementResult,
        now_ms: i64,
        session_id: &str,
        p: &SrsParams,
    ) -> Result<(), StoreError> {
        let ctx = ReviewContext {
            exercise_type: "placement".to_string(),
            verdict: Verdict::Correct,
            response_ms: None,
            session_id: session_id.to_string(),
        };
        let tx = self.conn.transaction()?;
        for skill_id in &result.tested_out {
            let flagged = tx.execute(
                "UPDATE skills SET tested_out = 1 WHERE id = ?1 AND lang = ?2",
                params![skill_id, lang.as_str()],
            )?;
            if flagged == 0 {
                return Err(StoreError::UnknownSkill(skill_id.clone()));
            }
            for item_id in item_ids_in(&tx, skill_id)? {
                introduce_in(&tx, &item_id)?;
                for facet in Facet::ALL {
                    let is_new = card_in(&tx, &item_id, facet)?
                        .is_some_and(|card| card.state == CardState::New);
                    if is_new {
                        review_in(&tx, &item_id, facet, Rating::Good, now_ms, &ctx, p)?;
                    }
                }
            }
        }
        tx.execute(
            "UPDATE courses SET placement_index = ?1, cefr_estimate = ?2 WHERE lang = ?3",
            params![result.estimate as i64, result.label, lang.as_str()],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Cards with `due ≤ now_ms` of the course's items that are not retired.
    pub(crate) fn due_cards(&self, lang: Lang, now_ms: i64) -> Result<Vec<Card>, StoreError> {
        query_cards(
            &self.conn,
            "SELECT c.item_id, c.facet, c.state, c.stability, c.difficulty, c.due, c.last_review,
               c.reps, c.lapses
             FROM cards c JOIN items i ON i.id = c.item_id
             WHERE i.lang = ?1 AND i.retired = 0 AND c.due IS NOT NULL AND c.due <= ?2
             ORDER BY c.item_id, CASE c.facet WHEN 'receptive' THEN 0 ELSE 1 END",
            params![lang.as_str(), now_ms],
        )
    }

    /// The cards of a skill's items that are not retired.
    pub(crate) fn skill_cards(&self, skill_id: &str) -> Result<Vec<Card>, StoreError> {
        query_cards(
            &self.conn,
            "SELECT c.item_id, c.facet, c.state, c.stability, c.difficulty, c.due, c.last_review,
               c.reps, c.lapses
             FROM cards c JOIN items i ON i.id = c.item_id
             WHERE i.skill_id = ?1 AND i.retired = 0
             ORDER BY c.item_id, CASE c.facet WHEN 'receptive' THEN 0 ELSE 1 END",
            [skill_id],
        )
    }

    /// A skill's row and `requires` list.
    pub(crate) fn skill_row(&self, skill_id: &str) -> Result<SkillRow, StoreError> {
        skill_row_in(&self.conn, skill_id)
    }

    /// The skill just before `ord` in the course's syllabus order, if any.
    pub(crate) fn previous_skill(
        &self,
        lang: &str,
        ord: i64,
    ) -> Result<Option<String>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM skills WHERE lang = ?1 AND ord >= 0 AND ord < ?2
                 ORDER BY ord DESC LIMIT 1",
                params![lang, ord],
                |row| row.get(0),
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::parse_syllabus;
    use crate::content::test_support::{EXAMPLE_SYLLABUS, example_forms};
    use crate::srs::{DAY_MS, replay};

    fn example() -> Syllabus {
        parse_syllabus("valid-es.yaml", EXAMPLE_SYLLABUS).unwrap()
    }

    fn ctx() -> ReviewContext {
        ReviewContext {
            exercise_type: "translate".into(),
            verdict: Verdict::Correct,
            response_ms: Some(1_200),
            session_id: "s1".into(),
        }
    }

    fn started() -> Store {
        let mut store = Store::open_in_memory().unwrap();
        store
            .start_course(&example(), &example_forms(), 1_000)
            .unwrap();
        store
    }

    fn count(store: &Store, sql: &str) -> i64 {
        store.conn.query_row(sql, [], |row| row.get(0)).unwrap()
    }

    #[test]
    fn a_new_file_gets_the_seven_tables_and_version_1() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("parley.sqlite3");
        let store = Store::open(&path).unwrap();
        assert_eq!(store.user_version().unwrap(), 1);
        let tables = count(
            &store,
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN
             ('meta','courses','skills','items','accepted_answers','cards','review_log')",
        );
        assert_eq!(tables, 7);
        let foreign_keys = count(&store, "PRAGMA foreign_keys");
        assert_eq!(foreign_keys, 1);
        drop(store);
        let again = Store::open(&path).unwrap();
        assert_eq!(again.user_version().unwrap(), 1, "reopening runs nothing");
    }

    #[test]
    fn a_newer_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("parley.sqlite3");
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
        drop(conn);
        let err = Store::open(&path).unwrap_err();
        assert_eq!(err, StoreError::TooNew(99));
        assert_eq!(
            err.to_string(),
            "This data file was written by a newer Parley. Update Parley to open it."
        );
    }

    #[test]
    fn a_failed_migration_leaves_the_file_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("parley.sqlite3");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        drop(conn);
        let err = Store::open(&path).unwrap_err();
        assert_eq!(err, StoreError::Migration(1));
        assert_eq!(
            err.to_string(),
            "Parley could not update its data file (step 1). Restore a backup or report this."
        );
        let conn = Connection::open(&path).unwrap();
        let tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!((tables, version), (1, 0));
    }

    #[test]
    fn starting_a_course_twice_changes_nothing() {
        let mut store = Store::open_in_memory().unwrap();
        let first = store
            .start_course(&example(), &example_forms(), 1_000)
            .unwrap();
        assert_eq!(
            first,
            CourseReport {
                items: 11,
                added: 11,
                retired: 0
            }
        );
        let answers = count(&store, "SELECT COUNT(*) FROM accepted_answers");
        let second = store
            .start_course(&example(), &example_forms(), 9_000)
            .unwrap();
        assert_eq!(
            second,
            CourseReport {
                items: 11,
                added: 0,
                retired: 0
            }
        );
        assert_eq!(store.item_count(Lang::Es).unwrap(), 11);
        assert_eq!(
            count(&store, "SELECT COUNT(*) FROM accepted_answers"),
            answers
        );
        assert_eq!(
            count(&store, "SELECT MAX(created_at) FROM accepted_answers"),
            1_000
        );
        assert_eq!(count(&store, "SELECT MAX(created_at) FROM items"), 1_000);
        assert_eq!(count(&store, "SELECT started_at FROM courses"), 1_000);
        let norm: String = store
            .conn
            .query_row(
                "SELECT norm FROM accepted_answers WHERE item_id = 'es.a1.u01.s01#e:1'
                 AND direction = 'to_target'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(norm, "hola, ¿qué tal?");
    }

    #[test]
    fn a_removed_example_is_retired_and_comes_back() {
        let mut store = started();
        let removed = "es.a1.u01.s01#e:4";
        store
            .review(
                removed,
                Facet::Receptive,
                Rating::Good,
                2_000,
                &ctx(),
                &SrsParams::default(),
            )
            .unwrap();
        let mut v2 = example();
        v2.version = 2;
        v2.units[0].skills[0].examples.retain(|e| e.id != 4);
        let report = store.start_course(&v2, &example_forms(), 3_000).unwrap();
        assert_eq!(
            report,
            CourseReport {
                items: 10,
                added: 0,
                retired: 1
            }
        );
        assert!(store.is_retired(removed).unwrap());
        assert_eq!(store.item_count(Lang::Es).unwrap(), 10);
        assert_eq!(
            store.cards().unwrap().len(),
            2,
            "the retired item keeps its cards"
        );
        assert_eq!(store.review_log().unwrap().len(), 1);
        assert!(
            !store
                .item_ids("es.a1.u01.s01")
                .unwrap()
                .contains(&removed.to_string())
        );
        assert_eq!(count(&store, "SELECT syllabus_version FROM courses"), 2);
        let back = store
            .start_course(&example(), &example_forms(), 4_000)
            .unwrap();
        assert_eq!(back.added, 1);
        assert!(!store.is_retired(removed).unwrap());
    }

    #[test]
    fn a_skill_gone_from_the_syllabus_keeps_its_row() {
        let mut store = started();
        let mut two = example();
        let mut second = two.units[0].skills[0].clone();
        second.id = "es.a1.u01.s02".into();
        two.units[0].skills.push(second);
        store.start_course(&two, &example_forms(), 2_000).unwrap();
        assert_eq!(store.skills(Lang::Es).unwrap().len(), 2);
        store
            .start_course(&example(), &example_forms(), 3_000)
            .unwrap();
        let skills = store.skills(Lang::Es).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(store.skill_row("es.a1.u01.s02").unwrap().ord, -1);
    }

    #[test]
    fn introducing_creates_two_new_cards_once() {
        let mut store = started();
        let id = "es.a1.u01.s01#l:hola";
        assert!(store.introduce(id, 2_000).unwrap());
        assert!(!store.introduce(id, 3_000).unwrap());
        let cards = store.cards().unwrap();
        assert_eq!(
            cards,
            [
                Card::new(id, Facet::Receptive),
                Card::new(id, Facet::Productive)
            ]
        );
        assert_eq!(
            store.introduce("nope", 0).unwrap_err().to_string(),
            "unknown item nope"
        );
        assert_eq!(
            store.card("es.a1.u01.s01#l:tal", Facet::Receptive).unwrap(),
            None
        );
    }

    #[test]
    fn items_come_in_syllabus_order() {
        let store = started();
        let ids = store.item_ids("es.a1.u01.s01").unwrap();
        assert_eq!(
            ids.first().map(String::as_str),
            Some("es.a1.u01.s01#l:hola")
        );
        assert_eq!(ids.last().map(String::as_str), Some("es.a1.u01.s01#e:4"));
        assert_eq!(ids.len(), 11);
        assert_eq!(
            store.item_ids("es.a1.u09.s01").unwrap_err(),
            StoreError::UnknownSkill("es.a1.u09.s01".into())
        );
        assert_eq!(
            store.is_retired("nope").unwrap_err(),
            StoreError::UnknownItem("nope".into())
        );
    }

    #[test]
    fn reviews_are_logged_and_replay_to_the_cards() {
        let mut store = started();
        let p = SrsParams::default();
        let ids = store.item_ids("es.a1.u01.s01").unwrap();
        let ratings = [Rating::Good, Rating::Again, Rating::Hard, Rating::Easy];
        let mut now = 10_000;
        for round in 0..6 {
            for (i, id) in ids.iter().enumerate() {
                let facet = Facet::ALL[(i + round) % 2];
                let rating = ratings[(i * 7 + round * 3) % 4];
                store.review(id, facet, rating, now, &ctx(), &p).unwrap();
                now += DAY_MS / 3 + i as i64 * 1_000;
            }
        }
        let log = store.review_log().unwrap();
        assert_eq!(log.len(), 66);
        let reviewed: Vec<Card> = store
            .cards()
            .unwrap()
            .into_iter()
            .filter(|card| card.reps > 0)
            .collect();
        assert_eq!(replay(&log, &p), reviewed);
        let first = &log[0];
        assert_eq!(first.exercise_type, "translate");
        assert_eq!(first.response_ms, Some(1_200));
        assert_eq!(first.elapsed_days, 0.0);
        assert_eq!(
            store
                .review("nope", Facet::Receptive, Rating::Good, now, &ctx(), &p)
                .unwrap_err(),
            StoreError::UnknownItem("nope".into())
        );
        assert_eq!(
            store.review_log().unwrap().len(),
            66,
            "a refused review logs nothing"
        );
    }

    #[test]
    fn placement_tests_out_skills_with_one_good_review_per_card() {
        let mut store = started();
        let p = SrsParams::default();
        let id = "es.a1.u01.s01#l:hola";
        store
            .review(id, Facet::Receptive, Rating::Again, 500, &ctx(), &p)
            .unwrap();
        let result = PlacementResult {
            questions: vec![0],
            answers: vec![true],
            reversals: vec![],
            estimate: 1,
            label: "Pre-A1".into(),
            tested_out: vec!["es.a1.u01.s01".into()],
        };
        store
            .apply_placement(Lang::Es, &result, 5 * DAY_MS, "place-1", &p)
            .unwrap();
        let log = store.review_log().unwrap();
        assert_eq!(log.len(), 1 + 21, "every card but the one already reviewed");
        assert!(log[1..].iter().all(|e| e.exercise_type == "placement"
            && e.rating == Rating::Good
            && e.verdict == Verdict::Correct
            && e.response_ms.is_none()
            && e.session_id == "place-1"));
        let productive = store.card(id, Facet::Productive).unwrap().unwrap();
        assert_eq!(productive.due, Some(5 * DAY_MS + 2 * DAY_MS));
        assert!(store.skill_row("es.a1.u01.s01").unwrap().tested_out);
        assert_eq!(count(&store, "SELECT placement_index FROM courses"), 1);
        let unknown = PlacementResult {
            tested_out: vec!["es.a1.u09.s01".into()],
            ..result
        };
        assert_eq!(
            store
                .apply_placement(Lang::Es, &unknown, 6 * DAY_MS, "place-2", &p)
                .unwrap_err(),
            StoreError::UnknownSkill("es.a1.u09.s01".into())
        );
        assert_eq!(store.review_log().unwrap().len(), 22);
    }

    #[test]
    fn values_that_are_not_ours_are_refused_when_read() {
        let store = started();
        store
            .conn
            .execute_batch("PRAGMA ignore_check_constraints = ON")
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO cards (item_id, facet, state) VALUES ('es.a1.u01.s01#l:hola', 'sideways', 'new')",
                [],
            )
            .unwrap();
        let err = store.cards().unwrap_err();
        assert!(err.to_string().contains("sideways"), "{err}");
    }
}
