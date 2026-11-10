# 07: Lesson generation

## Goal

With a provider set up, every lesson is written fresh for the learner's skill and known words, and
every unit ends with a short graded story. Nothing reaches the learner before it passes the
validators; anything that fails is replaced from the course pack. Learners can export the lessons
they have as a course pack and import one from someone else, checked the same way.

## Depends on

03, 06. Feature-map rows **P1** (fresh lesson) and **P2** (graded story) of
`docs/PUBLIK-INTEGRATION.md`; its §6.3 and §6.4 are the specification of both calls, §9 the
behaviour on errors.

## Scope

- `prompts/lesson.v1.md` and `prompts/story.v1.md` with the exact text of PUBLIK-INTEGRATION §6.3
  and §6.4; schemas `schemas/ai/lesson.v1.schema.json` and `schemas/ai/story.v1.schema.json`.
- `ai::lesson`: the prompt values from the course and the composer's plan (module 03), the call
  through `ai::call`, validators L1 to L9, template fill, the notice, the cache rule, the lesson
  stored with each exercise's `origin` and the lesson's AI summary.
- Language identification with `whatlang` (L4, S2, S6) and the Snowball stem fallback with
  `rust-stemmers` (L5, S3, S5).
- `lesson_start` with `ai.fresh_lessons` on and a ready provider; the loading state and **Use the
  course pack instead** after 5 s (`lesson_use_course_pack`); the error behaviour of
  PUBLIK-INTEGRATION §9 for P1.
- Setting `ai.prepare_next_lesson` (default on): after a lesson finishes, one P1 call for the next
  lesson of the same skill, never while `needs_credit`.
- Stories: the Story node after the last skill of a unit, `lesson_start(mode: story)`, the story
  screen, question grading, XP; stories stored in a `stories` table (migration `0003_stories`) and
  kept in backups.
- Course packs `parley.coursepack/1`: `course_pack_export`, `course_pack_import`, the report; an
  imported lesson is the next lesson its skill plays.
- Settings > AI: switches for `ai.fresh_lessons`, `ai.prepare_next_lesson` and `ai.stories`; the
  Local model section shows the validity rate of the last lessons (`ai_lesson_checks`).
- An AI command saves back only the AI settings it changed, so a switch turned while an AI request
  runs is kept.
- `parley lesson render <prompt.json>`: the P1 request body for a prompt, for the golden case.
- Gate G1: the live gate in `crates/parley-cli/tests/gates_live.rs` (`scripts/gates/run --live G1`);
  its scaled-down replay of `tests/fixtures/ai/P1/recorded/` in the normal suite.

## Files

- `crates/parley-core/src/ai/{lesson.rs,story.rs,validate.rs,langid.rs}`,
  `crates/parley-core/src/coursepack.rs`, `crates/parley-core/src/app/{lesson.rs,story.rs}`,
  `crates/parley-core/src/store/migrations/0003_stories.sql` and the store and backup additions
- `prompts/{lesson,story}.v1.md`, `schemas/ai/{lesson,story}.v1.schema.json`,
  `schemas/coursepack.schema.json`; `schemas/lesson.schema.json` and `schemas/backup.schema.json`
  (the optional fields below)
- `app/src-tauri/src/commands/{lesson.rs,coursepack.rs,ai.rs,app.rs}`;
  `app/src/screens/Story/Story.tsx`, `app/src/screens/Lesson/Loading.tsx`, the Path, Settings > AI
  and Settings > Data sections; `app/src/api/commands.ts`; `app/src/strings/en.json`
- `crates/parley-cli/src/cmd/lesson.rs` (`lesson render`); `scripts/gates/run`, `run.ps1`,
  `thresholds.toml`
- Tests: `crates/parley-core/tests/m07_lesson_generation.rs`, `tests/cases/07-lesson-generation/`,
  `app/src/test/m07/*.test.tsx`, `crates/parley-cli/tests/gates_live.rs`;
  fixtures `tests/fixtures/ai/P1/*.json`, `tests/fixtures/ai/P1/recorded/*.json`,
  `tests/fixtures/ai/P2/*.json`, `tests/fixtures/coursepack/*.json`, and a small test course under
  `tests/fixtures/course07/` when the tests need one

## Interfaces

### parley-core: `parley_core::ai` (additions, all re-exported from `ai/mod.rs`)

