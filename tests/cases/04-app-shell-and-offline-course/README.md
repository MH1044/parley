# Module 04 fixtures

Hand-written `parley.backup/1` files for the command tests in
`app/src-tauri/tests/m04_app_shell_and_offline_course.rs`. They are plain files, not golden
cases: this folder has no case folders, so the golden runner finds nothing to run here.

Every backup follows the format of `docs/modules/04-app-shell-and-offline-course.md` (keys in
order, rows as the table columns, `settings` the whole `docs/ARCHITECTURE.md` §4 object with
`active_course` `es`, `app_version` `0.4.0`). They describe constructed states, not histories the
app could replay: card values are chosen for their retrievability, and review-log rows for where
they fall against the 30-day window. Item ids are module 01's `<skill>#l:<lemma>` (vocabulary)
and `<skill>#e:<example id>` (phrase or pattern) of `content/es/syllabus.yaml` and
`content/fr/syllabus.yaml`. Arrays are in primary-key order (cards receptive before productive),
so an export after importing a file gives the same rows back.

## `stats-backup.json` and `expected-stats.json`

Read at 2026-10-15T12:00:00Z (a Thursday, ISO week 2026-W42) with UTC offset 0; the time is in
`expected-stats.json` under `clock`, the stats under `stats`, and floats compare within
`tolerance` (1e-6).

- Courses: `es`, started 2026-09-10, placement estimate 1, `cefr_estimate` "Pre-A1",
  `es.a1.u01.s01` tested out; `fr`, started 2026-10-01 with no placement. A French card and a
  French review prove that words known, the recall rate and the skills count Spanish only.
- `daily` (all courses): active 2026-09-10 to 09-22, a frozen day 09-23, nothing from 09-24 to
  09-30, active 10-01 to 10-07, a frozen day 10-08, active 10-09 to 10-15 (today). Module 03's
  streak rules, day by day: length 7 on 09-16 earns a freeze; 09-23 uses it (length 13 kept);
  09-24 resets to 0; length 7 on 10-07 earns one; 10-08 uses it; 10-09 to 10-15 bring the
  length to 14, which earns one. Streak 14, 1 freeze; `freeze_events` lists the same five
  events and the frozen rows match.
- Weeks (ISO, Monday first): W37 80 XP, W38 280 (09-14 to 09-20, two of its days before the
  30-day window), W39 100, W40 120, W41 150, W42 (this week) 90. `week_xp` 90,
  `record_week_xp` 280.
- `days`: the 30 local dates 2026-09-16 to 2026-10-15, XP from `daily`, active when the row
  has sessions, frozen from the row.
- Retrievability (module 02): R = (1 + f·t/S)^(−d), d = w[20] = 0.1542 of FSRS-6's default
  vector (as the scheduler's 32-bit value, 0.15420000255…), f = 0.9^(−1/d) − 1 = 0.98035, t the
  fractional days since `last_review`. R ≥ 0.9 exactly when t ≤ S, and every card sits far from
  that line: known productive words at t ≤ S/10 (R ≥ 0.985), weak ones at t ≥ 3S (R ≤ 0.81).
- Words known: of the 76 vocabulary items of the Spanish syllabus (`vocab_items`, none
  retired), the productive card is strong for 7 words of Greetings (`tal`, `bien` and `muy` are
  weak), `llamarse` and `nombre` of Names (`señor` weak, four never reviewed) and `café` of At
  the café (`agua` relearning and weak, `té` never reviewed): 10. Example items with strong
  productive cards and words with only a strong receptive card do not count.
- Recall rate: Spanish review-log rows with now − 30 days < `reviewed_at` ≤ now, placement rows
  left out. Left out: two rows on 09-12, one exactly 30 days before now, two placement rows on
  09-16 and the French row on 10-02. Counted: 13 rows (one exactly at now), 3 of them rated
  Again: `reviews_30` 13, `recall_rate` 10/13.
- Skills (module 02, mastery = sum of R over the skill's cards / (2 × its items)):
  Greetings, 17 items, every card reviewed, mastery 0.97359 → mastered; Names, 15 items, every
  receptive card reviewed, mastery 0.58493 → practised; Where you are from → available (Names is
  practised); Numbers and age and Family → locked; At the café, 3 of 19 items introduced,
  mastery 0.09260 → learning (that comes before available or locked); Days and time and Likes →
  locked.

The expected numbers were worked out from these definitions with a short script that evaluates
the formula above; nothing was taken from running the app.

## `next-review-backup.json`

Read at 2026-10-14T18:30:00Z with UTC offset +120 (20:30 local). The earliest Spanish due date
after now is 2026-10-15T07:00:00Z, tomorrow at 09:00 local, so an empty Spanish review session
says "Nothing to review. Next review: tomorrow at 09:00." A French card is due at
2026-10-14T20:00:00Z, earlier, but belongs to the other course; the French review session says
"Nothing to review. Next review: today at 22:00." Introduced cards that were never reviewed
have no due date and are ignored.

## Refused and partial backups

- `backup-other-schema.json`: a complete file whose `schema` is `other` → "This is not a Parley
  backup."
- `backup-v2.json`: the same with `parley.backup/2` → "This backup was made by a newer Parley.
  Update Parley to import it."
- `backup-missing-item.json`: a Spanish course, one item from a conversation (`es:conv:6f1c:1`)
  and three cards: one for that item, one for `es.a1.u01.s01#l:hola` and one for
  `es.a1.u01.s01#l:inexistente`, which the syllabus does not have. Import keeps two cards and
  skips one (`skipped_cards` 1).
