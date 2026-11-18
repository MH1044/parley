# 04: App shell and offline course

## Goal

A learner can install Parley, pick a language, take the placement test, follow the path, do lessons
and reviews, watch the streak and stats, change settings, and back up and restore their data, all
with no network and no AI. This is the app a fresh install at $0.00 has, and the shell every later
module adds a screen to.

## Depends on

03.

## Scope

1. The commands marked 04 in `docs/ARCHITECTURE.md` §5 (including `app_start` and `file_pick`),
   backed by `parley_core::app::Parley`, with the `AppError` messages below; the Tauri commands
   only marshal.
2. `settings.json` load, validate and save (defaults and a notice on a bad file).
3. Screens: Onboarding (with placement), Path, Lesson player (pick the meaning, translate both ways
   with typing or word bank, fill the gap, match pairs), Result sheet, Lesson summary, Reviews,
   Stats, Settings (Learning, Data, About), and the data-file error screen. No AI entry point is
   shown yet.
4. Course pack download from `content/packs.lock.toml`, with the size shown first, progress events
   as the pack arrives, size and SHA-256 verification and a working app without it; lessons use an
   installed pack's sentences.
5. Backup export and import (`parley.backup/1`), and "Delete everything".
6. Attribution: "Sentence by {author} on Tatoeba" under pack sentences; the About page credits
   Tatoeba (CC BY 2.0 FR) and Wiktionary (CC BY-SA 4.0).
7. Keyboard: Enter checks and continues, 1 to 4 pick options, a number and a letter make a pair,
   Esc asks to quit.
8. `app/src/strings/en.json` for every UI string; `app/src/api/commands.ts` typed wrappers.
9. The launch smoke test and the CI `e2e` job.

## Files

- `crates/parley-core/src/api.rs` (result types, `COMMANDS`, `megabytes`, `language_name`,
  `next_review_notice`), `crates/parley-core/src/app/{mod.rs,course.rs,pack.rs,lesson.rs,placement.rs,stats.rs}`
  (the `Parley` service), `crates/parley-core/src/{settings.rs,backup.rs}`,
  `schemas/{settings.schema.json,backup.schema.json}`
- `app/src-tauri/src/{lib.rs,main.rs,state.rs,env.rs,bundled.rs}`,
  `app/src-tauri/src/commands/{app.rs,course.rs,pack.rs,path.rs,placement.rs,lesson.rs,stats.rs,settings.rs,backup.rs}`
- `app/src/strings/{en.json,index.ts}`, `app/src/screens/{Onboarding,Path,Lesson,Review,Stats,Settings,DataError}/*.tsx`,
  `app/src/components/{ResultSheet,WordBank,MatchPairs,StreakBadge}.tsx`, `app/src/App.tsx`,
  `app/src/navigation.ts` (the screen history)
- `app/e2e/{launch.spec.ts,vitest.config.ts}`, `.github/workflows/ci.yml` (e2e job)
- Tests: `app/src-tauri/tests/m04_app_shell_and_offline_course.rs`, `app/src/test/m04/*.test.tsx`,
  `app/src/App.test.tsx` (updated), fixtures in `tests/cases/04-app-shell-and-offline-course/`

## Interfaces

```rust
// parley_core::app — the service every command calls. One instance per app, behind a mutex.
pub struct CourseFiles { pub syllabus_yaml: String, pub forms_tsv: String, pub numbers_yaml: Option<String> }
pub struct Config {
  pub data_dir: PathBuf,                      // absolute; SQLite files (store, packs) are opened under it
  pub courses: BTreeMap<Lang, CourseFiles>,   // the bundled content of every language
  pub packs_lock: String,                     // the text of content/packs.lock.toml
}
pub struct Env {
  pub clock: Box<dyn Clock + Send + Sync>, pub rng: Box<dyn Rng + Send>,
  pub fs: Box<dyn Fs + Send + Sync>, pub net: Box<dyn Net + Send + Sync>,
}
pub struct Parley { .. }
impl Parley {
  pub fn new(config: Config, env: Env) -> Parley;   // never fails; a store or settings problem is reported by app_start
  pub fn app_start(&mut self) -> Result<Launch, AppError>;
  pub fn app_info(&self) -> AppInfo;
  pub fn data_delete_all(&mut self, confirm_text: &str) -> Result<(), AppError>;
  pub fn cache_clear(&mut self, kind: &str) -> Result<CacheCleared, AppError>;
  pub fn settings_get(&self) -> Settings;
  pub fn settings_set(&mut self, patch: &serde_json::Value) -> Result<Settings, AppError>;
  pub fn course_list(&mut self) -> Result<Vec<CourseSummary>, AppError>;
  pub fn course_start(&mut self, lang: &str) -> Result<CourseSummary, AppError>;
  pub fn pack_status(&mut self, lang: &str) -> Result<PackStatus, AppError>;
  pub fn pack_download(&mut self, lang: &str, progress: &mut dyn FnMut(PackProgress))
    -> Result<PackStatus, AppError>;
  pub fn path_get(&mut self, lang: &str) -> Result<PathView, AppError>;
  pub fn skill_get(&mut self, skill_id: &str) -> Result<SkillDetail, AppError>;
  pub fn placement_start(&mut self, lang: &str) -> Result<PlacementQuestion, AppError>;
  pub fn placement_answer(&mut self, session_id: &str, answer: &Answer) -> Result<PlacementStep, AppError>;
  pub fn lesson_start(&mut self, skill_id: &str, mode: &str) -> Result<LessonStarted, AppError>;
  pub fn review_start(&mut self, lang: &str, limit: usize) -> Result<LessonStarted, AppError>;
  pub fn exercise_submit(&mut self, session_id: &str, exercise_id: &str, answer: &Answer,
    response_ms: Option<i64>, used_hint: bool) -> Result<Grade, AppError>;
  pub fn exercise_i_was_right(&mut self, session_id: &str, exercise_id: &str) -> Result<Grade, AppError>;
  pub fn lesson_finish(&mut self, session_id: &str) -> Result<LessonSummary, AppError>;
  pub fn stats_get(&mut self, lang: &str) -> Result<Stats, AppError>;
  pub fn backup_export(&mut self, dest_path: &Path) -> Result<ExportSummary, AppError>;
  pub fn backup_import(&mut self, src_path: &Path) -> Result<ImportReport, AppError>;
}
// A data_delete_all error after which every file is as it was (`invalid_input`, `busy`); any
// other error came after the progress was deleted.
pub fn nothing_was_deleted(error: &AppError) -> bool;
pub struct PackSentence { pub id: i64, pub text: String, pub en: Vec<String>, pub author: String }
pub fn attributions(lesson: &Lesson, pack: &[PackSentence], syllabus: &Syllabus) -> Vec<Attribution>;

// parley_core::api — result types (serde field names as written; Option serialises as null)
pub const COMMANDS: &[&str];   // every Tauri command, in the order of app/src/api/commands.ts
pub fn megabytes(bytes: u64) -> String;          // 8_200_000 → "8.2 MB"
pub fn language_name(lang: Lang) -> &'static str; // "Spanish", "French", "German"
pub fn next_review_notice(due_ms: i64, now_ms: i64, utc_offset_minutes: i32) -> String;
pub struct AppInfo { pub version: String, pub data_dir: String }                       // unchanged
pub struct Launch { pub onboarded: bool, pub active_course: Option<String>, pub notice: Option<String> }
pub struct CacheCleared { pub files_removed: u32 }
pub struct CourseSummary { pub lang: String, pub name: String, pub started: bool, pub active: bool,
  pub placement_done: bool, pub cefr_estimate: Option<String>, pub skills: usize }
pub struct PackStatus { pub lang: String, pub state: PackState, pub version: Option<String>,
  pub bytes: Option<u64>, pub size: Option<String> }
pub enum PackState { NotPublished, Available, Installed }       // serde snake_case
pub struct PackProgress { pub lang: String, pub bytes_done: u64, pub bytes_total: u64 }   // event pack://progress
pub struct PathView { pub lang: String, pub units: Vec<PathUnit>, pub next_skill_id: Option<String>,
  pub due: usize, pub streak: u32, pub freezes: u32, pub today_xp: u32, pub goal_xp: u32 }
pub struct PathUnit { pub id: String, pub n: usize, pub title: String, pub cefr: Cefr, pub skills: Vec<PathSkill> }
pub struct PathSkill { pub id: String, pub title: String, pub can_do: String, pub cefr: Cefr,
  pub state: SkillState, pub mastery: f64, pub tested_out: bool }
pub struct SkillDetail { pub id: String, pub title: String, pub can_do: String, pub cefr: Cefr,
  pub grammar_note: Option<String>, pub unit_title: String, pub state: SkillState, pub mastery: f64,
  pub words: Vec<Word>, pub opened_by: Option<String> }
pub struct Word { pub target: String, pub en: String }
pub struct PlacementQuestion { pub session_id: String, pub n: usize, pub skill_id: String, pub exercise: Exercise }
pub struct PlacementStep { pub next: Option<PlacementQuestion>, pub result: Option<PlacementOutcome> }
pub struct PlacementOutcome { pub estimate: usize, pub label: String, pub tested_out: usize,
  pub start_skill_id: String, pub start_unit: usize, pub start_unit_title: String }
pub struct LessonStarted { pub session_id: String, pub kind: String /* "lesson" | "review" */,
  pub skill_id: Option<String>, pub origin: Origin, pub exercises: Vec<Exercise>,
  pub attributions: Vec<Attribution>, pub notice: Option<String> }
pub struct Attribution { pub exercise_id: String, pub author: String }
pub struct Stats { pub lang: String, pub empty: bool, pub streak: u32, pub freezes: u32,
  pub days: Vec<StatsDay>, pub week_xp: u32, pub record_week_xp: u32, pub words_known: u32,
  pub vocab_items: u32, pub skills: Vec<SkillProgress>, pub cefr_estimate: Option<String>,
  pub recall_rate: Option<f64>, pub reviews_30: u32 }
pub struct StatsDay { pub local_date: LocalDate, pub xp: u32, pub active: bool, pub frozen: bool }
pub struct SkillProgress { pub id: String, pub title: String, pub state: SkillState, pub mastery: f64 }
pub struct ExportSummary { pub bytes: u64, pub courses: usize, pub cards: usize, pub reviews: usize }
pub struct ImportReport { pub courses: usize, pub items: usize, pub cards: usize, pub reviews: usize,
  pub skipped_cards: usize }
// Grade, Exercise, Answer, LessonSummary, Origin, SkillState, Cefr, LocalDate: modules 02 and 03.

// parley_core::settings
pub const SETTINGS_SCHEMA: &str = "parley.settings/1";
pub const SETTINGS_NOTICE: &str = "Your settings file could not be read, so Parley started with default settings.";
pub struct Settings { .. }   // exactly docs/ARCHITECTURE.md §4 settings.json; Default as there, but active_course null
pub fn parse_settings(bytes: &[u8]) -> Result<Settings, String>;
pub fn apply_patch(current: &Settings, patch: &serde_json::Value) -> Result<Settings, AppError>;

// parley_core::backup
pub const BACKUP_SCHEMA: &str = "parley.backup/1";

// parley_app (app/src-tauri/src/lib.rs; main.rs only calls run())
pub mod bundled { pub fn courses() -> BTreeMap<Lang, CourseFiles>; pub const PACKS_LOCK: &str; } // include_str! of content/
pub mod env { pub struct SystemClock; pub struct DirFs { .. } impl DirFs { pub fn new(root: PathBuf) -> DirFs }
  pub fn time_seeded_rng() -> SplitMix64; }
pub fn run();
```

