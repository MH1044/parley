# 07: Lesson generation

## Goal

With a provider set up, every lesson is written fresh for the learner's skill and known words, and
every unit ends with a short graded story. Nothing reaches the learner before it passes the
validators; anything that fails is replaced from the course pack. Learners can export the lessons
they have as a course pack and import one from someone else, checked the same way.

## Depends on

03, 06. Feature-map rows **P1** (fresh lesson) and **P2** (graded story) of
`docs/PUBLIK-INTEGRATION.md`; its §6.3 and §6.4 are the specification of both calls.

## Scope

- `prompts/lesson.v1.md` and `prompts/story.v1.md` with the exact text of PUBLIK-INTEGRATION §6.3
  and §6.4; schemas `schemas/ai/lesson.v1.schema.json` and `schemas/ai/story.v1.schema.json`.
- `ai::lesson::generate`: input from the composer's plan (module 03), the call through `ai::call`,
  validators L1 to L9, template fill, the notice, the cache rule, storage with `origin`.
- Language identification with `whatlang` (L4) and the Snowball stem fallback with `rust-stemmers`
  (L5).
- `lesson_start` with `ai.fresh_lessons` on and a ready provider; the loading state and **Use the
  course pack instead** after 5 s.
- Setting `ai.prepare_next_lesson` (default on): after a lesson finishes, one P1 call for the next
  lesson of the same skill, never while `needs_credit`.
- Stories: the Story node after the last skill of a unit is practised, `lesson_start(mode: story)`,
  the story screen, question grading, XP.
- Course packs `parley.coursepack/1`: `course_pack_export`, `course_pack_import`, the report.
- Settings > AI > Local model shows the validity rate of the last lessons.
- Gate G1 in `crates/parley-core/tests/gates.rs`, run by `scripts/gates/run --live G1`.

## Files

- `crates/parley-core/src/ai/{lesson.rs,story.rs,validate.rs,langid.rs}`, `crates/parley-core/src/coursepack.rs`
- `prompts/{lesson,story}.v1.md`, `schemas/ai/{lesson,story}.v1.schema.json`, `schemas/coursepack.schema.json`
- `app/src/screens/Story/*.tsx`, `app/src/screens/Lesson/Loading.tsx`, Settings and Data sections
- `tests/fixtures/ai/P1/*.json`, `tests/fixtures/ai/P2/*.json`, `tests/fixtures/coursepack/*.json`
- `crates/parley-core/tests/m07_lesson_generation.rs`

## Interfaces

```rust
pub fn generate_lesson(client: &dyn AiClient, cache: &Cache, plan: &Plan, ctx: &LessonCtx)
    -> Result<LessonDraft, AiError>;   // LessonDraft { exercises, dropped: Vec<(usize, RuleFailure)>, notice }
pub fn generate_story(client: &dyn AiClient, cache: &Cache, ctx: &StoryCtx) -> Result<Story, AiError>;
pub fn export_course_pack(store: &Store, lang: Lang, dest: &Path) -> Result<ExportSummary, PackError>;
pub fn import_course_pack(store: &mut Store, src: &Path, ctx: &CheckCtx) -> Result<ImportReport, PackError>;
```

Course pack (arrays shortened):

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

Messages: "A fresh lesson was not available, so this one comes from the course pack."; "You're
offline, so this lesson comes from the course pack." (a provider is set but unreachable); "Writing a
fresh lesson…"; **Use the course pack instead**; "The AI answer could not be used." with **Try
again** (stories); "This is not a Parley course pack."; "This course pack is for {French}; your
active course is {Spanish}."; "This course pack is larger than 5 MB."; "Imported {n} lessons and {m}
stories; {k} exercises were dropped because they did not pass Parley's checks."; "Local model:
{p}% of AI exercises passed Parley's checks (last {n} lessons)."

## Out of scope

- Listening and speaking exercises inside AI lessons (made by the core in 09 and 10 from validated
  sentences); judging free answers (08).

## Design notes

- Imported course packs are untrusted input: every exercise goes through the schema and L2 to L9
  against the local syllabus and forms; exercises whose `item_id` is not in the local syllabus are
  dropped. Imported lessons get the next `seq` numbers of their skill.
- A response that arrives after the learner chose the course pack is cached, not shown.
- Record the `whatlang` confidence threshold chosen from the fixtures (starting value 0.5).
- Decisions are filled in here during implementation.

## Acceptance tests

Golden cases in `tests/cases/07-lesson-generation/`; code tests in
`crates/parley-core/tests/m07_lesson_generation.rs`. Responses come from `tests/fixtures/ai/P1/` and
`P2/`, each written to break exactly one rule. Contract tests 1 to 18 (module 06) keep passing; this
module adds tests 21 to 23 of `docs/PUBLIK-INTEGRATION.md` §11.

1. The plan fixture renders exactly `render-es-a1/expected_request.json` (placeholders of §6.3).
2. The valid fixture → 5 exercises with `origin: "ai"`, none dropped.
3. L1: a non-JSON reply → a template lesson with the notice; nothing cached.
4. L2: 7 exercises for a 5-entry plan → the last 2 dropped, 5 shown.
5. L3: `item_id` wrong at position 2 → exercise 2 replaced by a template exercise (`origin: "template"`).
6. L4: an English sentence `I want water please now` as the Spanish target → dropped.
7. L5: `Quiero agua con hielo, por favor.` with `hielo` not allowed → 5 of 6 words covered (83%) →
   dropped; a 10-word sentence with one uncovered word (90%) → kept.
8. L6: `target` not in `accepted`; `fill_gap` with two gaps; a two-word `answer`; `full_sentence`
   mismatch; duplicate `pick_meaning` options; duplicate `match_pairs` sides → each dropped.
9. L7: two exercises with the same normalised target → the second dropped.
10. L8: a 9-word target at A1 (limit 8) → dropped.
11. L9: an exercise not containing its focus lemma in any form → dropped.
12. 3 of 5 exercises dropped → the whole lesson from templates with the notice.
13. Cache: a schema-valid response is stored; the same `seq` again makes no request; `seq` + 1 does.
14. With `needs_credit` set, `lesson_start` makes no call and returns a template lesson.
15. **Use the course pack instead** after 5 s shows a template lesson; the fake's delayed response is
    cached and not shown.
16. Prepare next lesson: exactly one P1 call for `seq + 1` after `lesson_finish`; none with the setting
    off or `needs_credit` set.
17. Story: the valid fixture → a story session; each of S1 to S6 → "The AI answer could not be
    used."; 3 correct answers of 3 → 16 XP; cards unchanged.
18. Course pack: export then import into fresh data → the same exercises with `origin:
    "course_pack"`; a tampered exercise with an unknown word → dropped and counted; a French pack in
    a Spanish course → the language message; `{"schema": "x"}` → "This is not a Parley course pack.";
    a 6 MB file → the size message.
19. Validity rate: three local lessons with 12, 9 and 6 valid of 12 → "Local model: 75% of AI
    exercises passed Parley's checks (last 3 lessons)."
20. Gate G1 (live, maintainer): ≥ 80% of exercises valid and p95 ≤ 20 s; in `verify`, the recorded
    responses in `tests/fixtures/ai/P1/recorded/` replayed through the validators.

## Status

Planned
