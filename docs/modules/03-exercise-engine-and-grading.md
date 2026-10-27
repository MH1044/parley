# 03: Exercise engine and grading

## Goal

The core can build a complete lesson offline, grade every answer honestly and forgivingly, and
reward the learner: exercise types, the normaliser, accent, article and typo leniency, template
exercises from the syllabus and sentence packs, the lesson composer, XP, the daily goal, streaks
with earned freezes and the weekly record. No AI and no screens yet.

## Depends on

01, 02.

## Scope

- Exercise types (`schemas/lesson.schema.json`, Rust enum tagged by `type`): `pick_meaning`,
  `translate_to_target` (typed or word bank), `translate_to_en`, `listen_type`, `speak`,
  `fill_gap`, `match_pairs`; each carries `id`, `item_id`, `facet`, `origin` (`template`, `ai`,
  `course_pack`).
- `normalise(lang, text)` and the offline grader: exact, accent (setting lenient, strict, ignore),
  German umlaut and ß alternatives, noun article, typo; verdicts `correct`, `almost`, `typo`,
  `wrong`, `unchecked`.
- Accepted-answer lookup and "I was right".
- Structural checks shared with module 07: answers, duplicates, length, focus, vocabulary coverage.
- The template generator for every exercise type, deterministic by seed.
- The composer: a plan of up to `lesson_length` exercises from new and due items, with a capability
  mask (`audio_out`, `mic`).
- Word bank tiles.
- XP, daily goal, streak and freezes, weekly record.
- Migration `0002_sessions.sql`: `sessions`, `lessons`, `judgements`, `daily`, `freeze_events`.
- CLI: `parley grade --lang <l> --type <t> --expected <s>... --answer <s> [--accent lenient]
  [--noun] --json`; `parley lesson template --lang <l> --skill <id> --seq <n> --seed <n> --json`;
  `parley streak simulate <days.json> --json`.

## Files

- `crates/parley-core/src/grading/{normalise.rs,typo.rs,grade.rs}`
- `crates/parley-core/src/exercise/{types.rs,check.rs,template.rs,compose.rs,wordbank.rs}`
- `crates/parley-core/src/game/{xp.rs,streak.rs}`, `crates/parley-core/src/store/migrations/0002_sessions.sql`
- `crates/parley-cli/src/cmd/{grade.rs,lesson.rs,streak.rs}`, `schemas/lesson.schema.json`
- `scripts/gates/run`, `scripts/gates/run.ps1`, `scripts/gates/thresholds.toml` (the gates directory,
  created here) and `crates/parley-core/tests/gates.rs` (`#[ignore]` gate tests, starting with the
  grading and template benchmarks and G3)

## Interfaces

```rust
pub fn normalise(lang: Lang, s: &str) -> String;
pub fn grade(ctx: &GradeCtx, ex: &Exercise, answer: &Answer) -> Grade;
// Grade { verdict, tags: Vec<Tag>, message: Option<String>, corrected: Option<Diff>, rating: Rating }
pub fn accept_learner_answer(store: &mut Store, ex: &Exercise, answer: &Answer) -> Grade;
pub fn check_exercise(ex: &Exercise, ctx: &CheckCtx) -> Result<(), CheckFailure>; // L5–L9
pub fn template(item: &Item, kind: ExerciseType, ctx: &TemplateCtx, rng: &mut dyn Rng) -> Option<Exercise>;
pub fn compose(req: &ComposeRequest, store: &Store, now_ms: i64, rng: &mut dyn Rng) -> Plan; // Plan { entries: Vec<PlanEntry{ n, item_id, facet, kind, focus, new }> }
pub fn word_bank(target: &str, lang: Lang, allowed: &Lemmas, forms: &Forms, rng: &mut dyn Rng) -> Vec<String>;
pub fn xp_for(session: &SessionResult) -> u32;
pub fn streak(daily: &[DayRow], freezes: &[FreezeEvent], today: NaiveDate) -> StreakState;
```

