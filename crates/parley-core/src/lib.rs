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
//! learner's SQLite file). Module 03 adds [`grading`] (the normaliser and the offline grader),
//! [`exercise`] (exercise types, structural checks, template lessons, the composer and the lesson
//! runner) and [`game`] (XP, streaks with earned freezes, the weekly record), with sessions,
//! lessons, judgements and the daily record in the store. Module 04 adds [`app`] (the service
//! every desktop command calls), [`settings`] and [`backup`]. Module 06 adds [`ai`]: the client
//! trait every AI feature calls, the call pipeline with its cache and idempotency keys, prompt
//! files and their schemas. Later modules add the AI features, one file each under `ai`.

#![forbid(unsafe_code)]

pub mod ai;
pub mod api;
pub mod app;
pub mod backup;
pub mod content;
pub mod coursepack;
pub mod env;
pub mod error;
pub mod exercise;
pub mod game;
pub mod grading;
pub mod settings;
pub mod srs;
pub mod store;
