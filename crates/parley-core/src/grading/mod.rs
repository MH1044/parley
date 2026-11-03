//! Grading (docs/modules/03-exercise-engine-and-grading.md): the normaliser, accent folding,
//! typo tolerance, the offline grader with its verdicts and messages, the store lookups it uses,
//! and "I was right".
//!
//! Grading is honest and forgiving in a fixed order (ADR-0005): an exact match, a missing or wrong
//! article, an accent slip, a one-letter typo in a few long words, a cached judgement; a free
//! translation nothing matches is `unchecked`, never guessed.

pub mod grade;
pub mod normalise;
pub mod typo;

pub use grade::{
    AccentLeniency, Answer, Direction, Grade, GradeCtx, NounInfo, StoredJudgement, Tag, Verdict,
    accept_learner_answer, grade, grade_ctx, judgement_key, noun_info,
};
pub use normalise::{fold_german, fold_marks, normalise};
pub use typo::osa_distance;
