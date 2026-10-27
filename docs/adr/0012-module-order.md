# ADR-0012: Module order: the offline app before publik API

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

Fifteen modules build Parley (`docs/ROADMAP.md`). The AI features are the visible novelty, but a
fresh install starts at $0.00 and must be fully useful before any provider is set up
(`docs/PUBLIK-CONTRACT.md` §1). Every AI feature is anchored on the syllabus and the sentence packs,
and every one of them needs a real interface to be tested by hand.

## Decision

Modules 01 to 04 build the complete offline app first: content formats, the scheduler and
placement, the exercise engine and grading, then the app shell with onboarding, the path, lessons,
reviews, stats, settings and backup. Content authoring (05) follows, so the course can be read in
the app as it grows. The publik module (06) comes before the first module that makes an AI call
(07); judging (08), listening (09), speaking (10), conversation (11), photo and media (12), Android
(13) and packaging (14) follow in dependency order.

## Consequences

- After four modules there is an app that teaches, grades and schedules with no provider at all,
  which is exactly what a fresh install has.
- Every later AI feature can be tried by hand in the real interface the day it lands.
- The live publik path waits for the maintainer's one-time listing and token step after module 06;
  until then it is tested against a fake server.
- Cost: the most distinctive features arrive late.

## Alternatives considered

- **Interface last**: the AI features would be untestable by hand for months, and the offline
  fallbacks would be designed after the features they fall back from. Rejected.
