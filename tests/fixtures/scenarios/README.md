# Scenario fixtures

Hand-written test content for module 05 (`docs/modules/05-course-content-a1-a2.md`, rules SC01
to SC06), released under CC0. Every value the tests expect is stated here, so it can be checked
by reading, never by running the checker. UTF-8, NFC, LF.

## The mini course they are checked against: `content/es/`

`content/es/syllabus.yaml` is a Spanish course (`lang: es`, in a folder named `es`) of two A1
units and three skills, in this syllabus order:

| skill | lemmas | examples |
|---|---|---|
| `es.a1.u01.s01` Greetings | hola, adiós, bueno, día | Hola, Ana. / Buenos días. / Adiós, Luis. / Buenos días, Ana. |
| `es.a1.u02.s01` Drinks | café, leche, agua (`article: el`), querer | Quiero un café. / Un café con leche. / Ana quiere agua. / Quiero agua. |
| `es.a1.u02.s02` Ordering | poner, cuánto, ser, gracias | ¿Qué le pongo? / ¿Cuánto es? / Un café, gracias. / Es un café con leche. |

Function words: el, la, un, una, y, de, a, en, con, que, qué, le, me, usted. Names: Ana, Luis,
Madrid. `content/es/numbers.yaml` is a copy of `tests/fixtures/syllabus/numbers.yaml` (Spanish 0
to 100 in words, `dos` at 2). Every rank is null and there is no `review.yaml`,
`checklist.yaml`, `ids.lock` or `exclude.txt`, so only V01 to V19 apply, and the syllabus passes
all of them:
`content/es/forms.tsv` (hand-written, six rows) maps `buenos` → bueno, `días` → día, `es` →
ser, `pongo` → poner, `quiere` and `quiero` → querer; every other word of an example is a
citation form, a function word or a name.

A word of an opening is taught (SC06) when it is a function word or a name of this syllabus, a
number (a digit, or a word of `numbers.yaml` beside the syllabus), or a form of a lemma of any
skill up to and including the latest listed skill in syllabus order.
So skills `[es.a1.u02.s01]` teach `Hola` and `Buenos días` (from the earlier Greetings skill)
but not `pongo`; skills `[es.a1.u01.s01]` teach neither `pongo` nor `café`.

## One fixture per rule

Checked one at a time as `parley content scenarios tests/fixtures/scenarios/<file> --content
tests/fixtures/scenarios/content`. Each gives exactly the messages below, in this order, with
`"warnings": []`; exit code 1 for every `bad-` file, 0 for the two `valid-` files.

| File | What is wrong | Messages |
|---|---|---|
| `valid-cafe.yaml` | nothing: id `valid-cafe`, 3 goals and `turn_cap: 6` (the lower bounds), opening `¡Buenos días! ¿Qué le pongo?`, skills listed out of syllabus order `[es.a1.u02.s02, es.a1.u02.s01]` (the latest in syllabus order is `es.a1.u02.s02`, which teaches `pongo`) | none: `{"ok":true,"errors":[],"warnings":[]}` |
| `valid-numbers.yaml` | nothing: skills `[es.a1.u02.s01]`, opening `¡Hola! ¿Quiere dos con leche y 3 con agua?`, where `dos` is a word of `numbers.yaml` and `3` a digit; every other word is taught (`Hola` by the earlier Greetings skill, `Quiere` a form of querer, `leche`, `agua`, and the function words `con`, `y`) | none: `{"ok":true,"errors":[],"warnings":[]}` |
| `bad-SC01.yaml` | no `title_en`; its id `cafe` also differs from the file name, but an SC01 failure stops the other rules | `tests/fixtures/scenarios/bad-SC01.yaml: /: title_en is required` |
| `bad-SC02.yaml` | `id: cafe`; otherwise valid, with 6 goals and `turn_cap: 20` (the upper bounds) | `tests/fixtures/scenarios/bad-SC02.yaml: id must match the file name` |
| `bad-SC03.yaml` | 7 goals | `bad-SC03: 7 goals; a scenario has 3 to 6` |
| `bad-SC04.yaml` | `turn_cap: 21` | `bad-SC04: turn_cap 21 is outside 6 to 20` |
| `bad-SC05.yaml` | a `fr` section (written first) listing `fr.a1.u03.s01` while the mini course has no French syllabus, and an `es` section listing `[es.a1.u09.s01, es.a1.u02.s02, es.a1.u01.s07]`, two of which do not exist. Languages are checked es, fr, de, skills in list order. The `es` opening uses `señor`, which no skill teaches, but SC06 runs only for a language whose skills all exist, so it is not reported | `bad-SC05: es: skill es.a1.u09.s01 does not exist`, `bad-SC05: es: skill es.a1.u01.s07 does not exist`, `bad-SC05: fr: skill fr.a1.u03.s01 does not exist` |
| `bad-SC06.yaml` | skills `[es.a1.u01.s01]` and the opening `¡Buenos días! ¿Qué le pongo? ¿Café?`: `pongo` (poner, taught later in `es.a1.u02.s02`) and `Café` (café, taught later in `es.a1.u02.s01`; at sentence start, so reported with its capital as written) | `bad-SC06: es: opening uses "pongo", which the listed skills do not teach`, `bad-SC06: es: opening uses "Café", which the listed skills do not teach` |

## The whole folder

`parley content scenarios tests/fixtures/scenarios --content tests/fixtures/scenarios/content`
checks every `*.yaml` of the folder in byte order of file name (`bad-SC01.yaml` to
`bad-SC06.yaml`, then `valid-cafe.yaml` and `valid-numbers.yaml`; `README.md` and the
`content/` folder are not scenarios), so it prints the messages of the table above in that
order, nine errors in all, and exits 1.

## Scenarios inside a course folder

`content/scenarios/shop.yaml` lies inside the mini course, so `parley content validate
tests/fixtures/scenarios/content --json` checks it after the Spanish syllabus (which has no
error and no warning). Its id matches its file name and its opening `¡Hola! ¿Quiere agua?` is
taught by `[es.a1.u02.s01]` (`Hola` from the earlier Greetings skill, `Quiere` a form of
querer), but it has 2 goals and `turn_cap: 5`, so the result is exit 1 and

    {"ok":false,"errors":["shop: 2 goals; a scenario has 3 to 6","shop: turn_cap 5 is outside 6 to 20"],"warnings":[]}
