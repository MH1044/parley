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
- In `exercise_submit`: an `unchecked` grade goes to the judge when a provider is ready and
  `needs_credit` is not set; the judgement is looked up first; the verdict becomes the grade.
- Effects of a verdict (§6.5): rating, `judgements` row, `accepted_answers` row for a correct
  answer to the item's own prompt.
- Without a ready provider: the result sheet offers **Check with AI** (opens the AI setup dialog
  when no provider is chosen), **I was right** and **Continue**.
- A 402 during a lesson stops judging for the rest of that lesson.
- `explain_answer` command, offered only after `almost` or `wrong`, cached.
- Result sheet states: "Checking…", the verdict, "Not checked." with the reason of §9.
- Gate G2 and its labelled set `tests/gates/judge-set.jsonl` (300 project-written cases, 100 per
  language, each with source, accepted answers, the answer and the expected verdict).

## Files

- `crates/parley-core/src/ai/{judge.rs,explain.rs}`, `prompts/{judge,explain}.v1.md`,
  `schemas/ai/{judge,explain}.v1.schema.json`
- `app/src/components/ResultSheet.tsx` (states), `app/src-tauri/src/commands/lesson.rs` (judge path,
  `explain_answer`)
- `tests/fixtures/ai/P3/*.json`, `tests/fixtures/ai/P4/*.json`, `tests/gates/judge-set.jsonl`
- `crates/parley-core/tests/m08_judging_and_explanations.rs`

## Interfaces

```rust
pub fn judge(client: &dyn AiClient, cache: &Cache, store: &mut Store, req: &JudgeRequest)
    -> Result<Judgement, AiError>;  // JudgeRequest { lang, direction, source, accepted, answer, cefr, item_id, own_prompt }
pub fn explain(client: &dyn AiClient, cache: &Cache, req: &ExplainRequest) -> Result<Explanation, AiError>;
pub fn judgement_key(lang: Lang, direction: Direction, source: &str, answer_norm: &str) -> String; // sha256 hex
```

Command `explain_answer(session_id, exercise_id) -> { text, tip: Option<String>, cached: bool }`;
errors: `not_found` "This exercise is no longer available.", `invalid_input` "Explanations are
available after an answer that was almost right or wrong.", and the AI errors of module 06.

Messages: "Checking…"; "Not checked." followed by the reason line of PUBLIK-INTEGRATION §9 for
402, disconnected, 429, 503 and offline ("You're offline, so Parley can't check free answers.");
"publik API did not answer in time." with **Try again**; "The AI answer could not be used." with
**Try again** and **I was right**; "Explanations need a connection or a local model." (offline).

## Out of scope

- Role-play corrections (11); explanations of speaking results (10 shows word-level feedback only).

## Design notes

- The judgement key (ARCHITECTURE `judgements.key`) is provider-independent: a verdict is learning
  data, while the response cache (module 06) stays per provider.
- The learner's answer can contain text aimed at the judge. It is sent as a JSON string, and the
  worst outcome is a learner fooling their own grading, so no further defence is added.
- Decisions are filled in here during implementation.

## Acceptance tests

Golden cases in `tests/cases/08-judging-and-explanations/`; code tests in
`crates/parley-core/tests/m08_judging_and_explanations.rs`; UI in
`app/src/components/ResultSheet.test.tsx`. Contract tests 1 to 18 (module 06) keep passing; this
module adds tests 24 to 27 of `docs/PUBLIK-INTEGRATION.md` §11.

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
8. A 402 on the first judged answer shows "Not checked.", the message and one link; a second
   unmatched answer in the same lesson makes no request (count stays 1); the next lesson judges again
   only after needs-credit clears.
9. With `FailingNet`, the sheet shows "You're offline, so Parley can't check free answers." and
   **I was right**; no request.
10. With `ai.provider = unset`, **Check with AI** opens the AI setup dialog; **Not now** leaves the
    answer unchecked (invariant I12).
11. A judge call that takes more than 10 s → "publik API did not answer in time." and **Try again**;
    exactly one request until the learner taps it.
12. Explain, valid fixture → the text and tip shown; E2 (81 words) and E3 (French text) → "The AI
    answer could not be used."; a second tap → `cached: true` and no request.
13. `explain_answer` after a correct answer → `invalid_input` with its message.
14. Gate G2 (live, maintainer): ≥ 90% agreement, ≤ 3% false accepts, p95 ≤ 4 s; in `verify`, 10
    recorded verdicts replayed through the validators and effects.

## Status

Planned
