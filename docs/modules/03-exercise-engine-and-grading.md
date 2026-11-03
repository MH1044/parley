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
- Accepted-answer lookup (the exercise, the item's stored answers, cached judgements) and "I was
  right".
- Structural checks shared with module 07 (L5 to L9): vocabulary coverage, answers, duplicates,
  length, focus.
- The template generator for every exercise type, deterministic by seed, and the template lesson.
- The composer: a plan of up to `lesson_length` exercises from new and due items, with a capability
  mask (`audio_out`, `mic`).
- Word bank tiles.
- The lesson runner: start a lesson or review, submit, "I was right", finish; the three submit
  messages; it writes `sessions`, `lessons`, `daily` and `freeze_events` and reviews cards through
  module 02's store.
- XP, daily goal, streak and freezes, weekly record.
- Migration `0002_sessions.sql`: `sessions`, `lessons`, `judgements`, `daily`, `freeze_events`.
- CLI: `parley grade`, `parley lesson template`, `parley streak simulate` (shapes under Interfaces).
- The quality-gates directory `scripts/gates/` and `crates/parley-core/tests/gates.rs`, starting
  with the grading and template benchmarks and G3.

## Files

- `crates/parley-core/src/grading/{mod.rs,normalise.rs,typo.rs,grade.rs}`
- `crates/parley-core/src/exercise/{mod.rs,types.rs,check.rs,template.rs,compose.rs,wordbank.rs,run.rs}`
- `crates/parley-core/src/game/{mod.rs,xp.rs,streak.rs}`
- `crates/parley-core/src/store/migrations/0002_sessions.sql` and the store additions in
  `crates/parley-core/src/store/`
- `crates/parley-core/src/env.rs` (date arithmetic and the two random helpers)
- `crates/parley-cli/src/cmd/{grade.rs,lesson.rs,streak.rs}`, `schemas/lesson.schema.json`
- `scripts/gates/run`, `scripts/gates/run.ps1`, `scripts/gates/thresholds.toml`
- Tests: `crates/parley-core/tests/m03_exercise_engine_and_grading.rs`,
  `crates/parley-cli/tests/m03_exercise_engine_and_grading.rs`, `crates/parley-core/tests/gates.rs`,
  `tests/cases/03-exercise-engine-and-grading/` (golden cases, and the hand-written fixture files
  directly in that folder)

## Interfaces

```rust
// parley_core::env (additions)
impl LocalDate {
  pub fn from_days(days: i64) -> LocalDate;      // days since 1970-01-01 → civil date
  pub fn days(self) -> i64;                       // the inverse
  pub fn parse(text: &str) -> Option<LocalDate>;  // exactly YYYY-MM-DD and a real date
  pub fn iso_week(self) -> (i32, u32);            // ISO 8601 (year, week 1..=53); weeks start on Monday
}
impl Serialize for LocalDate   // as the string YYYY-MM-DD
pub fn local_date_at(ms: i64, utc_offset_minutes: i32) -> LocalDate;  // local_date uses it
pub fn below(rng: &mut dyn Rng, n: usize) -> usize;   // next_u64() % n; n ≥ 1
pub fn shuffle<T>(items: &mut [T], rng: &mut dyn Rng); // Fisher–Yates: for i from len−1 down to 1, swap(i, below(rng, i + 1))

// parley_core::grading
pub fn normalise(lang: Lang, s: &str) -> String;     // the rules do not depend on lang; English uses it too
pub fn fold_marks(s: &str) -> String;                // F2: NFD, combining marks removed, NFC
pub fn fold_german(s: &str) -> String;               // F1: ä→ae, ö→oe, ü→ue, ß→ss (on normalised text)
pub fn osa_distance(a: &str, b: &str) -> usize;      // optimal string alignment over chars (strsim)
pub enum AccentLeniency { Lenient, Strict, Ignore }  // serde lowercase; Default Lenient
pub enum Verdict { Correct, Almost, Typo, Wrong, Unchecked }   // serde lowercase
impl Verdict { pub fn srs(self) -> Option<srs::Verdict> }      // typo → Almost; unchecked → None
pub enum Tag { Accent, Article, Typo }               // serde lowercase
pub enum Direction { ToTarget, ToEn }                // serde "to_target", "to_en"; as_str
pub struct NounInfo { pub article: String, pub noun: String, pub gender: Gender }
pub fn noun_info(lang: Lang, target: &str, forms: &Forms) -> Option<NounInfo>;
pub struct StoredJudgement { pub verdict: srs::Verdict, pub corrected: String, pub explanation: String,
  pub tags: Vec<String>, pub provider: String }
pub struct GradeCtx<'a> { pub lang: Lang, pub leniency: AccentLeniency, pub forms: &'a Forms,
  pub noun: Option<NounInfo>, pub stored: Vec<String>, pub judgement: Option<StoredJudgement> }
pub enum Answer { Text(String), Choice(u32), Pairs(Vec<[u32; 2]>) }   // serde: {"text": ".."} | {"choice": n} | {"pairs": [[l, r], ..]}
pub struct Grade { pub verdict: Verdict, pub tags: Vec<Tag>, pub message: Option<String>,
  pub corrected: Option<String>, pub words: Vec<String>, pub rating: Option<Rating> }
pub fn grade(ctx: &GradeCtx, ex: &Exercise, answer: &Answer) -> Grade;
pub fn judgement_key(lang: Lang, direction: Direction, source: &str, answer_norm: &str) -> String;
pub fn grade_ctx(store: &Store, forms: &Forms, leniency: AccentLeniency, ex: &Exercise,
  answer: &Answer) -> Result<GradeCtx<'_>, StoreError>;   // the lookups below, for the runner
pub fn accept_learner_answer(store: &mut Store, ex: &Exercise, answer: &Answer, now_ms: i64)
  -> Result<Grade, StoreError>;

// parley_core::exercise
pub enum ExerciseType { PickMeaning, TranslateToTarget, TranslateToEn, ListenType, Speak, FillGap, MatchPairs }
  // serde snake_case; as_str gives the same strings
pub enum Origin { Template, Ai, CoursePack }        // serde snake_case
pub struct Exercise { pub id: String, pub item_id: String, pub facet: Facet, pub origin: Origin,
  #[serde(flatten)] pub body: Body }
pub enum Body {                                     // #[serde(tag = "type", rename_all = "snake_case")]
  PickMeaning { prompt_target: String, options: Vec<String>, answer_index: u32 },
  TranslateToTarget { prompt_en: String, target: String, accepted: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] tiles: Option<Vec<String>> },
  TranslateToEn { prompt_target: String, accepted: Vec<String> },
  ListenType { text: String, accepted: Vec<String> },
  Speak { text: String, prompt_en: String },
  FillGap { sentence: String, answer: String, full_sentence: String, hint_en: String },
  MatchPairs { pairs: Vec<[String; 2]> },          // [target, English]; 4 or 5
}
impl Exercise { pub fn kind(&self) -> ExerciseType; pub fn direction(&self) -> Option<Direction>;
  pub fn targets(&self) -> Vec<&str>; }             // main target-language texts (below)
pub struct Lesson { pub schema: String, pub lang: Lang, pub skill_id: String, pub seq: u32,
  pub origin: Origin, pub exercises: Vec<Exercise> }   // schema "parley.lesson/1"
pub struct Allowed { .. }
impl Allowed {
  pub fn for_skill(syllabus: &Syllabus, numbers: &[String], skill_id: &str) -> Allowed;
  pub fn allows(&self, token: &Token) -> bool;      // module 01's V12 rule
  pub fn is_excluded(&self, token: &Token) -> bool; // a number or a name
}
pub fn coverage(lang: Lang, text: &str, allowed: &Allowed, forms: &Forms) -> (usize, usize); // (covered, counted)
pub fn max_words(cefr: Cefr) -> usize;              // A1 8, A2 12
pub struct CheckCtx<'a> { pub lang: Lang, pub forms: &'a Forms, pub allowed: &'a Allowed,
  pub max_words: usize, pub focus: &'a [Lemma], pub seen: &'a [(ExerciseType, String)] }
pub enum CheckFailure { Coverage { covered: usize, counted: usize }, Answers(String), Duplicate,
  TooLong { words: usize, max: usize }, Focus }
impl CheckFailure { pub fn rule(&self) -> &'static str }   // "L5" .. "L9"
pub fn check_exercise(ex: &Exercise, ctx: &CheckCtx) -> Result<(), CheckFailure>;
pub struct TemplateCtx<'a> { pub syllabus: &'a Syllabus, pub forms: &'a Forms, pub items: &'a [Item],
  pub allowed: &'a Allowed, pub sentences: &'a [Example], pub skill_id: &'a str, pub plan_items: &'a [String] }
pub fn template(item: &Item, kind: ExerciseType, ctx: &TemplateCtx, rng: &mut dyn Rng) -> Option<Exercise>;
pub fn template_lesson(plan: &Plan, ctx: &TemplateCtx, seq: u32, rng: &mut dyn Rng) -> Lesson;
pub fn word_bank(target: &str, lang: Lang, allowed: &Allowed, forms: &Forms, rng: &mut dyn Rng) -> Vec<String>;
pub struct Capabilities { pub audio_out: bool, pub mic: bool }
pub struct ComposeRequest { pub lang: Lang, pub skill_id: String, pub lesson_length: usize,
  pub new_items_per_lesson: usize, pub capabilities: Capabilities }
pub struct PlanEntry { pub n: u32, pub item_id: String, pub facet: Facet, pub kind: ExerciseType,
  pub focus: String, pub new: bool }
pub struct Plan { pub entries: Vec<PlanEntry> }
pub fn compose(req: &ComposeRequest, store: &Store, now_ms: i64) -> Result<Plan, StoreError>;
pub struct RunSettings { pub accent: AccentLeniency, pub daily_goal_xp: u32, pub srs: SrsParams }  // Default: lenient, 20, 0.90
pub struct Submission { pub exercise_id: String, pub answer: Answer, pub response_ms: Option<i64>, pub used_hint: bool }
pub enum RunError { EmptyAnswer, AlreadyChecked, NotAvailable, Store(StoreError) }
pub struct LessonSummary { pub xp: u32, pub correct: u32, pub total: u32, pub streak: u32,
  pub freezes: u32, pub record_week: bool, pub message: Option<String>, pub daily_xp: u32, pub goal_xp: u32 }
pub struct LessonRun { .. }
impl LessonRun {
  pub fn start_lesson(store: &mut Store, lesson: &Lesson, session_id: &str, clock: &dyn Clock)
    -> Result<LessonRun, StoreError>;
  pub fn start_review(store: &mut Store, lang: Lang, exercises: Vec<Exercise>, session_id: &str,
    clock: &dyn Clock) -> Result<LessonRun, StoreError>;
  pub fn exercises(&self) -> &[Exercise];
  pub fn submit(&mut self, store: &mut Store, forms: &Forms, settings: &RunSettings, sub: &Submission,
    clock: &dyn Clock) -> Result<Grade, RunError>;
  pub fn i_was_right(&mut self, store: &mut Store, settings: &RunSettings, exercise_id: &str,
    clock: &dyn Clock) -> Result<Grade, RunError>;
  pub fn finish(self, store: &mut Store, settings: &RunSettings, clock: &dyn Clock)
    -> Result<LessonSummary, RunError>;
}

// parley_core::game
pub enum SessionKind { Lesson, Review }             // serde lowercase; the `sessions.kind` values written here
pub struct SessionResult { pub kind: SessionKind, pub verdicts: Vec<grading::Verdict>, pub finished: bool }
pub fn xp_for(session: &SessionResult) -> u32;
pub struct DayRow { pub local_date: LocalDate, pub xp: u32, pub sessions: u32, pub goal_xp: u32, pub frozen: bool }
pub enum FreezeKind { Earned, Used }                // serde lowercase
pub struct FreezeEvent { pub local_date: LocalDate, pub kind: FreezeKind, pub streak_len: u32 }
pub struct StreakState { pub length: u32, pub freezes: u32, pub active_today: bool,
  pub frozen: Vec<LocalDate>, pub events: Vec<FreezeEvent> }
pub fn streak(daily: &[DayRow], today: LocalDate) -> StreakState;
pub fn weekly_record(daily: &[DayRow], today: LocalDate, session_xp: u32) -> bool;
pub const WEEKLY_RECORD: &str = "New weekly record!";

// parley_core::store (additions)
pub struct ItemRow { pub id: String, pub lang: Lang, pub skill_id: String, pub kind: ItemKind,
  pub target: String, pub en: Vec<String>, pub lemmas: Vec<String>, pub retired: bool }
pub enum AnswerSource { Content, Judge, Learner }   // serde lowercase
pub struct AcceptedAnswer { pub norm: String, pub display: String, pub source: AnswerSource }
impl Store {
  pub fn item(&self, id: &str) -> Result<ItemRow, StoreError>;              // UnknownItem
  pub fn accepted_answers(&self, item_id: &str, direction: Direction) -> Result<Vec<AcceptedAnswer>, StoreError>;
    // ordered content, judge, learner, then created_at, then norm
  pub fn add_accepted_answer(&mut self, item_id: &str, direction: Direction, display: &str,
    source: AnswerSource, now_ms: i64) -> Result<bool, StoreError>;         // false: (item, direction, norm) exists
  pub fn judgement(&self, key: &str) -> Result<Option<StoredJudgement>, StoreError>;
  pub fn put_judgement(&mut self, key: &str, j: &StoredJudgement, now_ms: i64) -> Result<(), StoreError>;
  pub fn next_lesson_seq(&self, skill_id: &str) -> Result<u32, StoreError>; // 1 + the highest stored seq, from 1
  pub fn daily(&self) -> Result<Vec<DayRow>, StoreError>;                   // by date
  pub fn freeze_events(&self) -> Result<Vec<FreezeEvent>, StoreError>;      // by id
}
```

Main target texts (`Exercise::targets`, used by L7 to L9): `pick_meaning` its `prompt_target`;
`translate_to_target` its `target`; `translate_to_en` its `prompt_target`; `listen_type` and
`speak` their `text`; `fill_gap` its `full_sentence`; `match_pairs` the target side of each pair.
Direction: `to_target` for `translate_to_target`, `listen_type`, `speak` and `fill_gap`; `to_en`
for `translate_to_en`; none for `pick_meaning` and `match_pairs`. Facet: `receptive` for
`pick_meaning`, `translate_to_en`, `listen_type` and `match_pairs`; `productive` for the others.

User-facing messages: "Almost: mind the accent: {words}", "Wrong: mind the accent: {words}", "Note
the accent: {words}", "Almost: remember the article: {target}", "{Noun} is {masculine|feminine|
neuter}: {target}.", "Typo: {words}", "Type an answer first.", "This exercise was already
checked.", "This exercise is no longer available.", "New weekly record!". `{words}` is the expected
spelling of the words concerned, joined with ", "; `{Noun}` is the noun with its first letter in
upper case. `RunError` displays the three run messages (`Store` displays the store's message).

CLI (one JSON object on stdout; `--json` is accepted and changes nothing; usage errors exit 2 with
one line on stderr and nothing on stdout, in module 01's wording):

    parley grade --lang <l> --type <t> --expected <s> [--expected <s>]... --answer <s>
                 [--accent lenient|strict|ignore] [--noun] [--forms <forms.tsv>] [--json]
        Grades one text answer against the expected answers (in order) of an exercise of type <t>
        (translate_to_target, translate_to_en, listen_type, speak or fill_gap), with nothing stored
        and no judgement. --forms defaults to content/<l>/forms.tsv (an empty table when that file
        does not exist). --noun applies the article rule to the first expected answer.
        → {"verdict": "...", "tags": [...], "message": "..." | null, "corrected": "..." | null,
           "words": [...], "rating": 1..4 | null}
        Usage errors: `error: --lang is required. Run parley --help.` (likewise --type, --expected,
        --answer), `error: --lang must be es, fr or de. Run parley --help.`,
        `error: --type must be translate_to_target, translate_to_en, listen_type, speak or fill_gap. Run parley --help.`,
        `error: --accent must be lenient, strict or ignore. Run parley --help.`,
        `error: {path} not found` (an explicit --forms), `error: --noun needs an expected answer that
        starts with an article. Run parley --help.`, unknown arguments as module 01.
        An answer that normalises to nothing: exit 1, `error: Type an answer first.`
    parley lesson template --lang <l> --skill <id> [--seq <n>] [--seed <s>] [--content <dir>] [--json]
        The template lesson of skill <id> for a learner who has done lessons 1 to n−1 of it: an
        in-memory store started from <dir>/<l>/ (syllabus.yaml, forms.tsv, numbers.yaml when
        present; default dir content) at time 0, the skill's first (n−1)×5 items (syllabus order)
        introduced at time 0 with no reviews, compose at time 0 with lesson_length 12,
        new_items_per_lesson 5 and no capabilities, then template_lesson with
        Allowed::for_skill, no pack sentences and SplitMix64(<s>). --seq defaults to 1, --seed to 0.
        → the Lesson object of schemas/lesson.schema.json
        Usage errors: `error: --lang is required. Run parley --help.`, the same for --skill,
        `error: --seq must be a whole number from 1. Run parley --help.`,
        `error: --seed must be a whole number. Run parley --help.`, `error: {path} not found` for a
        missing syllabus. A skill the syllabus does not have: exit 1, `error: unknown skill {id}`.
        A syllabus that does not load: exit 1 and the load errors as `content validate --json`
        prints them.
    parley streak simulate <days.json> [--json]
        days.json: {"today": "YYYY-MM-DD", "utc_offset_minutes": n (default 0), "goal_xp": n
        (default 20), "sessions": [{"started_at": ms, "xp": n}, ...]}. Each session counts on the
        local date of started_at at that offset. Sessions are taken in order: each one's weekly
        record is weekly_record(rows before it, its local date, its xp), then it is added to its
        day's row (xp + n, sessions + 1). Then streak(rows, today).
        → {"length": n, "freezes": n, "active_today": bool, "frozen": ["YYYY-MM-DD", ...],
           "events": [{"local_date": "...", "kind": "earned" | "used", "streak_len": n}, ...],
           "days": [{"local_date": "...", "xp": n, "sessions": n}, ...],
           "records": [{"local_date": "...", "message": "New weekly record!"}, ...]}
        Usage errors: `error: <days.json> is required. Run parley --help.`, `error: {path} not found`.
        A file that is not a days file: exit 1, `error: {path}: {parser message}`.

## Out of scope

- Screens and Tauri commands (04); the AI judge and explanations (08); audio playback and
  recording (09, 10); AI lessons (07). `listen_type` and `speak` exist as data only: they are
  generated and graded as typed text, nothing plays or records.
- Live quality gates (from 06); gate G6 (placement) is not added here.

## Design notes

- Normaliser, in order: NFC; typographic apostrophes (’ ‘ ʼ) to '; U+00A0 and U+202F to space;
  lowercase (`str::to_lowercase`, not case folding, so ß stays); every character that is not a
  letter or number (`char::is_alphanumeric`), a combining mark (`unicode-normalization`'s
  `is_combining_mark`) or white space becomes a space, except an apostrophe or hyphen-minus
  between two letters; collapse whitespace to single spaces; trim. (This removes Unicode
  categories P and S, and also controls and other symbols; replacing by a space keeps `Hola,¿qué`
  two words.) The display words of a text are the same steps without the lowercase step; the
  i-th display word is the i-th normalised word.
- Accent folding: F2 removes combining marks after NFD; for German also F1 maps ä→ae, ö→oe, ü→ue,
  ß→ss. Two texts are accent-equal when they differ but are equal under F2, or (German) under F1 on
  both sides. Words are compared the same way.
- Grading of a text answer `A = normalise(answer)` against the candidates — the exercise's answers
  (below) in order, then `ctx.stored`, without repeats by normal form — first rule that applies:
  1. exact: `A` equals a candidate's normal form → `correct`, no tags, no message, `corrected` none.
  2. article (only when `ctx.noun` is set): `A` equals the normalised noun → `almost`, `[article]`,
     "Almost: remember the article: {target}"; `A` is a definite article of the language other
     than the noun's, a space and the normalised noun → `wrong`, `[article]`, "{Noun} is
     {gender}: {target}." (`{target}` the first expected answer as written; `corrected` it too).
     Definite articles: es el, la, los, las; fr le, la, les, l'; de der, die, das, den, dem, des.
  3. accent: the first candidate accent-equal to `A`: lenient → `almost`, `[accent]`, "Almost: mind
     the accent: {words}", Hard; strict → `wrong`, `[accent]`, "Wrong: mind the accent: {words}",
     Again; ignore → `correct`, `[accent]`, "Note the accent: {words}", Good. `{words}`: the
     candidate's display words at the positions whose normalised words differ.
  4. typo: the first candidate with the same word count n > 0 where the differing words (not
     equal and not accent-equal) number from 1 to `max(1, n / 4)` (integer division), and each
     has an optimal string alignment distance of exactly 1 between the F2 forms, is at least 4
     characters long in the candidate, and is not itself a form in the forms table (`lemmas_of`
     the word, or of the word with its first letter in upper case, is not empty) → `typo`,
     `[typo]` plus `accent` when another word differs only in accents, "Typo: {words}" with every
     position whose words are not equal, Hard.
  5. judgement: `ctx.judgement` → its verdict, `corrected` its corrected text, its tags kept when
     they are accent, article or typo, no message.
  6. otherwise `translate_to_target` and `translate_to_en` → `unchecked` (rating none, `corrected`
     none; module 08 sends it to the judge, and without a provider the sheet offers "I was
     right"); every other type → `wrong`, `corrected` the first candidate, Again.
  `corrected` is the candidate the verdict was measured against (none for an exact match);
  `words` holds the words the message names. The rating is module 02's `rating_for` with the
  verdict's `srs()` value, so `typo` and `almost` rate Hard and an `ignore` accent slip rates Good.
  An empty `A` is `wrong` with no message; the runner refuses it before grading.
- The exercise's answers: `translate_to_target` and `translate_to_en` and `listen_type` their
  `accepted`; `speak` its `text`; `fill_gap` its `answer`. `pick_meaning` is `correct` when the
  answer is `Choice(answer_index)` or a text whose normal form equals that option's; `match_pairs`
  is `correct` when the answer is `Pairs` whose set equals {[i, i] for every pair i} (the interface
  shuffles the sides and maps them back to pair indexes); anything else is `wrong`.
- Lookups (`grade_ctx`), in the order of ARCHITECTURE's grading rule: `stored` holds the displays
  of `accepted_answers` for the item and the exercise's direction (all sources) when the
  exercise's prompt is the item's own — `translate_to_target` whose `prompt_en` is one of the
  item's English texts, `translate_to_en`, `listen_type` or `speak` whose prompt is the item's
  target (NFC compared); `judgement` is the `judgements` row for `judgement_key(lang, direction,
  source, A)` where source is the prompt (`prompt_en`, `prompt_target`, `text` or `sentence`);
  `noun` is set for a vocabulary item's `translate_to_target`, `listen_type` and `speak` when
  `noun_info` finds one whose noun is the item's lemma.
- `judgement_key`: SHA-256 of the UTF-8 bytes of `lang`, U+001F, the direction string, U+001F,
  the NFC source, U+001F, the answer's normal form; lower-case hex. Module 08 uses the same key.
- `noun_info`: the target is a definite article of the language, a space and a noun (French also
  `l'` and the noun with no space); the gender is the forms table's `gender_of(noun)`, else the
  article's (es el m, la f; fr le m, la f; de der m, die f, das n); none without a gender.
- `accepted_answers.norm` stays module 02's key (NFC lower case of the display); grading always
  compares `normalise(display)`, so the normaliser can change without rewriting the table.
- "I was right" (`accept_learner_answer`): for a text answer to an exercise with a direction, a
  `learner` row in `accepted_answers` (display: the answer, NFC and trimmed) when the prompt is the
  item's own, otherwise a `judgements` row for the answer's key (verdict correct, corrected the
  answer, explanation empty, tags empty, provider `learner`). The grade is `correct`, rating Good.
  Either way the same answer is `correct` next time, offline.
- Coverage (L5, also used by module 07): the tokens of the texts (`tokenise` with the forms table);
  numbers and names (`Allowed::is_excluded`: a number token, a word of `numbers.yaml`, a name) are
  not counted; a counted token is covered when `Allowed::allows` accepts it — a function word or a
  form of an allowed lemma by module 01's V12 rule (`TaughtForms::matches`), so each word of a
  multiword lemma (`por favor`) counts. `Allowed::for_skill` allows the function words, names and
  number words of the syllabus and the lemmas of every skill up to and including the given one in
  syllabus order, so every example of a valid syllabus is fully covered. Module 07 adds the
  Snowball fallback.
- Structural checks, in order, the first failure returned: **L5** coverage of the target-language
  texts (the main texts plus `accepted` of `translate_to_target` and `listen_type`) at least 90%
  (nothing counted passes); **L6** answers: `translate_to_target` and `listen_type` have their
  target's normal form among their accepted answers' (and at least one); `translate_to_en` at
  least one accepted answer; `fill_gap` exactly one `___` in `sentence`, a one-word `answer`, and
  `sentence` with the gap replaced by `answer` normalising to `full_sentence`'s normal form;
  `pick_meaning` 4 options with distinct normal forms and `answer_index` 0 to 3; `match_pairs` 4 or
  5 pairs with distinct normal forms on each side; **L7** no earlier exercise in `seen` has the
  same type and the same normal form of its first main text (`match_pairs` is exempt); **L8**
  every main text has at most `max_words` words; **L9** when `focus` is not empty, a token of a
  main text matches one of its lemmas (`TaughtForms::matches`).
- Template rules: `pick_meaning` (vocabulary) shows the item's target with 4 options, its first
  English gloss and three distractors: the first glosses of other items of the same kind (vocab
  with vocab, phrase and pattern together) in the same skill, then in earlier skills, never equal
  (normal form) to any accepted English answer of the item or to each other; up to 3 at random
  from the same skill, the rest at random from earlier skills; fewer than 3 → none; the 4 are
  shuffled. `translate_to_target`: the first English gloss, the target, the item's `to_target`
  answers and word-bank tiles. `translate_to_en`: the target and the item's `to_en` answers.
  `listen_type`: the target and its `to_target` answers. `speak`: the target and the first gloss.
  `fill_gap` (vocabulary, single-word lemmas only): one sentence at random among the examples of
  every skill up to and including the lesson's skill and the pack sentences that contain a form of
  the lemma, whose tokens are all allowed and with at most `max_words` words; the first such token
  is replaced by `___`; `answer` is that token as written, `hint_en` the item's first gloss.
  `match_pairs`: the item and the next vocabulary items of `plan_items`, 4 pairs with distinct
  targets and glosses, in random order; fewer than 4 → none.
- Template lesson: one exercise per plan entry, id `e{n}`; the entry's kind is tried first, then
  its fallbacks (`match_pairs` → `pick_meaning` → `translate_to_en`; `pick_meaning` →
  `translate_to_en`; `fill_gap` → `translate_to_target`; `listen_type` → `translate_to_en`;
  `speak` → `translate_to_target`), keeping the first that passes `check_exercise` with the
  entry's item's lemmas as focus and the lesson's earlier exercises as `seen`; an entry with none
  is dropped. `max_words` comes from the lesson skill's CEFR level.
- Word bank: the target tiles are the words of `normalise(target)`; up to 3 distractors at random
  from the allowed single-word forms (function words, allowed lemmas, the forms table's forms of
  allowed lemmas; lower case; sorted, without repeats) that are not target tiles; all tiles are
  then shuffled. Tiles joined with spaces in the right order are the target's normal form.
- Composer (`compose`), deterministic, from the store at `now_ms`:
  1. New items: the skill's first `new_items_per_lesson` items (syllabus order: lemmas before
     examples) that have no cards. Each gets its receptive intro, in order (`new: true`).
  2. If at least 4 distinct vocabulary items are among the new items and the due cards' items, a
     `match_pairs` entry for the first vocabulary item among the new items, then the due cards in
     queue order (facet receptive).
  3. Due cards: `due_queue(store, lang, now_ms, lesson_length)`. Alternate one due card and one
     productive entry of a new item (in order) until both run out.
  4. Fill: the cards of the skill's introduced items not already planned as (item, facet), by
     retrievability at `now_ms` ascending, then the item's syllabus position, receptive first.
  Stop at `lesson_length` entries. Kinds: receptive vocabulary → `pick_meaning`; receptive phrase
  or pattern → `translate_to_en`; productive vocabulary → `fill_gap` (`translate_to_target` when
  its lemma has a space); productive phrase or pattern → `translate_to_target`. Then with
  `audio_out` the first 2 `translate_to_en` entries become `listen_type`; with `mic` the first 2
  `translate_to_target` entries of phrase or pattern items whose focus has at most 8 words become
  `speak`. `focus` is the item's target; `n` counts from 1.
- Runner: `start_lesson` stores the lesson (`lessons` id `{skill_id}/{seq}`, content its JSON; an
  existing (skill, seq) row is kept) and a `sessions` row (kind lesson, `local_date` of the start);
  `start_review` only the session (kind review). `submit`: an unknown exercise id → "This exercise
  is no longer available."; one already submitted → "This exercise was already checked."; a text
  answer with an empty normal form or an empty `Pairs` → "Type an answer first." (nothing
  recorded); otherwise it grades (`grade_ctx`, `grade`), introduces the item when it has no cards,
  and unless the verdict is `unchecked` reviews the card (item, facet) with `rating_for` (response
  time and hint from the submission) and the verdict's `srs()` value. `i_was_right` applies to a
  submitted `unchecked` exercise only (unknown id → no longer available; not submitted → "Type an
  answer first."; any other verdict or a second time → already checked): it calls
  `accept_learner_answer` and reviews the card Good with `learner_override`. `finish`: XP from
  `xp_for` over the final verdicts, the session row completed (ended_at, xp, exercises submitted,
  correct), the day's `daily` row credited (xp, sessions + 1, goal_xp from the settings), the
  weekly record decided before the credit, the streak recomputed from `daily` with the frozen days
  written (`daily.frozen` = 1, a row with 0 XP and 0 sessions when the day had none) and the
  events not yet in `freeze_events` (by date and kind) appended. Freezes are never written any
  other way (invariant I7). Nothing limits the number of lessons (invariant I8).
- XP: 1 per correct, typo or almost answer (unchecked earns nothing and is not wrong); 10 for
  finishing a lesson, 5 for a review session; +5 for a finished lesson with no wrong answer.
  Daily goal: met when the day's XP reaches `goal_xp`.
- Streak, processed day by day from the first active day up to yesterday, then today if active
  (a day is active when its `daily` row has sessions > 0): an active day adds 1; at lengths 7,
  14, 21… a freeze is earned if fewer than 2 are banked; a missed day uses a banked freeze (length
  unchanged, the day marked frozen) or resets the length to 0. Today, not yet active, keeps
  yesterday's streak. Weekly record: ISO weeks on local dates; `weekly_record` is true when the
  best earlier week's XP is above 0, this week's XP before the session is at most that, and this
  week's XP after it is above it — so "New weekly record!" appears once per week, when this
  week's XP first exceeds the best earlier week.
- Migration `0002_sessions.sql` is ARCHITECTURE §4's DDL for `sessions` to `freeze_events` and
  `sessions_date`, verbatim. A new file now opens at `user_version` 2, so module 02's checks of a
  new file (seven tables, version 1) now expect version 2 and the seven tables among twelve.
- Decisions made during implementation:
  - `grade_ctx` is written `grade_ctx<'a>(store: &Store, forms: &'a Forms, ...) ->
    Result<GradeCtx<'a>, StoreError>`: with two borrowed inputs Rust cannot elide `GradeCtx<'_>`,
    and the context borrows only the forms table. Callers write the same call.
  - Grade details the rules leave open: the article messages name the expected answer, so
    `words` is `[target]`; a typo beside an accent slip is tagged `[typo, accent]` in that order;
    an empty `A` is `wrong` with `corrected` the first candidate and rating Again; a text answer
    with a `choice` or `pairs` answer is `wrong`; a wrong `pick_meaning` has `corrected` the right
    option, a wrong `match_pairs` none; a candidate that normalises to nothing is skipped. The
    rating `grade` returns assumes no hint and no response time; the runner rates again with both
    and returns that rating.
  - `noun_info` reads the French `l'` with either apostrophe (`l'`, `l’`) and keeps the article as
    written; `l'` gives no gender of its own, so such a noun needs the forms table's.
  - "I was right" returns `correct` with no tags, message or `corrected`, rated Good; an answer
    that normalises to nothing stores nothing. `put_judgement` replaces an earlier judgement of
    the same key, so the learner's word overrules an earlier verdict on the same answer.
  - `Allowed` shares module 01's V12 word lists (function words, names, numbers); a name is a
    name of the syllabus, as in V12. A skill the syllabus does not have allows no lemma.
  - The tokeniser splits a French elision prefix from its word (`s'appeler` gives `s'` and
    `appeler`), so module 01's `TaughtForms` now also teaches the word after the prefix. Without
    it the vocabulary item `s'appeler` could pass neither L5 nor L9, and a plan entry would be
    dropped; V12 only gains accepted words.
  - Checks: L5 sums covered and counted tokens over all its texts and passes at
    `covered × 10 ≥ counted × 9`; L7 normalises both sides; L8 counts the words of the normal
    form.
  - Template: `pick_meaning` works for every item kind (distractor groups as the rule says); the
    same-skill and earlier-skill pools are built in syllabus order, the same skill first, and a
    random choice is a `shuffle` and the first k; then the four options are shuffled. `fill_gap`
    candidates are the examples of the skills up to the lesson's, in syllabus order, then the
    pack sentences; `max_words` is the lesson skill's; the gap replaces the token at its place
    in the sentence. `match_pairs` needs a vocabulary item and takes the vocabulary items that
    follow its first place in `plan_items` (from the start when it is not there), exactly 4.
    `plan_items` is the plan's item ids in entry order. `template` leaves the id empty; `template_lesson` sets it to `e{n}` with the entry's `n` and
    sets the facet to the entry's.
  - Word bank: distractor candidates are normalised and must be one word; the 3 are drawn with
    `below` without replacement before the final shuffle.
  - Composer: the `match_pairs` entry is for the first vocabulary item among the new items, then
    among the due cards in queue order, facet receptive. `new` is true for every entry of an item
    the lesson introduces (its intro, its productive entry and a `match_pairs` entry for it). The
    two `speak` entries are the first two `translate_to_target` entries that qualify (a phrase or
    pattern item whose focus has at most 8 words), so an entry that does not qualify takes no
    place; the length limit counts the words of the focus's normal form.
  - Runner: a session counts on the local date of its start, so `finish` credits that day; the
    streak is computed for the local date at `finish`. `finish` always counts the session as
    finished for `xp_for`. `LessonSummary.correct` and `sessions.correct` count `correct`
    verdicts, `sessions.exercises` the answers submitted, `LessonSummary.total` the exercises of
    the run. `start_session` and `finish_session` each run in one transaction, and a freeze event
    already stored for its date and kind is not written again.
  - Streak: a `used` event's `streak_len` is the length the freeze kept; a frozen day stays in
    `frozen` after a later reset.
  - `LocalDate` also reads itself from `YYYY-MM-DD` text (serde), which `streak simulate` uses.
  - CLI: `grade` builds an exercise of the given type whose answers are the expected ones; for
    `speak` and `fill_gap`, which hold one answer, the others are tried after it as stored
    answers, so every expected answer counts in order. Usage errors check the required flags
    first, then their values. `lesson template` reports load errors before an unknown skill.
    `streak simulate` ignores fields it does not know; a `today` that is not a real date or an
    `xp` below 0 makes the file not a days file; `days` lists the dates that have sessions, in
    date order.

## Acceptance tests

Golden cases in `tests/cases/03-exercise-engine-and-grading/`; code tests in
`crates/parley-core/tests/m03_exercise_engine_and_grading.rs` and
`crates/parley-cli/tests/m03_exercise_engine_and_grading.rs`; gate tests in
`crates/parley-core/tests/gates.rs`. Expected values follow from the rules above and UAX #15;
plans are computed by hand from the composer rules. The hand-written fixtures sit as files in the
golden folder: a small Spanish course (`course-es.yaml`, `course-es.forms.tsv`) with a food skill
holding `café`, `leche`, `agua` (article `el`), `perro`, `querer` and examples such as `Quiero un
café con leche.`, and whose forms table has `pero`, `día`, `quiero`; and the composer fixtures.

1. `normalise`: es `  ¿Qué  TAL? ` → `qué tal`; fr `Aujourd’hui, c’est lundi !` → `aujourd'hui c'est
   lundi`; de `Die Straße.` → `die straße`; es `e` + U+0301 + `l` → `él`; fr `Est-ce que…` → `est-ce que`.
2. Expected `Hola, ¿qué tal?`: answer `hola qué tal` → correct; `Hola que tal` → almost, tags
   `[accent]`, "Almost: mind the accent: qué".
3. Expected `Buenos días.`, answer `Buenos dias`: lenient → almost (Hard); strict → wrong, "Wrong:
   mind the accent: días"; ignore → correct (Good), "Note the accent: días".
4. de expected `Das Mädchen`: `das Maedchen`, `das Madchen` → almost; expected `die Straße`:
   `die Strasse` → almost.
5. Vocabulary noun `el día`: `día` → almost, "Almost: remember the article: el día"; `la día` →
   wrong, "Día is masculine: el día."; `el agua` (feminine, article `el`): `la agua` → wrong
   "Agua is feminine: el agua.".
6. Expected `Quiero un café con leche.`: `Quiero un café con lecje` → typo, "Typo: leche"; expected
   `el perro`, answer `el pero` (`pero` is in the forms table) → not a typo; expected `le livre`,
   answer `la livre` → not a typo (the differing word has 2 letters).
7. An 8-word accepted answer with 2 one-letter slips in long words → typo; with 3 → not typo.
8. `translate_to_target` unmatched → `unchecked` (rating null); `fill_gap` unmatched → `wrong`.
9. "I was right" on an unchecked `translate_to_target` whose prompt is the item's own → rating
   Good, a `learner` row in `accepted_answers`; the same answer next time → correct offline. With a
   prompt that is not the item's own → a `judgements` row with provider `learner`, and the same
   answer next time → correct. A `judgements` row with verdict almost for an answer's key → almost
   with its corrected text. `judgement_key` of a fixed input equals the SHA-256 computed by a
   standard tool.
10. For every skill of the seed content and seeds 1 to 100, the template lesson built as `parley
    lesson template` builds it (seq 1, through the library) has one exercise per plan entry, every
    exercise passes `check_exercise` (focus the entry's item's lemmas, `seen` the earlier
    exercises, `Allowed::for_skill`, `max_words` of the skill's level), the lesson validates
    against `schemas/lesson.schema.json`, and the same seed gives the same lesson. A few runs of
    the CLI itself check the same and that seeds 1 and 2 give different lessons for a skill.
11. `pick_meaning` for `el agua` never offers an option equal to `water` or any other accepted gloss.
12. `fill_gap` for lemma `leche` over `Quiero un café con leche.` → `Quiero un café con ___.`,
    answer `leche`.
13. Composer fixtures `composer-a.json` (5 new items, no dues), `composer-b.json` (3 new, 5 due) and
    `composer-c.json` (capabilities on) → the plans in `composer-a.expected.json` and the others;
    no plan exceeds `lesson_length`; with `audio_out` false there is no `listen_type`, with `mic`
    false no `speak`. A composer fixture is `{"syllabus": path, "forms": path, "skill": id,
    "now_ms": n, "lesson_length": n, "new_items_per_lesson": n, "audio_out": bool, "mic": bool,
    "reviews": [{"item_id": id, "facet": f, "rating": 1..4, "at_ms": n}]}`: the test starts the
    course at 0, introduces each reviewed item and applies the reviews in order with the default
    retention; an expected file is `{"entries": [PlanEntry, ...]}`.
14. Word bank for `Quiero un café.` → 3 target tiles plus 3 distractor tiles, all allowed forms;
    the same seed gives the same order; tiles in the right order grade correct.
15. XP: 12 exercises, 10 right and 2 wrong → 20; 12 right → 27.
16. Streaks (`parley streak simulate`): active days 1 to 7 → length 7, 1 freeze; then day 8 missed and
    day 9 active → length 8, day 8 frozen, 0 freezes; days 1 to 21 active → 2 freezes (none at 21);
    1 freeze, days 8 and 9 missed, day 10 active → length 1; sessions at 23:30 and 00:10 local count
    as two days.
17. Weeks of 100 and 80 XP, then a third week reaching 101 → "New weekly record!" exactly once.
18. 100 lessons started and finished on one local date all succeed (invariant I8); the day's
    `daily` row has 100 sessions. Freezes appear in `freeze_events` only as the streak earns them
    (invariant I7).
19. Messages: empty answer → "Type an answer first."; second submit → "This exercise was already
    checked."; unknown id → "This exercise is no longer available."; "I was right" on a correct
    answer → "This exercise was already checked."; the CLI usage errors above.
20. Gate (`scripts/gates/run`, in `tests/gates.rs`): grading p99 ≤ 5 ms over 10,000 answers; a
    template lesson ≤ 200 ms; G3, 1,000 seeded template lessons over the real syllabi, all
    exercises pass `check_exercise` (test 10 is its scaled-down version in the normal suite).
21. Migration 0002 on a version-1 file creates `sessions`, `lessons`, `judgements`, `daily` and
    `freeze_events` with the columns of ARCHITECTURE §4 and sets `user_version` 2.
22. `LocalDate`: 2026-01-01 is ISO week 2026-W01; 2027-01-01 is 2026-W53; 2024-12-30 is
    2025-W01; `parse` refuses `2026-02-30`; `from_days(days(d)) == d`.

## Status

Done (2026-11-03)
