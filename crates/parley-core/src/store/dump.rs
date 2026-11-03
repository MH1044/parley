//! The store's tables as backup rows (`parley.backup/1`, docs/modules/04-app-shell-and-offline-course.md):
//! reading every table in primary-key order, and writing the rows into a new store.
//!
//! Rows carry the table's columns, with the JSON-text columns (`items.en`, `items.lemmas`,
//! `judgements.tags`, `lessons.content`) as JSON values. Syllabus items and `content` accepted
//! answers are not part of a backup: they are rebuilt from the bundled course by `start_course`.

use std::collections::HashSet;

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use super::{Store, StoreError};

/// A course, with the ids of its skills marked tested out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CourseDump {
    pub lang: String,
    pub syllabus_version: i64,
    pub pack_version: Option<String>,
    pub started_at: i64,
    pub placement_index: Option<i64>,
    pub cefr_estimate: Option<String>,
    #[serde(default)]
    pub tested_out: Vec<String>,
}

/// An item that did not come from the syllabus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemDump {
    pub id: String,
    pub lang: String,
    pub skill_id: String,
    pub kind: String,
    pub target: String,
    pub en: Vec<String>,
    pub lemmas: Vec<String>,
    pub cefr: String,
    pub source_kind: String,
    pub source_ref: Option<String>,
    pub created_at: i64,
    #[serde(default)]
    pub retired: i64,
}

