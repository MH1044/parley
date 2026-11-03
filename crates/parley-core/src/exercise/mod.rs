//! Exercises and lessons (docs/modules/03-exercise-engine-and-grading.md): the seven exercise
//! types, the structural checks L5 to L9 shared with AI lessons, the template generator and the
//! template lesson, the composer, word-bank tiles and the lesson runner.
//!
//! Everything is deterministic: randomness arrives as [`crate::env::Rng`], time as milliseconds
//! or a [`crate::env::Clock`], so a seed gives the same lesson on every machine.

pub mod check;
pub mod compose;
pub mod run;
pub mod template;
pub mod types;
pub mod wordbank;

pub use check::{Allowed, CheckCtx, CheckFailure, check_exercise, coverage, max_words};
pub use compose::{Capabilities, ComposeRequest, Plan, PlanEntry, compose};
pub use run::{LessonRun, LessonSummary, RunError, RunSettings, Submission};
pub use template::{TemplateCtx, template, template_lesson};
pub use types::{Body, Exercise, ExerciseType, Lesson, Origin};
pub use wordbank::word_bank;
