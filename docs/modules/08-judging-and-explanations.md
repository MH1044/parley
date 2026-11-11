# 08: Judging and explanations

## Goal

A free translation that the offline grader cannot match is judged by `publik-fast` in a few
seconds: correct, almost or wrong, with a corrected answer and a short reason. A correct verdict is
remembered, so the same answer is accepted offline from then on. After an almost or wrong answer,
**Explain** gives up to 80 words of plain English about the mistake.

## Depends on

03, 06. Feature-map rows **P3** (answer judge) and **P4** (explain my answer); §6.5 and §6.6 of
`docs/PUBLIK-INTEGRATION.md` specify both calls.

## Scope

- `prompts/judge.v1.md` and `prompts/explain.v1.md` with the exact text of §6.5 and §6.6; schemas
  `schemas/ai/judge.v1.schema.json` and `schemas/ai/explain.v1.schema.json`.
- In `exercise_submit`: an `unchecked` grade goes to the judge when a provider is ready (chosen,
  consent given) and `needs_credit` is not set; the judgement is looked up first; the verdict
  becomes the grade.
- Effects of a verdict (§6.5): rating, `judgements` row, `accepted_answers` row for a correct
  answer to the item's own prompt; the call's charge added to the session's cost.
- Without a ready provider: the result sheet offers **Check with AI** (opens the AI setup dialog
  when no provider is chosen), **I was right** and **Continue**. **Check with AI** and **Try again**
  are the new command `exercise_check`.
