-- Migration 3 (module 07): graded stories, kept apart from lessons so a story never takes a
-- lesson's number. The statements are docs/ARCHITECTURE.md §4 verbatim.
CREATE TABLE stories (id TEXT PRIMARY KEY, lang TEXT NOT NULL, skill_id TEXT NOT NULL, seq INTEGER NOT NULL,
  origin TEXT NOT NULL CHECK (origin IN ('ai','course_pack')), prompt_id TEXT,
  content TEXT NOT NULL,                 -- JSON, schemas/ai/story.v1.schema.json
  created_at INTEGER NOT NULL);
CREATE UNIQUE INDEX stories_skill_seq ON stories(skill_id, seq);
