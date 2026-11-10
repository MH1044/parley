//! Rewards (docs/modules/03-exercise-engine-and-grading.md): XP, the daily goal, streaks with
//! earned freezes, and the weekly record. All local (ADR-0011); no hearts, energy or lesson
//! limit (invariant I8), and freezes are only earned (invariant I7).

pub mod streak;
pub mod xp;

pub use streak::{
    DayRow, FreezeEvent, FreezeKind, StreakState, WEEKLY_RECORD, streak, weekly_record,
};
pub use xp::{SessionKind, SessionResult, story_xp, xp_for};