Commands (Tauri names, `#[tauri::command(rename_all = "snake_case")]`, so the web view passes the
argument names below as written):

| Command | Arguments | Result |
|---|---|---|
| `app_start` | | `Launch` |
| `app_info` | | `AppInfo` |
| `file_pick` | `kind`: `backup_export` or `backup_import` | `string \| null` (the chosen path; null when cancelled) |
| `data_delete_all` | `confirm_text` | `null` |
| `cache_clear` | `kind`: `ai`, `audio` or `all` | `CacheCleared` |
| `settings_get` / `settings_set` | – / `patch` | `Settings` |
| `course_list` / `course_start` | – / `lang` | `CourseSummary[]` / `CourseSummary` |
| `pack_status` / `pack_download` | `lang` | `PackStatus` (+ event `pack://progress`) |
| `path_get` / `skill_get` | `lang` / `skill_id` | `PathView` / `SkillDetail` |
| `placement_start` / `placement_answer` | `lang` / `session_id, answer` | `PlacementQuestion` / `PlacementStep` |
| `lesson_start` | `skill_id, mode` (`learn` or `review`) | `LessonStarted` |
| `review_start` | `lang, limit` | `LessonStarted` |
| `exercise_submit` | `session_id, exercise_id, answer, response_ms, used_hint` | `Grade` |
| `exercise_i_was_right` | `session_id, exercise_id` | `Grade` |
| `lesson_finish` | `session_id` | `LessonSummary` |
| `stats_get` | `lang` | `Stats` |
| `backup_export` / `backup_import` | `dest_path` / `src_path` | `ExportSummary` / `ImportReport` |

`COMMANDS` and `commands.ts` list them in this table's order: `app_start`, `app_info`, `file_pick`,
`data_delete_all`, `cache_clear`, `settings_get`, `settings_set`, `course_list`, `course_start`,
`pack_status`, `pack_download`, `path_get`, `skill_get`, `placement_start`, `placement_answer`,
`lesson_start`, `review_start`, `exercise_submit`, `exercise_i_was_right`, `lesson_finish`,
`stats_get`, `backup_export`, `backup_import`.

Backup (`parley.backup/1`; arrays shortened to one element; pretty-printed with two spaces, keys in
this order, rows as the table columns with the JSON-text columns `en`, `lemmas`, `tags` and
`lessons.content` as JSON values):

```json
{
  "schema": "parley.backup/1",
  "app_version": "0.4.0",
  "exported_at": "2026-10-12T08:30:00Z",
  "settings": {"schema": "parley.settings/1", "active_course": "es", "daily_goal_xp": 20, "accent_leniency": "lenient", "...": "the whole settings object"},
  "courses": [{"lang": "es", "syllabus_version": 1, "pack_version": "2026-10-01", "started_at": 1760000000000, "placement_index": 12, "cefr_estimate": "around A1", "tested_out": ["es.a1.u01.s01"]}],
  "items": [{"id": "es:conv:6f1c:1", "lang": "es", "skill_id": "es.personal.conversation", "kind": "phrase", "target": "un café con leche", "en": ["a coffee with milk"], "lemmas": ["un", "café", "con", "leche"], "cefr": "A1", "source_kind": "conversation", "source_ref": "6f1c", "created_at": 1760100000000, "retired": 0}],
  "accepted_answers": [{"item_id": "es.a1.u03.s01#e:2", "direction": "to_en", "norm": "i want a white coffee", "display": "I want a white coffee.", "source": "judge", "created_at": 1760100000000}],
  "cards": [{"item_id": "es.a1.u01.s01#l:hola", "facet": "receptive", "state": "review", "stability": 3.17, "difficulty": 5.28, "due": 1760300000000, "last_review": 1760000000000, "reps": 1, "lapses": 0}],
  "review_log": [{"id": 1, "item_id": "es.a1.u01.s01#l:hola", "facet": "receptive", "reviewed_at": 1760000000000, "rating": 3, "exercise_type": "pick_meaning", "verdict": "correct", "response_ms": 2100, "elapsed_days": 0, "stability_after": 3.17, "difficulty_after": 5.28, "session_id": "s-01"}],
  "sessions": [], "lessons": [], "judgements": [], "daily": [], "freeze_events": []
}
```

