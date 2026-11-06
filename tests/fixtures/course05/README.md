# course05

Hand-written fixture courses for module 05 (`docs/modules/05-course-content-a1-a2.md`): rule V22,
the checklist rules CL02 to CL04, the review rules RV02 to RV04, `parley content report`,
`parley freq check`, and the wiktextract metadata filter. Released under CC0. Every value the
tests expect is stated here, worked out by hand from the spec's rules, these files and the
module 01 fixtures, never by running the code. UTF-8, NFC, LF.

Each course is a folder with a Spanish course in `es/`, validated with the directory form,
`parley content validate tests/fixtures/course05/<folder> --json`. None has an `ids.lock`,
`numbers.yaml` or `exclude.txt`, so V16, V20 and V21 have nothing to report. Every `forms.tsv`
is hand-written (`# source: hand-written; sha256: -; date: 2026-11-04`) and holds exactly the
inflected forms the examples use; every other word of an example is a citation form, a
function word (el, la, los, las, un, una, y, de, a, en, con, no, que, qué) or a name (Ana,
Luis, Carmen, Pablo, Madrid). `al` and `del` pass as function words through their lemma keys
`a`, `el` and `de`.

## The fixture pack

Built at test time, never committed, from the module 01 fixtures:

    parley pack build --lang es --tatoeba-dir tests/fixtures/tatoeba-mini
        --wiktextract tests/fixtures/forms-mini.jsonl
        --syllabus tests/fixtures/syllabus/valid-es.yaml --built-at 2026-11-05
        --out <tmp>/es-2026-11-05.sqlite3

`tests/fixtures/tatoeba-mini/README.md` lists what it holds: the 22 kept sentences 1001 to 1022
and the `freq` table of 24 ranked lemmas (ser 1, tener 2, bueno 3, gato 4, ir 5, café 6,
casa 7, comer 8, amigo 9, grande 10, hablar 11, libro 12, mercado 13, mesa 14, noche 15, pan 16,
pueblo 17, a 18, agua 19, de 20, día 21, el 22, estar 23, hola 24). A sentence's `norm` is its
tokens' normalised forms joined by single spaces (`Hola, soy Ana.` → `hola soy ana`).

## `course/`: the report and freq check course

`course/es/syllabus.yaml`: three units, four skills, 16 examples, every single-word lemma
ranked, A1 ranks at most 2,900.

| skill (unit, level) | lemmas with their `rank` | examples (attested ones marked *) |
|---|---|---|
| `es.a1.u01.s01` (u01, A1) | hola 24, adiós 2900, bueno 3, día 21, ser 1, `buenas noches` (expr) null | *Hola, soy Ana. (1001) / *Buenos días, Luis. (1005) / Adiós, Ana. / Buenas noches, Luis. |
| `es.a1.u01.s02` (u01, A1) | gato 4, mesa 14, casa 9, grande 10, estar 23, perro 1200 | *La mesa es grande. (1006) / *Los gatos están en la mesa. (1017) / El perro está en casa. / El gato es grande. |
| `es.a1.u02.s01` (u02, A1) | pan 16, café 6, agua 19, comer 8, tener 2, leche 1500 | *El gato come pan. (1003) / *Tenemos agua y café. (1010) / Ana come pan con leche. / Luis tiene café. |
| `es.a2.u01.s01` (u01, A2) | mercado 13, pueblo 17, ir 5, casa 7, amigo 30, hablar 11 | *Voy al mercado del pueblo. (1002) / *Luis habla con un amigo. (1009) / *El pueblo tiene un mercado. (1016) / Ana va a casa. |

`casa` is taught again at A2, on purpose. `checklist.yaml` has six entries, two with no skills
(`a1.g.plural_nouns`, then `a2.f.directions`); every mapped skill exists and has its entry's
level. `review.yaml` lists the three units out of syllabus order: `es.a2.u01` unreviewed,
`es.a1.u01` reviewed, `es.a1.u02` unreviewed.

**Validate** → exit 0 and one V17 warning, nothing from V22, CL or RV:

    {"ok":true,"errors":[],"warnings":["warning: casa is taught in es.a1.u01.s02 and es.a2.u01.s01"]}

