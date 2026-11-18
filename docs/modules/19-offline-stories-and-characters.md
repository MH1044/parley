# 19: Offline stories and characters

## Goal

Every unit of every course ends with a short written story that needs no AI: 16 for Spanish, 16 for
French and 16 for German, using only the words taught up to that unit plus a few glossed new ones,
with every word translatable by a tap and three or four questions at the end. Each course has a
recurring person and place, introduced by a one-line banner on each unit and followed through its
stories, so the course has a world to come back to. AI stories stay as an extra, **Fresh story
(AI)**.

In testing, **Read a story** opened the AI setup every time, so none of the 16 stories could be
read without spending money, and nothing in the course had a person or a place: the two things
that most feel like "using French" were out of reach on the free path.

## Depends on

01 (syllabus, tokeniser, forms table), 03 (coverage, `Allowed`), 05 (the courses, `review.yaml` and
the decision that unreviewed content ships as a labelled beta), 07 (the story screen, question
grading, story XP, the AI story), 15 (the lesson end's moments on a story's summary), 16 (the AI
mark and the short panel on **Fresh story (AI)**).

## Scope

1. **Content files.** `content/<lang>/stories.yaml` for es, fr and de, with its JSON Schema
   `schemas/stories.schema.json`: the course's cast, the English glosses of the function words its
   stories use, and one story per unit (banner, title, English title, paragraphs with their English,
   glossed new words, questions, review status and date). Original text, no real people or
   brands, under the course content's licence (CC BY-SA 4.0, ADR-0003).
2. **Validation.** Rules ST01 to ST11 (Interfaces) in `parley content validate`, run when
   `stories.yaml` sits beside the syllabus, so `scripts/verify` checks the real stories; fixtures for
   every rule.
3. **Writing the stories.** 48 stories, one per unit: A1 in the present, A2 also with the past and
   near-future forms its units teach; each uses only words taught up to its unit, plus at most 4
   (A1) or 6 (A2) new words, each glossed.
4. **The cast.** One pair of people and one city per course, followed through its 16 stories:
   Lucía and Pablo in Sevilla (es), Camille and Hugo in Lyon (fr), Anna and Tom in Hamburg (de),
   with invented streets and businesses (Design notes). French adds Camille and Hugo to its
   syllabus's `names`; the Spanish and German names and all three cities are there already.
5. **Unit banners.** Each unit on the path shows its banner, one English sentence or two about the
   person and the unit's theme ("Unit 3: Food and drink · A1 — Camille runs a small café in Lyon.
   Today a new customer comes in.").
6. **Reading.** **Read a story** plays the unit's written story with no AI, no consent and no cost:
   every word is a button that shows its English (from the syllabus lemma's meanings, the story's new
   words or the function-word glosses), each paragraph has **Show in English**, then the questions
   as module 07 grades them; XP as module 07's stories (10, plus 2 per right answer); the day counts
   for the streak. It is available when the unit's last skill is practised, mastered or tested out,
   as module 07's story node.
7. **Fresh story (AI).** With `ai.stories` on, the unit also offers **Fresh story (AI)**, module 07's
   AI story unchanged, with module 16's mark and panel when AI is not set up.
