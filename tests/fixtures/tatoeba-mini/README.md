# tatoeba-mini

Forty Spanish and forty English sentences written for this project (CC0), in the column layout
of Tatoeba's per-language exports, so `parley pack build` can be tested offline with inputs
whose every outcome is known from this file. Nothing here comes from Tatoeba; the usernames
`fixture_a`, `fixture_b` and `fixture_c` are placeholders, and `\N` (Tatoeba's marker for a
sentence without an owner) appears on Spanish 1010 and English 2010, which the pack must store
as empty text.

Files (tab-separated, no header, LF):

- `spa_sentences_detailed.tsv`: `id, lang, text, username, added, modified`; ids 1001 to 1040,
  lang `spa`.
- `eng_sentences_detailed.tsv`: the same columns; ids 2001 to 2040, lang `eng`.
- `spa-eng_links.tsv`: `(spa id, eng id)`, 36 rows: every Spanish id except 1024, 1026, 1027
  and 1028 links to the English id 1000 higher, except 1025, which links to 2099, an id that
  is not in the English file.

## The build

    parley pack build --lang es --tatoeba-dir tests/fixtures/tatoeba-mini
        --wiktextract tests/fixtures/forms-mini.jsonl
        --syllabus tests/fixtures/syllabus/valid-es.yaml --built-at 2026-10-01 --out <tmp>/es.sqlite3

uses the function words and names of `valid-es.yaml`, the number words of the `numbers.yaml`
beside it, the exclude list `exclude.txt` beside it (`fumar`, `matar`, `odiar`) and the Spanish
entries of `forms-mini.jsonl` (84 rows: every headword and every inflected form). The filters
apply in the spec's order (no_translation, too_short, too_long, excluded_lemma, unknown_word,
duplicate), and the first one a sentence fails names its fate. Expected report:

    {"lang":"es","kept":22,"dropped":{"no_translation":5,"too_short":1,"too_long":3,"excluded_lemma":2,"unknown_word":5,"duplicate":2}}

## Every sentence and its fate

Words are UAX #29 tokens; `al` and `del` carry the lemma keys `a`+`el` and `de`+`el`. A kept
sentence uses only function words, names, number words or digits, `Name` tokens (a capitalised
word not at sentence start that the forms table does not know, such as `Valencia`) and forms of
`forms-mini` entries.

