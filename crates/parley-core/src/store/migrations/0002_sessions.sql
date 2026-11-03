-- Migration 2 (module 03): sessions, stored lessons, cached judgements, the daily record and
-- freeze events. The statements are docs/ARCHITECTURE.md §4 verbatim.
CREATE TABLE sessions (id TEXT PRIMARY KEY, lang TEXT NOT NULL, kind TEXT NOT NULL CHECK (kind IN
  ('lesson','review','placement','story','conversation','media')), skill_id TEXT,
  started_at INTEGER NOT NULL, ended_at INTEGER, local_date TEXT NOT NULL, xp INTEGER NOT NULL DEFAULT 0,
  exercises INTEGER NOT NULL DEFAULT 0, correct INTEGER NOT NULL DEFAULT 0,
  cost_micros INTEGER NOT NULL DEFAULT 0);
CREATE INDEX sessions_date ON sessions(local_date);
CREATE TABLE lessons (id TEXT PRIMARY KEY, lang TEXT NOT NULL, skill_id TEXT NOT NULL, seq INTEGER NOT NULL,
  origin TEXT NOT NULL CHECK (origin IN ('ai','template','course_pack')), prompt_id TEXT,
  content TEXT NOT NULL,                 -- JSON, schemas/lesson.schema.json
  created_at INTEGER NOT NULL);
CREATE UNIQUE INDEX lessons_skill_seq ON lessons(skill_id, seq);
CREATE TABLE judgements (key TEXT PRIMARY KEY,   -- sha256 hex of (lang, direction, source sentence, answer_norm)
  verdict TEXT NOT NULL CHECK (verdict IN ('correct','almost','wrong')), corrected TEXT NOT NULL,
  explanation TEXT NOT NULL, tags TEXT NOT NULL, provider TEXT NOT NULL, created_at INTEGER NOT NULL);
CREATE TABLE daily (local_date TEXT PRIMARY KEY, xp INTEGER NOT NULL, sessions INTEGER NOT NULL,
  goal_xp INTEGER NOT NULL, frozen INTEGER NOT NULL DEFAULT 0);
CREATE TABLE freeze_events (id INTEGER PRIMARY KEY, local_date TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('earned','used')), streak_len INTEGER NOT NULL);