**`parley content report --lang es --content tests/fixtures/course05/course --json`** → exit 0:

    {"lang":"es","units":{"A1":2,"A2":1},"skills":4,"skills_per_unit":{"min":1,"max":2},
     "lemmas":{"A1":18,"A2":5},"examples":16,"attested_share":null,"a1_rank_le_2000_share":null,
     "checklist_uncovered":["a1.g.plural_nouns","a2.f.directions"],
     "unreviewed_units":["es.a1.u02","es.a2.u01"]}

- `units`: u01 and u02 are A1, the A2 unit is one; `skills_per_unit`: es.a1.u01 has 2 skills,
  the other two units 1 each.
- `lemmas.A1`: the 18 distinct lemma strings of the three A1 skills (6 each, `buenas noches`
  included). `lemmas.A2`: the A2 skill's six lemmas less `casa`, taught at A1: 5.
- `unreviewed_units` is in syllabus order, not in the file's order.

**With `--pack <the fixture pack>`** the same, except:

- `attested_share` = 9 / 16 = **0.5625**: the nine starred examples have the norm of a kept
  sentence (the sentence id is in brackets); the seven others match none.
- `a1_rank_le_2000_share` = 14 / 17 = 0.823529… → **0.8235**: the 17 distinct single-word A1
  lemmas are hola, adiós, bueno, día, ser, gato, mesa, casa, grande, estar, perro, pan, café,
  agua, comer, tener, leche; all but adiós, perro and leche are in the pack's `freq` (ranks 1 to
  24, all at most 2,000), and a lemma absent from `freq` counts as above.

**`parley freq check --lang es --pack <the fixture pack> --content tests/fixtures/course05/course`**
→ exit 1:

    {"lang":"es","lemmas":23,
     "differs":[{"skill":"es.a1.u01.s02","lemma":"casa","rank":9,"pack_rank":7},
                {"skill":"es.a2.u01.s01","lemma":"amigo","rank":30,"pack_rank":9}],
     "missing":[{"skill":"es.a1.u01.s01","lemma":"adiós"},
                {"skill":"es.a1.u01.s02","lemma":"perro"},
                {"skill":"es.a1.u02.s01","lemma":"leche"}]}

`lemmas` counts the 23 single-word lemma entries (5 + 6 + 6 + 6; `buenas noches` has a space and
is not checked; `casa` counts twice). Every other entry's rank equals its `freq` rank. A lemma
absent from `freq` is listed under `missing` only, since it has no pack rank to differ from.

## `ranked/`: a course whose ranks all match

`ranked/es/syllabus.yaml` is a mini course of two units: `es.a1.u01.s01` (A1; hola 24, bueno 3,
día 21, ser 1; examples Hola, soy Ana. / Buenos días, Luis. / Hola, Luis. / Buenos días, Ana.)
and `es.a2.u01.s01` (A2; mercado 13, pueblo 17, ir 5; examples Voy al mercado del pueblo. / Ana
va al mercado. / Luis va al pueblo. / Voy al pueblo.). `review.yaml` lists both units.

- Validate → exit 0, `{"ok":true,"errors":[],"warnings":[]}`.
- `freq check` with the fixture pack → exit 0, `{"lang":"es","lemmas":7,"differs":[],"missing":[]}`.

## `bad-V22/`: rule V22

The `ranked` course with these changes, and a correct `review.yaml` beside it (so V22 runs):
`es.a1.u01.s01` also teaches `gato` (rank null), `mesa` (rank 3500) and `buenas noches` (expr,
rank null); in `es.a2.u01.s01`, `mercado` has rank 3500, `pueblo` rank null, and `ser` (rank 1)
is taught a second time.

Validate → exit 1:

    {"ok":false,
     "errors":["es.a1.u01.s01: gato needs a rank","es.a2.u01.s01: pueblo needs a rank"],
     "warnings":["warning: ser is taught in es.a1.u01.s01 and es.a2.u01.s01",
                 "warning: mesa has rank 3500; check that it belongs at A1"]}

- Errors in document order; the A2 lemma without a rank is an error too.
- `buenas noches` has no rank but a space in it, so it gives nothing.
- `mercado` has rank 3500 but is A2, so it gives no warning.
- The V22 warning comes after the V17 warning although `mesa` comes first in the file.

