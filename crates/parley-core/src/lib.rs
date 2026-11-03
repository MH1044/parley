//! Parley's core library: everything the app knows about learning.
//!
//! The core is deterministic. It never reads the clock, randomness, the filesystem or the network
//! directly: they arrive through the traits in [`env`], so every test can run offline with fakes
//! (docs/ARCHITECTURE.md, invariant I3). It also has no HTTP code of its own: AI calls go through
//! a client trait that the `parley-publik` crate implements.
//!
//! Module 00 laid down the shape: the environment traits, the error type every command returns,
//! and the command result types. Module 01 adds [`content`]: the syllabus format, its validator,
//! the tokeniser, forms tables, item derivation, the id lock and sentence packs. Module 02 adds
//! [`srs`] (FSRS cards, the review log, mastery, skill states and placement) and [`store`] (the
//! learner's SQLite file). Later modules add grading, exercises and the AI features, one folder
//! each.

#![forbid(unsafe_code)]

pub mod api;
pub mod content;
pub mod env;
pub mod error;
pub mod srs;
pub mod store;
