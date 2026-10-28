# Syllabus fixtures

Hand-written test content for module 01 (`docs/modules/01-content-model-and-syllabus.md`),
released under CC0. Every value the tests expect is stated here, so it can be checked by reading,
never by running the validator.

## Shared files

- `valid-es.yaml`: the syllabus example of the spec, byte for byte (UTF-8, LF, NFC). One unit,
  one skill `es.a1.u01.s01` of kind `function`, seven lemmas (`hola`, `adiós`, `bueno`, `día`,
  `noche`, `tal`, `gracias`) and four examples with ids 1 to 4. It validates with no error and no
  warning: `{"ok":true,"errors":[],"warnings":[]}`.
- `forms.tsv`: a hand-written Spanish forms table (`# source: hand-written; sha256: -;
  date: 2026-10-27`) with five rows: `buena`, `buenas`, `buenos` → `bueno` (adj), `días` → `día`
  (noun, m), `noches` → `noche` (noun, f). Every other word the fixtures use is a lemma's own
  citation form, a function word, a name or a number word. It sits beside every single-file
  fixture, so `parley content validate <file>` picks it up by itself.
- `numbers.yaml`: `parley.numbers/1`, Spanish, 101 entries from `cero` (0) to `cien` (100),
  entry index = number (`treinta y uno` at 31). The number word set it yields has 39 words,
  including `y`. The last line is `  - cien`, so removing it leaves 100 entries, which must
  give `numbers.yaml: words has 100 entries; 0 to 100 need 101`.
- `exclude.txt`: one comment line, then `fumar`, `matar`, `odiar`. `fumar` is the lemma
  `bad-V16.yaml` teaches and the lemma behind the two `excluded_lemma` drops of
  `tests/fixtures/tatoeba-mini`; no other fixture teaches or uses any of the three.
- `expected-items.json`: what `derive_items(valid-es, forms.tsv)` gives, in syllabus order: the
  seven vocabulary items in lemma order, then the four phrase items (the skill's kind is
  `function`, so examples are phrases, not patterns). Noun targets carry the definite article
  (`el día`, `la noche`); every other vocabulary target is the lemma. `accepted.to_target` is
  the target, `accepted.to_en` is the `en` list. Phrase `lemmas` are the syllabus lemmas the
  target uses, in token order: `#e:1` `[hola, tal]` (`qué` is a function word), `#e:2`
  `[bueno, día]` (through `buenos`, `días` in `forms.tsv`), `#e:3` `[bueno, noche]`, `#e:4`
  `[adiós, gracias]`. All items are `cefr` A1.
- `expected-forms.tsv`: the exact file `parley forms extract --lang es --wiktextract
  tests/fixtures/forms-mini.jsonl --lemma bueno --lemma noche --lemma ir --date 2026-10-01`
  writes: a header carrying the SHA-256 of `tests/fixtures/forms-mini.jsonl` as checked in,
  then 15 rows sorted by (form, lemma, pos) in byte order. `tests/fixtures/forms-mini.README.md`
  lists the rows.

## One fixture per rule

Each `bad-VNN` fixture is `valid-es.yaml` with the smallest change that breaks rule VNN and no
other rule, so the validator reports exactly one error (V17: one warning, `ok` true). The
expected message is the spec's text with this fixture's ids filled in. Single files are
validated as `parley content validate tests/fixtures/syllabus/bad-VNN.yaml --json`; the three
directory fixtures with the directory form (see below). Exit code 1 for every error case, 0
for V17.

