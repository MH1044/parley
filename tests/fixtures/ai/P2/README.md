# P2 (graded story) response fixtures

Hand-written answers to the story prompt of `docs/PUBLIK-INTEGRATION.md` §6.4 for the course in
`tests/fixtures/course07/es/`: the story of unit `es.a1.u03` ("Food and drink"), whose last skill is
`es.a1.u03.s01`, story 1, with `es.a1.u01.s01` introduced. At A1 the story has 3 paragraphs and 3
questions, and 80 to 120 words, so S4 accepts 72 to 132. The focus words of story 1 are the first
four single-word nouns, verbs and adjectives of the unit: café, té, agua, leche.

`valid.json` passes S1 to S6: 97 words (36, 33 and 28 by paragraph, "por favor" counting two), every
word an allowed lemma in some form or one of the names Ana, Luis and Madrid, every focus word used.
Each other file breaks one rule:

| File | Rule | What is wrong |
|---|---|---|
| `S1-two-options.json` | S1 | question 1 has 2 options; the schema asks for exactly 3 |
| `S2-english-paragraph.json` | S2 | paragraph 3 is English |
| `S3-uncovered-words.json` | S3 | paragraph 1 gains 13 words, 9 of them not allowed (hoy, hace, mucho, calor, ciudad twice, grande, muy, bonita): 81 of 90 counted words covered, 90%, under 92% |
| `S4-too-short.json` | S4 | 28 words, under 72 |
| `S5-focus-word-missing.json` | S5 | té never occurs (zumo in its place) |
| `S6-paragraph-out-of-range.json` | S6 | question 3 points at paragraph 3 of a 3-paragraph story |
