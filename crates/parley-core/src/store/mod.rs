//! The learner's store, `parley.sqlite3` (docs/ARCHITECTURE.md §4): the course, its skills and
//! items, accepted answers, FSRS cards and the append-only review log
//! (docs/modules/02-srs-and-learner-model.md).
//!
//! SQLite cannot go through the [`crate::env::Fs`] trait, so the caller passes the path of the
//! file, as it does for sentence packs; the store never chooses a path itself. Each migration runs
//! in its own transaction and `PRAGMA user_version` records the newest one applied.

use std::fmt;

use crate::srs::Verdict;

mod db;

pub use db::Store;

/// What `start_course` changed. `items` is the syllabus's item count; `added` counts items that
/// were not active before the call (new, or back in the syllabus), `retired` those it retired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CourseReport {
    pub items: usize,
    pub added: usize,
    pub retired: usize,
}

/// Where a review came from, as the review log records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewContext {
    pub exercise_type: String,
    pub verdict: Verdict,
    pub response_ms: Option<i64>,
    pub session_id: String,
}

/// Why the store refused. The first two are what a learner sees when the data file cannot be
/// opened; the others name what was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// Migration step `n` failed and was rolled back; the file is as it was.
    Migration(u32),
    /// The file's `user_version` is newer than any migration this build knows.
    TooNew(u32),
    UnknownItem(String),
    UnknownSkill(String),
    /// Any other SQLite failure, with SQLite's message.
    Sqlite(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Migration(step) => write!(
                f,
                "Parley could not update its data file (step {step}). Restore a backup or report this."
            ),
            StoreError::TooNew(_) => f.write_str(
                "This data file was written by a newer Parley. Update Parley to open it.",
            ),
            StoreError::UnknownItem(id) => write!(f, "unknown item {id}"),
            StoreError::UnknownSkill(id) => write!(f, "unknown skill {id}"),
            StoreError::Sqlite(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sqlite(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_has_its_wording() {
        assert_eq!(
            StoreError::Migration(1).to_string(),
            "Parley could not update its data file (step 1). Restore a backup or report this."
        );
        assert_eq!(
            StoreError::TooNew(99).to_string(),
            "This data file was written by a newer Parley. Update Parley to open it."
        );
        assert_eq!(
            StoreError::UnknownItem("es.a1.u01.s01#l:hola".into()).to_string(),
            "unknown item es.a1.u01.s01#l:hola"
        );
        assert_eq!(
            StoreError::UnknownSkill("es.a1.u09.s01".into()).to_string(),
            "unknown skill es.a1.u09.s01"
        );
        assert_eq!(
            StoreError::Sqlite("disk I/O error".into()).to_string(),
            "disk I/O error"
        );
    }
}
