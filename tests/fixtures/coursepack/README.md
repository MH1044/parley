# Course pack fixtures (`parley.coursepack/1`)

For the course in `tests/fixtures/course07/es/`.

| File | What it is | Import into a Spanish course gives |
|---|---|---|
| `es-valid.json` | one lesson of `es.a1.u03.s01` with the five exercises of `tests/fixtures/ai/P1/valid.json` (ids `x1` to `x5`, origin `ai`) and the story of `tests/fixtures/ai/P2/valid.json` | 1 lesson, 1 story, nothing dropped |
| `es-tampered.json` | the same, with exercise `x2` changed to "Quiero agua con hielo, por favor." (`hielo` is not a word of the course: 10 of 12 counted words covered, 83%) and an exercise `x6` whose item `es.a1.u09.s01#l:hielo` the course does not have | 1 lesson with 4 exercises, 1 story, 2 dropped |
| `fr-empty.json` | a valid French pack with no lessons or stories | "This course pack is for French; your active course is Spanish." |
| `not-a-pack.json` | `{"schema": "x"}` | "This is not a Parley course pack." |

A file over 5 MB is made at test time in a temporary folder, never committed.