```rust
// langid.rs: whatlang restricted to English, Spanish, French and German (Detector::with_allowlist).
pub enum TextLang { En, Es, Fr, De }
/// The confidence an identification needs (starting value 0.5; see Design notes).
pub const LANGID_THRESHOLD: f64 = 0.5;
/// Some((language, confidence)) or None when whatlang gives no answer.
pub fn identify(text: &str) -> Option<(TextLang, f64)>;
/// True when `text` has fewer than four words (normalise, then UAX #29 words), or when it is
/// identified as `expected` with a confidence of at least LANGID_THRESHOLD.
pub fn is_language(text: &str, expected: TextLang) -> bool;
/// The Snowball stem of one lower-cased word (rust-stemmers: Spanish, French, German).
pub fn stem(lang: Lang, word: &str) -> String;

// lesson.rs
pub const FRESH_LESSON_UNAVAILABLE: &str = "A fresh lesson was not available, so this one comes from the course pack.";
pub const OFFLINE_LESSON: &str = "You're offline, so this lesson comes from the course pack.";
pub fn lesson_prompt_file() -> &'static Prompt;   // prompts/lesson.v1.md + its schema, compiled in

/// What P1 sends, as data (serde field names as written; `kind` serialises as "type").
pub struct LessonPrompt {
    pub lang: Lang, pub cefr: Cefr, pub skill_id: String, pub seq: u32,
    pub skill_title: String, pub can_do: String, pub grammar_note: Option<String>,
    pub variety: String, pub allowed_lemmas: Vec<String>, pub names: Vec<String>,
    pub plan: Vec<PromptEntry>,
}
pub struct PromptEntry { pub n: u32, pub item_id: String, #[serde(rename = "type")] pub kind: ExerciseType,
    pub focus: String, pub gloss: Option<String>, pub new: bool }
impl LessonPrompt {
    /// From the course: the skill's title, can-do, grammar note and level; the syllabus's variety
    /// and names; the allowed lemmas (below); one entry per plan entry, `gloss` the item's first
    /// English gloss for a vocab item and None for phrase and pattern items.
    pub fn new(syllabus: &Syllabus, items: &[Item], introduced_skills: &[String], skill_id: &str,
        seq: u32, plan: &Plan) -> Option<LessonPrompt>;   // None: the skill is not in the syllabus
    /// The placeholder values of PUBLIK-INTEGRATION §6.3 (below).
    pub fn vars(&self) -> BTreeMap<String, String>;
    /// {"lang", "skill_id", "seq", "plan", "allowed_sha256", "names"} (below).
    pub fn key_input(&self) -> serde_json::Value;
}
/// The allowed lemma list of P1 and P2 (below).
pub fn allowed_lemmas(syllabus: &Syllabus, introduced_skills: &[String], skill_id: &str) -> Vec<String>;

pub struct LessonCtx<'a> {
    pub prompt: LessonPrompt,
    pub syllabus: &'a Syllabus, pub forms: &'a Forms, pub items: &'a [Item],
    pub numbers: &'a [String], pub sentences: &'a [Example],   // as the template lesson gets them
}
/// The AI summary a lesson keeps (`Lesson::ai`): who wrote it and how many of its planned
/// exercises passed L2 to L9.
pub struct LessonAiInfo { pub provider: ProviderKind, pub valid: u32, pub planned: u32 }
pub struct LessonDraft {
    pub lesson: Lesson,                      // origin Ai when any exercise is Ai, else Template
    pub dropped: Vec<(usize, RuleFailure)>,  // (position in the response, from 1; the first rule broken)
    pub notice: Option<String>,              // FRESH_LESSON_UNAVAILABLE or None
    pub cached: bool,
    pub charge_micros: Option<i64>,
}
/// P1 end to end: ai::call with the schema as the only rule (L1); then L2 to L9 per exercise,
/// template fill, the notice. An answer that fails L1 (AiError::Invalid) gives Ok: the whole
/// template lesson with the notice and nothing cached. Any other AiError is returned as it is.
pub fn generate_lesson(client: &dyn AiClient, cache: &Cache, plan: &Plan, ctx: &LessonCtx,
    rng: &mut dyn Rng) -> Result<LessonDraft, AiError>;
/// The two halves of generate_lesson, for a call made with no lock held:
pub fn request_lesson(client: &dyn AiClient, cache: &Cache, prompt: &LessonPrompt)
    -> Result<Called<LessonResponse>, AiError>;
pub fn build_lesson(response: Option<&LessonResponse>, provider: ProviderKind, plan: &Plan,
    ctx: &LessonCtx, rng: &mut dyn Rng) -> LessonDraft;   // None: L1 failed
pub struct LessonResponse { pub exercises: Vec<serde_json::Value> }   // schema-valid P1 answer
/// The validity rate of Settings > AI > Local model over `recent` (newest first, at most 10).
pub struct LessonChecks { pub lessons: u32, pub valid: u32, pub planned: u32, pub message: Option<String> }
pub fn lesson_checks(recent: &[LessonAiInfo]) -> LessonChecks;

// story.rs
pub fn story_prompt_file() -> &'static Prompt;    // prompts/story.v1.md + its schema, compiled in
pub struct Story { pub title: String, pub title_en: String, pub paragraphs: Vec<String>,
    pub questions: Vec<StoryQuestion> }                      // serde as the schema
pub struct StoryQuestion { pub question_en: String, pub options: Vec<String>, pub answer_index: u32,
    pub paragraph: u32 }
pub struct StoryPrompt { pub lang: Lang, pub cefr: Cefr, pub skill_id: String, pub seq: u32,
    pub theme: String, pub focus: Vec<String>, pub variety: String, pub allowed_lemmas: Vec<String>,
    pub names: Vec<String> }                                 // serde as written
impl StoryPrompt {
    /// For the unit whose last skill is `skill_id` (below). None: not the last skill of a unit.
    pub fn new(syllabus: &Syllabus, introduced_skills: &[String], skill_id: &str, seq: u32) -> Option<StoryPrompt>;
    pub fn vars(&self) -> BTreeMap<String, String>;
    pub fn key_input(&self) -> serde_json::Value;            // {"lang","skill_id","seq","focus","allowed_sha256"}
}
pub struct StoryCtx<'a> { pub prompt: StoryPrompt, pub syllabus: &'a Syllabus, pub forms: &'a Forms,
    pub numbers: &'a [String] }
/// P2: ai::call with S2 to S6 as its rules, so any failure is AiError::Invalid, stores nothing
/// and counts one attempt (PUBLIK-INTEGRATION §6.1).
pub fn generate_story(client: &dyn AiClient, cache: &Cache, ctx: &StoryCtx) -> Result<Called<Story>, AiError>;
/// S2 to S6 against the context, the first failure (rule "S2" .. "S6"). With `focus` empty (an
/// imported story) S5 passes.
pub fn check_story(story: &Story, ctx: &StoryCtx) -> Result<(), RuleFailure>;
/// XP of a finished story: 10 plus 2 per correct answer.
pub fn story_xp(correct: u32) -> u32;

// validate.rs: L2 to L9 of one AI exercise, and the coverage with the stem fallback.
/// The Allowed of an AI call: the syllabus's function words, names and numbers, and exactly the
/// lemmas of `allowed_lemmas` (which the prompt sent).
pub fn allowed_from_list(syllabus: &Syllabus, numbers: &[String], allowed_lemmas: &[String]) -> Allowed;
/// (covered, counted) as exercise::coverage, a token also counting as covered when its stem equals
/// the stem of an allowed lemma (each word of a multiword lemma, function words included).
pub fn coverage_with_stems(lang: Lang, text: &str, allowed: &Allowed, forms: &Forms) -> (usize, usize);
```