User-facing messages: "Almost: mind the accent: {words}", "Wrong: mind the accent: {words}", "Note
the accent: {words}", "Almost: remember the article: {target}", "{Noun} is {masculine|feminine|
neuter}: {target}.", "Typo: {words}", "Type an answer first.", "This exercise was already
checked.", "This exercise is no longer available.", "New weekly record!".

## Out of scope

- Screens (04); the AI judge and explanations (08); audio playback and recording (09, 10); AI
  lessons (07). `listen_type` and `speak` exist as data only.

## Design notes

- Normaliser, in order: NFC; typographic apostrophes (’ ‘ ʼ) to '; U+00A0 and U+202F to space;
  lowercase (not case folding, so ß stays); remove characters of Unicode categories P and S except
  an apostrophe or hyphen between two letters; collapse whitespace; trim.
- Accent folding: F2 removes combining marks after NFD; for German also F1 maps ä→ae, ö→oe, ü→ue,
  ß→ss. An accent match is an answer that differs from an accepted one but equals it under F2, or
  (German) under F1 on both sides or F2 on both sides. The message lists the expected spelling of
  the differing words.
- Article rule (vocabulary nouns only): the answer without the article → `almost`; with the wrong
  article → `wrong` and "{Noun} is masculine: el día." from the lemma's gender.
- Typo rule, after exact and accent fail: same word count; every differing word has an optimal
  string alignment distance of 1 to its accepted counterpart (compared after accent folding), is at
  least 4 letters long in the accepted answer, and is not itself a form in the forms table; at most
  `max(1, words / 4)` differing words. Tags include `accent` when accents also differ.
- `unchecked` only for `translate_to_target` and `translate_to_en` (module 08 sends it to the judge;
  with no provider the sheet offers "I was right"). Other types are `wrong` when unmatched.
- Lookup order: the exercise's `accepted`, then `accepted_answers` for the item and direction when
  the exercise's prompt is the item's own, then `judgements` by key (ARCHITECTURE §4).
- Ratings follow module 02. `typo` and `almost` rate Hard; `ignore` leniency rates accent slips Good.
- Coverage (used by L5 in module 07): the share of word tokens, numbers and names excluded, whose
  lemma keys include an allowed lemma; a multiword lemma (`por favor`, `s'il vous plaît`) covers its
  words where they appear in sequence.
- Template rules: `pick_meaning` takes three distractors from other items' first English glosses
  (same skill, then earlier skills), never equal to any accepted English answer of the item;
  `fill_gap` blanks the first token whose lemma is the item's lemma, in an example or a pack sentence
  whose words are all allowed; a productive vocabulary card uses `fill_gap` when such a sentence
  exists, else `translate_to_target`.
- Composer: new items are the skill's next unintroduced items (lemmas before examples), at most
  `new_items_per_lesson`. Order: the receptive intro of each new item; then `match_pairs` if at least
  4 vocabulary items are among the new and due; then alternate one due card (queue order) and one
  productive exercise of a new item; then the lowest-retrievability introduced items of the skill.
  With `audio_out`, up to 2 `translate_to_en` become `listen_type`; with `mic`, up to 2 productive
  phrase exercises of at most 8 words become `speak`.
- XP: 1 per correct, typo or almost answer; 10 for finishing a lesson, 5 for a review session; +5
  for a lesson with no wrong answer.
- Streak, processed day by day up to yesterday, then today if active: an active day adds 1; at
  lengths 7, 14, 21… a freeze is earned if fewer than 2 are banked; a missed day uses a banked freeze
  (length unchanged, the day marked frozen) or resets the length to 0. Today, not yet active, keeps
  yesterday's streak. Weekly record: ISO weeks on local dates; "New weekly record!" once, when this
  week's XP first exceeds the best earlier week (which must be above 0).