- A 402 during a lesson stops judging for the rest of that lesson.
- `explain_answer` command, offered only after `almost` or `wrong`, cached.
- Result sheet states: "Checking…", the verdict, "Not checked" with the reason of §9.
- `parley judge render <request.json>`: the P3 request body, for the golden case.
- Gate G2: the live gate `live_g2` in `crates/parley-cli/tests/gates_live.rs`
  (`scripts/gates/run --live G2`, the maintainer's) and its labelled set `tests/gates/judge-set.jsonl`
  (300 project-written cases, 100 per language, each with source, accepted answers, the answer and
  the expected verdict); its scaled-down replay of `tests/fixtures/ai/P3/recorded/` in the normal
  suite.

## Files

- `crates/parley-core/src/ai/{judge.rs,explain.rs}` (re-exported from `ai/mod.rs`),
  `crates/parley-core/src/app/judge.rs`, the grade and run additions in
  `crates/parley-core/src/grading/grade.rs` and `crates/parley-core/src/exercise/run.rs`,
  `crates/parley-core/src/api.rs` (`COMMANDS`)
- `prompts/{judge,explain}.v1.md`, `schemas/ai/{judge,explain}.v1.schema.json`
- `crates/parley-cli/src/cmd/judge.rs` (`judge render`); `scripts/gates/thresholds.toml` (`[g2]`)
- `app/src-tauri/src/commands/lesson.rs` (judge path, `exercise_check`, `explain_answer`) and the
  command registration; `app/src/components/ResultSheet.tsx` (states),
  `app/src/screens/Lesson/Lesson.tsx`, `app/src/api/commands.ts`, `app/src/strings/en.json`
- Tests: `crates/parley-core/tests/m08_judging_and_explanations.rs`,
  `tests/cases/08-judging-and-explanations/`, `app/src/test/m08/*.test.tsx`,
  `crates/parley-cli/tests/gates_live.rs` (`live_g2`); fixtures `tests/fixtures/ai/P3/*.json`,
  `tests/fixtures/ai/P3/recorded/*.json`, `tests/fixtures/ai/P4/*.json` (each folder with a
  README), `tests/gates/judge-set.jsonl`

## Interfaces

### parley-core: `parley_core::ai` (additions, all re-exported from `ai/mod.rs`)

```rust
// judge.rs
pub const JUDGE_PROMPT_ID: &str = "judge.v1";
pub fn judge_prompt_file() -> &'static Prompt;     // prompts/judge.v1.md + its schema, compiled in

/// What P3 judges. serde field names as written; `item_id` and `own_prompt` default to "" and
/// false when absent (the CLI's request file and the gate set leave them out).
pub struct JudgeRequest {
    pub lang: Lang,              // the course language
    pub direction: Direction,    // to_target: the answer is in `lang`; to_en: the answer is English
    pub cefr: Cefr,              // the item's level
    pub source: String,          // the exercise's prompt: prompt_en (to_target) or prompt_target (to_en)
    pub accepted: Vec<String>,   // the exercise's own accepted answers, in order
    pub answer: String,          // the learner's text as typed
    #[serde(default)] pub item_id: String,
    #[serde(default)] pub own_prompt: bool,  // the prompt is the item's own (module 03's rule)
}
impl JudgeRequest {
    /// The placeholder values of §6.5 (below).
    pub fn vars(&self) -> BTreeMap<String, String>;
    /// {"lang", "direction", "source", "accepted", "answer_norm"} (§6.5): lang and direction as
    /// their strings, `accepted` the JSON array, answer_norm = grading::normalise(lang, answer).
    pub fn key_input(&self) -> serde_json::Value;
}
/// A schema-valid P3 answer (serde as the schema).
pub struct JudgeResponse { pub verdict: srs::Verdict, pub corrected: String,
    pub explanation: String, pub error_tags: Vec<String> }
/// A verdict, after J4.
pub struct Judgement {
    pub verdict: srs::Verdict,   // correct | almost | wrong
    pub corrected: String,
    pub explanation: String,
    pub tags: Vec<String>,       // error_tags; always empty for correct (J4)
    pub cached: bool,            // from the judgements table or the AI cache: no request was made
    pub charge_micros: Option<i64>,
}
/// The key of `judgements` (provider-independent): module 03's grading::judgement_key, re-exported.
pub fn judgement_key(lang: Lang, direction: Direction, source: &str, answer_norm: &str) -> String;
/// P3 with no store: ai::call with the J2 and J3 rules, then J4. Any failure is the call's
/// AiError (a rule failure is AiError::Invalid, nothing cached, one attempt counted).
pub fn request_judgement(client: &dyn AiClient, cache: &Cache, req: &JudgeRequest)
    -> Result<Judgement, AiError>;
/// The effects of a valid verdict: the `judgements` row under judgement_key (provider =
/// `provider.as_str()`), and for `correct` with `own_prompt`, an `accepted_answers` row of the
/// item in the request's direction (display = the answer, NFC and trimmed; source `judge`).
pub fn apply_judgement(store: &mut Store, req: &JudgeRequest, judgement: &Judgement,
    provider: ProviderKind, now_ms: i64) -> Result<(), StoreError>;
/// End to end: a stored judgement for the key is returned (cached: true) with no request;
/// otherwise request_judgement, then apply_judgement. A store that cannot be written loses only
/// the saving: the verdict is still returned.
pub fn judge(client: &dyn AiClient, cache: &Cache, store: &mut Store, req: &JudgeRequest,
    now_ms: i64) -> Result<Judgement, AiError>;
/// The grade a verdict gives an exercise of `kind` (no hint, no response time; the run rates the
/// card again with both): Correct → rating Good, corrected None; Almost → Hard, corrected Some;
/// Wrong → Again, corrected Some and expected = `first_accepted`. explanation Some for all three;
/// tags: the error_tags that are module 03 tags (`accent`), others dropped; message None.
pub fn verdict_grade(kind: ExerciseType, judgement: &Judgement, first_accepted: Option<&str>) -> Grade;

// explain.rs
pub const EXPLAIN_PROMPT_ID: &str = "explain.v1";
pub fn explain_prompt_file() -> &'static Prompt;   // prompts/explain.v1.md + its schema
pub struct ExplainRequest {
    pub lang: Lang,
    pub cefr: Cefr,
    pub exercise_type: ExerciseType,
    pub prompt: String,          // below, per type
    pub expected: String,        // below, per type
    pub answer: String,          // the learner's text, or the text of the chosen option
    pub verdict: srs::Verdict,   // almost or wrong
    pub tags: Vec<String>,       // the judge's error_tags, else the grade's tags
    pub grammar_note: Option<String>,  // the skill's grammar note
}
impl ExplainRequest {
    pub fn vars(&self) -> BTreeMap<String, String>;
    /// {"lang", "exercise_type", "prompt", "expected", "answer_norm", "verdict"} (§6.6).
    pub fn key_input(&self) -> serde_json::Value;
}
pub struct ExplainResponse { pub explanation: String, pub tip: Option<String> }  // schema-valid
pub struct Explanation { pub text: String, pub tip: Option<String>, pub cached: bool,
    pub charge_micros: Option<i64> }
/// P4: ai::call with the E2 and E3 rules.
pub fn explain(client: &dyn AiClient, cache: &Cache, req: &ExplainRequest)
    -> Result<Explanation, AiError>;

/// "You're offline, so Parley can't check free answers."
pub const JUDGE_OFFLINE: &str;
/// "Explanations need a connection or a local model."
pub const EXPLAIN_OFFLINE: &str;
/// "Explanations are available after an answer that was almost right or wrong."
pub const EXPLAIN_NOT_AVAILABLE: &str;
```

### Prompt values

P3 (§6.5): `answer_language_name` is the course language's name (`Spanish`) for `to_target` and
`English` for `to_en`; `source_language_name` the other one; `cefr` `A1` or `A2`; `source_json` and
`answer_json` the text as one JSON string literal (serde_json, so non-ASCII stays as it is);
`accepted_json` the accepted answers as JSON string literals joined by `", "` inside `[` and `]`,
exactly as the §6.5 example shows (`["Quiero un café con leche.", "Quisiera un café con leche."]`).
The request is `ChatRequest` with the tier's model, `max_tokens` 200, `temperature` 0, timeout 10 s.

P4 (§6.6): `verdict` `almost` or `wrong`; `target_language_name` the course language's name;
`cefr`; `exercise_type_name`: `translate into {Spanish}` (translate_to_target), `translate into
English` (translate_to_en), `choose the meaning` (pick_meaning), `type what you hear`
(listen_type), `say the sentence` (speak), `fill the gap` (fill_gap); `prompt_json`, `expected_json`
and `answer_json` JSON string literals; `tags` the tags joined by `", "`, or `none`; `grammar_note`
the note as a JSON string literal, or `none` (as P1 does). Per type, PROMPT and EXPECTED are:
translate_to_target `prompt_en` and the first accepted answer; translate_to_en `prompt_target` and
the first accepted answer; pick_meaning `prompt_target` and the correct option; listen_type `text`
and `text`; speak `prompt_en` and `text`; fill_gap `sentence` and `answer`. `match_pairs` has no
explanation. `max_tokens` 300, `temperature` 0.3, timeout 15 s.

### Validation

- **J1** the schema (`verdict` enum; `corrected` and `explanation` strings of 1 to 300 characters;
  `error_tags` an array of the §6.5 enum, unique; `additionalProperties: false`).
- **J2** `explanation` has at most 25 words; **E2** `explanation` at most 80 words and `tip` at most
  15. Words are the UAX #29 words of the text (`unicode_words`).
- **J3** `corrected` of four words or more is in the answer language (module 07's langid:
  `is_language` with the course language for `to_target`; for `to_en`, English told apart from
  the course language, `is_language_in(…, TextLang::En, lang)`, see Design notes).
- **J4** `verdict = correct` with tags keeps the verdict and drops the tags (applied after the
  call; the cached answer keeps them and J4 applies again on every use).
- **E1** the schema (`explanation` 1 to 700 characters, `tip` optional, up to 120; nothing else);
  **E3** `explanation` is English, told apart from the course language
  (`is_language_in(…, TextLang::En, lang)`, see Design notes).

### parley-core: judging in the app service (`crates/parley-core/src/app/judge.rs`)

```rust
/// What a judge step gives: a final grade, or a call to make with no lock held.
pub enum JudgeBegin { Done(Grade), Pending(JudgeJob) }
pub struct JudgeJob { .. }                 // owns the client, the cache path, the request, the time
pub struct JudgeOutcome(..);
impl JudgeJob { pub fn run(&self) -> JudgeOutcome; }     // opens cache/ai.sqlite3, request_judgement
pub struct ExplainJob { .. }
pub struct ExplainOutcome(..);
impl ExplainJob { pub fn run(&self) -> ExplainOutcome; } // opens cache/ai.sqlite3, explain
/// `explain_answer`'s result.
#[derive(Serialize, Deserialize)]
pub struct Explained { pub text: String, pub tip: Option<String>, pub cached: bool }

impl Parley {
    /// After a submission graded `unchecked`: whether to ask the judge. `automatic` is true right
    /// after `exercise_submit`, false for Check with AI and Try again.
    pub fn judge_begin(&mut self, session_id: &str, exercise_id: &str, ai: Option<AiAccess>,
        automatic: bool) -> Result<JudgeBegin, AppError>;
    /// Applies a job's outcome to the run (below) and returns the grade the sheet shows.
    pub fn judge_complete(&mut self, job: JudgeJob, outcome: JudgeOutcome) -> Result<Grade, AppError>;
    /// exercise_submit, then (for an unchecked grade) judge_begin(.., true), run and complete, in
    /// one go (the CLI and tests). `exercise_submit` itself never calls the judge.
    pub fn exercise_submit_with(&mut self, session_id: &str, exercise_id: &str, answer: &Answer,
        response_ms: Option<i64>, used_hint: bool, ai: Option<AiAccess>) -> Result<Grade, AppError>;
    /// Check with AI and Try again in one go: judge_begin(.., false), run, complete.
    pub fn exercise_check_with(&mut self, session_id: &str, exercise_id: &str,
        ai: Option<AiAccess>) -> Result<Grade, AppError>;
    pub fn explain_begin(&mut self, session_id: &str, exercise_id: &str, ai: Option<AiAccess>)
        -> Result<ExplainJob, AppError>;
    pub fn explain_complete(&mut self, job: ExplainJob, outcome: ExplainOutcome)
        -> Result<Explained, AppError>;
    /// explain_begin, run and complete in one go.
    pub fn explain_answer_with(&mut self, session_id: &str, exercise_id: &str,
        ai: Option<AiAccess>) -> Result<Explained, AppError>;
}
```

`judge_begin` refuses an unknown session or exercise with `not_found` "This exercise is no longer
available.", an exercise not yet answered with `invalid_input` "Type an answer first.", and an
answer whose grade is not `unchecked` with `invalid_input` "This exercise was already checked."
Then, with `automatic`:

| `ai` | Result |
|---|---|
| None (no provider chosen) | `Done`, the grade as it is (**Check with AI** offered) |
| any, when this run's judging stopped after a 402 | `Done`, the grade as it is |
| `Some(Err(NeedsConsent))` | `Done`, the grade as it is |
| `Some(Err(e))` | `Done`, the grade with `judge_error` = e |
| `Some(Ok(client))` with `status().needs_consent` or `status().needs_credit` | `Done`, the grade as it is |
| `Some(Ok(client))` otherwise | `Pending` |

Without `automatic` (the learner asked): None, `Some(Err(NeedsConsent))` and a client whose
status needs consent are the command error `needs_consent` "AI features are not set up yet." (the
interface opens the AI setup dialog); `Some(Err(e))` is `Done` with `judge_error` = e; any other
client is `Pending`, even with `needs_credit` set or judging stopped (a learner-triggered call goes
out once, §9).

`judge_complete`: a valid verdict reviews the card with `rating_for` (the verdict, the submission's
hint and response time), replaces the run's grade with `verdict_grade` (rating as reviewed),
applies `apply_judgement` with the client's kind, and adds the charge to the session's
`cost_micros`. A failure leaves the grade `unchecked` with `judge_error` set (kept on the run, so
**I was right** still works): `NeedsCredit` also stops judging for the rest of the run; `Offline`
becomes `AppError { code: offline, message: JUDGE_OFFLINE }`; `NeedsConsent` is the command error
`needs_consent` after **Check with AI** or **Try again**, while after an automatic call the grade
stays as it is; every other error is its own `AppError`. A job whose answer was decided in the
meantime (**I was right**) changes nothing and returns the run's grade.

`explain_begin` refuses an unknown session or exercise with `not_found`, and an answer that is not
`almost` or `wrong` (or not yet given, or a `match_pairs` answer) with `invalid_input`
EXPLAIN_NOT_AVAILABLE; no provider chosen or no consent is `needs_consent`; `Some(Err(e))` is the
command error (Offline as EXPLAIN_OFFLINE). `explain_complete` returns `Explained` and adds the
charge to the session's cost; `Offline` is `AppError { code: offline, message: EXPLAIN_OFFLINE }`,
any other error its own `AppError` (a 402 carries its message and `top_up_url`).

`Grade` gains three fields, each left out of the JSON when None (so every grade of modules 03 and
04 serialises as before; `Grade` keeps `PartialEq` and drops `Eq`):

```rust
pub explanation: Option<String>,   // the judge's reason, on a verdict it gave (also a cached one)
pub expected: Option<String>,      // the first accepted answer, under a judged `wrong`
pub judge_error: Option<AppError>, // why an unchecked answer has no verdict (a call failed)
```

### Commands and events

- `exercise_submit(session_id, exercise_id, answer, response_ms, used_hint) -> Grade`: grades
  offline; an `unchecked` grade then goes through `judge_begin(.., automatic: true)` with a client
  asked for only when `ai.provider` is not `unset`; the call runs with no lock held; the status is
  emitted (`ai://status`) after a call.
- `exercise_check(session_id, exercise_id) -> Grade`: **Check with AI** and **Try again**
  (`automatic: false`). Errors as `judge_begin`.
- `explain_answer(session_id, exercise_id) -> { text, tip: Option<String>, cached: bool }`; errors:
  `not_found` "This exercise is no longer available.", `invalid_input` "Explanations are available
  after an answer that was almost right or wrong.", `needs_consent`, and the AI errors of module 06
  (offline: "Explanations need a connection or a local model.").

### CLI

    parley judge render <request.json> [--json]
        The P3 request body for publik API (model publik-fast) of the JudgeRequest in
        <request.json>: ChatRequest::body(false) of the rendered judge.v1 prompt.
        Usage errors: `error: <request.json> is required. Run parley --help.`, `error: {path} not
        found`. A file that is not a JudgeRequest: exit 1, `error: {path}: {parser message}`.

### UI

Strings in `app/src/strings/en.json`. The lesson screen shows the result sheet as "Checking…" (no
buttons) while `exercise_submit` of a `translate_to_target` or `translate_to_en` answer, or
`exercise_check`, is in flight. Then:

- A judged verdict: the verdict line of module 04 (Correct, Almost, Wrong), the grade's
  `explanation`, "Correct answer: {corrected}" when `corrected` is set, "Also correct: {expected}"
  when `expected` is set, **Explain** after `almost` or `wrong` (not after `typo`, and never on a
  `match_pairs` answer), **Continue**.
- `unchecked` without `judge_error`: "Not checked" (module 04), **Check with AI**, **I was right**,
  **Continue**. **Check with AI** calls `exercise_check`; a `needs_consent` error opens the AI setup
  dialog, whose Continue calls `exercise_check` again and whose **Not now** leaves the answer
  unchecked with no call (I12).
- `unchecked` with `judge_error`: "Not checked", then the error as `AiBanner` shows it (402: the
  message, exactly one link and "or use your own key or a local model in Settings"; disconnected:
  **Reconnect**; `busy` and `ai_invalid`: **Try again**, which calls `exercise_check`; anything else
  its message), **I was right**, **Continue**.
- **Explain** shows "Explaining…", then the text and "Tip: {tip}" when there is a tip, or the error
  as `AiBanner` shows it (with **Try again** for `busy` and `ai_invalid`); a `needs_consent` error
  opens the AI setup dialog.

Messages: "Checking…"; "Not checked" followed by the reason line of PUBLIK-INTEGRATION §9 for 402,
disconnected, 429, 503 and offline ("You're offline, so Parley can't check free answers.");
"publik API did not answer in time." with **Try again**; "The AI answer could not be used." with
**Try again** and **I was right**; **Check with AI**; "Also correct: {expected}"; **Explain**;
"Explaining…"; "Tip: {tip}"; "Explanations need a connection or a local model." (offline);
"Explanations are available after an answer that was almost right or wrong."

### Gate G2

`tests/gates/judge-set.jsonl`: one JSON object per line, `{"id", "lang", "direction", "cefr",
"source", "accepted", "answer", "expected"}`; ids `es-001` to `es-100`, `fr-001` to `fr-100`,
`de-001` to `de-100`; `direction` `to_target` or `to_en` (at least 30 of each per language);
`cefr` `A1` or `A2`; `expected` `correct`, `almost` or `wrong` (at least 25 of each per language,
labelled by the rules of the §6.5 prompt); `accepted` one to three answers; the answer never equal
to an accepted answer after `normalise`. The answers are ones the offline grader leaves unmatched:
not an accent slip or a one-letter typo of an accepted answer (those never reach the judge).

`live_g2` (with `PARLEY_LIVE=1`, publik API): each case as a `JudgeRequest` (no item, own_prompt
false) through `request_judgement` with a fresh in-memory cache; agreement = cases whose verdict
equals `expected` ÷ 300; false accepts = cases labelled `almost` or `wrong` judged `correct` ÷ the
cases labelled `almost` or `wrong`; p95 of the request times. Thresholds in
`scripts/gates/thresholds.toml` `[g2]`: `agreement = 0.9`, `false_accept_share = 0.03`,
`p95_s = 4.0`. A failed call counts as a disagreement. It writes ten cases (`es-001`, `es-031`,
`es-061`, `es-091`, `fr-001`, `fr-031`, `fr-061`, `de-001`, `de-031`, `de-061`) to
`tests/fixtures/ai/P3/recorded/<id>.json`, each only when its answer passed the checks, as
`{"case": id, "request": JudgeRequest, "content": the reply text, "ms": time}`. About $0.045
(300 × $0.00015).

Replay in the normal suite: every file in `tests/fixtures/ai/P3/recorded/` through `judge` with a
client that returns its `content`: each passes J1 to J3 and gives a verdict; at least 9 of 10 agree
with the case's `expected` in the set; each verdict's effects are its rating (`verdict_grade`) and
one `judgements` row. Until the first live run, the folder holds ten hand-written stand-ins.

## Out of scope

- Role-play corrections (11); explanations of speaking results (10 shows word-level feedback only).
- Explanations of `match_pairs` answers.
- Recording real P3 answers for the replay: the maintainer's first live G2 run writes them.

## Design notes

- The judgement key (ARCHITECTURE `judgements.key`) is provider-independent: a verdict is learning
  data, while the response cache (module 06) stays per provider.
- The learner's answer can contain text aimed at the judge. It is sent as a JSON string, and the
  worst outcome is a learner fooling their own grading, so no further defence is added.
- `exercise_submit` grades offline as before and asks the judge only for an `unchecked` answer, so
  a client is built only when a free translation is unmatched, and the call runs on a blocking
  thread with no lock held (begin, run, complete, as module 07's lessons do).
- ACCEPTED holds the exercise's own accepted answers only, so the cache key does not depend on
  what this learner had accepted before.
- Explain is not offered after a `typo`: the sheet already names the misspelt word.
- The renderer always writes a `_json` value as one JSON string literal itself, so learner text is
  escaped whatever it looks like; the features hand it the raw text. `accepted_json` is a list:
  `prompts/judge.v1.md` names it in the front matter (`json_lists: accepted_json`, a field added to
  the PUBLIK-INTEGRATION §6.1 format), its value is the accepted answers as a JSON array, and the
  renderer writes each as a JSON string literal joined by `", "` inside `[` and `]`, refusing
  anything that is not an array of strings. The grammar note of P4 is course text the feature
  writes as a JSON string literal (or `none`) under its plain placeholder.
- `prompts/judge.v1.md` follows the §6.5 example request where the prompt block wraps a line for
  display: "The learner was asked to translate SOURCE into … not the only correct answers." is one
  line, as the example request sends it.
- J3 for `to_en` and E3 tell English apart from the course language when the identification is
  unsure (`is_language_in`, as the lesson and story checks do). Measured on the §6.6 example
  response, whatlang's four-language choice is French at 0.43, and English loses a two-way choice
  with French but beats Spanish, because an explanation quotes the learner's words; explanations
  written in French, Spanish or German were named at 0.7 or more and are refused. J3 for
  `to_target` is `is_language` with the course language, the same check.
- A judge answer may leave out `error_tags` (none); `verdict`, `corrected` and `explanation` are
  required. A blank `tip` counts as no tip. `verdict_grade` keeps each module 03 tag once.
- The request's level is the item's in the bundled course (else its skill's; A1 for an item the
  course does not have), and Explain sends the grammar note of the item's skill. Explain sends the
  judge's error tags when it gave any, else the grade's own tags.
- An automatic judge call whose client answers with no consent leaves the grade as it is, like the
  begin table, so the setup dialog only ever opens after the learner asked; only Check with AI and
  Try again return `needs_consent`.
- After a valid verdict, a store that cannot be written loses only the saving (the judgement, the
  accepted answer, the cost): the review and the grade stand. `apply_judgement` stores nothing for
  an answer with no words. A judge job whose lesson has ended is "This exercise is no longer
  available." and changes nothing; its answer stays in the AI cache.
- The stored-judgement grade (module 03 step 5) carries the stored explanation when there is one,
  and no `expected`.
- "Checking…" is a state of the result sheet itself, which turns into the verdict in place. The
  judge's error shows right under "Not checked". Explain is hidden once asked for; its error offers
  Try again where the error allows. In a lesson, **Use my own key** or **Use a local model** in the
  setup dialog closes it and leaves the answer unchecked, so the lesson is not lost; the provider is
  chosen in Settings > AI afterwards. The 402 link's label follows the claim state of the
  `ai://status` events.
- `app/src/strings/en.json` also holds the three messages the core sends ("You're offline, so
  Parley can't check free answers.", "Explanations need a connection or a local model.",
  "Explanations are available after an answer that was almost right or wrong."); the sheet shows
  the error's own message.

## Acceptance tests

Golden cases in `tests/cases/08-judging-and-explanations/`; code tests in
`crates/parley-core/tests/m08_judging_and_explanations.rs`; UI in `app/src/test/m08/*.test.tsx`.
Contract tests 1 to 18 (module 06) keep passing; this module adds tests 24 to 27 of
`docs/PUBLIK-INTEGRATION.md` §11 (24: tests 2 to 4; 25: test 5; 26: tests 7 and 12; 27: test 8).
Invariants: I2 (test 7, 12), I4 (test 11), I5 (tests 4 and 12), I12 (test 10).

1. The judge request for the §6.5 example renders exactly the body shown there (golden).
2. Answers matched exactly, by accent or as a typo make no judge request (fake count 0).
3. An unmatched `fill_gap` is `wrong` with no request; an unmatched `translate_to_en` makes one.
4. A stored judgement for the same key → no request and the same verdict.
5. Fixture `correct` → rating Good, a `judgements` row, an `accepted_answers` row with source
   `judge`; submitting that answer to a new exercise of the same item grades correct offline.
6. Fixture `almost` (§6.5 example) → rating Hard, "Me gustaría un café con leche" shown; fixture
   `wrong` → rating Again, the corrected answer and the first accepted answer shown.
7. J1 (missing `verdict`), J2 (26-word explanation), J3 (English `corrected` of 5 words for a
   Spanish answer) → "The AI answer could not be used.", nothing cached or stored; J4 (`correct`
   with tags) → verdict kept, tags dropped.
8. A 402 on the first judged answer shows "Not checked", the message and one link; a second
   unmatched answer in the same lesson makes no request (count stays 1); the next lesson judges again
   only after needs-credit clears.
9. With a client that is offline (`AiError::Offline`, what the publik client gives over
   `FailingNet`), the sheet shows "You're offline, so Parley can't check free answers." and
   **I was right**; nothing is stored.
10. With `ai.provider = unset`, **Check with AI** opens the AI setup dialog; **Not now** leaves the
    answer unchecked (invariant I12); no automatic judge call is made without a provider or consent.
11. A judge call that takes more than 10 s (`timeout_s` 10 in the request; the client gives
    `AiError::Timeout`) → "publik API did not answer in time." and **Try again**; exactly one
    request until the learner taps it, and the Try again sends the same idempotency key.
12. Explain, valid fixture → the text and tip shown; E2 (81 words) and E3 (French text) → "The AI
    answer could not be used."; a second tap → `cached: true` and no request; the §6.6 example
    renders the §6.6 user content.
13. `explain_answer` after a correct answer → `invalid_input` with its message.
14. Gate G2 (live, maintainer): ≥ 90% agreement, ≤ 3% false accepts, p95 ≤ 4 s; in `verify`, the 10
    recorded verdicts replayed through the validators and effects, and the shape of
    `tests/gates/judge-set.jsonl` (300 cases, 100 per language, the field rules above).

## Status

Done (2026-11-11)