These are the tables that exist at this module (migrations 0001 and 0002). Module 11 adds
`conversations` and `conversation_turns`, and module 12 adds `media` and `media_sentences` (with
`path` always empty), as optional arrays of the same `parley.backup/1` schema, so a backup made
before them still imports. Syllabus-derived items (`source_kind` `syllabus`) and `content` accepted
answers are not exported; they are rebuilt from `content/`. `courses[].tested_out` lists the
course's skills marked tested out. Keys, the Publik credential, caches, recordings and pack files
are never in a backup.

Messages (code, then text):

| Situation | Code | Message |
|---|---|---|
| a `lang`, `mode`, `kind` or enumerated setting with an unknown value | `invalid_input` | "Unknown value {v} for {key}." |
| a setting of the wrong type or out of range | `invalid_input` | "Invalid value for {key}." |
| a setting key that does not exist | `invalid_input` | "Unknown setting {key}." |
| a course not started (`path_get`, `pack_download`, `placement_start`, `review_start`, `stats_get`) | `not_found` | "This course has not been started." |
| a skill not in the store | `not_found` | "This skill is not in the course." |
| an unknown session or exercise | `not_found` | "This exercise is no longer available." |
| the run's other refusals | `invalid_input` | "Type an answer first." / "This exercise was already checked." |
| no pack in the lock for the language | `pack_missing` | "No course pack is published for {Language} yet." |
| a request of the download got no response | `offline` | "You're offline. Parley works without the pack, with fewer example sentences." |
| a request of the download answered other than 200, 206, 404 or 410 (a 404 or 410 is not an error: see Design notes) | `pack_missing` | "The course pack could not be downloaded. Try again later." |
| the bytes do not match the lock's size or SHA-256, or a part arrived with another length than the one asked for | `checksum_mismatch` | "The download was damaged. Try again." |
| a backup that is not JSON, has another `schema`, or fails its JSON Schema | `invalid_input` | "This is not a Parley backup." |
| `parley.backup/N` with N > 1 | `invalid_input` | "This backup was made by a newer Parley. Update Parley to import it." |
| a backup file that cannot be read | `not_found` | "Parley could not read that file." |
| a backup or settings file that cannot be written | `storage` | "Parley could not write that file." |
| `data_delete_all` when `parley.sqlite3` cannot be moved aside (another program has it open); nothing is deleted | `busy` | "Parley could not delete your progress because another program is using it. Close it and try again." |
| `backup_import` when `parley.sqlite3` cannot be replaced (another program has it open); nothing is replaced | `busy` | "Parley could not replace your progress because another program is using it. Close it and try again." |
| `data_delete_all` when a file cannot be removed after `parley.sqlite3` was moved aside (the progress is deleted) | `storage` | "Parley could not write that file." |
| the data file cannot be opened or used | `storage` | "Parley could not open its data file." with action `{"details": "<the store's message>"}` |
| `data_delete_all` without `delete` | `invalid_input` | "Type delete to confirm." |

Notices (not errors): `SETTINGS_NOTICE` in `Launch.notice`; `review_start` with nothing due:
"Nothing to review. Next review: {today at 09:00 | tomorrow at 09:00 | on 14 October}." or, with no
reviewed card at all, "Nothing to review yet. Finish a lesson to start your reviews."

UI contract (visible text in `en.json`; tests find elements by these names):

- App start: `app_start`; on its `storage` error the data-file screen: "Parley could not open its
  data file." with buttons **Restore a backup** and **Show details** (shows `action.details`).
  `onboarded` false → Onboarding; true → Path of `active_course`. `notice` shows as a banner.
- Onboarding, one step at a time, each with a level-1 heading: **Welcome to Parley** ("Learn by
  speaking. No hearts, no ads, nothing to buy.", button **Start**); **Choose a language** ("Parley
  teaches from English", one button per `course_list` entry named by its `name`; choosing calls
  `course_start`); **Course pack** (`pack_status`: available → "Download the Spanish course pack
  (8.2 MB)?" with **Download** and **Skip for now**; not published → "No course pack is published
  for Spanish yet. Parley works without it, with fewer example sentences." with **Continue**;
  installed → "The Spanish course pack is installed." with **Continue**; while the download runs,
  "Downloading the Spanish course pack…", a progress bar **Download progress** that fills as the
  pack arrives, and under it "{percent}% · {done} of {total} MB", such as "42% · 73.4 of 174.7
  MB", from the start, with the total from `pack_status` until the first `pack://progress` event;
  a download error shows its message and **Continue**); **Daily goal** (radio buttons **10 XP**, **20 XP** (checked),
  **30 XP**, **50 XP** and **Continue**, which calls `settings_set` with
  `{"daily_goal_xp": n}`); **Where to start** (**Take the placement test (about 5 minutes)** and
  **Start from the beginning**). Placement: heading **Placement test**, "Question {n}", the
  exercise, a text box **Your answer**, buttons **Next** and **I don't know** (sends
  `{"text": ""}`); every new question puts the cursor in **Your answer**, however the previous one
  was answered (Enter in the box, **Next** or **I don't know**); the result "You're {label}.
  You'll start at Unit {n}: {title}." ("You'll start at Unit {n}: {title}." when the label is
  "Pre-A1") and **Go to my path**. No dialog appears in onboarding.
- Path: "{n} day streak", "{x} of {goal} XP today", buttons **Continue** (`lesson_start` of
  `next_skill_id`, mode `learn`), **Review ({due})**, **Stats**, **Settings**; a level-2 heading
  "Unit {n}: {title}" per unit and one button per skill named by its title; a skill opens its
  detail (can-do, words, **Start lesson**; a locked skill says "Finish {opened_by} to open this
  skill.").
- Lesson player: "Exercise {i} of {n}"; `pick_meaning` shows "What does {target} mean?" and its
  options as radio buttons in a radio group; `translate_to_target` "Translate into {Language}";
  `translate_to_en` "Translate into English"; `fill_gap` "Fill the gap" and the hint; text answers
  in a text box **Your answer**, or the word bank (tile buttons) when `answer_input` is
  `word_bank` and the exercise has tiles; `match_pairs` two columns of buttons, a group **Words**
  numbered 1 to n and a group **Meanings** lettered A on in the exercise's `english_order`, a word
  and a meaning chosen either way round make a pair, "{text}, pair {n}" names each half of a made
  pair, and **Start over** sits under the columns, disabled until there is a pair. Last comes the
  action bar (`.action-bar`), one element for the whole lesson, after the exercise and its result
  sheet and before any dialog: the line naming the keys, then the answer's one main button,
  **Check** until the answer is graded (disabled while it is checked, "Checking…" included) and
  **Continue** after (disabled while a command runs), always its last element. A lesson with no
  exercise shows **Continue** there, which finishes it. Esc opens a dialog "Quit this lesson?"
  with **Quit** and **Keep going**.
- Result sheet: a region named **Result** with the verdict **Correct**, **Almost**, **Typo**,
  **Wrong** or **Not checked**, the grade's message, "Correct answer: {corrected}" when there is
  one, the attribution line when the exercise has one ("Sentence by {author} on Tatoeba", or
  "Sentence from Tatoeba" for an empty author), and **I was right** for `unchecked` only. Its
  **Continue** is not in the region but in the action bar under it, where **Check** was; the
  sheet's other buttons (**I was right**, and from later modules **Check with AI**, **Explain**
  and **Play**) stay in the sheet.