- Decisions are filled in here during implementation.

## Acceptance tests

Golden cases in `tests/cases/03-exercise-engine-and-grading/`; code tests in
`crates/parley-core/tests/m03_exercise_engine_and_grading.rs`. Expected values follow from the rules
above and UAX #15; plans are computed by hand from the composer rules.

1. `normalise`: es `  ¿Qué  TAL? ` → `qué tal`; fr `Aujourd’hui, c’est lundi !` → `aujourd'hui c'est
   lundi`; de `Die Straße.` → `die straße`; es `e` + U+0301 + `l` → `él`; fr `Est-ce que…` → `est-ce que`.
2. Expected `Hola, ¿qué tal?`: answer `hola qué tal` → correct; `Hola que tal` → almost, tags
   `[accent]`, "Almost: mind the accent: qué".
3. Expected `Buenos días.`, answer `Buenos dias`: lenient → almost (Hard); strict → wrong, "Wrong:
   mind the accent: días"; ignore → correct (Good), "Note the accent: días".
4. de expected `Das Mädchen`: `das Maedchen`, `das Madchen` → almost; expected `die Straße`:
   `die Strasse` → almost.
5. Vocabulary noun `el día`: `día` → almost, "Almost: remember the article: el día"; `la día` →
   wrong, "Día is masculine: el día."
6. Expected `Quiero un café con leche.`: `Quiero un café con lecje` → typo, "Typo: leche"; expected
   `el perro`, answer `el pero` (`pero` is in the forms table) → not a typo; expected `le livre`,
   answer `la livre` → not a typo (the differing word has 2 letters).
7. An 8-word accepted answer with 2 one-letter slips in long words → typo; with 3 → not typo.
8. `translate_to_target` unmatched → `unchecked`; `fill_gap` unmatched → `wrong`.
9. "I was right" → rating Good, a `learner` row in `accepted_answers`; the same answer next time →
   correct offline.
10. For every skill of the seed content and seeds 1 to 100, every template exercise passes
    `check_exercise`.
11. `pick_meaning` for `el agua` never offers an option equal to `water` or any other accepted gloss.
12. `fill_gap` for lemma `leche` over `Quiero un café con leche.` → `Quiero un café con ___.`,
    answer `leche`.
13. Composer fixtures `composer-a.json` (5 new items, no dues), `composer-b.json` (3 new, 5 due) and
    `composer-c.json` (capabilities on) → the plans in their expected files; no plan exceeds
    `lesson_length`; with `audio_out` false there is no `listen_type`, with `mic` false no `speak`.
14. Word bank for `Quiero un café.` → 3 target tiles plus 3 distractor tiles, all allowed forms;
    the same seed gives the same order; tiles in the right order grade correct.
15. XP: 12 exercises, 10 right and 2 wrong → 20; 12 right → 27.
16. Streaks (`parley streak simulate`): active days 1 to 7 → length 7, 1 freeze; then day 8 missed and
    day 9 active → length 8, day 8 frozen, 0 freezes; days 1 to 21 active → 2 freezes (none at 21);
    1 freeze, days 8 and 9 missed, day 10 active → length 1; sessions at 23:30 and 00:10 local count
    as two days.
17. Weeks of 100 and 80 XP, then a third week reaching 101 → "New weekly record!" exactly once.
18. 100 lessons started and finished on one local date all succeed (invariant I8).
19. Messages: empty answer → "Type an answer first."; second submit → "This exercise was already
    checked."; unknown id → "This exercise is no longer available."
20. Gate (`scripts/gates/run`, in `tests/gates.rs`): grading p99 ≤ 5 ms over 10,000 answers; a template lesson ≤ 200 ms.
21. Migration 0002 on a version-1 file creates `sessions`, `lessons`, `judgements`, `daily` and
    `freeze_events` with the columns of ARCHITECTURE §4 and sets `user_version` 2.

## Status

Planned
