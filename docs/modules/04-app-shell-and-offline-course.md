# 04: App shell and offline course

## Goal

A learner can install Parley, pick a language, take the placement test, follow the path, do lessons
and reviews, watch the streak and stats, change settings, and back up and restore their data, all
with no network and no AI. This is the app a fresh install at $0.00 has, and the shell every later
module adds a screen to.

## Depends on

03.

## Scope

- Tauri commands marked 04 in `docs/ARCHITECTURE.md` §5, with `AppError` messages as below.
- `settings.json` load, validate and save (defaults on a bad file).
- Screens: Onboarding, Path, Lesson player (pick the meaning, translate both ways with typing or
  word bank, fill the gap, match pairs), Result sheet, Lesson summary, Reviews, Stats, Settings
  (Learning, Data, About). No AI entry point is shown yet.
- Course pack download from `content/packs.lock.toml`, with the size shown first, progress events,
  SHA-256 verification and a working app without it.
- Backup export and import (`parley.backup/1`), and "Delete everything".
- Attribution: "Sentence by {author} on Tatoeba" under pack sentences; the About page credits
  Tatoeba (CC BY 2.0 FR) and Wiktionary (CC BY-SA).
- Keyboard: Enter checks and continues, 1 to 4 pick options, Esc asks to quit.
- `app/src/strings/en.json` for every UI string; `app/src/api/commands.ts` typed wrappers.
- The launch smoke test and the CI `e2e` job.

## Files

- `app/src-tauri/src/commands/{course.rs,pack.rs,path.rs,placement.rs,lesson.rs,stats.rs,settings.rs,backup.rs}`
- `crates/parley-core/src/{api.rs,settings.rs,backup.rs}`, `schemas/{settings.schema.json,backup.schema.json}`
- `app/src/screens/{Onboarding,Path,Lesson,Review,Stats,Settings}/*.tsx`,
  `app/src/components/{ResultSheet,WordBank,MatchPairs,StreakBadge}.tsx`
- `app/e2e/launch.spec.ts`, `.github/workflows/ci.yml` (e2e job)
- `app/src-tauri/tests/m04_app_shell_and_offline_course.rs`, `app/src/**/*.test.tsx`

## Interfaces

Commands and result types as in ARCHITECTURE §5 (04 rows). Events: `pack://progress
{lang, bytes_done, bytes_total}`. Backup example (arrays shortened to one element):

```json
{
  "schema": "parley.backup/1",
  "app_version": "0.4.0",
  "exported_at": "2026-10-12T08:30:00Z",
  "settings": {"schema": "parley.settings/1", "active_course": "es", "daily_goal_xp": 20, "accent_leniency": "lenient"},
  "courses": [{"lang": "es", "syllabus_version": 1, "pack_version": "2026-10-01", "started_at": 1760000000000, "placement_index": 12, "cefr_estimate": "around A1"}],
  "items": [{"id": "es:conv:6f1c:1", "lang": "es", "skill_id": "es.personal.conversation", "kind": "phrase", "target": "un café con leche", "en": "a coffee with milk", "lemmas": ["un", "café", "con", "leche"], "cefr": "A1", "source_kind": "conversation", "source_ref": "6f1c", "created_at": 1760100000000, "retired": 0}],
  "accepted_answers": [{"item_id": "es.a1.u03.s01#e:2", "direction": "to_en", "norm": "i want a white coffee", "display": "I want a white coffee.", "source": "judge", "created_at": 1760100000000}],
  "cards": [{"item_id": "es.a1.u01.s01#l:hola", "facet": "receptive", "state": "review", "stability": 3.17, "difficulty": 5.28, "due": 1760300000000, "last_review": 1760000000000, "reps": 1, "lapses": 0}],
  "review_log": [{"id": 1, "item_id": "es.a1.u01.s01#l:hola", "facet": "receptive", "reviewed_at": 1760000000000, "rating": 3, "exercise_type": "pick_meaning", "verdict": "correct", "response_ms": 2100, "elapsed_days": 0, "stability_after": 3.17, "difficulty_after": 5.28, "session_id": "s-01"}],
  "sessions": [], "lessons": [], "judgements": [], "daily": [], "freeze_events": []
}
```