- Lesson summary: heading **Lesson complete**, "+{xp} XP", "{correct} of {total} correct", "{n} day
  streak", the summary's message when there is one, **Continue**.
- Reviews: `review_start` with `limit` 20; no exercises → the notice and **Back to my path**.
- Stats: heading **Stats**; `empty` → "Finish a lesson to see your stats."; **Back to my path**
  under the heading, before the calendar and the skills, and again at the foot.
- Settings: heading **Settings**, buttons **Learning**, **Data**, **About**. Data: **Export a
  backup**, **Restore a backup** (then a dialog with "Importing replaces all your progress on this
  computer. Export a backup first if you want to keep it.", **Replace my data** and **Cancel**), a
  text box named "Type delete to confirm." and **Delete everything**; a success of
  `data_delete_all` calls `app_start` again. A message (a result or an error) shows as a status
  under the buttons whose action it answers: under **Export a backup**, **Restore a backup** and
  **Clear caches** for those three and **Replace my data**, under the course pack buttons for
  theirs, and under **Delete everything** for it; a failed restore or delete keeps the Data
  screen. About: the version and data directory from
  `app_info`, "Example sentences come from Tatoeba (tatoeba.org) under CC BY 2.0 FR." and "Word
  forms come from Wiktionary (wiktionary.org) under CC BY-SA 4.0."
- Keyboard: Enter checks when an answer is given and continues when the result sheet shows; keys
  1 to 4 check the matching option of `pick_meaning`; in `match_pairs` a number (1 to n) chooses a
  word and a letter (A on, either case) the meaning on that row, a number and a letter in either
  order make a pair, and Backspace or Esc lets go of a half-made pair; Esc asks to quit (with a
  half-made pair, the first Esc only lets go of it). The line in the action bar names the keys:
  "Enter checks and continues. Keys 1 to 4 choose an option. Esc quits.", and for `match_pairs`
  "Enter checks and continues. Press a number (1 to {n}) and a letter (A to {last}) to make a
  pair. Backspace or Esc clears a half-made pair; otherwise Esc quits.".
- Leaving a screen: every screen the path opens (Reviews with nothing due, Stats, Settings) has
  **Back to my path** at the top, straight after its heading, and at the foot; Esc on it returns
  to the path. The app keeps a history of the screens visited, in order: the path and the screens
  it opens. The mouse's back button (`button` 3) and Alt+Left go to the previous screen, its
  forward button (`button` 4) and Alt+Right to the next; opening a screen drops everything forward
  of it; back on the first screen and forward on the last do nothing. The web view's own history
  navigation is always cancelled (`mousedown`, `mouseup` and `auxclick` with `button` 3 or 4,
  Alt+Left and Alt+Right), so the page is never left or reloaded. In a lesson, a story,
  onboarding and the placement test, back and forward do nothing, and in a lesson Esc still asks
  to quit. A dialog takes Esc first and closes (**Cancel**, **Keep going**, **Not now**); while a
  dialog is open, Esc, back and forward never leave the screen.

## Out of scope

- AI setup and every AI entry point (06 onwards); listening and speaking exercises (09, 10);
  stories and `lesson_start` mode `story` (07); conversation, photo and media screens (11, 12);
  Android layout (13); the HTTP transport (06), so the desktop app gives the core `FailingNet`
  until then (no pack is published before module 14: `packs.lock.toml` has no entries).

## Design notes

- `Parley::new` opens `parley.sqlite3` under `data_dir` (`Store::open`) and reads `settings.json`
  through `Fs`; it keeps a store error for `app_start` and every later command, which then answer
  `storage` with the details. `app_start` also brings every started course up to date with the
  bundled syllabus (`Store::start_course`). `onboarded` is true when `active_course` is set and that
  course is started.
- `Fs` paths are relative to the data directory; an absolute path (a file the learner picked) is
  used as it is. `DirFs::write` creates missing parent folders.
- Settings: a missing file gives the defaults and no notice; a file that is not UTF-8 JSON, has
  another `schema`, or fails `schemas/settings.schema.json` (unknown keys included) gives the
  defaults and `SETTINGS_NOTICE`, and is overwritten at the next save. A patch is an object of
  settings keys; nested objects merge key by key, `null` sets null, `schema` is ignored. Rules:
  `daily_goal_xp` 10, 20, 30 or 50; `accent_leniency` lenient, strict or ignore; `answer_input`
  typing or word_bank; `desired_retention` 0.80 to 0.95; `new_items_per_lesson` 1 to 10;
  `lesson_length` 8 to 20; `active_course` es, fr, de or null; enumerations of §4 elsewhere; `{key}`
  in a message is the dotted path (`audio.voice_source`). An enumeration given an unknown string
  answers "Unknown value {v} for {key}."; any other bad value "Invalid value for {key}."
- Pack: the lock's entry for the language; `size` is `megabytes(bytes)`, decimal megabytes to one
  decimal place, `(bytes + 50,000) / 100,000` tenths in integer arithmetic (8,200,000 bytes → "8.2
  MB"). `pack_download` reports `{bytes_done: 0, bytes_total: bytes}` before the first request,
  fetches the pack in parts (below) and reports the bytes it holds after each part, the last
  report being `{bytes_total, bytes_total}`; it checks the size and SHA-256 the lock pins before
  writing anything, then writes `packs/<lang>-<version>.sqlite3.part`, renames it to
  `packs/<lang>-<version>.sqlite3`, removes the course's previous pack file and records
  `courses.pack_version`. `installed` when `courses.pack_version` names a file that opens
  read-only as a pack. Lessons take up to 20 sentences per lemma of the plan's items from an
  installed pack (`sentence_lemmas`, by `max_rank` then id) as extra `fill_gap` candidates.
  `attributions` names the author of every `fill_gap` whose `full_sentence` is a pack sentence's
  text and no syllabus example's.
- A 404 or 410 from a pack URL means the pack is not published yet: the lock names a release
  asset that does not exist, and trying again cannot help until it does. `pack_download` then
  writes nothing, records no version and returns the `not_published` status (no version, bytes
  or size), so onboarding shows "No course pack is published for {Language} yet. Parley works
  without it, with fewer example sentences." with **Continue** instead of an error. Every other
  status but 200 (and 206 for a part, below) stays `pack_missing`, "The course pack could not be
  downloaded. Try again later.", since a server error may pass.
- The pack download shows real progress. In testing, "Download the French course pack (174.7
  MB)?" led to an empty bar with no percentage and no size until the whole pack had arrived: the
  core reported progress only before the request and after the whole body, because `Net::send`
  returns a body whole. Decision: the pack is fetched in parts of 8,000,000 bytes, each a GET with
  `Range: bytes=<first>-<last>` through the injected `Net::send`, and progress is reported after
  each part (22 steps for a 174.7 MB pack); the Course pack step shows the percentage and the
  megabytes done under the bar. Ranged requests were chosen over a streaming method with a
  progress callback on the injected network because they keep the `Net` trait and the transport
  as they are, keep the whole download in the core (which still has no HTTP code of its own) where
  fake networks test it, and work with any transport; the cost is one more request per part (on a
  release asset, each with its redirect) and a bar that moves every 8 MB rather than continuously.
  Rules: a 206 must carry exactly the bytes asked for, else `checksum_mismatch` (so a server that
  answers parts with nothing cannot keep the download going); a 200 is the whole file, which a
  server or proxy that ignores `Range` sends, so its body replaces any parts held and ends the
  download (one step from 0 to the total, as before); a 404 or 410 on any part is the not-published
  outcome, any other status `pack_missing`, no response `offline`. The parts are held in memory,
  in one buffer reserved at the pinned size when the first part arrives (so the pack is never
  copied while it grows), and nothing is written until the whole has the size and SHA-256 the lock
  pins, so a download that fails or stops partway installs nothing, leaves no `.part` file and
  keeps the course's previous pack. There is no automatic retry and no resume: a new download
  starts from the first byte. Settings has no pack download, so onboarding is the one place this
  progress shows.
- Lessons: `lesson_start` composes (module 03) with `lesson_length` and `new_items_per_lesson` from
  the settings (mode `review`: 0 new items) and no capabilities, then `template_lesson` with seq
  `Store::next_lesson_seq`, `Allowed::for_skill` and the pack sentences, then `LessonRun::start_lesson`.
  `review_start` takes `due_queue(lang, now, limit)`, makes one plan entry per card with the
  composer's kind rules, and runs `template_lesson` with the context of the latest skill (syllabus
  order) among the cards; nothing is stored but the session. With nothing due it starts no session
  and returns `session_id` "", no exercises and the notice. Session ids are `s-` and 16 lower-case
  hex digits from the injected `Rng` (`p-` for placement). Runs stay in memory until finished.