### Prompt values (P1 and P2)

`target_language_name`: Spanish, French or German. `cefr`: `A1` or `A2`. `variety`: the
syllabus's `variety` field verbatim. `allowed_names`: the syllabus's names joined with `, `.
`allowed_lemmas`: the list joined with `, `. `grammar_note`: the skill's note, or `none` when it has
none.

P1 only: `max_words`: 8 at A1, 12 at A2 (`exercise::max_words`). `skill_title`, `can_do` as the
syllabus writes them. `plan`: one line per entry, joined with `\n`:
`{n}. item_id={item_id} type={type} focus="{focus}" ({gloss}) new={true|false}`, and without
` ({gloss})` when the gloss is None. Cache key input:
`{"lang", "skill_id", "seq", "plan": <the plan value>, "allowed_sha256": <SHA-256 hex of the
allowed_lemmas value>, "names": [<the names>]}`.

P2 only: `theme`: the unit's title. `focus_lemmas`: the focus words joined with `, `. A1:
`paragraphs` 3, `min_words` 80, `max_words` 120, `questions` 3, `tenses` `present`; A2: 4, 120, 180,
4, and `tenses` `present, pretérito perfecto, pretérito indefinido and near future` (es),
`present, passé composé and near future` (fr), `present, Perfekt and near future` (de). Focus
words: the single-word lemmas of the unit's skills whose part of speech is noun, verb or adj, in
syllabus order; story `seq` takes 4 of them from index 4 × (seq − 1), wrapping round (all of them
when the unit has fewer than 4). Cache key input: `{"lang", "skill_id", "seq", "focus": [...],
"allowed_sha256"}`.

