# P4 (explain my answer) response fixtures

Hand-written answers to the explain prompt of `docs/PUBLIK-INTEGRATION.md` §6.6. Each file is the
JSON the model replies with (the test sends it as the reply text). Words are counted as UAX #29
words (`unicode_words`): runs of letters and digits, a word with an apostrophe between two letters
(`l'article`) counting once, punctuation not counted.

Every file answers the §6.6 example request: the Spanish course at A1, a `translate_to_target`
exercise ("translate into Spanish") with PROMPT "I would like a coffee with milk.", EXPECTED
"Quiero un café con leche.", LEARNER ANSWER "Me gustaría una café con leche", VERDICT almost, TAGS
gender, and the grammar note of `es.a1.u03.s01` in `tests/fixtures/course07/es/` ("Every noun is
masculine or feminine. Use el and un with masculine nouns, la and una with feminine nouns.").

| File | Rule | What it is |
|---|---|---|
| `valid.json` | none | the §6.6 example response: an English explanation of 29 words and a tip of 8 |
| `E1-extra-field.json` | E1 | the valid answer with a third field, `example`; the schema allows only `explanation` and `tip` |
| `E2-81-words.json` | E2 | an English explanation of 81 words, one over the limit of 80 (tip 8 words): You, wrote, una, café, but (5), café, is, a, masculine, noun (10), in, Spanish, Masculine, nouns, take (15), the, masculine, articles, un, for (20), a, and, el, for, the (25), while, feminine, nouns, take, una (30), and, la, Many, nouns, that (35), end, in, o, are, masculine (40), and, many, that, end, in (45), a, are, feminine, but, café (50), ends, in, e, so, you (55), simply, have, to, learn, its (60), gender, The, rest, of, your (65), sentence, is, right, and, me (70), gustaría, is, a, polite, way (75), to, ask, for, something, you (80), want (81) |
| `E2-tip-16-words.json` | E2 | the valid explanation (29 words) with a tip of 16 words, one over the limit of 15: Learn, every, new, noun, together (5), with, its, article, so, you (10), always, know, if, it, is (15), masculine (16) |
| `E3-french.json` | E3 | the explanation is French (35 words, no tip) |