/// An accepted answer from the judge or the learner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerDump {
    pub item_id: String,
    pub direction: String,
    pub norm: String,
    pub display: String,
    pub source: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardDump {
    pub item_id: String,
    pub facet: String,
    pub state: String,
    pub stability: Option<f64>,
    pub difficulty: Option<f64>,
    pub due: Option<i64>,
    pub last_review: Option<i64>,
    #[serde(default)]
    pub reps: i64,
    #[serde(default)]
    pub lapses: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewDump {
    /// Always written; a hand-written backup may leave it out and the row gets the next id.
    pub id: Option<i64>,
    pub item_id: String,
    pub facet: String,
    pub reviewed_at: i64,
    pub rating: i64,
    pub exercise_type: String,
    pub verdict: String,
    pub response_ms: Option<i64>,
    pub elapsed_days: f64,
    pub stability_after: f64,
    pub difficulty_after: f64,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDump {
    pub id: String,
    pub lang: String,
    pub kind: String,
    pub skill_id: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub local_date: String,
    #[serde(default)]
    pub xp: i64,
    #[serde(default)]
    pub exercises: i64,
    #[serde(default)]
    pub correct: i64,
    #[serde(default)]
    pub cost_micros: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LessonDump {
    pub id: String,
    pub lang: String,
    pub skill_id: String,
    pub seq: i64,
    pub origin: String,
    pub prompt_id: Option<String>,
    pub content: serde_json::Value,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgementDump {
    pub key: String,
    pub verdict: String,
    pub corrected: String,
    pub explanation: String,
    pub tags: Vec<String>,
    pub provider: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DailyDump {
    pub local_date: String,
    pub xp: i64,
    pub sessions: i64,
    pub goal_xp: i64,
    #[serde(default)]
    pub frozen: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreezeDump {
    /// Always written; a hand-written backup may leave it out and the row gets the next id.
    pub id: Option<i64>,
    pub local_date: String,
    pub kind: String,
    pub streak_len: i64,
}

/// Every backed-up table, in the order a backup lists them. A table a backup leaves out is empty.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Tables {
    pub courses: Vec<CourseDump>,
    pub items: Vec<ItemDump>,
    pub accepted_answers: Vec<AnswerDump>,
    pub cards: Vec<CardDump>,
    pub review_log: Vec<ReviewDump>,
    pub sessions: Vec<SessionDump>,
    pub lessons: Vec<LessonDump>,
    pub judgements: Vec<JudgementDump>,
    pub daily: Vec<DailyDump>,
    pub freeze_events: Vec<FreezeDump>,
}

/// What writing the rows into a new store kept and skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RestoreReport {
    pub items: usize,
    pub cards: usize,
    pub reviews: usize,
    /// Cards whose item does not exist in the new store.
    pub skipped_cards: usize,
}

/// A JSON array of strings from a TEXT column.
fn json_list(text: &str) -> rusqlite::Result<Vec<String>> {
    serde_json::from_str(text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn json_value(text: &str) -> rusqlite::Result<serde_json::Value> {
    serde_json::from_str(text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn rows<T>(
    conn: &Connection,
    sql: &str,
    map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, StoreError> {
    let mut statement = conn.prepare(sql)?;
    let rows = statement.query_map([], map)?.collect::<Result<_, _>>()?;
    Ok(rows)
}

fn exists(conn: &Connection, sql: &str, key: &str) -> Result<bool, StoreError> {
    let mut statement = conn.prepare_cached(sql)?;
    Ok(statement.exists([key])?)
}

impl Store {
    /// Every backed-up table in primary-key order: cards receptive before productive, the review
    /// log and freeze events by id. Syllabus items and content answers are left out.
    pub fn dump(&self) -> Result<Tables, StoreError> {
        let conn = &self.conn;
        let tested_out: Vec<(String, String)> = rows(
            conn,
            "SELECT lang, id FROM skills WHERE tested_out = 1 ORDER BY lang, ord, id",
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut courses = rows(
            conn,
            "SELECT lang, syllabus_version, pack_version, started_at, placement_index,
               cefr_estimate FROM courses ORDER BY lang",
            |row| {
                Ok(CourseDump {
                    lang: row.get(0)?,
                    syllabus_version: row.get(1)?,
                    pack_version: row.get(2)?,
                    started_at: row.get(3)?,
                    placement_index: row.get(4)?,
                    cefr_estimate: row.get(5)?,
                    tested_out: Vec::new(),
                })
            },
        )?;
        for course in &mut courses {
            course.tested_out = tested_out
                .iter()
                .filter(|(lang, _)| *lang == course.lang)
                .map(|(_, id)| id.clone())
                .collect();
        }
        Ok(Tables {
            courses,
            items: rows(
                conn,
                "SELECT id, lang, skill_id, kind, target, en, lemmas, cefr, source_kind,
                   source_ref, created_at, retired
                 FROM items WHERE source_kind <> 'syllabus' ORDER BY id",
                |row| {
                    Ok(ItemDump {
                        id: row.get(0)?,
                        lang: row.get(1)?,
                        skill_id: row.get(2)?,
                        kind: row.get(3)?,
                        target: row.get(4)?,
                        en: json_list(&row.get::<_, String>(5)?)?,
                        lemmas: json_list(&row.get::<_, String>(6)?)?,
                        cefr: row.get(7)?,
                        source_kind: row.get(8)?,
                        source_ref: row.get(9)?,
                        created_at: row.get(10)?,
                        retired: row.get(11)?,
                    })
                },
            )?,
            accepted_answers: rows(
                conn,
                "SELECT item_id, direction, norm, display, source, created_at
                 FROM accepted_answers WHERE source <> 'content'
                 ORDER BY item_id, direction, norm",
                |row| {
                    Ok(AnswerDump {
                        item_id: row.get(0)?,
                        direction: row.get(1)?,
                        norm: row.get(2)?,
                        display: row.get(3)?,
                        source: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                },
            )?,
            cards: rows(
                conn,
                "SELECT item_id, facet, state, stability, difficulty, due, last_review, reps,
                   lapses
                 FROM cards ORDER BY item_id, CASE facet WHEN 'receptive' THEN 0 ELSE 1 END",
                |row| {
                    Ok(CardDump {
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
                },
            )?,
            review_log: rows(
                conn,
                "SELECT id, item_id, facet, reviewed_at, rating, exercise_type, verdict,
                   response_ms, elapsed_days, stability_after, difficulty_after, session_id
                 FROM review_log ORDER BY id",
                |row| {
                    Ok(ReviewDump {
                        id: row.get(0)?,
                        item_id: row.get(1)?,
                        facet: row.get(2)?,
                        reviewed_at: row.get(3)?,
                        rating: row.get(4)?,
                        exercise_type: row.get(5)?,
                        verdict: row.get(6)?,
                        response_ms: row.get(7)?,
                        elapsed_days: row.get(8)?,
                        stability_after: row.get(9)?,
                        difficulty_after: row.get(10)?,
                        session_id: row.get(11)?,
                    })
                },
            )?,
            sessions: rows(
                conn,
                "SELECT id, lang, kind, skill_id, started_at, ended_at, local_date, xp, exercises,
                   correct, cost_micros
                 FROM sessions ORDER BY id",
                |row| {
                    Ok(SessionDump {
                        id: row.get(0)?,
                        lang: row.get(1)?,
                        kind: row.get(2)?,
                        skill_id: row.get(3)?,
                        started_at: row.get(4)?,
                        ended_at: row.get(5)?,
                        local_date: row.get(6)?,
                        xp: row.get(7)?,
                        exercises: row.get(8)?,
                        correct: row.get(9)?,
                        cost_micros: row.get(10)?,
                    })
                },
            )?,
            lessons: rows(
                conn,
                "SELECT id, lang, skill_id, seq, origin, prompt_id, content, created_at
                 FROM lessons ORDER BY id",
                |row| {
                    Ok(LessonDump {
                        id: row.get(0)?,
                        lang: row.get(1)?,
                        skill_id: row.get(2)?,
                        seq: row.get(3)?,
                        origin: row.get(4)?,
                        prompt_id: row.get(5)?,
                        content: json_value(&row.get::<_, String>(6)?)?,
                        created_at: row.get(7)?,
                    })
                },
            )?,
            judgements: rows(
                conn,
                "SELECT key, verdict, corrected, explanation, tags, provider, created_at
                 FROM judgements ORDER BY key",
                |row| {
                    Ok(JudgementDump {
                        key: row.get(0)?,
                        verdict: row.get(1)?,
                        corrected: row.get(2)?,
                        explanation: row.get(3)?,
                        tags: json_list(&row.get::<_, String>(4)?)?,
                        provider: row.get(5)?,
                        created_at: row.get(6)?,
                    })
                },
            )?,
            daily: rows(
                conn,
                "SELECT local_date, xp, sessions, goal_xp, frozen FROM daily ORDER BY local_date",
                |row| {
                    Ok(DailyDump {
                        local_date: row.get(0)?,
                        xp: row.get(1)?,
                        sessions: row.get(2)?,
                        goal_xp: row.get(3)?,
                        frozen: row.get(4)?,
                    })
                },
            )?,
            freeze_events: rows(
                conn,
                "SELECT id, local_date, kind, streak_len FROM freeze_events ORDER BY id",
                |row| {
                    Ok(FreezeDump {
                        id: row.get(0)?,
                        local_date: row.get(1)?,
                        kind: row.get(2)?,
                        streak_len: row.get(3)?,
                    })
                },
            )?,
        })
    }

    /// Writes the course rows of a backup into this (new) store, before `start_course` brings
    /// each course up to date with the bundled syllabus.
    pub fn restore_courses(&mut self, courses: &[CourseDump]) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        for course in courses {
            tx.execute(
                "INSERT INTO courses (lang, syllabus_version, pack_version, started_at,
                   placement_index, cefr_estimate)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    course.lang,
                    course.syllabus_version,
                    course.pack_version,
                    course.started_at,
                    course.placement_index,
                    course.cefr_estimate
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Writes the other rows of a backup into this (new) store, after the courses are rebuilt,
    /// in one transaction: the tested-out flags, items, answers, cards, the review log,
    /// sessions, lessons, judgements, the daily record and freeze events. A card or an answer
    /// whose item does not exist, or an item whose skill does not, is skipped; skipped cards
    /// are counted.
    pub fn restore_rows(&mut self, tables: &Tables) -> Result<RestoreReport, StoreError> {
        let tx = self.conn.transaction()?;
        let mut report = RestoreReport::default();
        for course in &tables.courses {
            for skill_id in &course.tested_out {
                tx.execute(
                    "UPDATE skills SET tested_out = 1 WHERE id = ?1 AND lang = ?2",
                    params![skill_id, course.lang],
                )?;
            }
        }
        let skill_exists = "SELECT 1 FROM skills WHERE id = ?1";
        let item_exists = "SELECT 1 FROM items WHERE id = ?1";
        for item in &tables.items {
            if !exists(&tx, skill_exists, &item.skill_id)? {
                continue;
            }
            let en = serde_json::to_string(&item.en).expect("strings serialise");
            let lemmas = serde_json::to_string(&item.lemmas).expect("strings serialise");
            report.items += tx.execute(
                "INSERT INTO items (id, lang, skill_id, kind, target, en, lemmas, cefr,
                   source_kind, source_ref, created_at, retired)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT (id) DO NOTHING",
                params![
                    item.id,
                    item.lang,
                    item.skill_id,
                    item.kind,
                    item.target,
                    en,
                    lemmas,
                    item.cefr,
                    item.source_kind,
                    item.source_ref,
                    item.created_at,
                    item.retired
                ],
            )?;
        }
        for answer in &tables.accepted_answers {
            if !exists(&tx, item_exists, &answer.item_id)? {
                continue;
            }
            tx.execute(
                "INSERT INTO accepted_answers (item_id, direction, norm, display, source,
                   created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (item_id, direction, norm) DO NOTHING",
                params![
                    answer.item_id,
                    answer.direction,
                    answer.norm,
                    answer.display,
                    answer.source,
                    answer.created_at
                ],
            )?;
        }
        let mut carded: HashSet<(&str, &str)> = HashSet::new();
        for card in &tables.cards {
            if !exists(&tx, item_exists, &card.item_id)? {
                report.skipped_cards += 1;
                continue;
            }
            if !carded.insert((card.item_id.as_str(), card.facet.as_str())) {
                continue;
            }
            tx.execute(
                "INSERT INTO cards (item_id, facet, state, stability, difficulty, due,
                   last_review, reps, lapses)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    card.item_id,
                    card.facet,
                    card.state,
                    card.stability,
                    card.difficulty,
                    card.due,
                    card.last_review,
                    card.reps,
                    card.lapses
                ],
            )?;
            report.cards += 1;
        }
        for entry in &tables.review_log {
            tx.execute(
                "INSERT INTO review_log (id, item_id, facet, reviewed_at, rating, exercise_type,
                   verdict, response_ms, elapsed_days, stability_after, difficulty_after,
                   session_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    entry.id,
                    entry.item_id,
                    entry.facet,
                    entry.reviewed_at,
                    entry.rating,
                    entry.exercise_type,
                    entry.verdict,
                    entry.response_ms,
                    entry.elapsed_days,
                    entry.stability_after,
                    entry.difficulty_after,
                    entry.session_id
                ],
            )?;
            report.reviews += 1;
        }
        for session in &tables.sessions {
            tx.execute(
                "INSERT INTO sessions (id, lang, kind, skill_id, started_at, ended_at, local_date,
                   xp, exercises, correct, cost_micros)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    session.id,
                    session.lang,
                    session.kind,
                    session.skill_id,
                    session.started_at,
                    session.ended_at,
                    session.local_date,
                    session.xp,
                    session.exercises,
                    session.correct,
                    session.cost_micros
                ],
            )?;
        }
        for lesson in &tables.lessons {
            let content = serde_json::to_string(&lesson.content).expect("JSON serialises");
            tx.execute(
                "INSERT INTO lessons (id, lang, skill_id, seq, origin, prompt_id, content,
                   created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    lesson.id,
                    lesson.lang,
                    lesson.skill_id,
                    lesson.seq,
                    lesson.origin,
                    lesson.prompt_id,
                    content,
                    lesson.created_at
                ],
            )?;
        }
        for judgement in &tables.judgements {
            let tags = serde_json::to_string(&judgement.tags).expect("strings serialise");
            tx.execute(
                "INSERT INTO judgements (key, verdict, corrected, explanation, tags, provider,
                   created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    judgement.key,
                    judgement.verdict,
                    judgement.corrected,
                    judgement.explanation,
                    tags,
                    judgement.provider,
                    judgement.created_at
                ],
            )?;
        }
        for day in &tables.daily {
            tx.execute(
                "INSERT INTO daily (local_date, xp, sessions, goal_xp, frozen)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    day.local_date,
                    day.xp,
                    day.sessions,
                    day.goal_xp,
                    day.frozen
                ],
            )?;
        }
        for event in &tables.freeze_events {
            tx.execute(
                "INSERT INTO freeze_events (id, local_date, kind, streak_len)
                 VALUES (?1, ?2, ?3, ?4)",
                params![event.id, event.local_date, event.kind, event.streak_len],
            )?;
        }
        tx.commit()?;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::test_support::{EXAMPLE_SYLLABUS, example_forms};
    use crate::content::{Lang, parse_syllabus};
    use crate::grading::Direction;
    use crate::srs::{Facet, Rating, SrsParams, Verdict};
    use crate::store::{AnswerSource, ReviewContext};

    fn started() -> Store {
        let mut store = Store::open_in_memory().unwrap();
        let syllabus = parse_syllabus("valid-es.yaml", EXAMPLE_SYLLABUS).unwrap();
        store
            .start_course(&syllabus, &example_forms(), 1_000)
            .unwrap();
        store
    }

    #[test]
    fn a_dump_restores_to_the_same_dump() {
        let mut store = started();
        let p = SrsParams::default();
        let ctx = ReviewContext {
            exercise_type: "pick_meaning".into(),
            verdict: Verdict::Correct,
            response_ms: Some(900),
            session_id: "s-1".into(),
        };
        let hola = "es.a1.u01.s01#l:hola";
        store
            .review(hola, Facet::Receptive, Rating::Good, 5_000, &ctx, &p)
            .unwrap();
        store
            .add_accepted_answer(
                "es.a1.u01.s01#e:2",
                Direction::ToEn,
                "Morning!",
                AnswerSource::Learner,
                6_000,
            )
            .unwrap();
        let tables = store.dump().unwrap();
        assert_eq!(tables.cards.len(), 2, "both cards of the reviewed item");
        assert_eq!(tables.cards[0].facet, "receptive");
        assert_eq!(
            tables.accepted_answers.len(),
            1,
            "content answers are left out"
        );
        assert!(tables.items.is_empty(), "syllabus items are left out");
        assert_eq!(tables.courses[0].lang, "es");

        let mut fresh = Store::open_in_memory().unwrap();
        fresh.restore_courses(&tables.courses).unwrap();
        let syllabus = parse_syllabus("valid-es.yaml", EXAMPLE_SYLLABUS).unwrap();
        fresh
            .start_course(&syllabus, &example_forms(), 9_000)
            .unwrap();
        let mut with_missing = tables.clone();
        let mut ghost = with_missing.cards[0].clone();
        ghost.item_id = "es.a1.u09.s09#l:nada".into();
        with_missing.cards.push(ghost);
        let report = fresh.restore_rows(&with_missing).unwrap();
        assert_eq!(
            report,
            RestoreReport {
                items: 0,
                cards: 2,
                reviews: 1,
                skipped_cards: 1
            }
        );
        assert_eq!(fresh.dump().unwrap(), tables);
        assert_eq!(fresh.course(Lang::Es).unwrap().unwrap().started_at, 1_000);
    }
}
