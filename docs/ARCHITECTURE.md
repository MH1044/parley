# Parley: architecture

Five pages at most. Rewritten at milestones, not patched. Module specs in `docs/modules/` hold the
detail; this file holds the shape and the rules that must never break.

## 1. Stack

Every licence is checked again at the module that adds the dependency ("verify at NN").

| Component | Choice | Licence | Why |
|---|---|---|---|
| App shell | Tauri 2 | MIT OR Apache-2.0 (00) | Small installers, Rust core, Android through Tauri mobile |
| UI | React + TypeScript, Vite | MIT (00) | Widely known; typed command wrappers |
| UI tests, launch smoke | Vitest, Testing Library; tauri-driver + WebdriverIO | MIT; Apache-2.0 / MIT (verify 04) | Unit tests without a browser; the official Tauri end-to-end route |
| Database | `rusqlite` with bundled SQLite | MIT; SQLite public domain (02) | One file, transactional, offline |
| YAML | `serde_yaml_ng` or `serde-saphyr` (`serde_yaml` is archived) | MIT / Apache-2.0 (verify 01) | Syllabus files are hand-edited YAML |
| JSON Schema | `jsonschema` | MIT (verify 01) | Validates content, backups and every AI response |
| Unicode | `unicode-normalization`, `unicode-segmentation` | MIT / Apache-2.0 (01) | NFC and UAX #29 words |
| Edit distance | `strsim` | MIT (03) | Optimal string alignment distance for typos |
| Scheduler | `fsrs` (open-spaced-repetition) | BSD-3-Clause (repository page, 2026-09-29; recheck 02) | Current best open scheduler |
| Language ID | `whatlang` | MIT (repository page, 2026-09-29; recheck 07) | Pure Rust, covers en, es, fr, de |
| Stemming fallback | `rust-stemmers` (Snowball) | MIT / BSD-3-Clause (verify 07) | Coverage check for forms missing from the forms table |
| HTTP | `reqwest` with rustls | MIT OR Apache-2.0 (06) | One transport on desktop and Android; streaming |
| Own-key storage | `keyring` | MIT OR Apache-2.0 (verify 06) | OS keychain on desktop |
| Hashing | `sha2` | MIT OR Apache-2.0 (01) | Cache keys, pack checksums |
| Audio I/O | `cpal` (capture), `rodio` (playback) | Apache-2.0; MIT OR Apache-2.0 (verify 09, 10) | Cross-platform, Android support |
| Resampling, WAV | `rubato`, `hound` | MIT; Apache-2.0 (verify 10) | 16 kHz mono for transcription |
| OS voices | `tts` 0.27 | MIT (Cargo.toml, 2026-09-29; recheck 09) | WinRT/SAPI, AVFoundation, Speech Dispatcher, Android; optional Tolk backend left off |
| Local transcription | `whisper-rs` over whisper.cpp, ggml Whisper models | Unlicense (moved to Codeberg); MIT; MIT (verify 10) | Offline speaking checks |
| Media decoding | `symphonia` | MPL-2.0 (verify 12) | Pure Rust; MP3, AAC, FLAC, Vorbis, WAV in MP4, MKV, OGG, WAV containers |
| Images | `image` | MIT OR Apache-2.0 (verify 12) | Resize to 1568 px; re-encoding drops all metadata |
| Test references | `proptest`; `py-fsrs` run once with `uv` | MIT OR Apache-2.0; MIT (verify 02) | Property tests; recorded FSRS reference values |
| Sentences | Tatoeba exports | CC BY 2.0 FR (tatoeba wiki, 2026-09-29) | Real sentences with translations and authors |
| Word forms | Wiktionary through wiktextract (kaikki.org) | CC BY-SA 4.0 and GFDL (verify 01) | Inflected form to lemma |
| Frequency | counted from Tatoeba (default); FrequencyWords as the alternative | CC BY 2.0 FR; CC BY-SA 4.0 (verified 2026-09-29) | No permissive (CC0 or CC BY) list was found; module 01 decides |

