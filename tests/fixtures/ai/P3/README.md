# P3 (answer judge) response fixtures

Hand-written answers to the judge prompt of `docs/PUBLIK-INTEGRATION.md` §6.5. Each file is the
JSON the model replies with (the test sends it as the reply text). Words are counted as UAX #29
words (`unicode_words`): runs of letters and digits, punctuation not counted.

## The requests

All three are for the Spanish course at A1 (`tests/fixtures/course07/es/`).

| Request | Direction | SOURCE | ACCEPTED | ANSWER |
|---|---|---|---|---|
| R1, the §6.5 example | `to_target` | "I would like a coffee with milk." | "Quiero un café con leche.", "Quisiera un café con leche." | "Me gustaría una café con leche" |
| R2 | `to_target` | the same | the same | "Me gusta un té con leche" |
| R3 | `to_target` | the same | the same | "Me gustaría un café con leche." |
| R4 | `to_en` | "Quiero un café con leche." (the item `es.a1.u03.s01#e:2` itself, so its own prompt) | "I want a coffee with milk.", "I'd like a white coffee.", "I want a white coffee." | "I would like a coffee with milk." |

None of the answers is matched offline: R1 to R3 have 6 words where every accepted answer has 5,
and R4 has 7 where its accepted answers (and the item's own English texts) have 5 or 6. No answer
equals an accepted one or differs from it only in accents, and the typo rule of module 03 needs the
same number of words, so the grade is `unchecked` and the judge is asked. The app tests also send R1 and R2 for
the same exercise with "Quiero un café con leche." as its only accepted answer.

## Valid answers

| File | For | Verdict | What it shows |
|---|---|---|---|
| `almost.json` | R1 | almost | the §6.5 example response: corrected "Me gustaría un café con leche" (6 words, Spanish), explanation 9 words, tag `gender` |
| `almost-25-words.json` | R1 | almost | the same with an explanation of exactly 25 words, the most J2 allows |
| `wrong.json` | R2 | wrong | corrected "Me gustaría un café con leche" (6 words, Spanish), explanation 19 words, tags `wrong_word` and `conjugation` |
| `correct.json` | R4 | correct | corrected "I would like a coffee with milk." (7 words, English), explanation 12 words, no tags |

## Failing answers

Each breaks exactly one rule; the rest is the valid answer for its request.

| File | For | Rule | What is wrong |
|---|---|---|---|
| `J1-missing-verdict.json` | R1 | J1 | `verdict` is missing, and the schema requires it |
| `J1-unknown-tag.json` | R1 | J1 | `error_tags` holds `grammar`, which is not one of the twelve tags of §6.5 |
| `J2-26-words.json` | R1 | J2 | the explanation has 26 words: Café, is, a, masculine, noun (5), so, it, takes, the, masculine (10), article, un, and, not, the (15), feminine, article, una, the, rest (20), of, your, answer, is, natural (25), Spanish (26) |
| `J3-english-corrected.json` | R1 | J3 | `corrected` is "I want coffee with milk": 5 words (I, want, coffee, with, milk), four or more, and English where the answer language is Spanish |
| `J4-correct-with-tags.json` | R3 | J4 | `correct` with the tags `accent` and `register`: the verdict is kept and both tags are dropped (the answer itself passes J1 to J3: corrected 6 words of Spanish, explanation 15 words) |

`almost-25-words.json` differs from `J2-26-words.json` only in its last words: "is right" (24, 25)
in place of "is natural Spanish" (24 to 26).

`recorded/` holds the replies of the gate G2 cases replayed in the normal suite: one file per case,
`{"case", "request", "content", "ms"}`, written by the live gate.
