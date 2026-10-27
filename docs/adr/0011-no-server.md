# ADR-0011: No server: streaks, XP and records are local; no leagues

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

Duolingo's leagues, friends and leaderboards need accounts, a server, moderation and an
operations budget, and they are the surface through which most of its pressure mechanics
(energy, paid streak repairs) arrive. Parley's promise is a free app with no account that keeps
every byte of a learner's data on their machine (`docs/PRODUCT.md` §6).

## Decision

Parley runs no server of its own. The streak, freezes, XP, the daily goal and the weekly record are
computed locally from the session log; "New weekly record!" compares the learner with themselves.
There are no leagues, friends or leaderboards, and no accounts or sync in v1 (folder sync is a
later idea).

## Consequences

- No accounts, no passwords, no data breach to have, no moderation, no hosting cost.
- The only outbound requests are the ones `SECURITY.md` lists: pack and model downloads, an optional
  update check, and AI calls to the provider the learner chose.
- Learners who want competition do not get it here.

## Alternatives considered

- **A leaderboard server**: accounts, moderation and cost, and the pressure mechanics that come
  with it. Rejected.