These are the tables that exist at this module (migrations 0001 and 0002). Module 11 adds
`conversations` and `conversation_turns`, and module 12 adds `media` and `media_sentences` (with
`path` always empty), as optional arrays of the same `parley.backup/1` schema, so a backup made
before them still imports. Syllabus-derived items and content answers are not exported; they are
rebuilt from `content/`.
Keys, the Publik credential, caches, recordings and pack files are never in a backup.

Messages: "You're offline. Parley works without the pack, with fewer example sentences."; "The
download was damaged. Try again."; "This is not a Parley backup."; "This backup was made by a newer
Parley. Update Parley to import it."; "Importing replaces all your progress on this computer. Export
a backup first if you want to keep it." with **Replace my data** and **Cancel**; "Your settings file
could not be read, so Parley started with default settings."; "Unknown value {v} for {key}.";
"Parley could not open its data file." with **Restore a backup** and **Show details**; "Type delete
to confirm."; "Nothing to review. Next review: {today at 09:00 | tomorrow at 09:00 | on 14 October}.".

## Out of scope

- AI setup and every AI entry point (06 onwards); listening and speaking exercises (09, 10);
  stories (07); conversation, photo and media screens (11, 12); Android layout (13).

## Design notes

- Import writes a new database file and renames it over the old one only after it is complete.
  Cards whose item no longer exists in the content are skipped and counted in the report.
- Pack size is shown in decimal megabytes to one decimal place (8,200,000 bytes → "8.2 MB").
- Stats "words known": vocabulary items whose productive card has retrievability ≥ 0.9 now.
  "Recall rate": the share of reviews in the last 30 days rated Hard, Good or Easy.
- Decisions are filled in here during implementation.

## Acceptance tests

Command tests in `app/src-tauri/tests/m04_app_shell_and_offline_course.rs` with a temporary data
directory and `FailingNet` unless stated; UI tests in `app/src/**/*.test.tsx`; the smoke in
`app/e2e/launch.spec.ts`. Expected values come from the fixtures and modules 02 and 03.

1. Onboarding renders Welcome, Language, Course pack, Daily goal, Where to start, in that order, and
   no AI setup dialog at any step (invariant I12).
2. `course_start("es")` then `path_get("es")` → the seed units with skill 1 available and skill 2
   locked.
3. `pack_download` with a fake network serving the locked bytes → installed; one changed byte →
   `checksum_mismatch`, "The download was damaged. Try again.", nothing left in `packs/`; with
   `FailingNet` → `offline` and the offline message; 8,200,000 bytes displays as "8.2 MB".
4. With `FailingNet`, a full run succeeds: placement, a lesson, its summary, a review session,
   `stats_get` (invariant I3).
5. `exercise_submit` returns the grade of module 03; `lesson_finish` returns XP, the streak and the
   weekly record flag.
6. `review_start` with nothing due and the next card due tomorrow at 09:00 local → empty with
   "Nothing to review. Next review: tomorrow at 09:00."
7. `stats_get` on `tests/fixtures/db/stats.sqlite3` (built by the fixture generator with known
   reviews) → the expected numbers in `expected-stats.json`.
8. Backup: export, delete everything, import, export again → identical JSON apart from
   `exported_at`; a file with `"schema": "other"` → "This is not a Parley backup."; `parley.backup/2`
   → "This backup was made by a newer Parley. Update Parley to import it."; a card for a missing
   item is skipped and reported.
9. Settings: a corrupt `settings.json` → defaults and the notice; `settings_set` with
   `accent_leniency: "loose"` → `invalid_input`, "Unknown value loose for accent_leniency."
10. "Delete everything" refuses without the text `delete` ("Type delete to confirm.") and afterwards
    the app shows onboarding.
11. The command names in `crates/parley-core/src/api.rs` equal those in `app/src/api/commands.ts`.
12. Keyboard: Enter submits and continues; keys 1 to 4 choose options in `pick_meaning`.
13. A pack-sentence exercise's result sheet shows "Sentence by fixture_a on Tatoeba"; the About page
    credits Tatoeba (CC BY 2.0 FR) and Wiktionary (CC BY-SA).
14. Every `t("…")` key used under `app/src/` exists in `en.json`.
15. A corrupt `parley.sqlite3` → `storage` error and the data-file screen with its two buttons.
16. Launch smoke: the debug build starts and shows the Welcome text within 20 s on Windows and Linux.

## Status

Planned