The allowed lemma list: the syllabus's function words in syllabus order; then every lemma of each
skill with at least one introduced item and of this skill (for P2: the unit's last skill), ordered
by frequency rank (lemmas without a rank after the others, each group in syllabus order), each
once, a lemma equal to a function word left out. When that is more than 600 entries, lemmas of
other skills are removed from the end until it is 600; function words and this skill's lemmas stay.

### P1 validation and fill

Each exercise of a schema-valid response, in order, position `n` from 1, is checked in this order
and dropped at the first failure:

- **L2** the plan has an entry `n` (an extra exercise is dropped);
- **L3** `item_id` and `type` equal that entry's;
- **L4** every target-language text of four words or more (`prompt_target`, `target`, `accepted`
  of `translate_to_target`, `full_sentence`, the left side of each pair) is the course language,
  and every English text of four words or more (`prompt_en`, `accepted` of `translate_to_en`,
  `options`, `hint_en`, the right side of each pair) is English (`langid::is_language`);
- **L5** coverage at least 90% (`coverage_with_stems` over the texts `check_exercise` counts, with
  `allowed_from_list` of the prompt's list);
- **L6** to **L9** `exercise::check_exercise`'s answer, length and focus rules, with the plan
  entry's item's lemmas as focus and `max_words(cefr)`; **L7** compares the first main text with
  every exercise kept before it, whatever its type (`match_pairs` exempt); duplicates inside
  `accepted` are removed (first kept) before the checks, not failed.

A kept exercise becomes an `Exercise` with id `e{n}`, the entry's facet, `origin: "ai"`, and for
`translate_to_target` the word-bank `tiles` a template exercise would get. An entry without a kept
exercise (dropped or missing) gets the exercise `template_lesson` would give it (its type, then the
fallbacks, checked against the exercises before it). When `2 × (entries without a kept exercise) >
entries`, the whole lesson is `template_lesson`'s, with the notice. `Lesson::ai` is
`{provider, valid: kept exercises, planned: plan entries}` whenever a response was received (L1
failure included, `valid` 0); the lesson's `prompt_id` column is `lesson.v1` then.

`Lesson` gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub ai:
Option<LessonAiInfo>`; `schemas/lesson.schema.json` gains the optional `ai` object
`{"provider", "valid", "planned"}`.

`lesson_checks`: over at most 10 entries, `message` is "Local model: {p}% of AI exercises passed
Parley's checks (last {n} lessons)." (`(last lesson)` when n is 1), p = round half up of 100 ×
valid ÷ planned in integers; None when `recent` is empty or `planned` is 0.

### parley-core: lessons, stories and course packs in the app service

```rust
// app (Parley)
/// The chosen provider's client, or why there is none.
pub type AiAccess = Result<Arc<dyn AiClient>, AiError>;
pub enum LessonBegin { Started(LessonStarted), Pending(LessonJob) }
/// A P1 or P2 call to make with no lock held; owns everything it needs (Send).
pub struct LessonJob { .. }
impl LessonJob {
    pub fn skill_id(&self) -> &str;
    pub fn seq(&self) -> u32;
    /// Opens cache/ai.sqlite3 and makes the call (request_lesson or generate_story).
    pub fn run(&self) -> JobOutcome;
}
pub struct JobOutcome { .. }
impl Parley {
    /// `ai` is None when the command did not ask for a client (ai.provider unset, or a learn lesson
    /// with ai.fresh_lessons off).
    pub fn lesson_begin(&mut self, skill_id: &str, mode: &str, ai: Option<AiAccess>) -> Result<LessonBegin, AppError>;
    pub fn lesson_complete(&mut self, job: LessonJob, outcome: JobOutcome) -> Result<LessonStarted, AppError>;
    /// begin, run and complete in one go (the CLI and tests); lesson_start(skill, mode) is this with None.
    pub fn lesson_start_with(&mut self, skill_id: &str, mode: &str, ai: Option<AiAccess>) -> Result<LessonStarted, AppError>;
    pub fn lesson_start(&mut self, skill_id: &str, mode: &str) -> Result<LessonStarted, AppError>;
    /// **Use the course pack instead**: the pending job of the skill is superseded; its template lesson starts.
    pub fn lesson_use_course_pack(&mut self, skill_id: &str) -> Result<LessonStarted, AppError>;
    /// The skill of a learn-mode lesson in progress (for the prefetch after lesson_finish).
    pub fn run_skill(&self, session_id: &str) -> Option<String>;
    /// After a finished learn lesson: the P1 job of the skill's next lesson, or None.
    pub fn next_lesson_job(&mut self, skill_id: &str, ai: Option<AiAccess>) -> Result<Option<LessonJob>, AppError>;
    pub fn course_pack_export(&mut self, lang: &str, dest_path: &Path) -> Result<CoursePackSummary, AppError>;
    pub fn course_pack_import(&mut self, src_path: &Path) -> Result<CoursePackReport, AppError>;
    pub fn ai_lesson_checks(&self) -> Result<LessonChecks, AppError>;
}
```

`api::LessonStarted` gains `ai_error: Option<AppError>` and `story: Option<Story>` (both null when
absent); `kind` is `lesson`, `review` or `story`. `api::PathUnit` gains `story: Option<PathStory>`,
`PathStory { skill_id, available: bool }`: present when `ai.stories` is on, `skill_id` the unit's
last skill, `available` when that skill is practised or mastered.

**Which lesson plays.** The lesson to play of a skill has `seq` = 1 + the number of `lesson`
sessions of that skill. A lesson already stored at that seq (an imported one) is played as stored,
with no call. Otherwise (learn mode) the plan is composed as in module 04, and P1 is asked when all
hold: `ai.fresh_lessons`, `ai.provider` is not `unset`, `ai` is `Some(Ok(client))`, and
`client.status().needs_credit` is false. Else the template lesson. Review mode never calls. Stories
do the same with `story` sessions of the skill and the `stories` table.

**Outcomes of P1** (`lesson_complete`; PUBLIK-INTEGRATION §9): a draft → its lesson, `notice` and
the call's charge added to the session's `cost_micros`; `Offline` → template lesson,
`notice: OFFLINE_LESSON`; `NeedsConsent` or `ai` None → template lesson, nothing shown; any other
error (including from `ai: Some(Err(e))`) → template lesson, `ai_error: Some(e.into())`, no notice.
A job superseded by `lesson_use_course_pack` (or by a later begin of the same skill) stores and
starts nothing: `not_found` "This lesson was replaced by one from the course pack."; its answer
stays cached.

**Prepare next lesson.** `next_lesson_job` returns a job when `ai.prepare_next_lesson`,
`ai.fresh_lessons`, a provider that is not `unset`, `Some(Ok(client))` without `needs_credit`, and
no lesson stored at the next seq; its plan is composed now for that seq. The desktop runs it in the
background and only its cache entry remains.

**Stories.** `lesson_begin(skill_id, "story", ai)`: the skill must be the last of its unit, else
`not_found` "This skill has no story."; a stored story at the story seq (an imported one) plays as
stored, with no client needed; otherwise `ai` None → `needs_consent` (nothing sent);
`Some(Err(e))` → that error; otherwise P2, always sent when asked (a learner action, even with
`needs_credit`). A failed check → `ai_invalid` "The AI answer could not
be used."; other errors as `AppError::from`. The story is stored (`stories`, origin `ai`, prompt
`story.v1`) and a `story` session starts; questions are graded by `exercise_submit` with ids `q1`,
`q2`, … and `Answer::Choice`: `correct` when it is `answer_index`, else `wrong` with `corrected` the
right option; `rating` null; no card and no review-log row is touched. A non-choice answer is
`invalid_input` "Choose an answer first."; a second answer "This exercise was already checked.";
`exercise_i_was_right` on a story "This exercise is no longer available.". `lesson_finish` gives
`story_xp(correct)`, credits the day and the streak like any session, and `total` is the number of
questions. Story cost goes to the session's `cost_micros`.

Migration `0003_stories.sql`:

```sql
CREATE TABLE stories (id TEXT PRIMARY KEY, lang TEXT NOT NULL, skill_id TEXT NOT NULL, seq INTEGER NOT NULL,
  origin TEXT NOT NULL CHECK (origin IN ('ai','course_pack')), prompt_id TEXT,
  content TEXT NOT NULL,                 -- JSON: the story, schemas/ai/story.v1.schema.json
  created_at INTEGER NOT NULL);
CREATE UNIQUE INDEX stories_skill_seq ON stories(skill_id, seq);
```

`id` is `{skill_id}/story/{seq}`. Backups (`parley.backup/1`) gain an optional `stories` array of
these rows (`content` as a JSON object); export writes it, import accepts a backup without it.

### Course packs (`parley_core::coursepack`)

```rust
pub const COURSE_PACK_SCHEMA: &str = "parley.coursepack/1";
pub const COURSE_PACK_MAX_BYTES: u64 = 5_000_000;
pub struct CoursePack { pub schema: String, pub lang: Lang, pub syllabus_version: u32,
    pub app_version: String, pub created_at: LocalDate, pub licence: String,   // "CC-BY-SA-4.0"
    pub lessons: Vec<PackLesson>, pub stories: Vec<PackStory> }
pub struct PackLesson { pub skill_id: String, pub prompt: Option<String>, pub exercises: Vec<serde_json::Value> }
pub struct PackStory { pub skill_id: String, pub prompt: Option<String>, #[serde(flatten)] pub story: serde_json::Value }
pub enum PackError { TooLarge, NotACoursePack, WrongLanguage { pack: Lang, active: Lang }, Store(StoreError) }
impl PackError { pub fn message(&self) -> String; }
pub fn export_course_pack(store: &Store, lang: Lang, syllabus_version: u32, created_at: LocalDate)
    -> Result<CoursePack, StoreError>;
/// Size, JSON and schemas/coursepack.schema.json (the envelope only).
pub fn parse_course_pack(bytes: &[u8]) -> Result<CoursePack, PackError>;
pub struct PackCheck<'a> { pub active: Lang, pub syllabus: &'a Syllabus, pub forms: &'a Forms,
    pub items: &'a [Item], pub numbers: &'a [String] }
pub fn import_course_pack(store: &mut Store, pack: &CoursePack, check: &PackCheck, now_ms: i64)
    -> Result<CoursePackReport, PackError>;
pub struct CoursePackSummary { pub bytes: u64, pub lessons: usize, pub stories: usize, pub exercises: usize }
pub struct CoursePackReport { pub lessons: usize, pub stories: usize, pub dropped: usize, pub message: String }
```

The file is the shape below (arrays shortened). Export: every stored lesson of the language with
origin `ai` or `course_pack`, with only its exercises of those origins (as stored, ids kept), in
skill order then seq, a lesson with none left out; every stored story of the language; `prompt`
from `prompt_id`; `created_at` the local date; `app_version` the app's.

```json
{
  "schema": "parley.coursepack/1",
  "lang": "es",
  "syllabus_version": 1,
  "app_version": "0.7.0",
  "created_at": "2026-11-02",
  "licence": "CC-BY-SA-4.0",
  "lessons": [{"skill_id": "es.a1.u03.s01", "prompt": "lesson.v1", "exercises": [
    {"id": "x1", "item_id": "es.a1.u03.s01#l:agua", "facet": "productive", "origin": "ai",
     "type": "translate_to_target", "prompt_en": "I want water, please.", "target": "Quiero agua, por favor.",
     "accepted": ["Quiero agua, por favor.", "Yo quiero agua, por favor."]}]}],
  "stories": [{"skill_id": "es.a1.u03.s04", "prompt": "story.v1", "title": "Un café en Madrid",
    "title_en": "A coffee in Madrid", "paragraphs": ["Ana está en un café en Madrid."],
    "questions": [{"question_en": "Where is Ana?", "options": ["In a café", "At home", "At school"], "answer_index": 0, "paragraph": 0}]}]
}
```

Import (untrusted input): over 5,000,000 bytes → `TooLarge` before parsing; not JSON, a `schema`
other than `parley.coursepack/1`, or an envelope the schema refuses → `NotACoursePack`; `lang` not
the active course → `WrongLanguage`. Then each exercise on its own: it must be one of the five P1
exercise shapes plus `id`, `item_id`, `facet`, `origin` (the shape of `schemas/ai/lesson.v1` with
those fields; `tiles` allowed), its `id` not repeated in its lesson, its `item_id` an item of the
local syllabus, and pass L4 to L9 with `Allowed::for_skill(lesson skill)` and the stem fallback,
`max_words` of the skill's level and the item's lemmas as focus; a failing exercise is dropped and
counted. A lesson whose skill is not in the syllabus has all its exercises dropped and counted. A
lesson with exercises left is stored at the skill's next seq (`Store::next_lesson_seq`), origin
`course_pack`, every exercise's origin `course_pack`, `prompt_id` the pack's `prompt`. A story is
checked against the P2 schema and S2 to S4 and S6 for its skill (the last skill of a unit, else
dropped); a failing story is not imported and not counted. Stored stories get the skill's next
story seq, origin `course_pack`. Nothing is stored unless the whole import succeeds (one
transaction).

`ImportReport` message: "Imported {n} lessons and {m} stories; {k} exercises were dropped because
they did not pass Parley's checks."

### Commands and events

`lesson_start` takes `mode: learn/review/story` and runs on a blocking thread: begin with the
Parley lock, run a pending job with no lock, complete with the lock. The client comes from
`AiService::client` only when `ai.provider` is not `unset` and the mode is `story`, or `learn` with
`ai.fresh_lessons` on. `lesson_finish` also starts the prefetch job in the background when
`run_skill` named a skill. Appended to `COMMANDS` (and `commands.ts`) after `ai_open_link`:

| Command | Arguments | Result |
|---|---|---|
| `lesson_use_course_pack` | `skill_id` | `LessonStarted` |
| `course_pack_export` | `lang, dest_path` | `CoursePackSummary` |
| `course_pack_import` | `src_path` | `CoursePackReport` |
| `ai_lesson_checks` | | `LessonChecks` |

`file_pick` also takes `course_pack_export` (save, default name `parley-course-<lang>.json`) and
`course_pack_import` (open, `.json`).

`with_ai` saves only the fields of `ai` the command changed, applied to the settings as they are
when it saves.

### CLI

    parley lesson render <prompt.json> [--json]
        The P1 request body for publik API (model publik-balanced) of the LessonPrompt in
        <prompt.json>: ChatRequest::body(false) of the rendered lesson.v1 prompt.
        Usage errors: `error: <prompt.json> is required. Run parley --help.`, `error: {path} not
        found`. A file that is not a LessonPrompt: exit 1, `error: {path}: {parser message}`.

### UI

Strings in `app/src/strings/en.json`.

- **Loading** (`Lesson/Loading.tsx`, `role="status"`): while `lesson_start` in learn mode is pending
  and AI lessons are on (`ai_status` provider not `unset` and `ai.fresh_lessons`), "Writing a fresh
  lesson…"; after 5 s the button **Use the course pack instead** (`lesson_use_course_pack` with the
  skill), whose lesson starts; the pending `lesson_start` answer is then ignored. A `notice` shows
  above the first exercise; an `ai_error` shows as the AI banner of module 06 there.
- **Path**: each unit with `story` ends with a **Read a story** button, disabled with "Practise the
  last skill of this unit first." while not `available`. It calls `lesson_start(skill_id, "story")`;
  `needs_consent` opens the consent dialog, whose **Continue** calls it again.
- **Story** (`Story/Story.tsx`): "Writing a story…" while pending; the title, the English title, the
  paragraphs, then each question with its options as buttons; an answer shows "Correct" or "Wrong:
  {right option}"; after the last, **Finish** (`lesson_finish`) and the summary with the XP. An
  `ai_invalid` failure shows "The AI answer could not be used." with **Try again** (the same call);
  other errors their message.
- **Settings > AI**: switches **Fresh lessons**, **Prepare the next lesson**, **Stories**
  (`settings_set` on `ai.fresh_lessons`, `ai.prepare_next_lesson`, `ai.stories`); the Local model
  section shows `ai_lesson_checks().message` when there is one.
- **Settings > Data**: a section **Course packs** with **Export a course pack** (`file_pick
  course_pack_export`, then `course_pack_export(active course, path)`, then "Exported {n} lessons and
  {m} stories.") and **Import a course pack** (`file_pick course_pack_import`, then
  `course_pack_import(path)`, then the report's message, or the error's message).

Messages: "A fresh lesson was not available, so this one comes from the course pack."; "You're
offline, so this lesson comes from the course pack."; "Writing a fresh lesson…"; **Use the course
pack instead**; "Writing a story…"; "The AI answer could not be used." with **Try again** (stories);
"This skill has no story."; "Choose an answer first."; "This lesson was replaced by one from the
course pack."; "This is not a Parley course pack."; "This course pack is for {French}; your active
course is {Spanish}."; "This course pack is larger than 5 MB."; "Imported {n} lessons and {m}
stories; {k} exercises were dropped because they did not pass Parley's checks."; "Exported {n}
lessons and {m} stories."; "Local model: {p}% of AI exercises passed Parley's checks (last {n}
lessons)."; "Practise the last skill of this unit first."

## Out of scope

- Listening and speaking exercises inside AI lessons (made by the core in 09 and 10 from validated
  sentences); judging free answers (08).
- Recording real P1 responses for the replay: the maintainer's first live G1 run writes them; until
  then `tests/fixtures/ai/P1/recorded/` holds hand-written responses (see Design notes).

## Design notes

- Imported course packs are untrusted input: every exercise goes through the exercise shape and L4
  to L9 against the local syllabus and forms; exercises whose `item_id` is not in the local syllabus
  are dropped. Imported lessons get the next `seq` numbers of their skill, and the lesson a skill
  plays is the one at 1 + its number of lesson sessions, so imported lessons are played next, in
  order.
- A response that arrives after the learner chose the course pack is cached, not shown.
- L5 for an AI lesson uses the list the prompt sent, not every lemma up to the skill, so the model
  is held to what it was told. Stories use the same list for their skill.
- Stories live in their own table, so a story never takes a lesson's `seq`, and in backups as an
  optional array, so older backups still import.
- The live gate sits in `parley-cli` because it needs the real client, and `parley-core` has no HTTP
  code and no dependency on `parley-publik`.
- Language identification (L4, S2, S6): `whatlang` restricted to English, Spanish, French and
  German. The threshold stays at the starting value 0.5, with one addition chosen from measurements
  on project-written sentences of four to twelve words: on such short sentences whatlang's
  four-language choice is often wrong with a low confidence ("Quiero un café con leche." comes out
  French at 0.10), while its choice between a course language and English is right. So a text is
  refused when whatlang names another language with a confidence of 0.5 or more; named with less,
  it passes when the expected language beats its counterpart in a two-language identification
  (English for a course language; the course language for English). Every wrong-language text
  measured was named at 0.95 or more or lost that comparison. The sentences and the rule are unit
  tests in `crates/parley-core/src/ai/langid.rs`.
- L5 for an AI lesson counts against the list the prompt sent (`allowed_from_list`), the stem
  fallback taking the Snowball stems of every word of those lemmas and of the function words. A
  course pack exercise counts against every lemma up to its skill (`Allowed::for_skill`), as a
  template exercise does.
- L7 for AI exercises compares the first main text with every exercise kept before it whatever its
  type (`match_pairs` exempt on either side), as §6.3 says; template exercises keep module 03's
  same-type rule, and a template filling a gap is checked against the exercises before it in the
  finished lesson.
- An answer that fails L1 still counts towards the validity rate: the stored lesson carries
  `ai: {provider, valid: 0, planned}`. Lessons with an AI summary store `prompt_id` `lesson.v1`.
  The rate covers the last 10 lessons whose `ai.provider` is `local`.
- A pending fresh lesson or story is superseded by number: each begin that asks for one registers
  a number for its skill, and Use the course pack instead (or a later such begin) replaces it, so a
  late answer's
  `lesson_complete` stores nothing and says "This lesson was replaced by one from the course
  pack."; the answer itself was already cached by the call. The prepared next lesson is never
  completed, only cached.
- The desktop asks `AiService::client` only when `ai.provider` is not `unset` (and for a learn
  lesson only with Fresh lessons on), reading the settings first and holding the AI service's lock
  only while the client is built; the call itself runs on a blocking thread with no lock held.
  After a call the status is emitted as `ai://status`, so a 402 shows everywhere at once.
- `cache/ai.sqlite3` is opened by the job on its own thread; if it cannot be opened, the call goes
  on with a cache in memory, so only the saving is lost.
- The lesson a skill plays is 1 + its started lesson sessions, so an imported lesson stored at that
  number plays next, and a template lesson started instead of a pending fresh one takes the same
  number as the fresh one would have.
- Story focus words skip lemmas of more than one word. Story S6 also refuses an `answer_index`
  outside the options, which the schema already bounds.
- Course pack import: an exercise must have the shape of a P1 exercise plus `id`, `item_id`,
  `facet` and `origin`, its `facet` must be its type's, and its `id` unique in its lesson. Its
  word-bank `tiles`, which no check looks at, are never kept: they are rebuilt from the exercise's
  target as a template exercise's are. A pack
  over 5 MB is `file_too_large`; a file that is not a pack and a pack for another language are
  `invalid_input`. An imported story is checked with no focus words (S5 passes) against every lemma
  up to its skill.
- `AppError` and `ErrorCode` deserialise as well as serialise, so `LessonStarted.ai_error` round
  trips; stories are a session kind of their own (`SessionKind::Story`).
- An AI command saves only the AI fields it changed, applied to the settings as they are when it
  saves (`changed_fields` in `app/src-tauri/src/commands/ai.rs`), so a switch turned while an AI
  request runs is kept.
- S5 finds a focus word through the forms table or, failing that, its Snowball stem, like L5 and
  S3.
- The charge of an answer that fails L1 is not added to the lesson's cost: the call pipeline
  reports no charge for an unusable answer. The prepared next lesson is reused only when the plan
  composed at the next start is the one composed at the finish (the same due items).
- The course pack commands are in `app/src-tauri/src/commands/lesson.rs`, beside the lesson
  commands.
- `tests/fixtures/ai/P1/recorded/` holds three hand-written stand-ins, one per course, until the
  maintainer's first live G1 run writes real answers there.

## Acceptance tests

Golden cases in `tests/cases/07-lesson-generation/`; code tests in
`crates/parley-core/tests/m07_lesson_generation.rs`; UI tests in `app/src/test/m07/`. Responses come
from `tests/fixtures/ai/P1/` and `P2/`, each written to break exactly one rule against one stated
plan and context. Contract tests 1 to 18 (module 06) keep passing; this module adds tests 21 to 23
of `docs/PUBLIK-INTEGRATION.md` §11.

1. `parley lesson render` of the §6.3 example prompt renders exactly
   `render-es-a1/expected.json` (the full body, every placeholder of §6.3 filled).
2. The valid fixture → 5 exercises with `origin: "ai"`, none dropped.
3. L1: a non-JSON reply → a template lesson with the notice; nothing cached.
4. L2: 7 exercises for a 5-entry plan → the last 2 dropped, 5 shown.
5. L3: `item_id` wrong at position 2 → exercise 2 replaced by a template exercise (`origin: "template"`).
6. L4: an English sentence `I want water please now` as the Spanish target → dropped.
7. L5: `Quiero agua con hielo, por favor.` with `hielo` not allowed → 5 of 6 words covered (83%) →
   dropped; a 10-word sentence with one uncovered word (90%) → kept (at A2, where 10 words fit L8).
8. L6: `target` not in `accepted`; `fill_gap` with two gaps; a two-word `answer`; `full_sentence`
   mismatch; duplicate `pick_meaning` options; duplicate `match_pairs` sides → each dropped.
9. L7: two exercises with the same normalised target → the second dropped.
10. L8: a 9-word target at A1 (limit 8) → dropped.
11. L9: an exercise not containing its focus lemma in any form → dropped.
12. 3 of 5 exercises dropped → the whole lesson from templates with the notice.
13. Cache: a schema-valid response is stored; the same `seq` again makes no request; `seq` + 1 does.
14. With `needs_credit` set, `lesson_start` makes no call and returns a template lesson.
15. **Use the course pack instead**: the template lesson starts; the delayed response is cached and
    not shown (`lesson_complete` of the superseded job stores and starts nothing); in the UI the
    button appears after 5 s and not before.
16. Prepare next lesson: exactly one P1 call for `seq + 1` after `lesson_finish` (the next
    `lesson_start` is then served from the cache); none with the setting off or `needs_credit` set.
17. Story: the valid fixture → a story session; each of S1 to S6 → "The AI answer could not be
    used."; 3 correct answers of 3 → 16 XP; cards and review log unchanged.
18. Course pack: export then import into fresh data → the same exercises with `origin:
    "course_pack"`, played as the skill's next lesson; a tampered exercise with an unknown word →
    dropped and counted; a French pack in a Spanish course → the language message; `{"schema": "x"}`
    → "This is not a Parley course pack."; a 6 MB file → the size message.
19. Validity rate: three local lessons with 12, 9 and 6 valid of 12 → "Local model: 75% of AI
    exercises passed Parley's checks (last 3 lessons)."
20. Gate G1 (live, maintainer): ≥ 80% of exercises valid and p95 ≤ 20 s; in `verify`, the responses
    in `tests/fixtures/ai/P1/recorded/` replayed through the validators give ≥ 80% valid.
21. P1 errors at lesson start: 402 → template lesson with `ai_error` `needs_credit` and its
    `top_up_url`; offline → `notice` "You're offline, so this lesson comes from the course pack.";
    `ai.provider = unset` → no client is asked for and no request is made.
22. Stories are stored and kept by a backup round trip; a backup without `stories` still imports;
    `lesson_start(skill, "story")` on a skill that is not the last of its unit → "This skill has no
    story."; the path shows the Story node only with `ai.stories` on, available once the unit's last
    skill is practised.
23. UI: the Story screen grades a question and shows **Try again** on `ai_invalid`; the Settings > AI
    switches call `settings_set`; the Data section's import shows the report's message.
24. The command lists of `api::COMMANDS` and `commands.ts` include the four new commands in order;
    an AI command's settings save keeps an `ai.fresh_lessons` change made while it ran.

## Status

Done (2026-11-10)