The same `syllabus.yaml` and `forms.tsv` copied without `review.yaml` (in testing, into a
temporary `es/` folder) → exit 0 and neither V22 message:
`{"ok":true,"errors":[],"warnings":["warning: ser is taught in es.a1.u01.s01 and es.a2.u01.s01"]}`.

`parley content report --lang es --content tests/fixtures/course05/bad-V22 --json` → exit 1 and
the validate output above.

`freq check` with the fixture pack → exit 1, `lemmas` 10 (7 + 4 entries less `buenas noches`),
`missing` empty, and `differs` in document order, a null rank included:
`{"skill":"es.a1.u01.s01","lemma":"gato","rank":null,"pack_rank":4}`,
`{"skill":"es.a1.u01.s01","lemma":"mesa","rank":3500,"pack_rank":14}`,
`{"skill":"es.a2.u01.s01","lemma":"mercado","rank":3500,"pack_rank":13}`,
`{"skill":"es.a2.u01.s01","lemma":"pueblo","rank":null,"pack_rank":17}`.

## `bad-CL/`: rules CL02 to CL04

The `ranked` syllabus and forms with this `checklist.yaml` and no `review.yaml` (so V22 does not
run): entry 1 `a1.f.greet`; entry 2 `a1.f.greet` again (same level, existing skill); entry 3
`a1.g.ser_present` mapped to `es.a1.u01.s01` and to `es.a1.u03.s01`, which does not exist;
entry 4 `a1.f.directions`, an A1 entry mapped to the A2 skill `es.a2.u01.s01`; entry 5
`a2.g.contractions`, correct. Validate → exit 1, `"warnings":[]`, and exactly:

    tests/fixtures/course05/bad-CL/es/checklist.yaml: duplicate entry a1.f.greet
    tests/fixtures/course05/bad-CL/es/checklist.yaml: a1.g.ser_present: skill es.a1.u03.s01 does not exist
    tests/fixtures/course05/bad-CL/es/checklist.yaml: a1.f.directions: skill es.a2.u01.s01 is A2, but the entry is A1

(rule order and document order agree).

## `bad-RV/`: rules RV02 to RV04

The `ranked` syllabus and forms (all ranks set, so V22 is quiet) with a `review.yaml` listing
`es.a1.u01` twice, `es.a1.u05` (no such unit) and not `es.a2.u01`. Validate → exit 1,
`"warnings":[]`, and exactly:

    tests/fixtures/course05/bad-RV/es/review.yaml: unit es.a1.u01 is listed twice
    tests/fixtures/course05/bad-RV/es/review.yaml: unit es.a1.u05 is not in the syllabus
    tests/fixtures/course05/bad-RV/es/review.yaml: unit es.a2.u01 is missing

## `bad-V01/`: a syllabus that does not load

The `ranked` syllabus without its `lang` line, the `ranked` forms, and beside them a checklist
whose second entry repeats `a1.f.greet` and maps it to `es.a1.u03.s01` (no such skill), and a
review file that lists `es.a1.u01` twice, then `es.a1.u05` (no such unit), and not
`es.a2.u01`. The syllabus fails V01, so the rules that need it (CL03, CL04, RV03, RV04, V22) are
left out, while the checklist and review file still get CL01, CL02, RV01 and RV02. Validate →
exit 1, `"warnings":[]`, and exactly:

    tests/fixtures/course05/bad-V01/es/syllabus.yaml: /: lang is required
    tests/fixtures/course05/bad-V01/es/checklist.yaml: duplicate entry a1.f.greet
    tests/fixtures/course05/bad-V01/es/review.yaml: unit es.a1.u01 is listed twice

- `parley content report --lang es --content tests/fixtures/course05/bad-V01 --json` → exit 1
  and that same validate output.
- `freq check` with the fixture pack → exit 1 and the V01 report only:
  `{"ok":false,"errors":["tests/fixtures/course05/bad-V01/es/syllabus.yaml: /: lang is required"],"warnings":[]}`.

## `bad-CL01-RV01/`: checklist and review file that break their schemas

The `ranked` syllabus and forms (all ranks set, so V22 is quiet), a checklist whose first entry
has no `text_en` (the second repeats its id, but CL01 stops the other checklist rules), and a
review file whose first entry names a reviewer (no names: an unknown field; the second entry
repeats the unit and `es.a2.u01` is not listed, but RV01 stops the other review rules).
Validate → exit 1, `"warnings":[]`, and exactly:

    tests/fixtures/course05/bad-CL01-RV01/es/checklist.yaml: /entries/0: text_en is required
    tests/fixtures/course05/bad-CL01-RV01/es/review.yaml: /units/0: unknown field reviewer

