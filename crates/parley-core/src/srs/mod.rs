//! What the learner knows (docs/modules/02-srs-and-learner-model.md): FSRS cards and their
//! scheduling, the rating of a graded outcome, the append-only review log and its replay,
//! retrievability, the due queue, skill mastery and states, and the placement test.
//!
//! Everything here is deterministic: time arrives as milliseconds since the epoch and randomness
//! through [`crate::env::Rng`].

pub mod fsrs;
pub mod mastery;
pub mod placement;
pub mod rating;

pub use self::fsrs::{
    Card, CardState, DAY_MS, Facet, MAX_INTERVAL_DAYS, Rating, ReviewLog, ReviewLogEntry,
    Scheduled, SrsParams, default_weights, replay, retrievability, schedule,
};
pub use self::mastery::{
    CardRef, SkillState, due_queue, mastery_of, queue_order, skill_mastery, skill_state,
};
pub use self::placement::{Placement, PlacementResult, SkillRef, simulate};
pub use self::rating::{Outcome, Verdict, rating_for};
