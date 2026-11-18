# 15: Lesson flow

## Goal

A learner always knows what to do next and what a lesson gave them. A skill's first lesson opens on
the page that teaches it; the lesson says which skill it practises and how far along it is; the
lesson end shows the XP earned, the words learned and the words to practise again, and offers the
next lesson as the main button; the daily goal fills and stops when it is reached, with a short
moment the first time each day; a mastered skill is marked. Everything works offline, with no AI.

In testing, a first-time learner pressed **Continue** and was quizzed on words nobody had shown
them (the skill's page, with its can-do line, grammar note and words, was only reachable by
clicking the skill's name), saw "125 of 20 XP today", finished four lessons of a skill with nothing
but a word changing from "Learning" to "Mastered", and was sent back to a long list after every
lesson. This module owns the path and the lesson end, and amends module 04's interface for them.

## Depends on

02 (skill states, mastery), 03 (XP, daily goal, streak, freezes, weekly record, the composer),
04 (the shell, the path, the lesson player and summary, including its rules that an unchecked
answer is not counted as wrong and that the lesson's main button sits in a fixed action bar).

## Scope

1. **The skill page first.** When the skill the learner is sent to has no finished lesson (its next
   lesson is lesson 1), **Continue** on the path, the Next card and **Next lesson** at a lesson end
   open the skill's page (can-do line, grammar note, words with their meanings) with the button
   **Start lesson (about {m} min)**; otherwise they start the lesson directly. Tapping a skill on
   the path opens its page, as today.
2. **Notes.** The lesson header has **Notes**, which opens the skill's page as a dialog over the
   lesson without ending it; in a review or practice session it opens the page of the current
   exercise's skill.
3. **The lesson header.** The skill's name and a progress bar: "Some and any · 4 of 12"; "Review ·
   4 of 20" in a review session; "Practice · 2 of 4" in a practice session.
4. **The lesson end.** A large "+{xp} XP"; "{correct} of {checked} correct" (and "{unchecked} not checked" when any) (with module 04's
   not-checked count); **Words you learned** (the items this lesson introduced); **Words to watch**
   (the items answered wrong) with **Practise these now (about {m} min)**; "1 more lesson to master
   {skill}." when the next lesson is predicted to master the skill (Design notes); the moments
   below; and the buttons **Next lesson** (main, Enter) and **Back to my path** (Esc).
5. **Moments at the lesson end** (in this order, each only when it applies): "Skill mastered:
   {skill}" when this lesson took the skill to Mastered; "Goal reached: {goal} of {goal} XP." (with
   "{n} days in a row." when the streak is 2 or more) the first time today's XP reaches the goal;
   "You earned a streak freeze." when the lesson earned one; "New weekly record!" as module 03
   decides it. Freezes and weekly records keep the rules of modules 03 and 04.
6. **Practice.** `practice_start(item_ids)` starts a short review-kind session over the given items
   (both cards of each, by the composer's kind rules), so **Practise these now** works offline and
   counts as a review session.
7. **The daily goal ring.** On the path, a ring that fills with today's XP and stops full at the
   goal: "12 of 20 XP today", then "20 of 20 · goal reached" however far past the goal the day goes
   (XP beyond the goal still counts in the day, the stats and the weekly record).
8. **XP explained.** The onboarding Daily goal step gets one line under the choices, worked out
   from module 03's XP rules for the chosen goal (Messages; the wording is the maintainer's
   decision of 2026-11-18, Design notes).
9. **The path.** A Next card at the top, "Next: {skill}, lesson {n} of about {m}", with one
   **Continue**; the current skill marked "You are here" and scrolled into view when the path
   opens; each unit labelled with its CEFR level ("Unit 3: Food and drink · A1"); a mastered skill
   shows a badge (an icon with the text "Mastered") in place of the state word; "Tested out" stays.
10. The same moments on the summary of a review, a practice and a story session (the goal moment
    and the freeze, when that session crossed or earned them); "Skill mastered" only after a
    lesson.

## Files

- `crates/parley-core/src/exercise/run.rs` (what the run introduced and missed; the summary
  additions), `crates/parley-core/src/game/{xp.rs,progress.rs}` (new: lessons about, minutes,
  goal crossing, the mastery prediction), `crates/parley-core/src/app/{lesson.rs,course.rs}`
  (path, skill page, practice), `crates/parley-core/src/api.rs` (result types, `COMMANDS`)
- `app/src-tauri/src/commands/lesson.rs` (`practice_start`), `app/src-tauri/src/lib.rs`
- `app/src/screens/Path/*` (Next card, goal ring, unit labels, badge, scroll),
  `app/src/screens/Lesson/*` (header, Notes, summary), `app/src/screens/Onboarding/*` (XP line),
  `app/src/components/{SkillPage,GoalRing,LessonEnd}.tsx` (new), `app/src/screens/Story/Story.tsx`
  (the goal moment on its summary), `app/src/api/commands.ts`, `app/src/strings/en.json`
- Docs: ARCHITECTURE §5 (`practice_start`, the changed results); `docs/modules/04-…` UI contract
  (the parts this module replaces, with a pointer here); `README.md`; `docs/HANDBOOK.md`
- Tests: `crates/parley-core/tests/m15_lesson_flow.rs`,
  `app/src-tauri/tests/m15_lesson_flow.rs` (the commands over the seed course, as module 04's),
  `app/src/test/m15/*.test.tsx`; module 04's UI tests that read the old header, summary or path
  line change with this spec (each change named in the commit body)

## Interfaces

Planning level: which types and commands change, made exact when the module starts.

- `api::PathView` gains `next: Option<NextUp>`, `NextUp { skill_id, skill_title, unit_n, lesson_n,
  lessons_about, first_lesson: bool, minutes }`, for `next_skill_id` (module 04's rule, unchanged).
  `today_xp` and `goal_xp` stay; the ring is drawn from them.
- `api::SkillDetail` gains `lesson_n`, `lessons_about`, `minutes`.
- `api::LessonStarted` gains `skill_title: Option<String>` and, per exercise, the `skill_id` of
  its item (for **Notes** in reviews; not stored in the lesson JSON).
- `exercise::LessonSummary` (and `api`) gains `learned: Vec<WordLine>` (item id, target, first
  English meaning, in the order introduced), `to_watch: Vec<WordLine>` (items with a `wrong`
  verdict in this session, each once, in order), `skill: Option<SkillOutcome { id, title,
  state_before, state_after, mastered_now, one_more_to_master }>` (lessons only),
  `goal_reached_now: bool`, `freeze_earned: bool`, `practice_minutes: Option<u32>`.
- New command `practice_start(item_ids) -> LessonStarted` (kind `review`, origin template, no new
  items), appended to `COMMANDS` and `commands.ts`.
- Pure functions (in `game::progress`), each with unit tests: `lessons_about(items, introduced,
  new_per_lesson, finished) -> u32`, `minutes_for(exercises) -> u32`, `goal_crossed(daily_xp,
  session_xp, goal) -> bool`, `xp_goal_line(goal) -> &'static str` key, and
  `one_more_to_master(store, skill, settings, now) -> bool` (Design notes).

## UI

Visible text in `app/src/strings/en.json`; tests find elements by these names.

- **Path**: the Next card first (a region named **Next**): "Next: {skill}, lesson {n} of about
  {m}" and **Continue**; with no next skill, module 04's "Every skill is practised. Review to keep
  them fresh." and **Review ({due})**. The goal ring beside the streak: a `progressbar` named
  "Today's goal" with `aria-valuenow` min(today, goal) and `aria-valuemax` goal, and the text "{x}
  of {goal} XP today" or "{goal} of {goal} · goal reached". Unit headings "Unit {n}: {title} · {cefr}".
  The current skill has the text "You are here" and is scrolled into view (instantly when the
  system asks for reduced motion). A mastered skill shows the badge (icon plus "Mastered"); a
  tested-out skill keeps "Tested out".
- **Skill page** (module 04's skill detail, now also the first lesson's start): title, can-do line,
  the grammar note under "Notes", the words as "{target}: {meaning}", **Start lesson (about {m}
  min)** (focused), and module 04's way back. As a dialog from **Notes** in a lesson: the same
  content, **Close** (and Esc), focus returned to the lesson; it never ends or pauses grading.
- **Lesson header**: "{skill} · {i} of {n}" (review: "Review · {i} of {n}"; practice: "Practice ·
  {i} of {n}") and a `progressbar` whose accessible name stays "Exercise {i} of {n}"; **Notes**.
- **Lesson end** (heading **Lesson complete**; **Review complete** and **Practice complete** for the
  other kinds): "+{xp} XP" as the largest text; "{correct} of {checked} correct" (and "{unchecked} not checked" when any); the streak line;
  the moments of Scope 5; **Words you learned** as a list (none: the section is left out);
  **Words to watch** as a list with **Practise these now (about {m} min)** (none: left out);
  "1 more lesson to master {skill}." when it applies; **Next lesson** (focused; Enter) and **Back to
  my path** (Esc) in the lesson's fixed action bar, where Continue sat. **Next lesson** follows
  Scope 1 (the skill page first for a new skill). After the practice session, **Next lesson** and
  **Back to my path** as well.
- **Onboarding, Daily goal**: the line for the selected goal under the radio buttons, updated as the
  choice changes.
- Motion: the ring's fill and the moments appear without animation in this module; module 20 adds
  sound and motion to them.

## Messages

| Where | Text |
|---|---|
| Path, Next card | "Next: {skill}, lesson {n} of about {m}"; **Continue** |
| Path, goal ring | "{x} of {goal} XP today"; "{goal} of {goal} · goal reached" |
| Path | "Unit {n}: {title} · {cefr}"; "You are here"; "Mastered" (badge) |
| Skill page | **Start lesson (about {m} min)**; "Notes"; **Close** |
| Lesson header | "{skill} · {i} of {n}"; "Review · {i} of {n}"; "Practice · {i} of {n}"; **Notes** |
| Lesson end | **Lesson complete**, **Review complete**, **Practice complete**; "+{xp} XP"; "{correct} of {checked} correct" (and "{unchecked} not checked" when any); **Words you learned**; **Words to watch**; **Practise these now (about {m} min)**; "1 more lesson to master {skill}."; "Skill mastered: {skill}"; "Goal reached: {goal} of {goal} XP."; "{n} days in a row."; "You earned a streak freeze."; **Next lesson**; **Back to my path** |
| Onboarding | the XP line (below) |
| `practice_start` errors | `invalid_input` "There is nothing to practise." (an empty list); `not_found` "This word is not in the course." (an id that is not an item of the active course) |

The XP line (decided 2026-11-18; from module 03: 1 XP per correct, almost or typo answer, 10 for a
finished lesson, 5 more for a lesson with no wrong answer; a lesson is 12 exercises by default and
took about 2 minutes in testing): the first sentence always, "XP are points: 1 for each right
answer and 10 for each lesson you finish, 5 more with no mistakes."; then by goal: 10 → "10 XP is
one lesson, about 2 minutes."; 20 → "20 XP is about one lesson, 2 to 3 minutes."; 30 → "30 XP is
about two lessons, 4 to 5 minutes."; 50 → "50 XP is about two or three lessons, 5 to 7 minutes."

## Out of scope

- Sound, motion and kinder answer feedback (module 20); exercise variety, tiles and the fill-the-gap
  prompt (17); the placement result (18); written stories (19); the no-voice note and the AI panel
  (16).
- Any change to how XP, streaks, freezes or the weekly record are earned (modules 03 and 04).
- Hearts, energy, lesson limits, ads, paywalls or social features (invariant I8: **Next lesson** and
  **Practise these now** are always offered, however many lessons the day already has); no AI
  entry point or consent at launch or in onboarding (I12); nothing leaves the machine.

## Design notes

- **First lesson.** A skill's next lesson is lesson `n` = 1 + its finished `lesson` sessions
  (module 07's count). `first_lesson` is `n = 1`. A learner who quits lesson 1 early still sees the
  page next time, which is the point: they have not yet finished being taught.
- **About m lessons.** `lessons_about = finished + max(1, ceil(unintroduced / new_items_per_lesson))`
  with the items not retired; it only changes as items are introduced, so it never jumps backwards
  within a skill except when the setting changes. French "Some and any" (9 words, 8 sentences, 5
  new items per lesson) gives "lesson 1 of about 4", which matches the four lessons it took to
  master in testing.
- **Minutes.** `minutes_for(n) = max(1, ceil(n / 6))`: about 10 seconds an exercise, as measured
  in testing (11 to 12 exercises in 1.5 to 2.2 minutes); 12 exercises → "about 2 min".
- **1 more lesson to master.** Shown only when it is known in this sense: the composer's plan for
  the skill's next lesson (deterministic, composed at the lesson end from the store) together with
  the cards already reviewed would make the skill Mastered by module 02's rule if every planned
  answer were right (each planned card taken at retrievability 1, every other card at its
  retrievability now). Otherwise nothing is said: no guess is shown as a promise.
- **Words.** `learned` lists the items whose cards this session created (`introduce` returned true),
  with the first English meaning; a phrase item shows its sentence. `to_watch` lists items with a
  `wrong` verdict; `almost`, `typo` and `unchecked` are not missed words. A `match_pairs` exercise
  answered wrong lists its own item.
- **Practice.** `practice_start` makes one plan entry per (item, facet), receptive first, with the
  composer's kind rules, at most 10 entries (so at most 5 words with both cards), then
  `template_lesson` with the context of the latest skill among them, as module 04's reviews do. It
  is a `review` session: 5 XP for finishing and 1 per right answer, cards reviewed as usual (a
  same-day review takes FSRS's short-term formula, module 02).
- **Goal crossing.** `goal_reached_now = daily_xp − session_xp < goal_xp ≤ daily_xp`, on the
  summary's own figures, so it is true for exactly one session a day (the goal setting at that
  moment counts). The ring and the path line use `min(today_xp, goal_xp)`.
- **Freeze earned.** `freeze_earned` is true when `finish` appended an `earned` event in this call
  (module 03 already appends only the events not yet stored); nothing about earning changes (I7).
- **Mastered now.** The skill's state is read before the run starts and after `finish`;
  `mastered_now` is `state_before ≠ mastered` and `state_after = mastered`.
- **Enter at the lesson end** starts the next lesson, the main button; Esc goes back to the path. A
  lesson end is not a history step (module 04), and the skill page opened from **Notes** is a
  dialog, so module 04's dialog rules apply (Esc closes it first).
- **XP line (maintainer decision, 2026-11-18).** The goal step's line is worked out from module
  03's XP rules, which do not change: a 12-exercise lesson earns 10 XP whatever happens and 20 to
  27 XP with ten or more right answers, and took about 2 minutes in testing. So the line reads "20
  XP is about one lesson, 2 to 3 minutes." for the default goal, with the 10, 30 and 50 XP lines
  under Messages. The earlier wording "20 XP is about two lessons, or 4 to 5 minutes" was not used,
  since it does not match those rules.

## Acceptance tests

Core tests in `crates/parley-core/tests/m15_lesson_flow.rs` (in-memory store, fixed clock, seeded
`SplitMix64`); command tests in `app/src-tauri/tests/m15_lesson_flow.rs` over the frozen seed course
with `FailingNet`; UI tests in `app/src/test/m15/` with `@tauri-apps/api/core` mocked. Expected
values come from this spec and modules 02 and 03.

1. `lessons_about`: 17 items, none introduced, 5 a lesson, 0 finished → 4; after a lesson that
   introduced 5 → 4 (1 + 3); every item introduced and 4 finished → 5 (4 + 1). `minutes_for(12)` →
   2, `minutes_for(6)` → 1, `minutes_for(20)` → 4.
2. `path_get` of a new course → `next` names skill 1, `lesson_n` 1, `first_lesson` true; after one
   finished lesson of it, `lesson_n` 2 and `first_lesson` false.
3. UI: Continue with `first_lesson` true opens the skill page with "Start lesson (about 2 min)",
   whose button calls `lesson_start` in learn mode; with `first_lesson` false, Continue calls
   `lesson_start` at once.
4. UI: the lesson header reads "Greetings · 1 of 12" with a progress bar named "Exercise 1 of 12";
   in a review session "Review · 1 of 20".
5. UI: **Notes** opens a dialog with the skill's can-do line and grammar note; Esc closes it and the
   same exercise is still shown with its answer box focused.
6. The summary of a lesson that introduced 5 items lists them in `learned` in order; an item answered
   wrong is in `to_watch` once even when answered wrong twice; an `almost`, a `typo` and an
   `unchecked` answer are not in it.
7. `practice_start` with two missed items → 4 exercises, kind `review`; finishing it gives 5 + the
   correct answers in XP; an empty list → "There is nothing to practise."; an unknown id → "This
   word is not in the course."
8. Goal crossing: goal 20, a day at 15 XP, a session of 12 → `goal_reached_now` true; the next
   session of the day → false; a first session of 25 → true; a session that ends at 19 → false.
9. UI: the path ring reads "12 of 20 XP today" at 12 and "20 of 20 · goal reached" at 20 and at
   125, with `aria-valuenow` 20 for both.
10. UI: the summary with `goal_reached_now` and streak 3 shows "Goal reached: 20 of 20 XP." and "3
    days in a row."; with streak 1 the second sentence is absent.
11. Mastered now: a skill one review short of Mastered becomes Mastered at `finish` →
    `mastered_now` true and "Skill mastered: {skill}" shown; a skill already Mastered → false.
12. Freeze earned: a seventh active day finished by this lesson → `freeze_earned` true and "You
    earned a streak freeze." shown; the eighth → false; `freeze_events` holds exactly the events
    module 03 writes (I7).
13. One more to master: on the seed course, a skill whose remaining items fit in the next lesson
    and whose reviewed cards are fresh → true and the line shown; a skill with more items left
    than the next lesson introduces → false and no line.
14. UI: Enter on the lesson end starts the next lesson (the skill page first when its
    `first_lesson` is true); Esc returns to the path; **Practise these now** calls
    `practice_start` with the `to_watch` ids.
15. UI: unit headings read "Unit 3: Food and drink · A1"; the next skill shows "You are here" and
    `scrollIntoView` was called on it; a mastered skill shows the badge with the text "Mastered".
16. UI: the Daily goal step shows "XP are points: 1 for each right answer and 10 for each lesson you
    finish, 5 more with no mistakes." and "20 XP is about one lesson, 2 to 3 minutes." by default,
    and "50 XP is about two or three lessons, 5 to 7 minutes." after choosing 50 XP (10 and 30 XP
    likewise); no dialog appears in onboarding (I12).
17. With `FailingNet`, a first lesson from the skill page, its summary, a practice session and the
    path's figures all work (I3); 100 lessons in a day are all offered **Next lesson** (I8).
18. `COMMANDS` in `api.rs` equals the list in `commands.ts` and every name is registered.

## Status

Planned