## `wiktextract-meta.jsonl`: the metadata filter

Three entries in wiktextract's line shape, with the table metadata the real extracts carry among
`forms`: entries tagged `table-tags`, `inflection-template`, `class` or `auxiliary`, which are not
forms of the word. 1,371 bytes, SHA-256 as checked in
`fd352e532a7ab6fad855c2b9aa40fb73e55a5dc99da6f9181f49418895f361ad`.

- `hablar` (es, verb): forms `irregular` (tags `table-tags`), `es-conj` (`inflection-template`),
  `hablo` and `habla`.
- `mesa` (es, noun, `feminine` in the entry's tags): forms `mesas` (`plural`), `es-noun`
  (`inflection-template`) and `regular` (`table-tags`).
- `sehen` (de, verb): forms `strong` (`table-tags`), `de-conj` (`inflection-template`),
  `5 strong` (`class`), `sieht`, `sah`, `haben` (`auxiliary`, the auxiliary of the conjugation
  table) and `gesehen`.

`parley forms extract --lang es --wiktextract tests/fixtures/course05/wiktextract-meta.jsonl
--lemma hablar --lemma mesa --date 2026-11-04 --out <tmp>/forms.tsv` → exit 0,
`{"lemmas":2,"rows":5,"missing":[]}`, and the file is the header
`# source: wiktextract-meta.jsonl; sha256: fd352e532a7ab6fad855c2b9aa40fb73e55a5dc99da6f9181f49418895f361ad; date: 2026-11-04`
then these rows (tab-separated, sorted by form, lemma and pos in byte order), with no row for
`irregular`, `es-conj`, `es-noun` or `regular`:

    habla   hablar  verb            indicative present singular third-person
    hablar  hablar  verb            canonical
    hablo   hablar  verb            first-person indicative present singular
    mesa    mesa    noun    f       canonical
    mesas   mesa    noun    f       plural

The same with `--lang de --lemma sehen` → exit 0, `{"lemmas":1,"rows":4,"missing":[]}`, the same
header, and these rows, with no row for `strong`, `de-conj`, `5 strong` or `haben`:

    gesehen sehen   verb            participle past
    sah     sehen   verb            first-person preterite singular
    sehen   sehen   verb            canonical
    sieht   sehen   verb            present singular third-person

Reading the whole file for Spanish, or for the two Spanish lemmas, gives the five Spanish rows
(the German entry is another language); for German, the four German rows.

## `parley pack install`

Uses the fixture pack and a lock written in testing beside it:

    schema = "parley.packs/1"
    [es]
    version = "2026-11-05"
    url = "https://github.com/MH1044/parley/releases/download/packs-2026-11-05/es-2026-11-05.sqlite3"
    sha256 = "<SHA-256 of the built file>"
    bytes = <its size>
    content_sha256 = "<its content_sha256 from parley pack info>"

- A copy with one byte changed (the last byte of the SQLite header's application id, which
  leaves it a readable pack) → exit 1, stderr `error: This pack does not match
  packs.lock.toml.`, nothing under `<data dir>/packs/`.
- The built file with no started course (an empty data directory) → exit 1, `error: Start the
  Spanish course in Parley first.`, and no `parley.sqlite3` and no pack written there.
- The built file once the Spanish course is started in `<data dir>/parley.sqlite3` → exit 0,
  `{"lang":"es","version":"2026-11-05","path":"packs/es-2026-11-05.sqlite3"}`, the file in
  place with the same bytes, no `.part` file, and `pack_status("es")` installed at that version.
- A second build with `--built-at 2026-11-06` and its own lock → the 2026-11-05 file is removed
  and 2026-11-06 is recorded.
- A lock whose `[es]` table has no `sha256` does not parse → exit 1, stderr
  `error: packs.lock.toml: [es] needs sha256` (the module 01 lock message), nothing written.

## A file that is not a pack

`content report --pack` and `freq check --pack` given a small text file that exists → exit 1,
one `error: ...` line on stderr, nothing on stdout.