| id | Spanish | author | words | fate (why) |
|---|---|---|---|---|
| 1001 | Hola, soy Ana. | fixture_a | 3 | kept; English 2001 `Hello, I am Ana.` by fixture_b |
| 1002 | Voy al mercado del pueblo. | fixture_b | 5 | kept |
| 1003 | El gato come pan. | fixture_c | 4 | kept |
| 1004 | Tengo tres gatos en casa. | fixture_a | 5 | kept (`tres` is a number word) |
| 1005 | Buenos días, Luis. | fixture_b | 3 | kept |
| 1006 | La mesa es grande. | fixture_c | 4 | kept |
| 1007 | Carmen va a Madrid con Pablo. | fixture_a | 6 | kept |
| 1008 | El libro de Ana es bueno. | fixture_b | 6 | kept |
| 1009 | Luis habla con un amigo. | fixture_c | 5 | kept |
| 1010 | Tenemos agua y café. | `\N` | 4 | kept; author stored as empty text, English 2010's author too |
| 1011 | Las noches son buenas. | fixture_b | 4 | kept |
| 1012 | Vamos a la casa de Carmen en Valencia. | fixture_c | 8 | kept (`Valencia` is a `Name` token) |
| 1013 | Yo no como pan por la noche. | fixture_a | 7 | kept |
| 1014 | Ana tiene dos libros grandes. | fixture_b | 5 | kept (`dos` is a number word) |
| 1015 | Ella fue a Sevilla. | fixture_c | 4 | kept (`fue` has the lemma keys `ir` and `ser`) |
| 1016 | El pueblo tiene un mercado. | fixture_a | 5 | kept |
| 1017 | Los gatos están en la mesa. | fixture_b | 6 | kept |
| 1018 | Hablamos con Pablo en el café. | fixture_c | 6 | kept |
| 1019 | Tengo 3 gatos. | fixture_a | 3 | kept (`3` is a `Number` token and counts as a word) |
| 1020 | Luis come en casa de Ana. | fixture_b | 6 | kept |
| 1021 | El café de Pablo es bueno. | fixture_c | 6 | kept |
| 1022 | Somos amigos de Carmen. | fixture_a | 4 | kept |
| 1023 | EL GATO COME PAN. | fixture_b | 4 | duplicate (norm `el gato come pan` = 1003) |
| 1024 | El gato está en casa. | fixture_c | 5 | no_translation (no link) |
| 1025 | Tenemos un gato grande. | fixture_a | 4 | no_translation (linked to 2099, which does not exist) |
| 1026 | Luis va al mercado. | fixture_b | 4 | no_translation (no link) |
| 1027 | La casa es grande. | fixture_c | 4 | no_translation (no link) |
| 1028 | Fumo. | fixture_a | 1 | no_translation (no link; also too short and excluded, but no_translation comes first) |
| 1029 | Hola, Ana. | fixture_b | 2 | too_short (fewer than 3 words) |
| 1030 | Ana, Luis, Carmen y Pablo van al mercado del pueblo con el gato. | fixture_c | 13 | too_long (more than 12 words) |
| 1031 | Nosotros hablamos constantemente, apasionadamente, … sinceramente. | fixture_a | 12 | too_long (182 characters; its adverbs are unknown too, but too_long comes first) |
| 1032 | Ana va al mercado, Luis va al pueblo, Carmen va a Madrid y Pablo va a Sevilla con el gato. | fixture_b | 20 | too_long |
| 1033 | Luis fuma en casa. | fixture_c | 4 | excluded_lemma (`fuma` has the lemma key `fumar`) |
| 1034 | Ana va a fumar. | fixture_a | 4 | excluded_lemma (`fumar` itself) |
| 1035 | El perro come pan. | fixture_b | 4 | unknown_word (`perro`) |
| 1036 | Quiero un café. | fixture_c | 3 | unknown_word (`Quiero` is at sentence start, so not a `Name`) |
| 1037 | Ana tiene mucho pan. | fixture_a | 4 | unknown_word (`mucho`) |
| 1038 | Vamos al cine con Luis. | fixture_b | 5 | unknown_word (`cine`) |
| 1039 | El gato es negro. | fixture_c | 4 | unknown_word (`negro`) |
| 1040 | ¡Hola, soy Ana! | fixture_a | 3 | duplicate (norm `hola soy ana` = 1001) |

Kept: 1001 to 1022, in id order (22). Each kept sentence has exactly one translation, so the
`translations` table has 22 rows. The English texts 2001 to 2022 translate their Spanish
sentence; the English authors cycle fixture_b, fixture_c, fixture_a from 2001.

## Tables derived from the kept sentences

`sentence_lemmas` holds every lemma key of every token; `freq` counts, per lemma, the kept
sentences containing it and ranks by count descending, then lemma ascending in byte order
(`café` before `casa`, `día` after `de`). 24 ranked lemmas:

| rank | lemma | count | rank | lemma | count |
|---|---|---|---|---|---|
| 1 | ser | 7 | 13 | mercado | 2 |
| 2 | tener | 5 | 14 | mesa | 2 |
| 3 | bueno | 4 | 15 | noche | 2 |
| 4 | gato | 4 | 16 | pan | 2 |
| 5 | ir | 4 | 17 | pueblo | 2 |
| 6 | café | 3 | 18 | a | 1 |
| 7 | casa | 3 | 19 | agua | 1 |
| 8 | comer | 3 | 20 | de | 1 |
| 9 | amigo | 2 | 21 | día | 1 |
| 10 | grande | 2 | 22 | el | 1 |
| 11 | hablar | 2 | 23 | estar | 1 |
| 12 | libro | 2 | 24 | hola | 1 |

`a`, `de` and `el` are lemma keys only through `al` and `del` in 1002. Sentence 1001 has
`tokens` 3 and `max_rank` 24 (`hola`); 1002 has `max_rank` 22 (`el`); 1007 has `max_rank` 5
(`ir` alone, since `a` by itself has no lemma key).

`parley pack info` on the built pack: `schema` `parley.pack/1`, `lang` `es`, `built_at`
`2026-10-01`, `licence` `CC BY 2.0 FR`, `sentences` 22, `translations` 22, `forms` 84,
`ranked_lemmas` 24, and `source_sha256` keyed by file name: `spa_sentences_detailed.tsv`,
`eng_sentences_detailed.tsv`, `spa-eng_links.tsv`, `forms-mini.jsonl`, `valid-es.yaml` and,
as the exclude list comes from beside the syllabus, `exclude.txt`. Two builds of these inputs
give the same `content_sha256` whatever `--built-at` says.
