# P1 (fresh lesson) response fixtures

Hand-written answers to the lesson prompt of `docs/PUBLIK-INTEGRATION.md` §6.3. Every file answers
the same request: the course in `tests/fixtures/course07/es/`, skill `es.a1.u03.s01` ("Food and
drink", A1, at most 8 words a sentence) with `es.a1.u01.s01` introduced, so the allowed lemma list
is the §6.3 example's, and this five-entry plan (the §6.3 example plan):

| n | item_id | type | facet | focus |
|---|---|---|---|---|
| 1 | `es.a1.u03.s01#l:café` | `pick_meaning` | receptive | el café |
| 2 | `es.a1.u03.s01#l:agua` | `translate_to_target` | productive | el agua |
| 3 | `es.a1.u03.s01#e:2` | `translate_to_en` | receptive | Quiero un café con leche. |
| 4 | `es.a1.u03.s01#l:pan` | `fill_gap` | productive | el pan |
| 5 | `es.a1.u03.s01#l:té` | `match_pairs` | receptive | el té |

`valid.json` is the §6.3 example response; every exercise passes L1 to L9. Each other file changes
that response so that it breaks one rule (the first rule the checks meet, in the order L2 to L9):

| File | Rule | What is wrong |
|---|---|---|
| `L1-not-json.json` | L1 | the reply is prose (this file holds it as a JSON string) |
| `L2-seven-exercises.json` | L2 | 7 exercises for 5 entries: positions 6 and 7 have no plan entry |
| `L3-wrong-item-id.json` | L3 | exercise 2 names `#l:leche` where the plan has `#l:agua` |
| `L4-english-target.json` | L4 | exercise 2's Spanish target is the English "I want water please now" |
| `L5-uncovered-word.json` | L5 | "Quiero agua con hielo, por favor.": 6 words, `hielo` not allowed, 5 of 6 covered (83%) |
| `L5-ninety-percent-a2.json` | none at A2 | "Yo quiero agua con hielo y un té sin azúcar.": 10 words, 9 covered (90%), kept; 10 words only fit at A2 (limit 12) |
| `L6-target-not-accepted.json` | L6 | exercise 2's target is not among its accepted answers |
| `L6-two-gaps.json` | L6 | exercise 4's sentence has two gaps |
| `L6-two-word-answer.json` | L6 | exercise 4's answer is two words |
| `L6-full-sentence-mismatch.json` | L6 | exercise 4's gap filled is not its full sentence |
| `L6-duplicate-options.json` | L6 | exercise 1's options "a coffee" and "A coffee." normalise alike |
| `L6-duplicate-pairs.json` | L6 | exercise 5's right side has "tea" twice |
| `L7-duplicate-target.json` | L7 | exercise 3's sentence equals exercise 4's full sentence, so exercise 4 is dropped |
| `L8-nine-words.json` | L8 | exercise 2's target has 9 words at A1 (limit 8) |
| `L9-no-focus.json` | L9 | exercise 2 practises `agua` but its sentence has no form of it |
| `three-dropped.json` | L9, L6, L6 | exercises 2, 4 and 5 break a rule each: more than half dropped |

`recorded/` holds whole requests and answers for the replay of gate G1.
