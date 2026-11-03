//! What the app's screens read from the store besides cards and the log: the course rows, a
//! skill's place in the course, due counts, finished sessions and the counts the stats page shows
//! (docs/modules/04-app-shell-and-offline-course.md, Design notes).

use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use super::{Store, StoreError};
use crate::content::Lang;
use crate::srs::Card;

/// One row of `courses`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CourseRow {
    pub lang: Lang,
    pub syllabus_version: i64,
    /// The installed sentence pack's version, if any.
    pub pack_version: Option<String>,
    pub started_at: i64,
    /// The skill index the placement test placed the learner at.
    pub placement_index: Option<i64>,
    pub cefr_estimate: Option<String>,
}

/// A skill's row in `skills`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillInfo {
    pub id: String,
    pub lang: String,
    pub unit_id: String,
    /// The position in syllabus order; −1 for a skill gone from the syllabus.
    pub ord: i64,
    pub title: String,
    pub tested_out: bool,
}

/// Reviews of a period: how many, and how many were recalled (rated Hard, Good or Easy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct ReviewCounts {
    pub reviews: u32,
    pub recalled: u32,
}

const COURSE_COLUMNS: &str =
    "lang, syllabus_version, pack_version, started_at, placement_index, cefr_estimate";

fn course_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CourseRow> {
    Ok(CourseRow {
        lang: row.get(0)?,
        syllabus_version: row.get(1)?,
        pack_version: row.get(2)?,
        started_at: row.get(3)?,
        placement_index: row.get(4)?,
        cefr_estimate: row.get(5)?,
    })
}

