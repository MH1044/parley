# forms-mini.jsonl

Thirty dictionary entries written for this project (CC0) in the line shape `parley forms
extract` and `parley pack build` read from a wiktextract dump: one JSON object per line with
`word`, `pos`, `lang_code`, optional `tags`, `forms` (`{form, tags}`) and `senses` (optional
`tags`; the `glosses` are for the reader). UTF-8, NFC, LF, a final newline, no BOM, 8,469 bytes.

SHA-256 as checked in: `bdd2092585e56451edb8e59e844bfca7a0659c3cc5cf51c3716430dea3b33919`.
It is the hash a forms table extracted from this file carries in its header line.

## Entries

Spanish (`es`), 22 entries, 84 extracted rows (every headword plus every form that differs
from it):

| word | pos | gender (where it is found) | forms |
|---|---|---|---|
| bueno | adj | | bueno (a `canonical` entry equal to the headword, so no extra row), buena, buenos, buenas |
| noche | noun | f (entry `tags`) | noches |
| mesa | noun | f (the `canonical` forms entry's tags) | mesa (canonical), mesas |
| día | noun | m (the sense's `tags`) | días |
| ir | verb | | voy, vas, va, vamos, van, fui, fue, fueron |
| ser | verb | | soy, eres, es, somos, son, fui, fue, fueron, era |
| mercado | noun | m | mercados |
| pueblo | noun | m | pueblos |
| gato | noun | m | gatos, gata, gatas |
| tener | verb | | tengo, tienes, tiene, tenemos, tienen |
| casa | noun | f | casas |
| comer | verb | | como, comes, come, comemos, comen |
| pan | noun | m | panes |
| agua | noun | f | aguas |
| café | noun | m | cafés |
| libro | noun | m | libros |
| amigo | noun | m | amigos, amiga, amigas |
| grande | adj | | grandes |
| hablar | verb | | hablo, hablas, habla, hablamos, hablan |
| fumar | verb | | fumo, fumas, fuma, fumamos, fuman (the excluded lemma of the tatoeba-mini build) |
| hola | intj | | none |
| estar | verb | | estoy, estás, está, estamos, están |

German (`de`), 5 entries, 14 rows: Tisch (noun, m; Tische), stehen (verb; stehe, stehst,
steht), Bahnhof (noun, m; Bahnhöfe), gehen (verb; gehe, gehst, geht), Straße (noun, f; Straßen).

French (`fr`), 3 entries, 15 rows: homme (noun, m; hommes), être (verb; suis, es, est, sommes,
êtes, sont), venir (verb; viens, vient, venons, venez, viennent).

Verb form tags follow wiktextract's vocabulary (`first-person singular present indicative`);
noun plurals are tagged `plural`; the gender column of a row is `m`, `f` or `n` for nouns and
empty otherwise.

## Lookups the tests rely on

- `lemmas_of("buenas")` = `["bueno"]`; `lemmas_of("fue")` = `["ir", "ser"]` (sorted);
  `gender_of("mesa")` = f; `gender_of("Tisch")` = m (German table).
- Tokenising `Voy al mercado del pueblo.` with the Spanish table gives the lemma keys `ir`,
  `a`+`el`, `mercado`, `de`+`el`, `pueblo`; `Ich gehe zum Bahnhof.` with the German table gives
  `gehen`, `zu`+`dem`, `Bahnhof`.
- A German skill teaching `aufstehen` with `separable: auf` accepts `Ich stehe um sieben auf.`
  because `stehe` maps to the base verb `stehen` here and `auf` is the particle; without that
  lemma `stehe` is untaught.

## `parley forms extract --lang es --lemma bueno --lemma noche --lemma ir --date 2026-10-01`

Report `{"lemmas":3,"rows":15,"missing":[]}`. The file written is exactly
`tests/fixtures/syllabus/expected-forms.tsv`: the header
`# source: forms-mini.jsonl; sha256: bdd2092585e56451edb8e59e844bfca7a0659c3cc5cf51c3716430dea3b33919; date: 2026-10-01`
and these 15 rows, sorted by (form, lemma, pos) in byte order (`fue` < `fueron` < `fui`;
`va` < `vamos` < `van` < `vas` < `voy`):

    buena   bueno   adj             feminine singular
    buenas  bueno   adj             feminine plural
    bueno   bueno   adj             canonical
    buenos  bueno   adj             masculine plural
    fue     ir      verb            third-person singular preterite indicative
    fueron  ir      verb            third-person plural preterite indicative
    fui     ir      verb            first-person singular preterite indicative
    ir      ir      verb            canonical
    noche   noche   noun    f       canonical
    noches  noche   noun    f       plural
    va      ir      verb            third-person singular present indicative
    vamos   ir      verb            first-person plural present indicative
    van     ir      verb            third-person plural present indicative
    vas     ir      verb            second-person singular present indicative
    voy     ir      verb            first-person singular present indicative

With `--lemma zzz` alone the report is `{"lemmas":0,"rows":0,"missing":["zzz"]}` and the file
holds the header line only; with `--lemma bueno --lemma zzz` it is
`{"lemmas":1,"rows":4,"missing":["zzz"]}`.
