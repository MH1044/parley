-- Migration 1 (module 02): the course, its skills and items, accepted answers, FSRS cards and
-- the append-only review log. The statements are docs/ARCHITECTURE.md §4 verbatim.
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE courses (lang TEXT PRIMARY KEY, syllabus_version INTEGER NOT NULL, pack_version TEXT,
  started_at INTEGER NOT NULL, placement_index INTEGER, cefr_estimate TEXT);
CREATE TABLE skills (id TEXT PRIMARY KEY, lang TEXT NOT NULL, unit_id TEXT NOT NULL, ord INTEGER NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('grammar','vocab','function','personal')), cefr TEXT NOT NULL,
  title TEXT NOT NULL, tested_out INTEGER NOT NULL DEFAULT 0);        -- rebuilt from syllabus.yaml
CREATE INDEX skills_lang_ord ON skills(lang, ord);
CREATE TABLE items (id TEXT PRIMARY KEY, lang TEXT NOT NULL, skill_id TEXT NOT NULL REFERENCES skills(id),
  kind TEXT NOT NULL CHECK (kind IN ('vocab','phrase','pattern')), target TEXT NOT NULL, en TEXT NOT NULL,
  lemmas TEXT NOT NULL,                  -- JSON array of lemma strings
  cefr TEXT NOT NULL, source_kind TEXT NOT NULL CHECK (source_kind IN
  ('syllabus','pack','conversation','photo','media','course_pack')), source_ref TEXT,
  created_at INTEGER NOT NULL, retired INTEGER NOT NULL DEFAULT 0);
CREATE INDEX items_skill ON items(skill_id);
CREATE TABLE accepted_answers (item_id TEXT NOT NULL REFERENCES items(id),
  direction TEXT NOT NULL CHECK (direction IN ('to_target','to_en')), norm TEXT NOT NULL,
  display TEXT NOT NULL, source TEXT NOT NULL CHECK (source IN ('content','judge','learner')),
  created_at INTEGER NOT NULL, PRIMARY KEY (item_id, direction, norm));
CREATE TABLE cards (item_id TEXT NOT NULL REFERENCES items(id),
  facet TEXT NOT NULL CHECK (facet IN ('receptive','productive')),
  state TEXT NOT NULL CHECK (state IN ('new','review','relearning')), stability REAL, difficulty REAL,
  due INTEGER, last_review INTEGER, reps INTEGER NOT NULL DEFAULT 0, lapses INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (item_id, facet));
CREATE INDEX cards_due ON cards(due);
CREATE TABLE review_log (id INTEGER PRIMARY KEY, item_id TEXT NOT NULL, facet TEXT NOT NULL,
  reviewed_at INTEGER NOT NULL, rating INTEGER NOT NULL CHECK (rating BETWEEN 1 AND 4),
  exercise_type TEXT NOT NULL, verdict TEXT NOT NULL, response_ms INTEGER, elapsed_days REAL NOT NULL,
  stability_after REAL NOT NULL, difficulty_after REAL NOT NULL, session_id TEXT NOT NULL);
CREATE INDEX review_log_item ON review_log(item_id, reviewed_at);