## 2. Repository layout

    Cargo.toml               workspace; version shared by every crate
    rust-toolchain.toml      pinned stable toolchain
    deny.toml                licence gate: fails on GPL, AGPL, SSPL, BUSL (transitive included)
    crates/
      parley-core/           library: content, srs, grading, exercises, game, store, backup, ai features
        src/content/         syllabus.rs, items.rs, forms.rs, tokenise.rs, pack.rs, validate.rs
        src/srs/             fsrs.rs, mastery.rs, placement.rs
        src/grading/         normalise.rs, typo.rs, grade.rs
        src/exercise/        types.rs, template.rs, compose.rs, wordbank.rs
        src/game/            xp.rs, streak.rs
        src/store/           db.rs, migrations/NNNN_name.sql
        src/ai/              client.rs (AiClient trait), call.rs, cache.rs, prompts.rs, schema.rs,
                             validate.rs, langid.rs, lesson.rs, story.rs, judge.rs, explain.rs,
                             speech.rs, transcribe.rs, roleplay.rs, photo.rs, translate.rs, media.rs
        src/speech/          align.rs, score.rs (speaking checks)
        src/api.rs           command argument and result types, mirrored in app/src/api/commands.ts
        src/env.rs           Clock, Rng, Fs, Net traits (injected; never read directly)
        tests/               hygiene.rs, mNN_<slug>.rs per module
      parley-publik/         publik.rs (reference copy), http.rs, providers.rs, status.rs, errors.rs
        publik-app-token.txt the app token, public by design (next to this crate's Cargo.toml)
      parley-audio/          capture.rs, playback.rs, resample.rs, trim.rs, wav.rs, os_tts.rs,
                             whisper_local.rs (feature `local-whisper`), media_decode.rs, chunk.rs
      parley-cli/            binary `parley`; also `parley-fixtures`, the fixture generator
        tests/golden.rs      runs tests/cases/*/ through the `parley` binary
    app/
      package.json, vite.config.ts, tsconfig.json, index.html
      src/api/commands.ts    typed wrappers for every Tauri command and event
      src/screens/           Onboarding, Path, Lesson, Review, Stats, Conversation, Photo, Media, Settings
      src/components/        exercise views, ResultSheet, AiSetupDialog, BalanceLine, StreakBadge
      src/strings/en.json    every UI string (the copy-rule test reads this file)
      src-tauri/             Tauri crate: main.rs, commands/*.rs, state.rs, events.rs, capabilities/
    content/
      LICENSE                the course-content licence (ADR-0003)
      ATTRIBUTION.md         Tatoeba and Wiktionary credits
      es/ fr/ de/            syllabus.yaml, ids.lock, forms.tsv, numbers.yaml, exclude.txt, checklist.yaml, review.yaml
      scenarios/             cafe.yaml, directions.yaml, shop.yaml, hotel.yaml, doctor.yaml, job_interview.yaml
      packs.lock.toml        release-asset packs: URL, SHA-256, size per language
      voices.toml            natural-voice model and default voice per language (module 09)
    prompts/                 <feature>.v<N>.md, loaded at build time, never edited in place
    schemas/                 JSON Schemas: content, backup, course pack, and one per AI response
    models/registry.toml     local Whisper models: URL, SHA-256, size
    scripts/                 verify, verify.ps1, publik-proof, check-release-notes,
                             reference/ (py-fsrs reference script)
    scripts/gates/           run, run.ps1 (quality gates, created by module 03), thresholds.toml
    tests/cases/NN-slug/     golden cases: args.txt or input.json, expected.json
    tests/fixtures/          generated fixtures (by parley-fixtures) and hand-written small ones
    docs/                    this folder

## 3. Components and data flow

    React UI (webview) --invoke/listen--> app/src-tauri (commands, events, AppState)
      app/src-tauri --> parley-core   (logic, SQLite)  --> app data directory
      app/src-tauri --> parley-publik (implements AiClient) --> publik API | own key | localhost
      app/src-tauri --> parley-audio  (mic, speakers, OS voices, local Whisper, media decoding)
      parley-core   --> AiClient trait (injected; parley-core has no HTTP code of its own)

**UI.** Renders state and sends intents. It never holds a key, never opens a URL it made up, and
never touches the filesystem except through file-picker dialogs whose chosen path goes to a
command. **Tauri crate.** Thin marshalling: owns the `AppState` (database handle, settings, AI
client, audio devices), turns `AppError` into the serialised error, emits events. **parley-core.**
All learning logic, deterministic: clock, randomness, filesystem and network arrive as traits.
Feature code (lesson generation, judging, role-play) calls the `AiClient` trait and runs every
response through its schema and validators before anything is stored or returned. **parley-publik.**
The copied reference module plus the three providers behind one OpenAI-compatible client, the
response cache, the status object and the error mapping. **parley-audio.** Device I/O and signal
work; nothing in it knows about lessons.

A lesson, end to end: `lesson_start` → the composer picks new and due items and the allowed
vocabulary → if AI lessons are on and a provider is ready, `ai::lesson` asks for exercises, validates
each, drops failures and fills gaps from `exercise::template`; else the template generator makes all
of them → the lesson is stored → the UI shows exercises → `exercise_submit` grades (exact, accent,
typo, cached judgement, AI judge), updates the FSRS card, appends the review log and awards XP.

## 4. Data model

Everything lives in the Tauri app data directory (`AppData\Roaming\<identifier>` on Windows,
`~/Library/Application Support/<identifier>` on macOS, `~/.local/share/<identifier>` on Linux; the
identifier is set at module 00, placeholder `org.parley.app`).

| Path | What | Truth or cache |
|---|---|---|
| `parley.sqlite3` | progress, items, cards, review log, lessons, judgements, conversations | source of truth |
| `settings.json` | settings, never a key | source of truth |
| `packs/<lang>-<date>.sqlite3` | sentence packs, verified by SHA-256 | derived, re-downloadable |
| `cache/ai.sqlite3` | AI response cache | derived, safe to delete |
| `cache/audio/<sha256>.mp3` | natural-voice audio | derived |
| `cache/recordings/<uuid>.wav` | the learner's takes, deleted at session end unless kept | local only |
| `models/whisper/<name>.bin` | local Whisper models | downloaded on request |
| `publik-status.json` | status object, never the key | derived |
| `logs/parley.log` | rotating log, no content | local only |

The Publik credential lives at the contract's `credential_path()`, outside this directory.

### SQLite (`parley.sqlite3`, `PRAGMA user_version` = migration number)

```sql
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
CREATE TABLE conversations (id TEXT PRIMARY KEY, lang TEXT NOT NULL, scenario_id TEXT NOT NULL,
  started_at INTEGER NOT NULL, ended_at INTEGER, turns INTEGER NOT NULL DEFAULT 0,
  goals_done TEXT NOT NULL DEFAULT '[]', cost_micros INTEGER NOT NULL DEFAULT 0, summary TEXT);
CREATE TABLE conversation_turns (conversation_id TEXT NOT NULL REFERENCES conversations(id),
  n INTEGER NOT NULL, role TEXT NOT NULL CHECK (role IN ('learner','partner')), text TEXT NOT NULL,
  text_en TEXT, correction TEXT, input_mode TEXT CHECK (input_mode IN ('typed','spoken')),
  PRIMARY KEY (conversation_id, n));
CREATE TABLE media (id TEXT PRIMARY KEY, lang TEXT NOT NULL, title TEXT NOT NULL, file_sha256 TEXT NOT NULL,
  path TEXT,                             -- local only: never logged, never in a backup
  duration_ms INTEGER NOT NULL, chunks_total INTEGER NOT NULL, chunks_done INTEGER NOT NULL DEFAULT 0,
  provider TEXT, imported_at INTEGER NOT NULL);
CREATE TABLE media_sentences (media_id TEXT NOT NULL REFERENCES media(id), n INTEGER NOT NULL,
  start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, text TEXT NOT NULL, translation TEXT, item_id TEXT,
  PRIMARY KEY (media_id, n));
```

Migrations: `0001_init` (module 02) creates `meta` to `review_log`; `0002_sessions` (03) `sessions`
to `freeze_events`; `0003_conversations` (11); `0004_media` (12).

`cache/ai.sqlite3`: `ai_cache(key TEXT PRIMARY KEY, provider TEXT, model TEXT, prompt_id TEXT,
prompt_version INTEGER, response TEXT, charge_micros INTEGER, created_at INTEGER)` and
`audio_cache(key TEXT PRIMARY KEY, provider TEXT, voice TEXT, speed REAL, file TEXT, bytes INTEGER,
created_at INTEGER)` and `ai_attempts(key TEXT PRIMARY KEY, attempts INTEGER)` (the Try again count
that enters the idempotency key). Keys are SHA-256 hex (see `docs/PUBLIK-INTEGRATION.md` §6.1).

### `settings.json`

```json
{
  "schema": "parley.settings/1", "active_course": "es", "daily_goal_xp": 20,
  "accent_leniency": "lenient", "answer_input": "typing", "desired_retention": 0.9,
  "new_items_per_lesson": 5, "lesson_length": 12,
  "audio": { "voice_source": "os", "os_voice": { "es": null, "fr": null, "de": null },
             "natural_voice": { "es": null, "fr": null, "de": null }, "slow_speed": 0.75, "autoplay": true },
  "speaking": { "enabled": true, "transcriber": "provider", "pass_threshold": 0.8, "keep_recordings": false },
  "ai": { "provider": "unset", "fresh_lessons": true, "prepare_next_lesson": true, "stories": true,
          "share_device_name": false,
          "own_key": { "base_url": null, "chat_model": null, "transcribe_model": null,
                       "speech_model": null, "speech_voice": null },
          "local": { "base_url": "http://localhost:11434/v1", "chat_model": null } },
  "update_check": true
}
```

Enumerations: `accent_leniency` lenient | strict | ignore; `answer_input` typing | word_bank;
`voice_source` os | natural; `transcriber` provider | local; `ai.provider` unset | publik | own_key |
local. The own key itself is in the OS keychain (desktop) or app-private storage (Android).

Content formats (`syllabus.yaml`, forms table, sentence pack, `packs.lock.toml`, scenarios) are
specified with complete examples in `docs/modules/01-content-model-and-syllabus.md` and
`05-course-content-a1-a2.md`; the course pack in `07-lesson-generation.md`; the backup in
`04-app-shell-and-offline-course.md`. Each has a JSON Schema in `schemas/`.

## 5. Interfaces (Tauri commands)

Every command returns `Result<T, AppError>`; `AppError` serialises as
`{"code": "...", "message": "...", "action": {...} | null}`. Codes: `offline`, `needs_consent`,
`needs_credit` (action carries `top_up_url`), `disconnected`, `rate_limited`, `busy`, `ai_invalid`,
`provider_error`, `no_provider`, `pack_missing`, `checksum_mismatch`, `no_microphone`,
`mic_permission`, `too_quiet`, `no_voice`, `turn_cap_reached`, `file_too_large`,
`unsupported_media`, `invalid_input`, `not_found`, `storage`. Module specs spell out each message.

| Command | Arguments | Result | Module |
|---|---|---|---|
| `app_info` | | `{version, data_dir}` | 04 |
| `data_delete_all` | `confirm_text` (must be `delete`) | `()` | 04 |
| `cache_clear` | `kind: ai/audio/all` | `{files_removed}` | 04, 09 |
| `session_skip` | `kind: listen/speak` (60 minutes) | `()` | 09, 10 |
| `update_check` | | `{available?}` | 14 |
| `settings_get` / `settings_set` | – / `patch` | `Settings` | 04 |
| `course_list` | | `[CourseSummary]` | 04 |
| `course_start` | `lang` | `CourseSummary` | 04 |
| `pack_status` / `pack_download` | `lang` | `PackStatus` (+ `pack://progress`) | 04 |
| `path_get` | `lang` | `Path {units[{id,title,skills[{id,title,state,mastery,cefr}]}]}` | 04 |
| `skill_get` | `skill_id` | `SkillDetail` | 04 |
| `placement_start` | `lang` | `PlacementQuestion` | 04 |
| `placement_answer` | `session_id, answer` | `{next?: PlacementQuestion, result?: PlacementResult}` | 04 |
| `lesson_start` | `skill_id, mode: learn/review/story` | `LessonStarted {session_id, origin, exercises, notice?}` | 04, 07 |
| `review_start` | `lang, limit` | `LessonStarted` | 04 |
| `exercise_submit` | `session_id, exercise_id, answer, response_ms, used_hint` | `Grade` | 04, 08 |
| `exercise_i_was_right` | `session_id, exercise_id` | `Grade` | 04 |
| `explain_answer` | `session_id, exercise_id` | `{text, tip?, cached}` | 08 |
| `lesson_finish` | `session_id` | `LessonSummary {xp, correct, total, streak, record_week}` | 04 |
| `stats_get` | `lang` | `Stats` | 04 |
| `backup_export` / `backup_import` | `dest_path` / `src_path` | `ExportSummary` / `ImportReport` | 04 |
| `course_pack_export` / `course_pack_import` | `lang, dest_path` / `src_path` | `ExportSummary` / `ImportReport` | 07 |
| `ai_status` | | `AiStatus` (also `ai://status`) | 06 |
| `ai_consent_accept` / `ai_consent_decline` | | `AiStatus` | 06 |
| `ai_set_own_key` | `base_url, key, chat_model, transcribe_model?, speech_model?, speech_voice?` | `AiStatus` | 06 |
| `ai_clear_own_key` | | `AiStatus` | 06 |
| `ai_local_probe` / `ai_set_local` | `base_url` / `base_url, chat_model?` | `[LocalModel]` / `AiStatus` | 06 |
| `ai_check_connection` | | `{ok, charge_micros?}` | 06 |
| `ai_disconnect_publik` / `ai_reconnect_publik` | | `AiStatus` | 06 |
| `ai_open_link` | `kind: claim/top_up/add_credit` | `()` | 06 |
| `voices_list` | `lang` | `[Voice {id, name, source, installed}]` | 09 |
| `tts_play` / `tts_stop` | `text, lang, slow` / – | `{source}` (+ `audio://ended`) | 09 |
| `mic_start` / `mic_stop` | – | `()` / `Recording {id, duration_ms}` (+ `mic://level`) | 10 |
| `recording_play` | `recording_id` | `()` | 10 |
| `speak_check` | `session_id, exercise_id, recording_id` | `SpeakResult` | 10 |
| `whisper_models` / `whisper_download` | – / `name` | `[ModelInfo]` (+ `model://progress`) | 10 |
| `scenario_list` | `lang` | `[Scenario]` | 11 |
| `conversation_start` | `lang, scenario_id` | `ConversationState` | 11 |
| `conversation_transcribe` | `conversation_id, recording_id` | `{text}` (editable before sending) | 11 |
| `conversation_turn` | `conversation_id, text, spoken` | `TurnResult` (+ `conversation://delta`, `conversation://withdrawn`) | 11 |
| `conversation_end` | `conversation_id` | `ConversationSummary` | 11 |
| `photo_vocab` / `photo_save` | `lang, image_path` / `lang, words` | `PhotoResult` / `{added}` | 12 |
| `media_import` / `media_transcribe` | `lang, path` / `media_id, confirmed` | `MediaPlan` / `()` (+ `media://progress`) | 12 |
| `media_get` / `media_translate` / `media_save_sentence` | `media_id` / `media_id, n` / `media_id, n` | `Transcript` / `String` / `Item` | 12 |
| `media_translate_all` | `media_id, confirmed` | `{translated}` (+ `media://progress`) | 12 |

Result types are defined in `crates/parley-core/src/api.rs` and mirrored in `app/src/api/commands.ts`
by hand; a test compares the two lists of command names.

## 6. Core algorithms

- **Scheduling**: FSRS (Ye, Su, Cao, "A Stochastic Shortest Path Algorithm for Optimizing Spaced
  Repetition Scheduling", KDD 2022; the FSRS algorithm description in the open-spaced-repetition
  wiki), through the `fsrs` crate; reference values recorded from `py-fsrs`.
- **Placement**: a transformed up-down staircase (Levitt 1971, "Transformed up-down methods in
  psychoacoustics") over the ordered skill list; estimate from reversal points.
- **Normalisation**: Unicode NFC (UAX #15), lowercase, UAX #29 word boundaries.
- **Typo tolerance**: optimal string alignment distance (Damerau 1964; Levenshtein 1966) ≤ 1 per word.
- **Speaking score**: word-level edit alignment with backtrace (Wagner and Fischer 1974).
- **Silence trimming**: short-time energy endpoint detection with hangover (Rabiner and Sambur 1975).
- **Language identification**: trigram profiles (Cavnar and Trenkle 1994) as implemented by `whatlang`.
- **Streaks and freezes**: rules defined in `docs/modules/03-exercise-engine-and-grading.md`.

## 7. Invariants

Each has a test; the module that owns it names the test.

1. **I1** No command result, event, log line or status file contains a key (`pk_…` or the own key).
2. **I2** No AI output is stored or shown before it passes its JSON Schema and validators; a failure
   stores nothing (streamed role-play text is provisional and withdrawn on failure).
3. **I3** Offline core: placement, template lessons, cached lessons, reviews, exact and typo grading,
   streaks and stats work with a network fake that fails every call.
4. **I4** No automatic retry on a Publik call; one user action makes at most one attempt per call.
5. **I5** Identical AI input never spends twice: a cache hit makes no network call.
6. **I6** The review log is append-only, and card state equals a replay of the log.
7. **I7** Freezes are only earned (one per 7 active days, at most 2 banked); no code path buys one.
8. **I8** No hearts, energy or lesson limit: 100 lessons in one day are all allowed.
9. **I9** Only the chosen provider is contacted; the local provider never contacts publikhq.com.
10. **I10** Every request body is under 4,000,000 bytes, base64 included.
11. **I11** Every AI-generated exercise refers to an item the core supplied, in a skill of the syllabus.
12. **I12** The consent screen appears at the first AI action, never at launch or in onboarding.
13. **I13** Recordings, photos and media files never leave the machine except in the request the
    learner triggered, after consent.

## 8. Errors, logging and privacy

Logs (`logs/parley.log`, 5 files × 1 MB) record command names, durations, error codes, HTTP status
codes, `x-publik-request-id` and cache hit or miss. Never answers, sentences, transcripts, audio,
images, file paths chosen by the learner, keys or balances.

Every network call the app can make, and nothing else: (1) `pack_download` from the release
URLs in `content/packs.lock.toml`; (2) `whisper_download` from `models/registry.toml`; (3) the
update check against the public releases page (off in development builds, switchable); (4) AI calls
to the chosen provider only: publik API (`/installs`, `/installs/revoke`, `/wallet` on start and in
Settings only while `ai.provider = publik`,
`/chat/completions`, `/audio/transcriptions`, `/audio/speech`), the learner's own OpenAI-compatible
base URL, or a localhost server (`/v1/models`, Ollama's `/api/tags`, `/v1/chat/completions`).
`GET /models` on publik API is made only by developers during implementation and by `scripts/publik-proof`.
The built-app self-test (module 06) makes one `POST /installs` only when the desktop app is started
with `PUBLIK_SELF_TEST` set, and then exits.
`SECURITY.md` repeats this list.

## 9. Decisions (ADRs written by module 00)

| ADR | Status | Decision | Rejected |
|---|---|---|---|
| 0001 Stack | Accepted | Tauri 2, Rust core, React/TS UI, Android through Tauri mobile | Electron (size; no shared Rust core on Android); Flutter (a second language for the core); a web app (key custody, offline) |
| 0002 Code licence | Accepted | MIT OR Apache-2.0 | GPL (would bar permissive reuse of the core) |
| 0003 Content licence | Proposed: **maintainer decides** before module 01 | CC BY-SA 4.0 recommended | CC0: forbids CC BY-SA sources (Wiktionary forms, FrequencyWords), so forms would need hand-written rules |
| 0004 Scheduler | Accepted | FSRS, two cards per item (receptive, productive) | SM-2 (weaker), Leitner boxes, half-life regression (no maintained open implementation) |
| 0005 Grading order | Accepted | exact, accent, typo, cached judgement, then AI judge only for free translations | AI first (cost, latency, offline) |
| 0006 Storage | Accepted | one SQLite file plus derived caches; JSON backup | a server; many JSON files (no transactions) |
| 0007 Sentence packs | Accepted | built from Tatoeba by `parley pack build`, published as release assets, pinned by SHA-256, never committed | committing Tatoeba data (CC BY data is downloaded by checksum, never committed; size) |
| 0008 AI client | Accepted | the reference `publik.rs` in `crates/parley-publik`, reqwest transport, one OpenAI-compatible client for all providers, no retries on Publik | vendor SDKs; calls from the webview |
| 0009 Default voice | Proposed: **maintainer decides** at module 09 | OS voices by default, natural voices opt-in | natural by default (speech is the largest audio cost and earns no share) |
| 0010 iOS | Proposed: **maintainer decides** after module 13 | later, needs an Apple developer account | in v1 |
| 0011 No server | Accepted | streak, XP and records are local; no leagues | a leaderboard server (accounts, moderation, cost) |
| 0012 Module order | Accepted | offline app (04) before Publik (06) | UI last (AI features would be untestable by hand for months) |

## 10. Platforms

- **Windows 10 and 11 (first)**, CI `windows-latest`: WebView2; OS voices through WinRT (OneCore)
  with SAPI fallback; the microphone needs "Let desktop apps access your microphone".
- **Linux**, CI `ubuntu-latest`: WebKitGTK 4.1; Speech Dispatcher for OS voices (optional); ALSA
  through `cpal` (`libasound2-dev`).
- **macOS**, CI from module 14: AVFoundation voices; a microphone usage string in `Info.plist`;
  signing and notarisation need the Apple developer account (ADR-0010).
- **Android** (module 13): `reqwest` with rustls, Android TTS, the microphone runtime permission,
  word bank by default, no local Whisper. **iOS**: later (ADR-0010).
