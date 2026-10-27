//! Parley's core library: everything the app knows about learning.
//!
//! The core is deterministic. It never reads the clock, randomness, the filesystem or the network
//! directly: they arrive through the traits in [`env`], so every test can run offline with fakes
//! (docs/ARCHITECTURE.md, invariant I3). It also has no HTTP code of its own: AI calls go through
//! a client trait that the `parley-publik` crate implements.
//!
//! Module 00 lays down the shape only: the environment traits, the error type every command
//! returns, and the command result types. Later modules add content, scheduling, grading,
//! exercises, storage and the AI features, one folder each.

#![forbid(unsafe_code)]

pub mod api;
pub mod env;
pub mod error;