- Match the pairs works with the keyboard, keeps still and shows each pair. In testing, the
  number keys did nothing though the footer said they choose an option; after the first pair both
  columns moved left as **Start over** appeared beside them in the same wrapping row; matched
  items turned grey with nothing showing which went with which; and once the meanings came in the
  same order as the words (the template shuffled whole pairs and the meanings were then sorted
  alphabetically, so the columns lined up whenever the words happened to be in alphabetical
  order). Decision:
  - Keyboard: the words are numbered 1 to n and the meanings lettered A to the nth letter, the key
    shown on each button (`aria-keyshortcuts` names it, and the button's accessible name stays its
    text). A number chooses a word and a letter (either case) the meaning on that row; a number
    then a letter, or a letter then a number, makes a pair. The mouse works the same way round:
    a word then a meaning or a meaning then a word. Choosing the chosen half again lets go of it,
    another of the same column takes its place, and a side already in a pair cannot be chosen.
    Backspace or Esc lets go of a half-made pair; Esc with none asks to quit as before. The footer
    names these keys for `match_pairs`; `pick_meaning` and the other types keep "Keys 1 to 4
    choose an option.".
  - Layout: nothing moves when a pair is made. The columns sit side by side in a two-column grid
    of equal widths, and **Start over** is always under them, disabled until a pair exists; no
    element is added or removed as pairs are made, and a made pair changes colours and the key
    shown, never a size (the pair's border is drawn as a border colour plus an inset shadow, not a
    wider border).
  - Each made pair shows which went with which: a pair takes its word's number, so both halves
    show that number in a filled badge of the pair's colour (the meaning's letter gives way to
    it) and the accessible name "{text}, pair {n}". Colour is never the only cue. The five pair
    colours (`--pair-1` to `--pair-5`, by the word's number) have their own light and dark values,
    each at least 5.5:1 against the badge's text and the page in both themes. A made pair is
    disabled but not faded (`button.matched:disabled` keeps opacity 1).
  - Order: the meanings are shown in the exercise's `english_order` (module 03), drawn with the
    injected randomness and never the words' order, replacing the alphabetical sort. A lesson that
    arrives without a usable order (an AI or course-pack exercise, or a lesson stored before
    exercises carried one) gets one drawn with the injected `Rng` when `lesson_start` starts its
    run, after the session id; a usable order is kept. Should the interface ever get none, it shows
    each meaning one row up from its word (the first at the foot), so the columns still never
    line up.
- **Check** and **Continue** stay in one place. In testing, playing with the mouse, **Continue**
  sat at four different heights depending on how many lines the result sheet had (it was the
  sheet's last element, after the verdict, the message, the correct answer, the attribution and
  any explanation), so the learner had to look for it after every answer. Decision:
  - The lesson player has an action bar fixed at the foot of the lesson area, within the lesson's
    44rem column. The answer's main button, **Check** and then, after grading, **Continue**, sits
    at the same place in it on every exercise: the bar's end, at its foot, one width for both
    (`min-width: 9rem`). The line naming the keys sits in the bar before it. The result sheet's
    other buttons (**I was right**, **Check with AI**, **Explain**, **Play**) stay in the sheet
    above; its row of buttons is left out when it would be empty.
  - The bar is one element for the whole lesson, the last before any dialog. **Check** and
    **Continue** are separate elements in it, so a **Check** pressed with the mouse never leaves
    **Continue** focused, and the keyboard is unchanged: Enter checks and continues from anywhere.
    While "Checking…" shows the bar keeps **Check**, disabled; during **Check with AI**, **I was
    right** or the lesson's finish, **Continue** stays, disabled.
  - Layout: the lesson (`main.lesson`) is at least as tall as the window (`min-height: 100vh`,
    border-box) with no padding below the bar, and the bar is pushed to its foot (`margin-top:
    auto`) and held there with `position: sticky; bottom: 0`, so its foot is the window's whether
    the exercise is short or the page scrolls. Sticky rather than fixed, the bar keeps its own room
    in the page: scrolled to its foot, nothing is ever covered by it. It is drawn on the page's
    background with a line above it, so content scrolling under it does not show through; it
    needs no z-index, coming after all the lesson's content and before its dialogs. During a
    lesson the page keeps the scrollbar's room (`html:has(main.lesson)` sets `scrollbar-gutter:
    stable`), so a long exercise that brings the scrollbar in does not move the centred column,
    and the button with it, sideways.
- Next review notice: the earliest `due` after now of a card of the course's items not retired, at
  the clock's UTC offset: "today at HH:MM" on today's local date, "tomorrow at HH:MM" on the next,
  otherwise "on {day} {Month}" (day without a leading zero, English month name).
- Placement: `Placement` of module 02 over `Store::skills`; each question (id `p{n}`, origin
  template) is about one of the skill's vocabulary items (any of its items when it has none): a
  `translate_to_en` exercise (facet receptive) with the item's `to_en` answers, or a typed
  `translate_to_target` (facet productive, no tiles) of the item's first English meaning with its
  `to_target` answers. Both are graded offline at the settings' accent leniency; correct, almost
  and typo count as right, an empty answer as wrong. At the end `Store::apply_placement` with the
  placement's session id; the start skill is the skill at the estimate.
- A revisited skill asks a question it has not asked before. The staircase returns to a skill
  after a reversal and stays at index 0 after a wrong answer there, so each visit draws, from the
  injected `Rng`, one of the skill's items not yet asked in this test as `translate_to_en`; once
  every item has been asked that way, a visit draws one not yet asked as `translate_to_target`.
  No item is asked twice in the same form, and a word comes back only translated the other way.
  A skill with no unasked item-and-form left gets its most recent answer again, without a
  question, and the staircase moves on as usual (a skill without items is answered wrong). The
  test in progress keeps, per skill, the item-and-form pairs it has asked and its last answer;
  the same seed asks the same questions. Asking the first vocabulary item on every visit repeated
  the same word up to three times in one test; this rule changes the questions every placement
  test asks, but not the staircase, how it estimates, or how a finished test is recorded.
- The cursor stays in the placement test's answer box. In testing, after **I don't know** the box
  was no longer active and the learner had to click it before typing the next answer: the click
  had focused the button, which is disabled while the answer is sent, so the focus fell to the
  page, and the box, one element for the whole test, took the focus (`autoFocus`) for the first
  question only. **Next** did the same; Enter in the box never moved the focus. Decision: every
  new placement question puts the cursor in **Your answer**, however the previous one was
  answered (Enter, **Next** or **I don't know**). The box is a new element for each question
  (keyed by its number), as the lesson player's is for each exercise, so its `autoFocus` applies
  to every question.
- Stats: `streak`, `freezes`, `days` (the 30 local dates ending today, oldest first, from `daily`),
  `week_xp` (this ISO week) and `record_week_xp` (the best earlier ISO week, 0 without one) cover
  all courses; the rest is the language's. "Words known": vocabulary items not retired whose
  productive card has retrievability ≥ 0.9 now (`vocab_items` counts the vocabulary items not
  retired). "Recall rate": among the language's review-log rows with `now − 30 days < reviewed_at ≤
  now` and `exercise_type` other than `placement`, the share rated Hard, Good or Easy; null when
  there are none (`reviews_30` is their count). `empty` when the language has no finished session.
- Backup: export reads every table in primary-key order (`cards` receptive before productive,
  `review_log` and `freeze_events` by id). Import checks the file, then writes a new store
  `parley.sqlite3.import` (courses, then `start_course` from the bundled content, then items,
  answers, cards, log, sessions, lessons, judgements, daily, freeze events), closes the current
  store and renames the new file over `parley.sqlite3` only after it is complete; settings are
  then replaced. Cards whose item does not exist after the rebuild are skipped and counted in
  `skipped_cards`. Import works when the current data file cannot be opened. When the rename
  fails, the old store is opened again, the new file is removed and the answer is `busy` (see
  "One copy at a time" below).
- Delete everything: `confirm_text` must equal `delete` after trimming; then the store is moved
  aside, the settings file, pack files and `cache/ai.sqlite3` are removed, then the moved store,
  and a fresh store is opened; a store that cannot be moved aside means nothing is removed (see
  "One copy at a time" below). `cache_clear` removes `cache/ai.sqlite3` for `ai` and `all`
  (module 09 adds audio).
- Dialogs sit above everything: `.dialog-backdrop` in `app/src/index.css` has `z-index:
  var(--layer-dialog)`, one value (1000) defined once on `:root`, and nothing else in the
  stylesheet may use a z-index that high. An element with an opacity below 1, such as a disabled
  or locked button, paints as its own layer, so a fixed backdrop without a z-index was drawn under
  every such button that came after it: on the path, the locked skills and the story button
  showed over **Set up AI features** and its buttons. Each dialog also comes last on its screen:
  the AI setup dialog after the units on the path, the lesson's dialogs after its controls, and
  the restore dialog after the Data buttons, followed only by **Back to my path**, which is never
  disabled.
- A way back from every screen. On a full course (French has 64 skills) Stats put its only
  **Back to my path** after the calendar and a row for every skill, nothing else left the screen,
  and the mouse's back and forward buttons did nothing, since screens are state in the app shell
  and the web view had no history to walk. The app shell (`app/src/navigation.ts`, used by
  `App.tsx`) now keeps a real screen history rather than only a way back to the path:
  - The history's steps are the path, Reviews (nothing due), Stats and Settings. **Back to my
    path** and Esc open the path as a new step, like any other screen: after the path, Stats, the
    path again and Settings, back shows the path, then Stats, then the path. A step shows its
    screen afresh: the path and Stats read their data again, Settings opens on the part it was
    opened on, and Reviews shows the notice it showed.
  - A part of a screen is not a step: Settings' parts (Learning, Audio, AI, Data, About) and a
    skill's detail on the path add nothing to the history.
  - A lesson or a story is not a step and is left only through its own controls (a lesson's quit
    dialog, a story's buttons): back and forward do nothing during one, starting one drops what
    was forward, and finishing or quitting returns to the path it started from. Onboarding, the
    placement test, loading and the error screens are outside the history, where back and forward
    do nothing; the history starts at the path, and starts again after a restore or Delete
    everything.
  - A side button arrives as `mousedown`, `mouseup` and `auxclick` with `button` 3 or 4. All three
    are cancelled, so the web view never navigates; the screen changes once per press, on
    `mouseup`. Alt+Left and Alt+Right in a text field are left to the field, where macOS moves the
    caret by a word.
  - Dialogs: Esc in the restore dialog is **Cancel**, in the AI setup dialog **Not now** (on its
    first-run card it closes without running the action that opened it, so nothing more is sent),
    and in a lesson's quit dialog **Keep going**. A dialog listens on the document, which hears a
    key before the window where the app shell and the lesson player listen; the shell also
    ignores Esc, back and forward while any element with the dialog role is on the page.
  - Scrolling: nothing in the stylesheet clips the page or fixes its height (the body has only a
    minimum height), so a long screen scrolls to its foot with the wheel and the keys, and the
    shell takes only Esc and Alt with an arrow, never a scrolling key. Tests keep both true.
- Desktop: the Tauri crate is a library with `run()`; `AppState` holds `Mutex<Parley>`;
  `SystemClock` takes the local UTC offset from `chrono` (clock feature only);
  `time_seeded_rng` seeds `SplitMix64` from the system time; the file picker is
  `tauri-plugin-dialog` called from Rust only, so the capability stays `core:default`;
  `tauri-plugin-single-instance`, registered first, keeps one copy running (below).
- One copy at a time, and never half a delete. In testing, Settings > Data > **Delete
  everything** answered "Parley could not write that file." and the old progress stayed;
  **Replace my data** answered the same; a fresh first run later started with "1 day streak" and
  the XP of the day before; and one launch from a desktop shortcut opened two Parley windows,
  one blank. Nothing stopped a second copy, so two processes shared the data folder and
  `parley.sqlite3`. SQLite on Windows opens its file without sharing delete access, so while one
  copy has the store open the other can neither rename nor remove it. Delete everything removed
  file by file and carried on after a failure: the store stayed, `settings.json` went, and the
  next launch, finding no settings, ran onboarding over the old store. Nothing in onboarding or
  placement writes activity (`daily` is written only when a session finishes), so the streak and
  XP were the old progress. Restore's rename over the held store failed the same way and left
  `parley.sqlite3.import` behind. Decisions:
  - Only one copy runs. `run()` registers `tauri-plugin-single-instance` (Tauri's own plugin,
    Apache-2.0 OR MIT) before any other plugin, on desktop builds only (it has nothing for
    Android or iOS). A second launch hands over to the copy already running, whose `main` window
    is unminimised, shown and focused, and exits before the rest of its start-up runs, so it
    never opens the store. The plugin finds the first copy through a named mutex and a hidden
    window on Windows, a D-Bus session name on Linux (through zbus, already used for the
    keychain) and a socket on macOS; without a D-Bus session bus it launches normally. Both
    builds use the identifier `org.parley.app`, so a debug build started while another copy runs
    only brings that copy forward. A lock file in the data folder would stop a second copy with
    no dependency, but could not bring the first copy's window to the front, so the plugin was
    chosen.
  - Delete everything is all or nothing. After the word is checked, the store is closed and
    `parley.sqlite3` is renamed to `parley.sqlite3.deleting` in the same folder before anything
    else is touched. When that rename fails (a missing file is not a failure), nothing at all is
    removed, the store is opened again as it was, lessons and placement tests in progress stay,
    and the answer is `busy`, "Parley could not delete your progress because another program is
    using it. Close it and try again." `busy` because the file is in use and the same action can
    succeed once that program lets go. Once the store is aside the progress is gone: the
    sessions in memory are dropped, every other file is removed (the journal, an import leftover,
    `settings.json`, the AI cache, pack files and their `.part` files, `cache/audio/`, the takes,
    the local speech models), then the moved store; a fresh store is opened, and the first
    failure is reported as "Parley could not write that file.". Such a failure still leaves no
    store with the old progress where Parley opens one, so the next launch starts afresh at the
    welcome screen; a moved store that could not be removed is never opened again and the next
    Delete everything removes it.
  - Restore gives the same kind of message when the store cannot be replaced: `busy`, "Parley
    could not replace your progress because another program is using it. Close it and try
    again.", with the old store opened again, the settings and the sessions in memory unchanged,
    and the new `parley.sqlite3.import` removed. The two messages name what did not happen.
  - The AI status. The desktop command forgets the AI status (`publik-status.json` and what the
    AI service holds in memory) after a successful delete and after a failure that came once
    the store was moved aside, since the progress is gone either way. A refusal ("Type delete to
    confirm.") or `busy` removed nothing, so it forgets nothing either.
    `parley_core::app::nothing_was_deleted` tells the two apart.
  - Settings > Data shows each message under the buttons whose action it answers, so the
    `busy` messages show under **Delete everything** and under **Restore a backup**, not at the
    foot of the screen; a failed delete or restore keeps the Data screen.
- Launch smoke: `npm --prefix app run e2e` runs `app/e2e/launch.spec.ts` with Vitest; it starts
  `tauri-driver`, opens a W3C WebDriver session on `target/debug/parley-app` (or
  `PARLEY_E2E_APP`), waits up to 20 s for "Learn by speaking", and closes. It speaks the WebDriver
  protocol with `fetch`, so no WebdriverIO packages are needed. The CI `e2e` job builds the front
  end and the debug app and installs `tauri-driver` (and `webkit2gtk-driver` with xvfb on Linux,
  the Edge driver on Windows) on the runner.
- Decisions made during implementation:
  - `next_review_notice` returns the whole notice ("Nothing to review. Next review: tomorrow at
    09:00."), not only the time; `api::NOTHING_TO_REVIEW_YET` is the other notice. The "yet"
    notice shows whenever no card of the course has a due time after now.
  - `Settings` keeps enumerated values as the strings of the file (`active_course:
    Option<String>`, `accent_leniency: String`, …) with `accent()` and `srs()` helpers. Every key
    but `schema` may be left out of `settings.json` and takes its default; unknown keys still make
    the file unreadable. Ranges the spec leaves open: `audio.slow_speed` 0.5 to 1.0,
    `speaking.pass_threshold` 0 to 1. A patch that is not an object answers "Invalid value for
    patch."; a non-object given for a nested object, "Invalid value for {key}.".
  - `course_start` also makes the course the active course and saves the settings, so onboarding
    needs no separate settings call; `course_list` lists the bundled courses in the order es, fr,
    de.
  - `next_skill_id` is the first available or learning skill in syllabus order, none when every
    skill is practised or mastered. `SkillDetail.words` are the skill's vocabulary items (target
    with its article, first gloss); `opened_by` is the title of the first unmet prerequisite
    (`requires`, else the previous skill).
  - Placement writes no `sessions` row, so stats stay empty until a lesson is finished; an answer
    that is not text counts as wrong; a skill without items is answered wrong and skipped.
  - A pack "opens as a pack" when it opens read-only and its `sentences`, `translations` and
    `sentence_lemmas` tables can be read. An installed pack's `bytes` and `size` come from the
    lock when the versions match. Pack sentences are looked up by the lemma as written and in NFC
    lower case. A lock that does not parse publishes nothing. A failed write removes the `.part`
    file and answers "Parley could not write that file.". Reviews use pack sentences too; a
    review's template lesson has seq 0 and is not stored.
  - A store failure while a command runs is the data-file error with the store's message as
    `details`. `file_pick` with another kind answers "Unknown value {v} for kind.".
  - Backup: every top-level key is required (an empty array for an empty table); within a row, a
    column that is nullable or has a default may be left out, and `review_log` and
    `freeze_events` rows without an id get the next one. Dates must be real dates. A backup's
    settings without `schema` are taken as version 1; settings that fail their schema make the
    file "not a Parley backup", as does a row the new store refuses. An item whose skill is not
    in the rebuilt course, and an answer whose item is not, are skipped without a count;
    `ImportReport.items` counts the items written. A leftover `parley.sqlite3.import` is removed
    first.
  - Delete everything removes the pack files the store and the lock name (the `Fs` trait has no
    directory listing), the store's journal and any import leftover. Once the store is moved
    aside it attempts every other removal, always opens a fresh store and reloads the settings,
    then reports the first failure as "Parley could not write that file.". An import whose
    settings cannot be saved keeps the restored store open and the backup's settings in use, and
    reports the same message.
  - Desktop: `pack_download` and `file_pick` are async commands, so the download and the blocking
    native dialog stay off the main thread; the dialog filters JSON files and proposes
    `parley-backup.json`. The app data directory is created at start; when it cannot be, the
    data-file screen shows.
  - Interface: **Restore a backup** on the data-file screen opens the file picker without the
    replace dialog (nothing there can be lost); in Settings the dialog comes first, then the
    picker. **Check** with no answer shows "Type an answer first." (or "Choose an answer first.",
    "Match every pair first.") without calling the command. Enter is handled from any focused
    element with its default prevented, so a focused button is never pressed twice. The English
    meanings of `match_pairs` are listed in the exercise's `english_order` (see Match the pairs
    above); a word-bank answer is the chosen tiles joined by spaces. The skill detail is a view of
    the Path screen; the Path's level-1 heading is the course's language. Data also offers **Clear
    caches** (`cache_clear("all")`). An `app_start` failure other than `storage` shows "Something
    went wrong: {message}" with **Try again**, which calls `app_start` again.
  - Launch smoke in CI: on Windows the runner's Edge WebDriver is used when its major version
    matches the installed WebView2 runtime, else the matching driver is downloaded; its folder is
    put on the `PATH` for tauri-driver.

## Acceptance tests

Command tests in `app/src-tauri/tests/m04_app_shell_and_offline_course.rs` call `Parley` with
`parley_app::bundled::courses()`, a temporary data directory (`DirFs`), a fixed clock, a seeded
`SplitMix64` and `FailingNet` unless stated; UI tests in `app/src/test/m04/*.test.tsx` render
`<App />` with `@tauri-apps/api/core` mocked; the smoke in `app/e2e/launch.spec.ts`. Expected values
come from this spec, the fixtures and modules 02 and 03.

1. Onboarding shows Welcome to Parley, Choose a language, Course pack, Daily goal, Where to start,
   in that order, and no dialog at any step (invariant I12).
2. `course_start("es")` then `path_get("es")` → the seed units in syllabus order with skill 1
   available and skill 2 locked; `next_skill_id` is skill 1. `course_start("it")` → "Unknown value
   it for lang."; `path_get("fr")` before `course_start("fr")` → "This course has not been
   started."; `skill_get` of an unknown id → "This skill is not in the course."
3. `pack_download` with a fake network serving bytes whose SHA-256 and size are in the given lock
   (answering 200 with the whole file, as a server that ignores `Range` does) → installed and
   `pack://progress` reported 0 then the total; one changed byte → `checksum_mismatch`, "The
   download was damaged. Try again.", nothing left in `packs/`; with `FailingNet` → `offline` and
   the offline message; a 404 or 410 → the `not_published` status and nothing in `packs/`; a 500
   → `pack_missing` and its message; a language missing from the lock → `not_published` and "No
   course pack is published for Spanish yet."; `megabytes(8_200_000)` is "8.2 MB".
4. With `FailingNet`, a full run succeeds: placement, a lesson, its summary, a review session,
   `stats_get` (invariant I3).
5. `exercise_submit` returns the grade of module 03 (a right `pick_meaning` choice is `correct`, an
   empty text "Type an answer first.", a second submit "This exercise was already checked.", an
   unknown session "This exercise is no longer available."); `lesson_finish` returns XP, the streak
   and the weekly record flag by module 03's rules.
6. `review_start` with nothing due and the next card due tomorrow at 09:00 local → no exercises and
   "Nothing to review. Next review: tomorrow at 09:00."; `next_review_notice` gives the today and
   "on 14 October" forms; with no reviewed card, "Nothing to review yet. Finish a lesson to start
   your reviews."
7. `stats_get` after importing `tests/cases/04-app-shell-and-offline-course/stats-backup.json` (a
   hand-written backup with known cards, reviews and days) at a fixed time → the numbers in
   `expected-stats.json` beside it, worked out by hand from the definitions above (floats within
   1e-6; retrievability from FSRS-6's forgetting curve, module 02).
8. Backup: export, delete everything, import, export again → identical JSON apart from
   `exported_at`; a file with `"schema": "other"` → "This is not a Parley backup."; `parley.backup/2`
   → "This backup was made by a newer Parley. Update Parley to import it."; a card for a missing
   item is skipped and reported; a missing file → "Parley could not read that file."; an unwritable
   destination → "Parley could not write that file."
9. Settings: a corrupt `settings.json` → defaults and the notice; `settings_set` with
   `accent_leniency: "loose"` → `invalid_input`, "Unknown value loose for accent_leniency.";
   `lesson_length: 30` → "Invalid value for lesson_length."; `colour: "blue"` → "Unknown setting
   colour."; a good patch is saved and read back by a new `Parley` on the same directory.
10. "Delete everything" refuses without the text `delete` ("Type delete to confirm.") and afterwards
    `app_start` says not onboarded and the UI shows Welcome to Parley. `cache_clear("ai")` on a new
    directory removes 0 files; `cache_clear("other")` → "Unknown value other for kind."
11. `COMMANDS` in `crates/parley-core/src/api.rs` equals the `COMMANDS` list in
    `app/src/api/commands.ts`, in order, and every name is registered in `app/src-tauri/src/lib.rs`.
12. Keyboard: in `pick_meaning`, key 2 checks option 2, Enter submits `{"choice": 1}`, and Enter on
    the result sheet shows the next exercise; Esc opens the quit dialog.
13. `attributions` names fixture_a for a `fill_gap` on fixture_a's pack sentence and nobody for one
    on a syllabus example; without a pack `lesson_start` gives no attributions; the result sheet of
    an exercise with an attribution shows "Sentence by fixture_a on Tatoeba"; the About page credits
    Tatoeba (CC BY 2.0 FR) and Wiktionary (CC BY-SA 4.0).
14. Every `t("…")` key used under `app/src/` exists in `en.json`.
15. A corrupt `parley.sqlite3` → `app_start` answers `storage`, "Parley could not open its data
    file." with details, and the UI shows that screen with **Restore a backup** and **Show
    details**; `backup_import` still works and the next `app_start` succeeds.
16. Launch smoke: the debug build starts and shows the Welcome text within 20 s on Windows and Linux.
17. One copy at a time, and never half a delete. `app/src-tauri/src/lib.rs` registers
    `tauri_plugin_single_instance` before any other plugin, its callback unminimises, shows and
    focuses the `main` window, and it is a dependency of the desktop builds only (read from the
    source: the plugin cannot run without opening a window, and a second app in the test process
    would be the copy it ends). With a filesystem that refuses to rename or remove
    `parley.sqlite3` (the unit tests in `crates/parley-core/src/app/mod.rs`): Delete everything →
    `busy`, "Parley could not delete your progress because another program is using it. Close it
    and try again.", every other file (settings, AI cache, audio, a take, a pack, a model) byte
    for byte as it was and the progress unchanged, and once the file is free the same delete
    leaves only a fresh `parley.sqlite3`; Restore → `busy`, "Parley could not replace your
    progress because another program is using it. Close it and try again.", no
    `parley.sqlite3.import` left and the progress unchanged. A removal that fails after the store
    is moved aside → "Parley could not write that file.", and the next launch is a first run with
    no streak and no XP; a moved store that cannot be removed is never opened again and the next
    Delete everything removes it. On Windows, with a second `Parley` open on the same folder,
    Delete everything and Restore both answer `busy` and change nothing. The UI shows the delete
    message under **Delete everything** and the restore message under **Restore a backup**,
    before the course packs, and stays on the Data screen.
18. Placement: after **I don't know**, after **Next** and after Enter in **Your answer**, the next
    question's **Your answer** box has the focus and is empty.
19. Pack download in parts (unit tests in `crates/parley-core/src/app/pack.rs`, with a pack of a
    little over two parts and a fake network that answers `Range` with 206 and those bytes): the
    requests ask for `bytes=0-7999999`, `bytes=8000000-15999999` and `bytes=16000000-<last>`,
    `pack://progress` reports 0, 8,000,000, 16,000,000 and the total, nothing is in `packs/` until
    the last part has arrived, and the pack is then installed whole. The second part failing (no
    connection, timed out, a 500, half a part, an empty part) → `offline`, `pack_missing` or
    `checksum_mismatch` with its message, progress stopped after the first part, no new pack and
    no `.part` file, and the course's previous pack still installed; a 404 partway → the
    `not_published` status and nothing written. A server that answers 200 with the whole file, at
    the first request or after a part → installed, with the whole file in place of the parts. A
    lock with the file's SHA-256 but a size one byte larger → `checksum_mismatch`, whether the
    server honours `Range` or not. UI: while the download runs, the bar and "0% · 0.0 of 174.7 MB"
    show at once for a 174,747,648-byte pack, and progress events at 8,000,000, 73,400,000 and
    174,747,648 bytes show "4% · 8.0 of 174.7 MB", "42% · 73.4 of 174.7 MB" and "100% · 174.7 of
    174.7 MB" with the bar's value at those bytes.
20. Match the pairs (UI tests in `app/src/test/m04/screens.test.tsx` and
    `app/src/screens/Lesson/draft.unit.test.ts`; the lesson start in
    `crates/parley-core/src/app/lesson.rs`): the words show 1 to 4 and the meanings A to D in the
    exercise's `english_order`, with `aria-keyshortcuts` and the words as accessible names; with no
    order given, the meanings are still not in the words' order. Keys 1 then B, C then 4, 2 then D
    and A then 3 make the four pairs (key 5 does nothing) and Enter sends `[0,0]`, `[3,3]`, `[1,1]`
    and `[2,2]`; a meaning clicked before its word makes a pair. Esc and Backspace each let go of a
    half-made pair with no dialog and leave made pairs; Esc with none opens the quit dialog. **Start
    over** is present and disabled from the start, outside the row of the columns; after a pair the
    groups, their buttons and **Start over** are the same elements in the same places, and **Start
    over** is enabled until it is used. Both halves of a made pair show its word's number, have the
    class `matched pair-3` and are named "gracias, pair 3" and "thank you, pair 3"; the stylesheet
    keeps `button.matched:disabled` at opacity 1 and gives `.pair-1` to `.pair-5` their colours. The
    footer names the match-the-pairs keys, and the `pick_meaning` keys on a `pick_meaning`. A stored
    lesson whose `match_pairs` has no order or the pairs' own order starts with a usable one, a
    usable order is kept, and the answer by pair index is graded `correct`.
21. Check and Continue stay in one place (UI tests in `app/src/test/m04/screens.test.tsx`; jsdom
    has no layout, so the element and its place in the page are asserted): **Check** is the last
    element of an element with the class `action-bar`, itself the last element of the lesson, with
    the line naming the keys in it. After a one-line result (a correct `pick_meaning`: the sheet
    holds its verdict only) and after a many-line one (a wrong `fill_gap` with a message, the
    correct answer, the attribution and **Explain**), **Continue** is the last element of that same
    bar, after the region **Result** and not in it, and the only **Continue** on screen; the next
    exercise's **Check** is in the same bar again, and Enter still continues. While "Checking…"
    shows, the sheet has no button and the bar's **Check** is disabled; a `Not checked` sheet keeps
    **I was right** and **Check with AI**, and **Continue** is the bar's. The stylesheet makes
    `.action-bar` `position: sticky`, `bottom: 0`, `margin-top: auto` on `var(--background)`, and
    `main.lesson` `min-height: 100vh`, border-box, with no bottom padding, and
    `html:has(main.lesson)` keeps `scrollbar-gutter: stable`.

## Status

Done (2026-11-03)
