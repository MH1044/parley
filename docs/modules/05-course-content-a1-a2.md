# 05: Course content A1 to A2

## Goal

Spanish, French and German each have a complete A1 and A2 course: a syllabus that passes every
validator, word forms, frequency ranks, a grammar and functions checklist it covers, the six
role-play scenarios, and a sentence pack pinned by checksum. A learner can go from nothing to A2 in
any of the three languages with the offline app.

## Depends on

01, 04. The two decisions this module needed from the maintainer were made on 2026-11-04 and are
recorded under Design notes: the languages are authored in the order Spanish, French, German, and
units not yet reviewed by a native speaker ship as a labelled beta.

## Scope

- `content/{es,fr,de}/syllabus.yaml`: A1 with at least 8 units and A2 with at least 8 units, 3 to
  6 skills per unit, every skill meeting module 01's rules. The seed units of module 01 stay; their
  published example ids are never reused (`ids.lock`).
- `content/<lang>/checklist.yaml` and `schemas/checklist.schema.json`: the grammar points and
  communicative functions of each level in the project's own words (the CEFR Companion Volume,
  Council of Europe 2020, is the reference for levels; its text is not copied), each mapped to the
  skills that teach it; rules CL01 to CL04.
- `content/<lang>/ids.lock` updated with `parley content lock` for every new item.
- `content/<lang>/forms.tsv` regenerated with `parley forms extract` from the real wiktextract
  extract for every syllabus lemma; the extract reader skips wiktextract's table metadata (forms
  tagged `table-tags`, `inflection-template`, `class` or `auxiliary`).
- Frequency ranks for every single-word lemma from the pack's `freq` table; `parley freq check`.
- Rule **V22** (ranks) in `parley content validate`.
- Scenarios `content/scenarios/{cafe,shop,directions,hotel,doctor,job_interview}.yaml`
  (`schemas/scenario.schema.json`) and their rules SC01 to SC06, checked by `parley content
  validate <dir>` and by `parley content scenarios`.
- `content/<lang>/review.yaml` and `schemas/review.schema.json`: every unit with
  `status: unreviewed | reviewed` and a date, no names; rules RV01 to RV04.
- `parley content report --lang <l> [--content <dir>] [--pack <file>] --json`.
- Packs for es, fr and de built from Tatoeba with `parley pack build`; `content/packs.lock.toml`
  filled; `parley pack install <file> --data-dir <dir>` for development (verifies against the lock).

## Files

- `content/**` as above; `schemas/{scenario,checklist,review}.schema.json`
- `crates/parley-core/src/content/{scenario.rs,report.rs,checklist.rs,review.rs}`, V22 in
  `validate.rs`, the metadata filter in `wiktextract.rs`, `pack_install` in `src/app/pack.rs`
- `crates/parley-cli/src/cmd/{freq.rs,report.rs}`, `content scenarios` in `cmd/content.rs`,
  `pack install` in `cmd/pack.rs`
- `tests/fixtures/scenarios/` (`bad-SC01.yaml` to `bad-SC06.yaml`, `valid-cafe.yaml`, and the mini
  course `content/es/` they are checked against), `tests/fixtures/course05/` (fixture course for
  V22, CL, RV, report, freq check and pack install), all hand-written, CC0, with a README of
  intended outcomes
- `app/src-tauri/tests/m04_app_shell_and_offline_course.rs`: the two checks that read the bundled
  lock expect a published Spanish pack once the lock is filled, and the tests load the frozen seed
  course of `tests/fixtures/seed-course/` (see Design notes)

## Interfaces

### Scenario file (`content/scenarios/<id>.yaml`)

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

Schema: every field above is required except that `languages` needs at least one of `es`, `fr`,
`de` (no other key); `cefr` is A1 or A2; a goal has `id` (lower case letters, digits and `_`) and
`text_en`; a language section has `setting_en`, `opening`, `opening_en` and `skills` (at least one
skill id); no other field anywhere.

Rules, in this order, each message on its own line; `{file}` is the path as typed with `/`
separators, `{id}` the scenario's `id`:

- **SC01** YAML or schema: `{file}: {pointer}: {message}`, exactly as V01 words them (a file that
  is not YAML: `{file}: {parser message}`). An SC01 failure stops the other rules for that file.