| Fixture | The one change | Expected message |
|---|---|---|
| `bad-V01.yaml` | `version: "one"` | `tests/fixtures/syllabus/bad-V01.yaml: /version: expected an integer` |
| `bad-V02/es/syllabus.yaml` | a French syllabus (`lang: fr`, ids `fr.a1.u01.s01`, French function words and names, examples made only of citation forms, names and function words, since no forms table lies beside it) placed in a folder named `es` | `tests/fixtures/syllabus/bad-V02/es/syllabus.yaml: lang is "fr" but the folder is "es"` |
| `bad-V03.yaml` | skill id `es.a1.u01.s1` | `es.a1.u01.s1: ids look like es.a1.u01.s01` |
| `bad-V04.yaml` | a second unit whose id is also `es.a1.u01`, holding the Food skill (below) as `es.a1.u01.s02` | `duplicate id es.a1.u01` |
| `bad-V05.yaml` | the first unit becomes `es.a2.u01`, A2 (its skill `es.a2.u01.s01`, A2); a second unit `es.a1.u02`, A1, holds the Food skill as `es.a1.u02.s01` | `unit es.a1.u02 is A1 after an A2 unit` |
| `bad-V06.yaml` | `es.a1.u01.s01` requires `es.a1.u01.s02`, the Food skill that follows it | `es.a1.u01.s01: requires es.a1.u01.s02, which is not an earlier skill` |
| `bad-V07.yaml` | `can_do: Say hello and goodbye.` | `es.a1.u01.s01: can_do must start with "I can" and end with a full stop` |
| `bad-V08.yaml` | the grammar note grows to exactly 121 whitespace-separated words | `es.a1.u01.s01: grammar_note has 121 words; the limit is 120` |
| `bad-V09.yaml` | 14 more lemmas, none used by an example: 21 lemmas | `es.a1.u01.s01: 21 lemmas; a skill has 3 to 20` |
| `bad-V10.yaml` | example 4 removed: 3 examples | `es.a1.u01.s01: 3 examples; a skill has 4 to 15` |
| `bad-V11.yaml` | `día` loses its `gender` | `es.a1.u01.s01: noun día needs a gender (m or f)` |
| `bad-V12.yaml` | example 5 `Buenas tardes.`; `tardes` is in no list and no table | `es.a1.u01.s01: example 5 uses "tardes", which no earlier skill teaches` |
| `bad-V13.yaml` | example 5 `Ana y Luis.`: two names and a function word | `es.a1.u01.s01: example 5 uses none of this skill's lemmas` |
| `bad-V14.yaml` | example 5 `¡Buenos días!`, the same normalised tokens as example 2 | `es.a1.u01.s01: example 5 repeats es.a1.u01.s01 example 2` |
| `bad-V15.yaml` | example 2 has `en: []` | `es.a1.u01.s01: example 2 needs at least one English translation` |
| `bad-V16.yaml` | an eighth lemma `fumar`, which `exclude.txt` lists | `es.a1.u01.s01: fumar is on the exclude list` |
| `bad-V17.yaml` | the Food skill `es.a1.u01.s02` teaches `hola` again | warning `warning: hola is taught in es.a1.u01.s01 and es.a1.u01.s02`; `ok` true, exit 0 |
| `bad-V18.yaml` | example 2's target is written `"Buenos di\u0301as."`, a YAML escape for a combining acute accent, so the file itself is NFC and only that target decomposes | `es.a1.u01.s01: example 2 is not in Unicode NFC; save the file as NFC` |
| `bad-V19.yaml` | example 3 is given `id: 2` | `es.a1.u01.s01: example id 2 is used twice` |
| `bad-V20/es/` | `valid-es.yaml` verbatim beside an `ids.lock` holding all 11 item ids, with `es.a1.u01.s01#e:2` retired | `es.a1.u01.s01#e:2 was retired; give the example a new id` |
| `bad-V21/es/` | `valid-es.yaml` verbatim beside an `ids.lock` holding 10 of the 11 ids, `es.a1.u01.s01#e:4` missing | `es.a1.u01.s01#e:4 is not in ids.lock; run parley content lock --lang es` |

The Food skill used by V04, V05, V06 and V17 is a `vocab` skill with four nouns (`pan`, `agua`
with `article: el`, `café`, `leche`) and four examples made of those citation forms, function
words and the name `Ana`, so it passes every rule by itself and shares no lemma with the
Greetings skill (V17 adds `hola` to it on purpose).

Directory fixtures are validated as `parley content validate tests/fixtures/syllabus/bad-V02
--json` (the directory form reads `<dir>/<lang>/syllabus.yaml`; only `es` exists). `bad-V20`
and `bad-V21` hold the example syllabus verbatim, whose examples need `buenos`, `días`, `buenas`
and `noches` from a forms table, so their cases pass `--forms tests/fixtures/syllabus/forms.tsv`;
`numbers.yaml` and `exclude.txt` are not beside them and are not needed. V20 and V21 run only
because an `ids.lock` lies beside the syllabus; V02 runs because the folder is named `es`.

Both lock files are sorted by item id in byte order (`#e:` before `#l:`), tab-separated, with
version `1` as first seen.

The V18 fixture's example 2 (`Buenos di\u0301as.`) still passes V12 because the tokeniser
normalises to NFC before the forms lookup, and V14 because no other example has those tokens.
