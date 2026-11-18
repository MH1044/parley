# 18: Placement rework

## Goal

The placement test is clear and earns a beginner's trust. It says how far along it is ("Question 4
of 15"), asks about each skill through its words, verbs and phrases in several exercise types it can
grade offline, ends with a score and where to start ("You got 7 of 15. You're around A1. You'll
start at Unit 3: Food and drink."), and lets the learner start earlier. It starts at a whole unit,
and the skills it tests out stay open and come back in reviews, so a wrong guess is caught.

In testing, the test said "Question 1", "Question 2"… with no total, asked fifteen single-word
translations almost all about food and clothes, showed no score, and marked nine skills (Units 1
and 2 and the first skill of Unit 3) as tested out for a learner who knew about twenty words, who
then said they would not trust the rest of the course.

## Depends on

02 (the staircase, `apply_placement`), 04 (the placement screens and question builder, including
its rule that no word is asked twice and that each question puts the cursor in the answer box),
17 (fill the gap with the sentence's English, and its tiles).

## Scope

1. **A known length.** The test asks exactly 15 questions, or stops early after 4 wrong answers in a
   row at the first skill (module 02's kindness to beginners, kept). The screen shows "Question {n}
   of 15" and a progress bar. The staircase's moves and its estimate do not change; its stop rule
   does (Design notes; the maintainer's decision of 2026-11-18).
2. **Questions about the whole skill.** The question for the staircase's skill draws from all its
   items (words, verbs, numbers, phrases), in four types graded offline with no guessing: a typed
   `translate_to_en` of a word, a typed `translate_to_target` of a word, a `fill_gap` on one of the
   skill's sentences with the gap on one of its single-word lemmas (the sentence's English shown,
   module 17), and a `match_pairs` of four of the skill's words. The type rotates with the question
   number, falling back when the skill cannot make it. `pick_meaning` is not used.
3. **No repeated word.** An item is asked at most once in a test (the four words of a
   `match_pairs`, and both the sentence and the gap's word of a `fill_gap`, count as asked); a skill
   with nothing left to ask gets its most recent answer again without a question, as module 04's
   rule already does when a skill runs out.
4. **Start at a whole unit.** The start is the first skill of the estimate's unit; the tested-out
   skills are those of the earlier units.
5. **The result.** "You got {right} of {asked}. You're {label}. You'll start at Unit {n}: {title}."
   (for Pre-A1, without the middle sentence), then **Start at Unit {n}** (focused) and **Start at
   Unit 1**, or **Go to my path** alone when the start is Unit 1.
6. **The choice applies the test.** Nothing is recorded until the learner chooses: **Start at Unit
   {n}** applies module 02's `apply_placement` with the tested-out skills; **Start at Unit 1** tests
   nothing out and still records the estimate and label (Stats shows the level).
7. **Tested-out skills stay testable.** A tested-out skill keeps its page and **Start lesson** (a
   lesson of its words, all already introduced), its cards come back in reviews from about two to
   three days after the test (module 02's Good rating at placement), and module 17 brings them into
   later lessons by retrievability; a missed review lowers its mastery, so the path shows what the
   learner really recalls ("Tested out" stays as a label).

## Files

- `crates/parley-core/src/srs/placement.rs` (stop rule, unit start, right count),
  `crates/parley-core/src/app/placement.rs` (question builder, held result, choice),
  `crates/parley-core/src/api.rs`, `crates/parley-cli/src/cmd/placement.rs` (`placement simulate`
  output)
- `app/src-tauri/src/commands/placement.rs`, `app/src-tauri/src/lib.rs`
- `app/src/screens/Onboarding/Placement.tsx`, `app/src/api/commands.ts`, `app/src/strings/en.json`
- Docs: `docs/modules/02-…` (stop rule and start, with a pointer here), `04-…` (placement UI and
  questions), ARCHITECTURE §5 (`placement_choose`) and §6 (placement), `docs/PRODUCT.md` F2 and
  first run (see the product changes), `README.md`, `docs/HANDBOOK.md`
- Tests: `crates/parley-core/tests/m18_placement_rework.rs`, `tests/cases/18-placement-rework/`
  (`placement simulate` cases), `app/src-tauri/tests/m18_placement_rework.rs`,
  `app/src/test/m18/*.test.tsx`. Module 02's placement golden cases (tests 11 and 12) change with
  the stop rule, and module 04's placement tests with the questions; each named in the commit body.

## Interfaces

Planning level, made exact when the module starts.

- `srs::Placement`: `PLACEMENT_QUESTIONS = 15`; the stop rule below; `SkillRef` gains its unit, so
  `PlacementResult` gains `right: usize` (questions answered right) and `start: usize` (the first
  skill index of the estimate's unit); `tested_out` becomes the skills below `start`.
- `parley placement simulate` prints `right` and `start` beside the existing fields.
- `api::PlacementQuestion` gains `total` (15). `api::PlacementOutcome` gains `right`, `asked`, and
  its `start_skill_id`, `start_unit`, `start_unit_title` follow `start`.
- `placement_answer` no longer applies the result; new command `placement_choose(session_id,
  from_unit_one: bool) -> CourseSummary` applies it (`apply_placement` with the tested-out skills,
  or with none and the estimate recorded). Appended to `COMMANDS`.

## UI

- Heading **Placement test**; "Question {n} of 15"; a `progressbar` named **Placement progress**
  (`aria-valuenow` n, `aria-valuemax` 15); the exercise in its own view (typed answer box, the gap
  with its English, or the two columns of pairs with module 04's keys); **Next** and **I don't
  know**. The cursor goes to the answer box of every question that has one (module 04's rule).
- The result: the sentence of Scope 5, **Start at Unit {n}** (focused) and **Start at Unit 1**, or
  **Go to my path**. Choosing calls `placement_choose`, then shows the path.
- No dialog appears anywhere in the test or its result (I12).

## Messages

"Question {n} of 15"; "Placement progress"; "You got {right} of {asked}. You're {label}. You'll
start at Unit {n}: {title}."; "You got {right} of {asked}. You'll start at Unit {n}: {title}."
(Pre-A1); **Start at Unit {n}**; **Start at Unit 1**; **Go to my path**. Errors of
`placement_choose`: `not_found` "This placement test is no longer available."; `invalid_input`
"Finish the placement test first.".

## Out of scope

- Retaking the placement test later, or a "test out of this unit" button (a later idea; a
  tested-out skill can always be practised).
- Any AI in placement (it stays offline, I3), and any change to how a finished placement reviews
  the tested-out cards (module 02).
- Hearts, energy, lesson limits, ads, paywalls or social features (I8); consent never at launch or
  in onboarding (I12); nothing leaves the machine.

## Design notes

- **Does the staircase change?** Its start (N / 4), steps (halved at each reversal), moves and
  estimate (the mean of the reversals after the first two, else of the last four questions) stay
  exactly as module 02 defines them. Two things change: the stop rule (exactly 15 questions,
  instead of 12 to 20 with 8 reversals, keeping the early exit), so the total can be shown; and the
  start, which rounds down to the first skill of the estimate's unit.
- **Fifteen questions (maintainer decision, 2026-11-18).** The test asks exactly 15 questions, with
  module 02's early exit after 4 wrong answers in a row at the first skill kept. Keeping the
  variable 12 to 20 and showing "Question 4 of up to 20" was not chosen: a learner should know when
  the test ends, and the evidence below shows the fixed length costs about two points of accuracy.
  PRODUCT F2 says 15 questions.
- **Evidence at planning** (20,000 simulated learners per row, boundaries uniform, the module 02
  learner model): with slip 0.1 on 64 skills, the share within ±4 skills of the boundary is 93.6%
  under today's rule and 91.7% with 15 questions (slip 0.05: 98.6% and 97.7%); on the 80-skill mini
  syllabus 92.1% and 89.4%. Today's rule already averages 14.4 questions on 64 skills, so the test
  is no longer for most learners. Rounding the start down to a unit halves the share of learners
  who have a skill they do not know tested out (9.6% to 5.0% on 64 skills, slip 0.1). Module 02's
  200-learner test keeps its 85% floor; PRODUCT §7's 90% target holds for the real 64-skill courses.
- **Why not guessable questions.** A four-option `pick_meaning` in every fourth place raised the
  share of learners placed more than 4 skills too high from 2.5% to 4.8% in the same simulation;
  `match_pairs` is right by chance once in 24 and stays.
- **Why the skill's own items, not the whole unit's.** Drawing each question from any skill of the
  staircase's unit up to its skill kept ±4 accuracy but raised the share with an unknown skill
  tested out from 5% to 12%. Variety therefore comes from the skill's own words, verbs, numbers and
  sentences and from the four types, and a test's questions come from the few units near the
  learner's level by design.
- **Rotation.** Question n prefers, by n mod 4: 1 `translate_to_en`, 2 `fill_gap`, 3
  `translate_to_target`, 0 `match_pairs`; falling back in the order `translate_to_en`, `fill_gap`,
  `translate_to_target`. A phrase or pattern item is asked only through `fill_gap` (a free
  translation of a sentence could come back unchecked, which placement would have to count wrong).
  Answers are graded offline at the settings' accent leniency; correct, almost and typo count as
  right, an empty answer as wrong, as module 04 says. Items and types are drawn from the injected
  `Rng`; the same seed asks the same questions.
- **The choice.** Until the learner chooses, the result is kept in memory with the test, as a
  lesson run is; closing Parley before choosing leaves the course started with nothing tested out,
  the same as **Start at Unit 1** without the recorded level.
- **Worked values** (from the rules, for the golden cases): on `syllabus-mini` (N = 80, 10 skills a
  unit) with a learner right exactly below 23, the questions are at 20, 40, 30, 20, 25, 23, 21, 22,
  23, 22, 23, 22, 23, 22, 23; the reversals at 40, 20, 25, 21, 23, 22, 23, 22, 23, 22, 23; the
  estimate is (2 × 204 + 9) div 18 = 23; 7 answers are right; the start is skill 20 (unit 3).

## Acceptance tests

Golden cases in `tests/cases/18-placement-rework/` through `parley placement simulate`; core tests in
`crates/parley-core/tests/m18_placement_rework.rs`; command tests in
`app/src-tauri/tests/m18_placement_rework.rs` over the frozen seed course with `FailingNet`; UI tests
in `app/src/test/m18/`.

1. `syllabus-mini`, boundary 23: the 15 questions, 11 reversals, estimate 23, `right` 7, `start` 20
   and skills 0 to 19 tested out, label "around A1" (Design notes).
2. Boundary 0: questions at 20, 0, 0, 0, 0; stops after 5; estimate 0; label "Pre-A1"; `right` 0;
   `start` 0.
3. Boundary 80: questions at 20, 40, 60, then 79 twelve times (15 in all); estimate 79; `start` 70;
   label "around A2"; `right` 15.
4. 200 seeded learners with slip 0.1 and boundaries uniform over 0 to 80: at least 85% of estimates
   within ±4 of the boundary; every run asks exactly 15 questions or exits early at index 0.
5. On the seed course with a scripted learner, a test asks at least one `translate_to_en`,
   `fill_gap`, `translate_to_target` and `match_pairs` question, never a `pick_meaning`, and no item
   twice (a `match_pairs` counting its four words); the same seed asks the same questions.
6. A `fill_gap` question built from a skill's sentence grades "almost" (an accent slip) as right
   and an empty answer as wrong; a `match_pairs` question is right only with all four pairs right.
7. `placement_answer` on the last question returns the result and records nothing: the course has
   no tested-out skill and no `cefr_estimate` until `placement_choose`.
8. `placement_choose(.., false)` tests out exactly the skills before the start unit, each card
   reviewed once Good with `exercise_type` "placement" (module 02's test 14 holds);
   `placement_choose(.., true)` tests out none and records the estimate and label; before the end,
   "Finish the placement test first."; an unknown session, "This placement test is no longer
   available."
9. A tested-out skill: `skill_get` offers its page, `lesson_start` gives a lesson of its words
   with no new item, and its cards are due `round(w[2])` days after the test.
10. UI: "Question 4 of 15" with a progress bar at 4 of 15; after **I don't know**, the next
    question's answer box has the cursor.
11. UI: the result reads "You got 7 of 15. You're around A1. You'll start at Unit 3: Food and
    drink." with **Start at Unit 3** focused and **Start at Unit 1**; a Unit 1 start shows **Go to my
    path** alone; choosing calls `placement_choose` and shows the path.
12. With `FailingNet` the whole test and the choice work (I3), and no dialog appears (I12).
13. `COMMANDS` in `api.rs` equals the list in `commands.ts` and every name is registered.

## Status

Planned
