# 01: Content model and syllabus

## Goal

The course exists as validated open data. A contributor can write a syllabus in YAML, run
`parley content validate`, and get exact messages for every problem; the core derives learnable
items from it; `parley pack build` turns Tatoeba exports into a sentence pack with attribution. Seed
content (Spanish A1 units 1 and 2, French and German unit 1) passes the validator.

## Depends on

00, and ADR-0003 (content licence) Accepted. **Maintainer decides**: while ADR-0003 is still
Proposed, this module stops until the maintainer has chosen the content licence (CC BY-SA 4.0 or CC0).

## Scope

- `schemas/syllabus.schema.json` and Rust types `Syllabus`, `Unit`, `Skill`, `Lemma`, `Example`.
- A YAML loader (a maintained serde YAML crate; `serde_yaml` is archived) feeding the JSON Schema
  check, then semantic rules V01 to V21 below.
- `parley content validate`: exit 0 with warnings allowed, 1 on any error.
- Stable item ids: every example has its own integer `id`, and `content/<lang>/ids.lock` records every
  item id ever published, so an id is never renumbered or reused; `parley content lock`.
- Tokeniser per language (UAX #29 words plus the rules in Design notes), with lemma keys.
- Forms table: TSV format, loader, lookups; `parley forms extract` from a wiktextract extract.
- The exact CLI syntax and output of every command below (Interfaces).
- Item derivation: vocabulary items from lemmas and phrase or pattern items from examples, with
  deterministic ids and accepted answers.
- Sentence packs: `parley pack build`, `parley pack info`, the pack SQLite schema, a content hash,
  and a build report with a reason for every dropped sentence.
- `content/packs.lock.toml` format and `verify_pack(path, sha256)`.
- Seed content: `content/es/` A1 units 1 and 2 (8 skills), `content/fr/` and `content/de/` unit 1
  (4 skills each), each with `syllabus.yaml`, `ids.lock`, `forms.tsv` (syllabus lemmas only), `numbers.yaml`
  (0 to 100 in words), `exclude.txt` (lemmas never taught or shown); `content/LICENSE`,
  `content/ATTRIBUTION.md`.

## Files

- `crates/parley-core/src/content/{syllabus.rs,validate.rs,tokenise.rs,forms.rs,items.rs,pack.rs,lock.rs}`
- `crates/parley-cli/src/cmd/{content.rs,forms.rs,pack.rs}`
- `schemas/syllabus.schema.json`, `schemas/item.schema.json`
- `scripts/verify`, `scripts/verify.ps1`: add the `parley content validate content` step
- `content/**` as above; `tests/fixtures/syllabus/*.yaml`, `tests/fixtures/tatoeba-mini/`,
  `tests/fixtures/forms-mini.jsonl` (hand-written, CC0, each with a README of intended outcomes)

## Interfaces

```rust
pub fn load_syllabus(path: &Path) -> Result<Syllabus, Vec<ContentError>>;
pub fn validate(s: &Syllabus, forms: &Forms) -> Report;          // Report { errors, warnings }
pub fn tokenise(lang: Lang, text: &str) -> Vec<Token>;           // Token { surface, norm, kind, lemma_keys }
impl Forms { pub fn load_tsv(r: impl Read) -> Result<Forms, ContentError>;
  pub fn lemmas_of(&self, form: &str) -> Vec<&str>; pub fn gender_of(&self, noun: &str) -> Option<Gender>; }
pub fn derive_items(s: &Syllabus, forms: &Forms) -> Vec<Item>;
pub fn build_pack(input: PackInput, out: &Path) -> Result<PackReport, PackError>;
pub fn verify_pack(path: &Path, expected_sha256: &str) -> Result<(), PackError>;
```

`PackError::ChecksumMismatch` displays "The download was damaged. Try again."

CLI (every command prints one JSON object on stdout; exit 0 on success, 1 on content errors, 2 on
usage errors: `error: {path} not found`, `error: {flag} is required. Run parley --help.` or
`error: unknown argument {arg}. Run parley --help.`):

    parley content validate <file-or-dir> [--json]
        → {"ok": bool, "errors": [string], "warnings": [string]}   (plain lines without --json)
    parley content lock --lang <es|fr|de> [--content <dir>]
        → {"added": [item_id], "retired": [item_id]}              (appends to ids.lock, never deletes)
    parley forms extract --lang <l> --wiktextract <file.jsonl> (--syllabus <file> | --lemma <lemma>...)
                         --out <forms.tsv>
        → {"lemmas": n, "rows": n, "missing": [lemma]}
    parley pack build --lang <l> --tatoeba-dir <dir> --wiktextract <file.jsonl> --syllabus <file>
                      [--exclude <file>] [--built-at <YYYY-MM-DD>] --out <file.sqlite3>
        reads <dir>/<iso3>_sentences_detailed.tsv, <dir>/eng_sentences_detailed.tsv and
        <dir>/<iso3>-eng_links.tsv (decompressed first)
        → {"lang": "es", "kept": n, "dropped": {"no_translation": n, "too_short": n, "too_long": n,
           "excluded_lemma": n, "unknown_word": n, "duplicate": n}}
    parley pack info <file.sqlite3>
        → {"schema": "parley.pack/1", "lang": "es", "built_at": "2026-10-01", "tool_version": "0.1.0",
           "licence": "CC BY 2.0 FR", "sentences": n, "translations": n, "forms": n,
           "ranked_lemmas": n, "bytes": n, "sha256": "<hex of the file>",
           "content_sha256": "<hex>", "source_sha256": {"spa_sentences_detailed.tsv": "<hex>", ...}}

`ids.lock` (TSV, sorted by item id): `item_id<TAB>active|retired<TAB>syllabus version first seen`,
e.g. `es.a1.u01.s01#e:2	retired	1`.

Syllabus example (`rank` values are filled by module 05 from the pack's frequency table):

```yaml
schema: parley.syllabus/1
lang: es
name: Spanish for English speakers
variety: Spain Spanish; usted and tú both taught
version: 1
function_words: [el, la, los, las, un, una, y, o, pero, de, a, en, con, sin, por, para, no, sí,
  yo, tú, él, ella, usted, nosotros, ellos, ellas, ustedes, me, te, se, lo, le, que, qué]
names: [Ana, Luis, Carmen, Pablo, Madrid, Sevilla]
units:
  - id: es.a1.u01
    title: Hello
    cefr: A1
    skills:
      - id: es.a1.u01.s01
        kind: function            # grammar | vocab | function
        title: Greetings
        cefr: A1
        can_do: I can say hello and goodbye and ask how someone is.
        requires: []
        grammar_note: >-
          Questions and exclamations open with an upside-down mark: ¿Qué tal? ¡Hola!
          Say buenos días until lunch, buenas tardes until dark, then buenas noches.
        lemmas:
          - {lemma: hola, pos: intj, en: [hello, hi], rank: null}
          - {lemma: adiós, pos: intj, en: [goodbye, bye], rank: null}
          - {lemma: bueno, pos: adj, en: [good], rank: null}
          - {lemma: día, pos: noun, gender: m, en: [day], rank: null}
          - {lemma: noche, pos: noun, gender: f, en: [night], rank: null}
          - {lemma: tal, pos: adv, en: [such], rank: null}
          - {lemma: gracias, pos: intj, en: [thank you, thanks], rank: null}
        examples:
          - {id: 1, target: "Hola, ¿qué tal?", en: ["Hi, how are you?", "Hello, how are you?"]}
          - {id: 2, target: "Buenos días.", en: ["Good morning."]}
          - {id: 3, target: "Buenas noches, Ana.", en: ["Good night, Ana.", "Good evening, Ana."]}
          - {id: 4, target: "Adiós, gracias.", en: ["Goodbye, thank you.", "Bye, thanks."]}
```

Forms TSV (`content/<lang>/forms.tsv` and the pack's `forms` table), first line a comment with the
source file's SHA-256 and date: `form<TAB>lemma<TAB>pos<TAB>gender<TAB>tags`, e.g.
`buenas	bueno	adj		feminine plural`, `noches	noche	noun	f	plural`.

Pack (`<lang>-<YYYY-MM-DD>.sqlite3`): `meta(key, value)` with `schema` = `parley.pack/1`, `lang`,
`built_at`, `tool_version`, `source_sha256.<file>`, `licence` = `CC BY 2.0 FR`, `attribution`;
`sentences(id INTEGER PRIMARY KEY, text, norm, tokens INTEGER, max_rank INTEGER, author)`;
`translations(sentence_id, en_id, en_text, en_author, PRIMARY KEY(sentence_id, en_id))`;
`sentence_lemmas(lemma, sentence_id, PRIMARY KEY(lemma, sentence_id)) WITHOUT ROWID`;
`forms(form, lemma, pos, gender, tags, PRIMARY KEY(form, lemma, pos)) WITHOUT ROWID`;
`freq(lemma PRIMARY KEY, rank INTEGER, count REAL)`. `content_sha256` is SHA-256 over every row
of every table in primary-key order, as TSV lines, `meta.built_at` excluded.

`packs.lock.toml`: `schema = "parley.packs/1"` and per language a table `[es]` with `version`,
`url` (this repository's release asset), `sha256`, `bytes`, `content_sha256`.

## Out of scope

- A1 beyond unit 2 for Spanish, A2, frequency ranks, scenarios (module 05); publishing packs
  (maintainer, module 14); FSRS state (02); grading (03).

## Design notes

- Rules: **V01** schema (`{file}: {pointer}: {message}`); **V02** `lang` equals the folder
  (`{file}: lang is "fr" but the folder is "es"`); **V03** id shape `<lang>.<a1|a2>.uNN[.sNN]`
  (`{id}: ids look like es.a1.u01.s01`); **V04** unique ids (`duplicate id {id}`); **V05** units
  never go from A2 back to A1 (`unit {id} is A1 after an A2 unit`); **V06** `requires` names earlier
  skills only (`{skill}: requires {other}, which is not an earlier skill`); **V07** `can_do` starts
  "I can " and ends with a full stop (`{skill}: can_do must start with "I can" and end with a full
  stop`); **V08** grammar note at most 120 words (`{skill}: grammar_note has {n} words; the limit is
  120`); **V09** 3 to 20 lemmas (`{skill}: {n} lemmas; a skill has 3 to 20`); **V10** 4 to 15
  examples (`{skill}: {n} examples; a skill has 4 to 15`); **V11** nouns have a gender (es, fr: m or
  f; de: m, f or n) (`{skill}: noun {lemma} needs a gender (m or f)`); **V12** every example word is a
  function word, a name, a number, or a form of a lemma of this or an earlier skill (`{skill}: example
  {n} uses "{word}", which no earlier skill teaches`); **V13** every example uses a lemma of its skill
  (`{skill}: example {n} uses none of this skill's lemmas`); **V14** no repeated example target
  (`{skill}: example {n} repeats {other} example {m}`); **V15** at least one English translation
  (`{skill}: example {n} needs at least one English translation`); **V16** no lemma from
  `exclude.txt` (`{skill}: {lemma} is on the exclude list`); **V17** warning only: a lemma taught in
  two skills (`warning: {lemma} is taught in {a} and {b}`); **V18** all text is NFC (`{skill}:
  example {n} is not in Unicode NFC; save the file as NFC`); **V19** example ids are positive
  integers, unique within their skill (`{skill}: example id {n} is used twice`); **V20** no item id
  that `ids.lock` marks retired (`{item_id} was retired; give the example a new id`); **V21** every
  item id is active in `ids.lock` and every active id is in the syllabus (`{item_id} is not in
  ids.lock; run parley content lock --lang {lang}` and `{item_id} is in ids.lock but not in the
  syllabus; run parley content lock --lang {lang} to retire it`). In messages, `{n}` is the example's
  `id`.
- Tokeniser: UAX #29 words; digits become `Number`; French elisions (`l' d' j' m' t' s' n' c' qu'
  jusqu' lorsqu' puisqu'`) split after the apostrophe, `aujourd'hui` kept whole, typographic ’ read
  as '; French inverted pronouns (`-je -tu -il -elle -on -nous -vous -ils -elles -ce -t-`) split off;
  Spanish `al` → a + el and `del` → de + el; German `am an+dem`, `im in+dem`, `ins in+das`,
  `zum zu+dem`, `zur zu+der`, `vom von+dem`, `beim bei+dem`; a German lemma marked
  `separable: auf` also allows its base verb and particle (`aufstehen` allows `stehe … auf`); a
  capitalised word not at sentence start, unknown to the forms table, is a `Name`.
- Item ids: `<skill>#l:<lemma>` and `<skill>#e:<id>`, where `<id>` is the example's own `id`, so
  deleting or reordering examples never renumbers the others. Moving a lemma to another skill changes
  its id; `content lock` then retires the old id and module 02 keeps its cards as retired. A
  vocabulary noun's target carries
  its definite article (`el día`); its English accepted answers are the `en` list; phrase items
  take `en` as accepted English answers and `target` as the accepted target answer.
- Pack filters, in order, each a report reason: `no_translation`, `too_short` (< 3 words),
  `too_long` (> 12 words or > 120 characters), `excluded_lemma`, `unknown_word` (a word neither
  lemmatised nor a name), `duplicate` (same normalised text; the lowest id is kept).
- Tatoeba inputs (verify names and columns at this module): `<iso3>_sentences_detailed.tsv`
  (id, lang, text, username, added, modified) and the pair links file (`spa-eng_links.tsv`).
  Tatoeba also publishes a CC0 subset (`<iso3>_sentences_CC0.tsv`); packs use the full CC BY 2.0 FR
  set with authors kept for attribution.
- Frequency source: counts over the Tatoeba sentences (default; CC BY 2.0 FR) or FrequencyWords
  (CC BY-SA 4.0); no CC0 or CC BY list was found on 2026-09-29. Record the choice here.
- `forms.tsv` is course content, not a test fixture: it is committed because the validator in
  `verify` and offline grading need it, under the content licence with Wiktionary credited. With
  CC0 content (ADR-0003) it could not come from Wiktionary.
- Decisions are filled in here during implementation.

## Acceptance tests

Golden cases in `tests/cases/01-content-model-and-syllabus/`; code tests in
`crates/parley-core/tests/m01_content_model_and_syllabus.rs`. Expected values come from how each
fixture was written (its README) and from UAX #29.

1. `parley content validate tests/fixtures/syllabus/valid-es.yaml --json` → `{"ok":true,"errors":[],"warnings":[]}`.
2. One fixture per rule, `bad-V01.yaml` to `bad-V21.yaml`, each → exactly its message from Design
   notes with the fixture's ids filled in (for V08: `es.a1.u01.s01: grammar_note has 121 words; the
   limit is 120`); exit code 1, or 0 for V17.
3. Tokenise fr `L'homme est aujourd'hui à Paris.` → surfaces `L'`, `homme`, `est`, `aujourd'hui`,
   `à`, `Paris` with `Paris` a Name; fr `Est-ce que tu viens ?` → `Est`, `ce`, `que`, `tu`, `viens`.
4. Tokenise es `Voy al mercado del pueblo.` → lemma keys `ir`, `a`+`el`, `mercado`, `de`+`el`,
   `pueblo` (with forms-mini); de `Ich gehe zum Bahnhof.` → `zum` gives `zu`+`der`; `Straße` stays
   one token; `Tengo 3 gatos.` → `3` is a Number.
5. With forms-mini: `lemmas_of("buenas")` = `["bueno"]`; `lemmas_of("fue")` = `["ir","ser"]`;
   `gender_of("mesa")` = `f`; `gender_of("Tisch")` = `m`.
6. A de fixture skill allowing `aufstehen` (separable `auf`) accepts example `Ich stehe um sieben auf.`
   under V12; without that lemma it fails V12 on `stehe`.
7. `derive_items(valid-es)` → the items in `expected-items.json` (ids, kinds, targets `el día`, `la
   noche`, accepted answers), in syllabus order.
8. `parley pack build --lang es --tatoeba-dir tests/fixtures/tatoeba-mini --wiktextract
   tests/fixtures/forms-mini.jsonl --syllabus tests/fixtures/syllabus/valid-es.yaml --built-at
   2026-10-01 --out <tmp>/es.sqlite3` → `{"lang":"es","kept":22,"dropped":{"no_translation":5,"too_short":1,"too_long":3,"excluded_lemma":2,"unknown_word":5,"duplicate":2}}`
   (the fixture README assigns each of the 40 sentences its fate); sentence 1001 is kept with English
   2001 and author `fixture_a`.
9. Two builds from the same inputs give the same `content_sha256` in `parley pack info`; its output
   has every key of the Interfaces format, `licence` = `CC BY 2.0 FR`, `sentences` = 22, and one
   `source_sha256` entry per input file.
10. `verify_pack` on a copy with one changed byte → `ChecksumMismatch` displaying "The download was
    damaged. Try again."; on the original → `Ok`.
11. `parley content validate content` → exit 0 with no errors (the seed content).
12. `packs.lock.toml` with a missing `sha256` → `packs.lock.toml: [es] needs sha256`.
13. `parley forms extract --lang es --wiktextract tests/fixtures/forms-mini.jsonl --lemma bueno
    --lemma noche --lemma ir --out <tmp>/forms.tsv` → `{"lemmas":3,"rows":<as in the README>,"missing":[]}`
    and exactly the rows of `expected-forms.tsv` (written from the fixture's entries), with the
    source SHA-256 in the header line; `--lemma zzz` → `"missing":["zzz"]`.
14. `content/LICENSE` names the licence ADR-0003 accepted; `content/ATTRIBUTION.md` credits Tatoeba
    (CC BY 2.0 FR) and Wiktionary (CC BY-SA).
15. Stable ids: deleting example 2 of `valid-es` leaves items `#e:1`, `#e:3` and `#e:4` with the same
    ids; V21 then reports `#e:2` until `parley content lock --lang es` prints `{"added":[],"retired":
    ["es.a1.u01.s01#e:2"]}`; a new example with `id: 2` → V20; with `id: 5` → V21 until locked, then valid.
16. `parley pack build` without `--out` → exit 2 and `error: --out is required. Run parley --help.`;
    with a missing Tatoeba file → exit 2 and
    `error: tests/fixtures/tatoeba-mini/spa-eng_links.tsv not found`.

## Status

Planned