- **SC02** `{file}: id must match the file name` (the file name without `.yaml`).
- **SC03** `{id}: {n} goals; a scenario has 3 to 6`.
- **SC04** `{id}: turn_cap {n} is outside 6 to 20`.
- **SC05** `{id}: {lang}: skill {skill} does not exist` (not a skill of
  `<content>/<lang>/syllabus.yaml`, or that syllabus is missing), once per skill in list order.
- **SC06** `{id}: {lang}: opening uses "{word}", which the listed skills do not teach`, once per
  offending word in order, `{word}` the surface form. A word of the opening is taught when it is a
  function word or a name of the language's syllabus, a number, or a form of a lemma of any skill
  up to and including the latest listed skill in syllabus order (V12's taught-forms rule, with the
  language's `forms.tsv`). SC06 runs only for a language whose skills all exist.

Languages are checked in the order es, fr, de.

### Checklist (`content/<lang>/checklist.yaml`)

```yaml
schema: parley.checklist/1
lang: es
entries:
  - {id: a1.g.present_regular, cefr: A1, kind: grammar,
     text_en: "The present tense of regular -ar, -er and -ir verbs", skills: [es.a1.u02.s04]}
  - {id: a1.f.greet, cefr: A1, kind: function, text_en: Greet people and say goodbye,
     skills: [es.a1.u01.s01]}
```

`id` is `a1.` or `a2.`, then `g.` (grammar) or `f.` (function), then lower case letters, digits and
`_`; `kind` is grammar or function; `skills` may be empty (the entry is then uncovered). Each level
has at least 10 grammar entries and 8 function entries. Rules: **CL01** YAML or schema,
`{file}: {pointer}: {message}`, stops the others; **CL02** `{file}: duplicate entry {id}`;
**CL03** `{file}: {id}: skill {skill} does not exist`; **CL04** `{file}: {id}: skill {skill} is
{level}, but the entry is {cefr}` (`{level}` the skill's `cefr`).

### Review file (`content/<lang>/review.yaml`)

```yaml
schema: parley.review/1
lang: es
units:
  - {unit: es.a1.u01, status: unreviewed, date: "2026-11-05"}
```

`status` is `unreviewed` or `reviewed`, `date` (YYYY-MM-DD, quoted) the day the status was last
set. No reviewer names. Rules: **RV01** YAML or schema, `{file}: {pointer}: {message}`, stops the
others; **RV02** `{file}: unit {unit} is listed twice`; **RV03** `{file}: unit {unit} is not in
the syllabus`; **RV04** `{file}: unit {unit} is missing` (syllabus order).

### Rule V22 (ranks)

Runs when `review.yaml` exists beside the syllabus (a course folder, as `ids.lock` switches on V20
and V21). A single-word lemma is one with no space in `lemma`.

- Error: `{skill}: {lemma} needs a rank` for a single-word lemma whose `rank` is null, in document
  order.
- Warning: `warning: {lemma} has rank {n}; check that it belongs at A1` for a lemma of an A1 skill
  whose `rank` is above 3,000, after the V17 warnings, in document order.

### `parley content validate <dir>` (extended)

For each of es, fr, de in that order: the syllabus rules V01 to V22, then CL01 to CL04 when
`checklist.yaml` exists beside the syllabus, then RV01 to RV04 when `review.yaml` exists; then,
when `<dir>/scenarios/` exists, every `*.yaml` in it in byte order of file name, SC01 to SC06
against the syllabi of `<dir>`. Exit 0 with warnings allowed, 1 on any error. The file form is
unchanged.

### `parley content scenarios <file-or-dir> [--content <dir>]`

`{"ok": bool, "errors": [string], "warnings": []}`, exit 0 or 1 as `content validate`; a directory
means every `*.yaml` in it in byte order; `--content` defaults to `content`. A missing path:
`error: {path} not found`, exit 2.

### `parley content report --lang <l> [--content <dir>] [--pack <file>] --json`

```json
{"lang": "es", "units": {"A1": 8, "A2": 8}, "skills": 64,
 "skills_per_unit": {"min": 3, "max": 6}, "lemmas": {"A1": 410, "A2": 380}, "examples": 420,
 "attested_share": 0.6214, "a1_rank_le_2000_share": 0.8122,
 "checklist_uncovered": [], "unreviewed_units": ["es.a1.u01"]}
```

- `units`: units per level; `skills`: all skills; `skills_per_unit`: the smallest and largest
  skill count of a unit; `lemmas.A1`: distinct lemma strings of A1 skills; `lemmas.A2`: distinct
  lemma strings of A2 skills not taught at A1; `examples`: all examples.
- `attested_share`: the share of examples whose normalised text (the tokens' normalised forms
  joined by single spaces, as the pack's `norm`) equals the `norm` of a pack sentence.
- `a1_rank_le_2000_share`: the share of distinct single-word A1 lemmas whose rank in the pack's
  `freq` table is at most 2,000 (a lemma absent from `freq` counts as above).
- Both shares are rounded to 4 decimal places, and null without `--pack`.
- `checklist_uncovered`: ids of checklist entries with no skills, in file order (empty without a
  checklist); `unreviewed_units`: units marked `unreviewed` in `review.yaml`, in syllabus order
  (every unit when there is no `review.yaml`).
- The syllabus must validate: otherwise exit 1 and the validate output. `--json` is accepted and
  is the only output form. Usage errors as module 01 (`error: --lang is required. Run parley
  --help.`, `error: {path} not found`).

### `parley freq check --lang <l> --pack <file> [--content <dir>]`

`{"lang": "es", "lemmas": n, "differs": [{"skill", "lemma", "rank", "pack_rank"}], "missing":
[{"skill", "lemma"}]}`: `lemmas` counts the single-word lemma entries checked; `differs` lists, in
document order, those whose `rank` (null included) is not the pack's `freq` rank; `missing` those
absent from `freq`. Exit 0 when both lists are empty, else 1. A missing pack or content file:
`error: {path} not found`, exit 2.

### `parley pack install <file> --data-dir <dir> [--lock <file>]`

For development: installs a locally built pack (the app downloads the published ones). Reads the
pack's language from its `meta`; the lock (default `content/packs.lock.toml`) must have an entry
for that language whose
`sha256` and `bytes` match the file, else stderr `error: This pack does not match
packs.lock.toml.`, exit 1, nothing written. The course must be started in
`<dir>/parley.sqlite3`, else stderr `error: Start the {Language} course in Parley first.`, exit 1.
Then the pack is written through the same path as `pack_download` (`packs/<lang>-<version>.sqlite3`
via a `.part` file, the course's previous pack removed, `pack_version` recorded) and stdout is
`{"lang": "es", "version": "2026-11-05", "path": "packs/es-2026-11-05.sqlite3"}`. A file that is
not a pack: stderr `error: {message}` from `pack info`, exit 1; a missing file or `--data-dir`:
exit 2 as usual.

### `content/packs.lock.toml`

One table per language: `version` (the pack's `built_at`), `url`
(`https://github.com/MH1044/parley/releases/download/packs-<version>/<lang>-<version>.sqlite3`),
`sha256`, `bytes`, `content_sha256` of the file in `data-cache/packs/<lang>-<version>.sqlite3`.

## Out of scope

- Publishing the packs as release assets (maintainer): they are published on the `packs-<version>`
  tag the lock's URLs name (`packs-2026-11-05` holds the three packs the lock pins), and the app's
  Download fetches them; `pack install` installs a locally built pack for development.
- Making the packs smaller: the full-size packs are published now; module 14 brings each to at most
  60 MB before the first app release, publishes them under a new `packs-<date>` tag and re-pins the
  lock (see Design notes).
- The About page listing unreviewed units (module 14, test 7); B1 and other languages;
  AI-generated content (07); filling ranks automatically (`freq check` reports, the author edits).

## Design notes

- **Maintainer decisions (2026-11-04).** Language order Spanish, French, German: each language is
  authored, checked and pinned in that order, and if the module had to shrink the last language
  would be cut first. Units still marked `unreviewed` may ship as a labelled beta: they appear on
  the path, the About page lists them (module 14) and the release notes say the course awaits
  native-speaker review. This also settles the same question at module 14.
- **Unit plan**, the same themes in each language so that checklists and scenarios line up. A1:
  u01 Hello, u02 Day to day, u03 Food and drink (café), u04 Around town (directions), u05 Shopping
  (shop), u06 Home and family, u07 Time and routine, u08 Travel and hotel (hotel). A2: u01 What I
  did (a past tense), u02 Health (doctor), u03 Work (job interview), u04 Plans, u05 Weather and
  seasons, u06 Free time, u07 Comparing and describing, u08 Experiences.
- **Sources**, downloaded into the git-ignored `data-cache/` on 2026-11-04, never committed:
  Tatoeba per-language exports (`spa`, `fra`, `deu` and `eng` `_sentences_detailed.tsv.bz2`, the
  three `-eng_links.tsv.bz2`; 85 MB compressed, 430 MB decompressed) and the kaikki.org
  wiktextract extracts of the English Wiktionary for Spanish, French and German
  (`kaikki.org-dictionary-<Language>.jsonl.gz`; 261 MB compressed, 2.8 GB decompressed). The
  SHA-256 of every file is recorded in each pack's `source_sha256`.
- **Wiktextract in practice.** The real extracts list table metadata among `forms`: entries
  tagged `table-tags` (`irregular`, `strong`), `inflection-template` (`es-conj`), `class`
  (`7 strong`) or `auxiliary` (the `haben` or `sein` of a German conjugation table) are not forms
  of the word and are skipped. Without that, `haben` was a form of 9,488 German verbs, so every
  sentence with `haben` carried thousands of lemma keys and any sentence with `haben` matched the
  exclude list through `ficken`. Everything else of module 01's field mapping held.
- Content is authored in plain, current, standard language. Every example is checked against the
  pack first: an attested Tatoeba sentence is preferred where it fits the skill, with its English
  translations from the pack.
- Ranks are the pack's `freq` ranks (counts over the kept Tatoeba sentences, module 01). A lemma
  that the pack never ranks cannot be taught: V22 requires a rank, so the author picks another
  word or a multi-word lemma.
- Native-speaker review is a maintainer step; units stay `unreviewed` until then.
- **The bundled lock now publishes packs**, so two module 04 checks that read it (t03 and the
  onboarding flow) change: with the bundled lock, Spanish is `available` at the locked version, and
  t03 keeps its French-only lock for the not-published case. The bundled course has grown too, and
  module 04's placement staircase, stats and backup fixtures were worked out by hand over the
  eight-skill seed course, so its command tests now load that course from
  `tests/fixtures/seed-course/` (the module 01 files, frozen); none of their expected values
  changes.
- **Counts reached** (packs built on 2026-11-05 from the sources above):

  | | es | fr | de |
  |---|---|---|---|
  | units A1 + A2 | 8 + 8 | 8 + 8 | 8 + 8 |
  | skills (per unit) | 65 (4 to 5) | 64 (4) | 64 (4) |
  | lemmas A1 / A2 | 310 / 234 | 291 / 258 | 289 / 261 |
  | examples | 518 | 531 | 516 |
  | `attested_share` | 0.8147 | 0.8757 | 0.8682 |
  | `a1_rank_le_2000_share` (the recorded threshold) | 0.8771 | 0.9004 | 0.8865 |
  | checklist entries A1 / A2 (grammar + function) | 17 + 19 / 21 + 16 | 22 + 20 / 21 + 17 | 20 + 22 / 18 + 17 |
  | pack: sentences kept, size | 209,760, 247 MB | 304,851, 175 MB | 449,283, 401 MB |

  The packs are far above ADR-0007's "tens of megabytes": most of the size is the `forms` table
  (the whole extract, as module 01 specifies) and, for German, `sentence_lemmas`, because the
  extract lists every German personal pronoun as a form of every other one. The maintainer
  decided on 2026-11-06 that this module closes with the packs as built and pinned at these sizes,
  and then to publish them as built: `packs-2026-11-05` holds these full-size packs. Shrinking them
  (only the forms of ranked lemmas, a pronoun filter, or a compressed download) is in module 14's
  Scope, since it changes module 01's pack format: module 14 brings each pack to at most 60 MB
  before the first app release, publishes the smaller packs under a new `packs-<date>` tag and
  re-pins the lock.
- **Content decisions.** Wiktionary lists Spanish reflexive and French pronominal forms with their
  pronoun (`me llamo`, `m'appelle`), so those verbs are taught through the base verb (`llamar`,
  `levantar`, `appeler`) and the grammar note explains the reflexive use. French feminine and plural
  past participles have their own headwords, so `allé`, `venu`, `parti` and the like are lemmas of
  the passé composé skill. Spanish `buen` and German `bisschen` and `freuen` are lemmas of their
  own (the seed's `freut mich` and `ein bisschen` have no headword in the extract). The months,
  the days of the week and a few topic words (`talla`, `réservation`, `geradeaus`) rank above
  3,000 and keep their V22 warning: they are core vocabulary for their unit. The syllabi's
  `function_words` and `names` lists grew with the closed-class words and the first names and
  cities the examples use. Every example keeps to its level's word limit for exercises (module
  03's L8: at most 8 words at A1 and 12 at A2), so a template lesson can turn every example into a
  translation exercise; ten longer A1 examples were replaced by shorter Tatoeba sentences.

Decisions made during implementation:

- **V22** is `check_ranks` beside `validate` (the module 01 `Context` keeps its fields). It runs
  whenever `review.yaml` is beside the syllabus, in the file form as in the directory form, as
  `ids.lock` switches on V20 and V21 in both; its errors follow V21's and its warnings V17's.
  `content lock` still refuses only on V01 to V19: ranks are filled in after the ids are
  recorded, so a missing rank does not hold the lock up.
- **A syllabus that does not load** (V01) in `content validate <dir>`: its checklist and review
  file still get CL01, CL02, RV01 and RV02; the rules that need the syllabus (CL03, CL04, RV03,
  RV04, V22) are left out for that language, and for SC05 the syllabus counts as missing. The
  same holds when a file beside the syllabus (`forms.tsv`, `numbers.yaml`, `ids.lock`) does not
  load: that file's error is reported, the syllabus rules after V01 do not run, CL03, CL04, RV03,
  RV04 and V22 are left out, and for SC05 the syllabus counts as missing; nothing is lost, since
  the run already fails on that error. SC06 counts the words of `numbers.yaml` beside the
  syllabus as numbers, as V12.
- **Checklist**: the level and kind in an entry's id are checked by the schema's pattern (the
  library's wording, as V01 allows) and not compared with `cefr` and `kind`; `lang` is not
  compared with the folder; the 10 grammar and 8 function entries per level are a content
  threshold checked by acceptance test 3, not a rule.
- **Review file**: RV02 names a unit once, where it is listed the second time, and RV03 once,
  where it is first listed, however often it appears; `date` is checked for the shape
  `YYYY-MM-DD` by the schema's pattern (an unquoted date is also text under YAML 1.2).
- **Scenario schema**: `languages` needs at least one of es, fr, de (`minProperties`) and a
  language's `skills` at least one entry (`minItems`), both in the library's wording; a goal's
  `id` has a pattern; `turn_cap` is any integer, so SC04 names a value out of range.
- **Scenario order**: SC02, SC03, SC04, then SC05 for es, fr and de, then SC06 for es, fr and de,
  following module 01's order by rule, then by document.
- **`content scenarios`** accepts `--json`, which changes nothing; a `--content` directory that
  does not exist is `error: {dir} not found`, exit 2.
- **`content report`**: "the syllabus must validate" means everything `content validate <dir>`
  checks for that language (V01 to V22, CL and RV; not the scenarios), so the report never counts
  a broken checklist or review file. `units` always has both keys. Lemma strings are compared in
  NFC. A share is rounded half up in integer arithmetic, and is 0 when it has nothing to count (no
  examples, or no single-word A1 lemma). A pack lemma is looked up as written, then in NFC. A pack
  that cannot be read gives `error: {message}`, exit 1.
- **`freq check`** needs only V01 to pass: it is the tool that helps fill the ranks, so V22 and the
  other rules do not stop it; a syllabus that does not load prints the V01 report, exit 1.
  `lemmas` counts entries, so a lemma taught in two skills is checked twice. The pack is checked
  for existence before the syllabus; `--json` is accepted and changes nothing.
- **`pack install`**: the version and file name are the lock entry's `version` (the pack's
  `built_at`, by the lock format), and the file must match both `sha256` and `bytes`. The order is:
  the file, `--data-dir` and the lock must exist (exit 2); the file must open as a pack (`error:
  {message}` as `pack info` prints it, exit 1); a lock that does not parse gives its own message,
  exit 1; then the lock match; then the started course. A data directory without
  `parley.sqlite3` counts as not started, and no data file is created. The core part is
  `LockedPack::verify` and `LockedPack::install` in `src/app/pack.rs`, which share `write_pack`
  with `pack_download`. `write_pack` records the new version before it removes the previous pack;
  if recording fails, the new file is removed and the old pack and the store stay as they were.
  The command line gives the core an `Fs` rooted at `--data-dir` and opens the data file by path,
  as the app does.
- **Command groups**: `content`, `pack` and `freq` without a subcommand name every choice
  (`error: content validate, content lock, content scenarios or content report is required. Run
  parley --help.`, `error: pack build, pack info or pack install is required. Run parley
  --help.`, `error: freq check is required. Run parley --help.`).
- **Wiktextract**: a `forms` entry is skipped when any of its tags is `table-tags`,
  `inflection-template`, `class` or `auxiliary`; the gender search never looked at them, since
  they are not tagged `canonical`.

## Acceptance tests

Golden cases in `tests/cases/05-course-content-a1-a2/`; code tests in
`crates/parley-core/tests/m05_course_content_a1_a2.rs` (the real content, through the library
API of modules 01 to 03 and plain YAML and TOML parsing) and
`crates/parley-cli/tests/m05_course_content_a1_a2.rs` (the binary: every new command, exit codes,
stderr, files written). The new rules and commands are tested through the binary, so the tests
depend on the command-line interface above and not on the library's new types. Expected values
come from the rules and the fixtures; content thresholds are stated here.
"Skipped" means the test prints `pack not found in data-cache/packs; run parley pack build` and
passes, when `data-cache/packs/<lang>-<version>.sqlite3` for the lock's version is absent.

1. `parley content validate content` → exit 0, no errors, for es, fr and de (scenarios included).
2. For each language the report shows at least 8 A1 units and 8 A2 units, `skills_per_unit` within
   3 to 6, and `unreviewed_units` equal to the units `review.yaml` marks unreviewed.
3. `checklist_uncovered` is empty for each language, every mapped skill has the level of its entry,
   and each level has at least 10 grammar and 8 function entries.
4. `parley freq check --lang <l> --pack <pack>` reports 0 lemmas in `differs` and `missing` for each
   language (skipped without the pack).
5. `attested_share` ≥ 0.60 for each language; `a1_rank_le_2000_share` ≥ the threshold the first run
   records (in the test, with the date), and never below 0.75 (skipped without the pack).
6. The six scenarios validate and each has an opening for es, fr and de; fixtures `bad-SC01.yaml`
   to `bad-SC06.yaml`, checked with `parley content scenarios <fixture> --content
   tests/fixtures/scenarios/content`, give exactly their messages (exit 1); `valid-cafe.yaml`
   gives `{"ok":true,...}`.
7. Gate G3 over the full content: 1,000 seeded template lessons all pass `check_exercise`; in
   `verify`, 50 seeds.
8. Every syllabus lemma has at least one row in its `forms.tsv`, and no `forms.tsv` row has the
   tag `table-tags`, `inflection-template`, `class` or `auxiliary`; a wiktextract line whose `forms`
   hold such entries gives no row for them.
9. `packs.lock.toml` has `version`, `url`, `sha256`, `bytes` and `content_sha256` for es, fr and de;
   each matches the built file in `data-cache/packs/` (skipped without the pack).
10. `parley pack install` with a file one byte different from the lock → exit 1, stderr
    `error: This pack does not match packs.lock.toml.`, nothing under the data directory's
    `packs/`; with the matching file and the course not started → `error: Start the Spanish course
    in Parley first.`; started → the JSON above, the file in place and `pack_status` installed.
11. Every unit of every syllabus appears exactly once in its `review.yaml`; fixtures give RV02,
    RV03 and RV04 exactly.
12. An A1 fixture lemma with rank 3,500 gives the V22 warning; a single-word lemma with no rank
    gives the V22 error; a multi-word lemma with no rank gives neither; without `review.yaml`
    beside the syllabus neither appears.
13. Checklist fixtures give CL02, CL03 and CL04 exactly.
14. `parley content report` on the fixture course and fixture pack gives the counts and shares its
    README states; without `--pack` both shares are null.
15. `parley freq check` on the fixture course gives the `differs` and `missing` its README states
    and exit 1.

## Status

Done (2026-11-06)
