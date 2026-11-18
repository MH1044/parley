# 17: Variety and recall

## Goal

Lessons feel varied and ask for recall, not copying. Exercise types are interleaved, never more than
three of one type in a row; a word the lesson does not introduce is the focus of at most two
exercises (a new word keeps module 03's three); once a skill's words are all introduced, its lessons bring back the weakest words of earlier skills too. Fill the
gap shows what the whole sentence means, not the missing word, with the first letter as a hint on
request. Every typed exercise can be answered with tiles, switched in the lesson and remembered,
so a mouse or touch learner can do every exercise.

In testing, a lesson had five translations into English and then five into French in a row; the
same six words of one skill came round in four lessons; Fill the gap showed the English of the
missing word in brackets, so it was copying; and typed exercises could not be done with the mouse
at all.

## Depends on

01 (examples and their English, the forms table), 02 (retrievability, the rating table), 03 (the
composer, the template generator, word-bank tiles, `used_hint`), 04 (the lesson player), 07 (the
plan sent to fresh lessons), 13 (Planned: the Android word-bank default, which this module keeps).

## Scope

1. **Interleaved types.** The composer alternates the exercise type within each class of entry
   (Design notes) and then reorders so that no more than three exercises of the same type are in a
   row; the template lesson applies the same rule after its fallbacks. An item's first exercise in a
   lesson always comes before its others.
2. **A word at most twice.** An item the lesson introduces keeps the appearances module 03's
   composer gives it (its intro, its productive exercise, and for the first new vocabulary item
   the `match_pairs` of step 2, which stays: at most three). Every other item is the focus of at
   most two exercises of the lesson. A `match_pairs` exercise counts once, for its own item (its
   partners are the lesson's words shown together). A plan the cap leaves short of `lesson_length`
   is shorter, never padded with a third appearance of an item it did not introduce.
3. **Earlier skills come back.** When every item of the skill is introduced, the composer's fill
   step alternates between this skill's cards and the cards of every earlier skill of the course
   (tested-out skills included), each by retrievability ascending, instead of this skill's alone.
   Due cards keep module 03's place in the plan.
4. **Fill the gap asks for recall.** The exercise shows the English of the whole sentence
   (`sentence_en`) and no longer the missing word's meaning; **Show the first letter** reveals the
   gap's first letter on request and marks the answer `used_hint`, which module 02's rating table
   already rates Hard when right (unchanged). Template exercises take the sentence's English from
   the syllabus example or the pack sentence they use. A fresh (AI-written) fill-the-gap exercise,
   which has no sentence English, offers the missing word's meaning (`hint_en`) as its first hint
   on request and the first letter as its second, both marking `used_hint`.
5. **Tiles for every typed exercise.** `translate_to_target`, `translate_to_en`, `fill_gap` and
   `listen_type` all carry tiles (rules in Design notes); a tiles answer is graded as the text of the
   tiles, exactly like a typed one.
6. **Type or Tiles, in the lesson.** Every typed exercise has a switch **Type** / **Tiles**; the
   choice is saved in `answer_input` (`typing` or `word_bank`) and used from then on. Desktop keeps
   `typing` as the first-run default and Android `word_bank` (module 13).
7. **Keyboard.** With tiles, keys 1 to 9 add the tile with that number, Backspace removes the last
   tile added, Enter checks; with typing, the box has the focus as today.

## Files

- `crates/parley-core/src/exercise/{compose.rs,template.rs,wordbank.rs,types.rs}`,
  `schemas/lesson.schema.json` (`sentence_en`, `tiles` on the four types),
  `crates/parley-core/src/ai/lesson.rs` (tiles rebuilt for AI exercises, as module 07 already does
  for `translate_to_target`)
- `app/src/components/{WordBank,FillGap,AnswerInput}.tsx`, `app/src/screens/Lesson/*`,
  `app/src/api/commands.ts`, `app/src/strings/en.json`
- Docs: `docs/modules/03-…` (composer and template rules, with a pointer here), `04-…` (lesson
  player), ARCHITECTURE §3 if the lesson description changes, `README.md`, `docs/HANDBOOK.md`
- Tests: `crates/parley-core/tests/m17_variety_and_recall.rs`, `tests/cases/17-variety-and-recall/`
  (composer fixtures and a `parley lesson template` case), `app/src/test/m17/*.test.tsx`. Expected
  results that change elsewhere, each named in the commit body: module 03's composer fixtures and
  `lesson template` cases, module 07's `render-es-a1` request body and the plans of
  `tests/fixtures/ai/P1/recorded/` (the replay's L3 compares types with the plan), and module 04's
  fill-the-gap UI test.

## Interfaces

Planning level, made exact when the module starts.

- `exercise::Body::FillGap` gains `sentence_en: Option<String>` (serde default, left out when
  None); `TranslateToEn`, `FillGap` and `ListenType` gain `tiles: Option<Vec<String>>` as
  `TranslateToTarget` has. Stored lessons without them still load.
- `exercise::compose` keeps its signature; its rules change (Scope 1 to 3). `template_lesson`
  applies the run rule after fallbacks.
- `word_bank` stays for target-language tiles; new `word_bank_en(answer, pool, rng)` and
  `gap_tiles(answer, lemma, allowed, forms, rng)`.
- No new command. `exercise_submit` already carries `used_hint`; the interface now sends true when
  the first letter was shown. `settings_set` with `answer_input` already exists.

## UI

- **Fill the gap**: the sentence with its gap, "In English: {sentence_en}", **Show the first
  letter** (then "Starts with {letter}" and the button disabled); with tiles, the tiles under the
  sentence, a chosen tile shown in the gap and replaced by another. Without `sentence_en` (a fresh
  lesson's exercise): no English line, **Show a hint** (then "Hint: {hint_en}" and the button
  becomes **Show the first letter**).
- **Type / Tiles**: a two-button group named **Answer with** above the answer of every typed
  exercise, the current one pressed (`aria-pressed`); switching clears the answer and saves the
  setting. Tiles are numbered buttons; chosen tiles form the answer line in order, each removable by
  a press.
- The footer hint names the keys of the exercise shown: tiles "1 to 9 add a tile, Backspace removes
  one, Enter checks".

## Messages

"In English: {sentence}"; **Show the first letter**; "Starts with {letter}"; **Show a hint**;
"Hint: {meaning}"; **Answer with**;
**Type**; **Tiles**; "1 to 9 add a tile, Backspace removes one, Enter checks"; "Choose tiles
first." (Check with no tile chosen, shown without calling the command, as module 04's "Type an
answer first.").

## Out of scope

- New exercise types (listening and speaking come from 09 and 10), a second try after a wrong
  answer (one answer per exercise stays), and any change to grading verdicts or the rating table.
- Changing the fresh-lesson prompt. A later lesson prompt version (a new file `lesson.v2.md` with
  its schema, the live gate G1 re-run and new recorded fixtures) is to ask for the sentence's
  English for fill-the-gap exercises; that is later work, not this module.
- Hearts, energy, lesson limits, ads, paywalls or social features (I8); consent never at launch or
  in onboarding (I12); nothing leaves the machine except as modules 06 to 10 allow.

## Design notes

- **Classes and alternation.** The composer's kind rules of module 03 become a rotation per class,
  in plan order: receptive vocabulary, `pick_meaning` then `translate_to_en` (an item's first
  exercise of the lesson is always `pick_meaning`, the gentlest); productive vocabulary, `fill_gap`
  then `translate_to_target` (multiword lemmas always `translate_to_target`); receptive phrase or
  pattern, `translate_to_en` then `pick_meaning` (module 03's template makes `pick_meaning` for any
  item kind); productive phrase or pattern, `translate_to_target`. `listen_type` and `speak` come
  from capabilities as before. Then the run rule: walking the plan, an entry that would be the
  fourth of a type in a row has the nearest later entry of another type moved in front of it, when
  that move keeps every item's first exercise first and every `match_pairs` partner already met
  earlier in the lesson; when no such entry exists the run stays (a lesson whose remaining entries
  are all of one type). The same walk runs over the template lesson's exercises,
  since a fallback (`fill_gap` → `translate_to_target`) can make a new run.
- **The cap (maintainer decision, 2026-11-18).** A word the lesson introduces keeps its
  appearances: module 03's step 2 stays, so the first new vocabulary item has its intro, the
  `match_pairs` and its productive exercise, and every other new item its intro and productive
  exercise. Every other word (a due card's, or one the fill brings back) is the focus of at most two
  exercises. A first lesson with five new words and nothing to review therefore stays at eleven
  exercises (five intros, one `match_pairs`, five productive), as in testing; capping new words at
  two as well would have cut it to ten by moving the first word's productive exercise to the next
  lesson, which was not chosen. The repetition seen in testing came from old words, which is what
  the cap and the earlier-skills fill address. A lesson is never padded by repeating an old word a
  third time.
- **Earlier skills.** The fill alternates: this skill's card with the lowest retrievability, then
  the earlier skills' card with the lowest, and so on, skipping (item, facet) pairs already planned
  and items at the cap; ties as module 03 (syllabus position, receptive first). Tested-out skills'
  cards count, so a placement's guess is checked inside lessons as well as in reviews.
- **`sentence_en`.** For a syllabus example, its first `en`; for a pack sentence, its first English
  translation (by `en_id`, as module 01 stores them). `hint_en` stays in the exercise (fresh lessons
  and course packs carry it) but the template's is no longer shown.
- **Fill the gap in fresh lessons (maintainer decision, 2026-11-18).** Lesson prompt v1 asks for
  `hint_en` (the missing word's meaning) and not the sentence's English. Until a later lesson prompt
  version adds the sentence's English, a fresh fill-the-gap exercise shows no English line and
  offers the word's meaning as its first hint on request, then the first letter; either marks
  `used_hint`. This module changes no prompt file and needs no live gate. The later version (a new
  file `lesson.v2.md` asking for `full_sentence_en`, its schema, the live G1 re-run and new recorded
  fixtures) is later work, outside this module.
- **Tiles.** Target-language tiles are module 03's (`translate_to_target`, now also `listen_type`).
  `translate_to_en`: the words of `normalise(accepted[0])` (an apostrophe between letters keeps
  "don't" one tile) plus up to 3 distractors drawn without replacement from the single words of the
  first English meanings of the items of the allowed skills, lower case, none equal to a word of any
  accepted answer; all shuffled with the lesson's seed. `fill_gap`: the answer plus up to 3 other
  forms of its lemma from the forms table that are allowed and differ in normal form (so a verb
  offers its other persons), then allowed single-word lemmas of the same part of speech; shuffled.
  Any accepted answer built from tiles is correct, since the tiles answer is graded as text.
- **Rating of a tiles answer (maintainer decision, 2026-11-18).** No hint penalty: a tiles answer
  is rated exactly as the same typed answer would be (module 02's table, unchanged, has no row for
  tiles). Rating it as with a hint would shorten every interval of a learner who uses tiles,
  Android's default; FSRS corrects an easy card through its later reviews.
- **AI lessons.** The plan sent to fresh lessons carries the new types, so fresh lessons follow the
  same variety; L3 still compares each exercise with its plan entry. Invariant I11 is unchanged:
  every exercise still refers to a planned item.
- Gates: the template generator and the composer change, so `scripts/gates/run` (G3 over the real
  syllabi) is re-run; live G1 is the maintainer's.

## Acceptance tests

Core tests in `crates/parley-core/tests/m17_variety_and_recall.rs` and golden cases in
`tests/cases/17-variety-and-recall/` (plans worked out by hand from the rules above; a fixture is
module 03's composer fixture shape); UI tests in `app/src/test/m17/`.

1. Composer fixture `variety-a` (5 new vocabulary items of the course's first skill, no dues): the
   plan worked out by hand in `variety-a.expected.json`, 11 entries: five `pick_meaning` intros
   (the run rule breaks them after the third), one `match_pairs` for the first item, five
   productive entries alternating `fill_gap` and `translate_to_target`; no run of more than three
   of one type; the first item is the focus of three entries and every other of two.
2. Composer fixture `variety-b` (a skill with every item introduced, an earlier skill with a card of
   retrievability 0.4 and this skill's lowest at 0.7): the fill plans the earlier card first, then
   this skill's, alternating; no item of the plan is the focus of more than two entries, since the
   lesson introduces none.
3. Composer fixture `variety-c` (a phrase-heavy skill whose old plan had five `translate_to_en` in a
   row): the plan has no run of more than three of one type.
4. For every skill of `content/{es,fr,de}` and seeds 1 to 50, five lessons played in a simulated
   store (each lesson's answers all right, one day apart): every template lesson has no run of more
   than three exercises of one type unless all its remaining exercises share one type; no item the
   lesson introduces is the focus of more than three exercises and no other item of more than two;
   every exercise passes `check_exercise` (G3's rule).
5. `sentence_en` of a template `fill_gap` on fixture example "Je mange de la viande." is "I eat
   meat."; on a pack sentence, its first translation; `schemas/lesson.schema.json` validates both,
   and a stored lesson without the field still loads and plays.
6. UI: a `fill_gap` shows "In English: I eat meat." and not the word "meat" alone; **Show the first
   letter** shows "Starts with v" and the submit carries `used_hint: true`; without it, false.
7. A right answer with `used_hint` is rated Hard (module 02's table, unchanged); a right tiles
   answer without a hint gets the same rating as the same answer typed (Good, or Easy where module
   02's table gives it).
8. `word_bank_en` for accepted "I eat meat." → tiles `i`, `eat`, `meat` and 3 distractors, none a
   word of any accepted answer; the same seed gives the same tiles; the tiles in order grade
   `correct`, and tiles forming another accepted answer grade `correct`.
9. `gap_tiles` for the answer `mange` (lemma `manger`) → `mange` and up to 3 other allowed forms of
   `manger`, all with distinct normal forms; choosing `mange` grades `correct`.
10. `listen_type` and `translate_to_en` template exercises carry tiles; AI exercises get rebuilt
    tiles; the lesson schema validates them.
11. UI: **Tiles** on a `translate_to_en` calls `settings_set` with `{"answer_input": "word_bank"}`
    and the next typed exercise opens with tiles; **Type** saves `typing`; keys 1, 3 add tiles 1
    and 3, Backspace removes tile 3, Enter checks with the tiles' text; Check with no tile shows
    "Choose tiles first." and calls nothing.
12. Desktop first-run settings keep `answer_input` `typing` (module 13's Android test keeps
    `word_bank`).
13. UI: a fresh-lesson `fill_gap` without `sentence_en` shows no English line; **Show a hint** shows
    "Hint: meat" (its `hint_en`), then **Show the first letter** shows "Starts with v", and the
    submit carries `used_hint: true`; `prompts/lesson.v1.md` is the only lesson prompt file.
14. Module 07's offline replay of the recorded P1 fixtures passes with the new plans (L3), and the
    `render-es-a1` request body matches its updated expected file.
15. With `FailingNet`, lessons, reviews and practice keep working (I3).

## Status

Planned
