# 05: Course content A1 to A2

## Goal

Spanish, French and German each have a complete A1 and A2 course: a syllabus that passes every
validator, word forms, frequency ranks, a grammar and functions checklist it covers, the six
role-play scenarios, and a sentence pack pinned by checksum. A learner can go from nothing to A2 in
any of the three languages with the offline app.

## Depends on

01, 04. **Maintainer decides** the order of languages (default Spanish, French, German) and whether
unreviewed units may ship (see `review.yaml` below).

## Scope

- `content/{es,fr,de}/syllabus.yaml`: A1 with at least 8 units and A2 with at least 8 units, 3 to
  6 skills per unit, every skill meeting module 01's rules.
- `content/<lang>/checklist.yaml`: the grammar points and communicative functions of each level in
  the project's own words (the CEFR Companion Volume, Council of Europe 2020, is the reference for
  levels; its text is not copied), each mapped to the skills that teach it.
- `content/<lang>/ids.lock` updated with `parley content lock` for every new item.
- `content/<lang>/forms.tsv` for every syllabus lemma; frequency ranks for every single-word lemma
  from the pack's `freq` table; `parley freq check`.
- Rule **V22**: warning `warning: {lemma} has rank {n}; check that it belongs at A1` for an A1 lemma
  ranked above 3,000; error `{skill}: {lemma} needs a rank` for a single-word lemma with none.
- Scenarios `content/scenarios/{cafe,shop,directions,hotel,doctor,job_interview}.yaml`
  (`schemas/scenario.schema.json`) and their rules SC01 to SC06.
- `parley content report --lang <l> --pack <file> --json`.
- Packs for es, fr and de built from Tatoeba; `content/packs.lock.toml` filled; `parley pack install
  <file>` for development (verifies against the lock).
- `content/<lang>/review.yaml`: every unit with `status: unreviewed | reviewed` and a date, no names.

## Files

- `content/**` as above; `schemas/scenario.schema.json`
- `crates/parley-core/src/content/{scenario.rs,report.rs,checklist.rs}`
- `crates/parley-cli/src/cmd/{freq.rs,report.rs}`; `tests/fixtures/scenarios/bad-SC0*.yaml`

## Interfaces

Scenario file:

```yaml
schema: parley.scenario/1
id: cafe
cefr: A1
title_en: At a café
partner_role_en: the waiter
learner_role_en: a customer
turn_cap: 12
goals:
  - {id: greet, text_en: Greet the waiter}
  - {id: order_drink, text_en: Order a drink}
  - {id: order_food, text_en: Order something to eat}
  - {id: ask_price, text_en: Ask how much it costs}
  - {id: pay_leave, text_en: Pay and say goodbye}
languages:
  es:
    setting_en: a small café in Madrid in the morning
    opening: "¡Buenos días! ¿Qué le pongo?"
    opening_en: "Good morning! What can I get you?"
    skills: [es.a1.u03.s01, es.a1.u03.s02]
```

`parley content report` prints `{"lang", "units": {"A1", "A2"}, "skills", "lemmas": {"A1", "A2"},
"examples", "attested_share", "a1_rank_le_2000_share", "checklist_uncovered": [], "unreviewed_units": []}`.
`attested_share` is the share of examples whose normalised text equals a pack sentence.

Messages: SC01 `{file}: {pointer}: {message}`; SC02 `{file}: id must match the file name`; SC03
`{id}: {n} goals; a scenario has 3 to 6`; SC04 `{id}: turn_cap {n} is outside 6 to 20`; SC05 `{id}:
{lang}: skill {skill} does not exist`; SC06 `{id}: {lang}: opening uses "{word}", which the listed
skills do not teach`. `parley pack install` on a file that does not match the lock: `This pack does
not match packs.lock.toml.`

## Out of scope

- Publishing the packs as release assets (maintainer, module 14; until then `pack install` is used).
- B1 and other languages; AI-generated content (07).

## Design notes

- Downloading the Tatoeba exports and the wiktextract extracts (sizes stated in the commit body) into
  the git-ignored `data-cache/` is part of this module; nothing downloaded is committed.
- Content is authored in plain, current, standard language. Every example is checked against the
  pack first: an attested Tatoeba sentence is preferred where it fits the skill.
- Native-speaker review is a maintainer step; units stay `unreviewed` until then, and the About page
  lists unreviewed units.
- Decisions are filled in here during implementation, including the counts reached per language.

## Acceptance tests

Golden cases in `tests/cases/05-course-content-a1-a2/`; code tests in
`crates/parley-core/tests/m05_course_content_a1_a2.rs`. Expected values come from the rules and the
fixtures; content thresholds are stated here.

1. `parley content validate content` → exit 0, no errors, for es, fr and de.
2. For each language the report shows at least 8 A1 units and 8 A2 units, each with 3 to 6 skills.
3. `checklist_uncovered` is empty for each language, and every mapped skill has the level of its entry.
4. `parley freq check --lang <l> --pack <pack>` reports 0 lemmas whose `rank` differs from the pack
   (skipped with "pack not found in data-cache/packs; run parley pack build" when absent).
5. `attested_share` ≥ 0.60 for each language; `a1_rank_le_2000_share` ≥ the threshold the first run
   records, and never below 0.75 (both need the pack: skipped with the message of test 4 when
   `data-cache/packs/` does not have it).
6. The six scenarios validate; fixtures `bad-SC02.yaml` to `bad-SC06.yaml` give exactly their
   messages; each scenario has an opening for es, fr and de.
7. Gate G3 over the full content: 1,000 seeded template lessons all pass `check_exercise`; in
   `verify`, 50 seeds.
8. Every syllabus lemma has at least one row in its `forms.tsv`.
9. `packs.lock.toml` has `sha256`, `bytes` and `content_sha256` for es, fr and de; each matches the
   built file in `data-cache/packs/` (skipped with the message of test 4 when absent).
10. `parley pack install` with a file one byte different from the lock → "This pack does not match
    packs.lock.toml."
11. Every unit of every syllabus appears exactly once in its `review.yaml`.
12. An A1 fixture lemma with rank 3,500 gives the V22 warning; a single-word lemma with no rank
    gives the V22 error.

## Status

Planned