impl Store {
    /// The course of `lang`, when it has been started.
    pub fn course(&self, lang: Lang) -> Result<Option<CourseRow>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COURSE_COLUMNS} FROM courses WHERE lang = ?1"),
                [lang.as_str()],
                course_from_row,
            )
            .optional()?)
    }

    /// Every started course, by language code.
    pub fn courses(&self) -> Result<Vec<CourseRow>, StoreError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {COURSE_COLUMNS} FROM courses ORDER BY lang"
        ))?;
        let rows = statement
            .query_map([], course_from_row)?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Records the installed pack of a started course (none: no pack).
    pub fn set_pack_version(
        &mut self,
        lang: Lang,
        version: Option<&str>,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE courses SET pack_version = ?2 WHERE lang = ?1",
            params![lang.as_str(), version],
        )?;
        Ok(())
    }

    /// A skill's row; `UnknownSkill` when the store has no such skill.
    pub fn skill_info(&self, skill_id: &str) -> Result<SkillInfo, StoreError> {
        self.conn
            .query_row(
                "SELECT id, lang, unit_id, ord, title, tested_out FROM skills WHERE id = ?1",
                [skill_id],
                |row| {
                    Ok(SkillInfo {
                        id: row.get(0)?,
                        lang: row.get(1)?,
                        unit_id: row.get(2)?,
                        ord: row.get(3)?,
                        title: row.get(4)?,
                        tested_out: row.get::<_, i64>(5)? != 0,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownSkill(skill_id.to_string()))
    }

    /// How many cards of the course's items not retired are due at `now_ms`.
    pub fn due_count(&self, lang: Lang, now_ms: i64) -> Result<usize, StoreError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM cards c JOIN items i ON i.id = c.item_id
             WHERE i.lang = ?1 AND i.retired = 0 AND c.due IS NOT NULL AND c.due <= ?2",
            params![lang.as_str(), now_ms],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as usize)
    }

    /// The earliest `due` after `now_ms` of a card of the course's items not retired.
    pub fn next_due_after(&self, lang: Lang, now_ms: i64) -> Result<Option<i64>, StoreError> {
        Ok(self.conn.query_row(
            "SELECT MIN(c.due) FROM cards c JOIN items i ON i.id = c.item_id
             WHERE i.lang = ?1 AND i.retired = 0 AND c.due IS NOT NULL AND c.due > ?2",
            params![lang.as_str(), now_ms],
            |row| row.get(0),
        )?)
    }

    /// Whether the course has a session that ended.
    pub fn has_finished_session(&self, lang: Lang) -> Result<bool, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM sessions WHERE lang = ?1 AND ended_at IS NOT NULL LIMIT 1",
                [lang.as_str()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// How many vocabulary items of the course are not retired.
    pub fn vocab_item_count(&self, lang: Lang) -> Result<u32, StoreError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM items WHERE lang = ?1 AND kind = 'vocab' AND retired = 0",
            [lang.as_str()],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as u32)
    }

    /// The productive cards of the course's vocabulary items not retired, by item id.
    pub fn vocab_productive_cards(&self, lang: Lang) -> Result<Vec<Card>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT c.item_id, c.facet, c.state, c.stability, c.difficulty, c.due, c.last_review,
               c.reps, c.lapses
             FROM cards c JOIN items i ON i.id = c.item_id
             WHERE i.lang = ?1 AND i.kind = 'vocab' AND i.retired = 0 AND c.facet = 'productive'
             ORDER BY c.item_id",
        )?;
        let cards = statement
            .query_map([lang.as_str()], |row| {
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
            })?
            .collect::<Result<_, _>>()?;
        Ok(cards)
    }

    /// The review-log rows of the course's items with `after < reviewed_at ≤ until` whose
    /// exercise type is not `placement`, and how many of them were rated Hard, Good or Easy.
    pub fn review_counts(
        &self,
        lang: Lang,
        after: i64,
        until: i64,
    ) -> Result<ReviewCounts, StoreError> {
        let (reviews, recalled): (i64, Option<i64>) = self.conn.query_row(
            "SELECT COUNT(*), SUM(CASE WHEN r.rating >= 2 THEN 1 ELSE 0 END)
             FROM review_log r JOIN items i ON i.id = r.item_id
             WHERE i.lang = ?1 AND r.reviewed_at > ?2 AND r.reviewed_at <= ?3
               AND r.exercise_type <> 'placement'",
            params![lang.as_str(), after, until],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(ReviewCounts {
            reviews: reviews.max(0) as u32,
            recalled: recalled.unwrap_or(0).max(0) as u32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::parse_syllabus;
    use crate::content::test_support::{EXAMPLE_SYLLABUS, example_forms};
    use crate::srs::{DAY_MS, Facet, Rating, SrsParams, Verdict};
    use crate::store::ReviewContext;

    fn started() -> Store {
        let mut store = Store::open_in_memory().unwrap();
        let syllabus = parse_syllabus("valid-es.yaml", EXAMPLE_SYLLABUS).unwrap();
        store
            .start_course(&syllabus, &example_forms(), 1_000)
            .unwrap();
        store
    }

    fn ctx(exercise_type: &str) -> ReviewContext {
        ReviewContext {
            exercise_type: exercise_type.into(),
            verdict: Verdict::Correct,
            response_ms: None,
            session_id: "s".into(),
        }
    }

    #[test]
    fn course_rows_and_pack_versions() {
        let mut store = started();
        assert_eq!(store.course(Lang::Fr).unwrap(), None);
        let row = store.course(Lang::Es).unwrap().unwrap();
        assert_eq!((row.started_at, row.pack_version), (1_000, None));
        store
            .set_pack_version(Lang::Es, Some("2026-10-01"))
            .unwrap();
        assert_eq!(
            store.courses().unwrap()[0].pack_version.as_deref(),
            Some("2026-10-01")
        );
        let skill = store.skill_info("es.a1.u01.s01").unwrap();
        assert_eq!((skill.ord, skill.tested_out), (0, false));
        assert_eq!(
            store.skill_info("nope").unwrap_err(),
            StoreError::UnknownSkill("nope".into())
        );
    }

    #[test]
    fn due_counts_and_review_counts() {
        let mut store = started();
        let p = SrsParams::default();
        let hola = "es.a1.u01.s01#l:hola";
        store
            .review(
                hola,
                Facet::Receptive,
                Rating::Again,
                0,
                &ctx("pick_meaning"),
                &p,
            )
            .unwrap();
        store
            .review(
                hola,
                Facet::Productive,
                Rating::Good,
                0,
                &ctx("placement"),
                &p,
            )
            .unwrap();
        assert_eq!(store.due_count(Lang::Es, 30 * DAY_MS).unwrap(), 2);
        assert_eq!(store.due_count(Lang::Es, 0).unwrap(), 0);
        let next = store.next_due_after(Lang::Es, 0).unwrap().unwrap();
        assert!(next > 0);
        let counts = store.review_counts(Lang::Es, -1, DAY_MS).unwrap();
        assert_eq!(
            counts,
            ReviewCounts {
                reviews: 1,
                recalled: 0
            },
            "placement reviews are left out"
        );
        assert_eq!(store.vocab_productive_cards(Lang::Es).unwrap().len(), 1);
        assert_eq!(store.vocab_item_count(Lang::Es).unwrap(), 7);
        assert!(!store.has_finished_session(Lang::Es).unwrap());
    }
}