8. **Beta.** A story whose status is `unreviewed` shows a visible "Beta" tag and the line "This
   story has not been checked by a native speaker yet." until its status is `reviewed`; `parley
   content report` lists unreviewed stories beside unreviewed units.

## Files

- `content/{es,fr,de}/stories.yaml`, `content/fr/syllabus.yaml` (Camille and Hugo added to `names`
  only; no item changes, so `ids.lock` is untouched), `schemas/stories.schema.json`
- `crates/parley-core/src/content/stories.rs` (types, parse, rules ST01 to ST11, the reading view),
  `crates/parley-core/src/content/{validate.rs,report.rs}`, `crates/parley-core/src/app/{story.rs,
  course.rs}`, `crates/parley-core/src/api.rs`
- `app/src-tauri/src/bundled.rs` (the stories files compiled in, as the syllabi are)
- `app/src/screens/Story/*` (the reading view, word glosses, Beta), `app/src/screens/Path/*`
  (banners, the two story buttons), `app/src/api/commands.ts`, `app/src/strings/en.json`
- Docs: ARCHITECTURE §2 (`stories.yaml`) and §5 (`lesson_start` mode `written_story`),
  `docs/modules/05-…` and `07-…` (a pointer here where they describe stories), `README.md`,
  `docs/HANDBOOK.md` (the stories format beside the syllabus format)
- Tests: `crates/parley-core/tests/m19_offline_stories_and_characters.rs`,
  `crates/parley-cli/tests/m19_offline_stories_and_characters.rs`,
  `tests/cases/19-offline-stories-and-characters/` (`content validate` on fixtures),
  `tests/fixtures/stories/` (`valid-fr.yaml`, `bad-ST01.yaml` … `bad-ST11.yaml`, beside a small
  course, hand-written, CC0, with a README of intended outcomes), `app/src/test/m19/*.test.tsx`

## Interfaces

Planning level, made exact when the module starts.

### `content/<lang>/stories.yaml`

```yaml
schema: parley.stories/1
lang: fr
cast:
  - {name: Camille, about_en: runs a small café in Lyon}
  - {name: Hugo, about_en: her brother, a student}
glosses:                                # English for the function words the stories use
  le: [the]
  de: [of, from, some]
stories:
  - unit: fr.a1.u03
    banner_en: Camille runs a small café in Lyon. Today a new customer comes in.
    status: unreviewed                  # unreviewed | reviewed
    date: "2026-11-20"
    title: Un client au café
    title_en: A customer at the café
    new_words:
      - {word: croissant, en: [croissant]}
    paragraphs:
      - {target: "Camille a un petit café à Lyon. …", en: "Camille has a small café in Lyon. …"}
    questions:
      - {question_en: Where is the café?, options: [In Lyon, In Paris, At home], answer_index: 0, paragraph: 0}
```

Every field is required except `new_words` (may be empty); no other field anywhere.

### Rules (errors, in this order, each on its own line, as module 01's rules are reported)

- **ST01** YAML or schema, worded as V01; stops the other rules for the file.
- **ST02** `{file}: lang is "{l}" but the syllabus is "{s}"`.
- **ST03** `{file}: unit {unit} is not in the syllabus` and `{file}: unit {unit} is listed twice`.
- **ST04** `{file}: unit {unit} has no story` (syllabus order).
- **ST05** `{file}: {name} is not in the syllabus's names` (every cast name).
- **ST06** `{unit}: {n} paragraphs; {level} stories have {k}` (A1 3, A2 4) and `{unit}: {n} words;
  this story needs {min} to {max}` (A1 units 1 and 2: 40 to 90; other A1: 80 to 120; A2: 120 to
  180; words counted as module 07's S4 counts them).
- **ST07** `{unit}: "{word}" is not taught by this unit and is not a new word`: every word token
  that is not a number or a name is a function word or a form of a lemma of a skill up to and
  including the unit's last skill (V12's taught-forms rule) or one of the story's new words.
- **ST08** `{unit}: {n} new words; the limit is {k}`; `{unit}: new word "{w}" is not in the
  story`; `{unit}: "{w}" is already taught; it is not a new word`; `{unit}: "{w}" is on the
  exclude list`.
- **ST09** `{file}: "{word}" needs a gloss` (a function word used by a story with no entry in
  `glosses`).
- **ST10** `{unit}: {n} questions; {level} stories have {k}` (A1 3, A2 4); `{unit}: question {q}
  has repeated options`; `{unit}: question {q} points to paragraph {p}, which does not exist`.
- **ST11** `{unit}: {field} is not in {Language}` or `… is not in English` for texts of four words
  or more, identified as module 07's L4 does (paragraph `target`, `title`; `en`, `title_en`,
  `banner_en`, questions and options); `{unit}: banner_en has {n} words; the limit is 30`.

### Core and commands

- `content::stories`: `Stories`, `parse_stories(file, text)`, `validate_stories(&Stories,
  &Syllabus, &Context) -> Report`, and `reading_view(story, syllabus, forms, glosses) -> Reading
  { paragraphs: [[Span]], paragraphs_en, beta }`, where a `Span` is plain text, a `Word { surface,
  lemma, en }`, a `Name` or a `Number`.
- `app::CourseFiles` gains `stories_yaml: Option<String>`; `bundled::courses()` compiles in
  `content/<lang>/stories.yaml`.
- `lesson_start(skill_id, "written_story")`: the skill must be the last of a unit with a story
  (`not_found` "This unit has no written story." otherwise); no client is asked for. It returns
  `LessonStarted` with kind `story`, `story` (module 07's shape) and `reading`. Questions are graded
  by `exercise_submit` as module 07's (`q1`, `q2`, …, `Answer::Choice`); `lesson_finish` gives
  `story_xp(correct)`.
- `api::PathUnit` gains `banner: Option<String>` and `written_story: Option<PathStory>` (same rule
  for `available` as module 07's `story`, which stays and now labels **Fresh story (AI)**).
- `parley content report` gains `stories` (count) and `unreviewed_stories` (units, syllabus order).

## UI

- **Path**: under each unit heading, the banner (when the unit has one). At the unit's end, **Read a
  story** (disabled with module 07's "Practise the last skill of this unit first." while not
  available) and, when `ai.stories` is on, **Fresh story (AI)** with module 16's mark while AI is not
  set up.
- **Story** (written): the title and English title; "Beta" and its line when unreviewed; each
  paragraph as words that are buttons (one tab stop per paragraph; the arrow keys move between its
  words; Enter or a click shows a popover "{surface}: {meaning}", with "({lemma})" when the lemma
  differs, "a name" for a name, the digits for a number; Esc closes it); **Show in English** under
  each paragraph (then **Hide the English**); then module 07's questions, **Finish** and the summary
  with module 15's moments.
- Nothing on the written story's screen opens a dialog or makes a request.

## Messages

**Read a story**; **Fresh story (AI)**; "Beta"; "This story has not been checked by a native
speaker yet."; **Show in English**; **Hide the English**; "{surface}: {meaning}"; "a name";
`not_found` "This unit has no written story."; the validator messages ST01 to ST11 above.

## Out of scope

- Audio for stories (a learner with a voice can play each paragraph through module 09 in a later
  change), stories beyond one per unit, and learner-written stories.
- Changing module 07's AI story (prompt, validation, cache, cost) or course packs, which keep
  exporting only stored stories (a written story is content, not learner data).
- Hearts, energy, lesson limits, ads, paywalls or social features (I8: a written story can be read
  any number of times, each for XP); consent never at launch or in onboarding (I12: the written
  story needs none); nothing leaves the machine.

## Design notes

- **Storage.** Stories are course content, so they live beside the syllabus in
  `content/<lang>/stories.yaml` under `content/LICENSE` (CC BY-SA 4.0) and are compiled into the
  binary as the syllabi are, so they work offline from the first launch. They are not in the
  sentence packs, which are optional downloads built from Tatoeba under CC BY 2.0 FR, and not in
  the `stories` table, which holds the learner's AI and imported stories (and their backups).
- **Validation.** ST01 to ST11 run in `parley content validate` (and so in `scripts/verify`) when
  `stories.yaml` is beside the syllabus, after the CL and RV rules; a syllabus that does not load
  leaves out the rules that need it, as module 05 does for its files. Coverage is exact (every word
  token), unlike an AI story's 92%, since the text is written by hand and can always be fixed.
- **Glosses.** Every word must be translatable: a syllabus lemma's `en` (its first two meanings),
  a new word's `en`, or the file's function-word `glosses`; ST07 and ST09 together guarantee one
  for every word token, which a test checks over every real story. A form that maps to several
  lemmas takes the first taught one, in syllabus order.
- **Review and the Beta tag (maintainer decision, 2026-11-18).** Each story has `status` and
  `date` (no names), as `review.yaml` has for units (module 05's rules). Every unreviewed story
  shows a visible "Beta" tag on its screen until a native speaker has reviewed it and its status is
  set to `reviewed`. This goes further than module 05's decision for units, which are labelled
  beta only on the About page (module 14) and in the release notes, with nothing on the path: the
  stories are new text with no Tatoeba attestation behind them, so a reader is told at the point of
  reading. Module 14's About page lists unreviewed stories with the units.
- **The cast (maintainer decision, 2026-11-18).** Spanish: Lucía, who runs a small bakery-café in
  Sevilla, and her neighbour Pablo. French: Camille, who runs a small café in Lyon, and her brother
  Hugo. German: Anna, who runs a bicycle shop in Hamburg, and her friend Tom. The people are common
  first names, not real people; the cities are real and already in the syllabi's names; every
  street, café, shop and other business is invented, and no brand appears. The person's day follows the unit
  themes of module 05 (Hello, Day to day, Food and drink, … Experiences), so Unit 3's story happens
  in the café and Unit 8's on a trip.
- **Sessions.** A written story's session is kind `story` with `skill_id` the unit's id, so module
  07's count of a skill's story sessions, which numbers its AI and imported stories (an imported
  story plays at the next number), is unchanged by reading the written one. The alternative, a new
  session kind, needs a migration that rebuilds `sessions`, since SQLite cannot change a CHECK
  constraint; the unit id keeps the store as it is.
- **Writing them.** Each story is written from its unit's words: sentences short (as module 03's L8
  limits exercises: 8 words at A1, 12 at A2, where possible), present tense at A1, the unit's tenses
  at A2, the cast's names doing ordinary things of the unit's theme, a little humour where it fits.
  Unit 1 and 2 stories are short dialogues, since few words are taught by then.

## Acceptance tests

Golden cases through the binary in `tests/cases/19-offline-stories-and-characters/`; core tests in
`crates/parley-core/tests/m19_offline_stories_and_characters.rs`; command tests in
`crates/parley-cli/tests/m19_offline_stories_and_characters.rs`; UI tests in `app/src/test/m19/`.

1. `parley content validate content` → exit 0 with stories for es, fr and de, one per unit (16
   each; Spanish's 65 skills still make 16 units).
2. Fixtures `bad-ST01.yaml` to `bad-ST11.yaml`, beside the fixture course, each give exactly their
   message(s) and exit 1; `valid-fr.yaml` gives `{"ok":true,…}`.
3. ST07: a fixture story with an untaught word fails with its message; the same word listed in
   `new_words` passes; a word on `exclude.txt` listed as new fails ST08.
4. For every real story, `reading_view` gives every word span an English meaning (none empty) and
   every name and number its own kind.
5. `lesson_start(<unit's last skill>, "written_story")` with `ai.provider` unset and `FailingNet`
   returns the story with `reading`; no client is asked for, no request is made (I3) and no consent
   is asked (I12); a skill that is not a unit's last → "This unit has no written story."
6. Answering 2 of 3 questions right and finishing gives 14 XP (module 07's `story_xp`), credits the
   day, and writes a `story` session whose `skill_id` is the unit id.
7. Reading the written story twice leaves module 07's next story number for that skill unchanged: a
   course-pack story imported at number 1 still plays next as **Fresh story (AI)**.
8. `path_get` gives each unit its banner and `written_story`, available only when the unit's last
   skill is practised, mastered or tested out; `story` (AI) only with `ai.stories` on.
9. UI: **Read a story** opens the written story; clicking a word shows "{surface}: {meaning}" and
   Esc closes it; the arrow keys move between words of a paragraph; **Show in English** shows the
   paragraph's English.
10. UI: an unreviewed story shows the "Beta" tag and "This story has not been checked by a native
    speaker yet."; the same story marked `reviewed` shows neither; the path shows no Beta tag on
    units (module 05's rule for units is unchanged).
11. UI: with `ai.provider` unset, **Fresh story (AI)** carries module 16's mark and opens the short
    panel; with AI set up it calls `lesson_start` with mode `story` as module 07 does.
12. `parley content report` on the fixture course gives `stories` and `unreviewed_stories` as its
    README states.
13. The cast of each `stories.yaml` is exactly Lucía and Pablo (es), Camille and Hugo (fr), Anna
    and Tom (de), every cast name is in its syllabus's `names` (ST05 on the real content), and the
    hygiene test passes over the new content files.

## Status

Planned
