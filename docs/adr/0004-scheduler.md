# ADR-0004: Scheduler: FSRS with two cards per item

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

Reviews are the core of retention. The scheduler decides when each item comes back, and a learner
feels every mistake it makes as either boredom (too early) or forgetting (too late). The desired
retention must be a setting (80% to 95%, default 90%), the card state must be rebuildable from an
append-only log (invariant I6), and the expected values in tests must come from a reference, not
from the implementation.

## Decision

FSRS (Ye, Su and Cao, "A Stochastic Shortest Path Algorithm for Optimizing Spaced Repetition
Scheduling", KDD 2022; the algorithm description in the open-spaced-repetition wiki), through the
`fsrs` crate (BSD-3-Clause, re-checked at module 02) with its default parameters, no fuzz, a
maximum interval of 365 days. Every item has two cards, receptive (recognise the target language)
and productive (produce it), because the two skills decay independently. Reference values are
recorded once from `py-fsrs` with its version.

## Consequences

- The best open scheduler available, with a published model and a maintained reference
  implementation to test against.
- Two cards per item doubles the review log and lets a learner be strong at reading and weak at
  producing without the scheduler hiding it.
- The crate's parameter vector is pinned in the reference file; upgrading the crate means re-running
  the reference and reading the diff.

## Alternatives considered

- **SM-2**: simple, but measurably weaker at predicting recall.
- **Leitner boxes**: no notion of difficulty or desired retention.
- **Half-life regression**: no maintained open implementation to use as a reference.
