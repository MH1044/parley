# ADR-0006: Storage: one SQLite file plus derived caches, JSON backup

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

A learner's progress is the one thing that must never be corrupted or lost: cards, an append-only
review log, lessons, judgements, conversations. Writes happen mid-lesson, on every answer, and a
crash must not leave half a lesson. The app has no server (ADR-0011), so the local store is the
source of truth, and the learner must be able to carry it to another computer.

## Decision

Everything the learner owns lives in one SQLite database, `parley.sqlite3`, through `rusqlite` with
the bundled SQLite, migrated by numbered SQL files with `PRAGMA user_version`. Derived data lives
beside it in separate files that are safe to delete: sentence packs (`packs/`), the AI response
cache (`cache/ai.sqlite3`), natural-voice audio, recordings, local models and the status object.
Settings are a JSON file that never contains a key. A backup is one JSON document
(`parley.backup/1`) with everything the learner owns and nothing derived; import replaces the current
data after a confirmation.

## Consequences

- Transactions: an answer, its card update, its log row and its XP commit together or not at all.
- One file to back up, one to delete; caches can be cleared without touching progress.
- Backups are plain JSON, readable by hand and by a later version (older backups keep importing as
  optional arrays are added).
- Cost: schema migrations must be written and tested for every table change, and the backup format
  is a public commitment.

## Alternatives considered

- **A server**: accounts, moderation, cost, and a privacy promise harder to keep. Rejected
  (ADR-0011).
- **Many JSON files**: no transactions, so a crash between two writes corrupts the relationship
  between a card and its log.
