# 01: Content model and syllabus

## Goal

The course exists as validated open data. A contributor can write a syllabus in YAML, run
`parley content validate`, and get exact messages for every problem; the core derives learnable
items from it; `parley pack build` turns Tatoeba exports into a sentence pack with attribution. Seed
content (Spanish A1 units 1 and 2, French and German unit 1) passes the validator.

## Depends on

00, and ADR-0003 (content licence), Accepted on 2026-10-27: everything under `content/` is
CC BY-SA 4.0, so the forms tables may be derived from Wiktionary.

## Scope

- `schemas/syllabus.schema.json` and Rust types `Syllabus`, `Unit`, `Skill`, `Lemma`, `Example`.
- A YAML loader (`serde_yaml_ng`, a maintained serde YAML crate that follows YAML 1.2; `serde_yaml`
  is archived) feeding the JSON Schema check (`jsonschema`), then semantic rules V01 to V21 below.
- `parley content validate`: exit 0 with warnings allowed, 1 on any error.
- Stable item ids: every example has its own integer `id`, and `content/<lang>/ids.lock` records every
  item id ever published, so an id is never renumbered or reused; `parley content lock`.
- Tokeniser per language (UAX #29 words plus the rules in Design notes), with lemma keys.
- Forms table: TSV format, loader, lookups; `parley forms extract` from a wiktextract extract.
- The exact CLI syntax and output of every command below (Interfaces).
- Item derivation: vocabulary items from lemmas and phrase or pattern items from examples, with
  deterministic ids and accepted answers.
- Sentence packs: `parley pack build`, `parley pack info`, the pack SQLite schema, a content hash,
  and a build report with a reason for every dropped sentence.
- `content/packs.lock.toml` format and `verify_pack(path, sha256)`.
- Seed content: `content/es/` A1 units 1 and 2 (8 skills), `content/fr/` and `content/de/` unit 1
  (4 skills each), each with `syllabus.yaml`, `ids.lock`, `forms.tsv` (syllabus lemmas only), `numbers.yaml`
  (0 to 100 in words), `exclude.txt` (lemmas never taught or shown); `content/LICENSE`,
  `content/ATTRIBUTION.md`.
- Golden harness: a case may carry `exit.txt` (the expected exit code, default 0) and may use the
  token `{tmp}` in `args.txt` for a fresh temporary directory, so cases that must exit 1 and cases
  that write a file can be golden cases.
- `cargo test -p parley-cli` works from a plain PowerShell: the module 00 test that runs the
  release-notes script finds Git for Windows' `sh.exe` next to `git` when neither `sh` nor `bash`
  is on the PATH, and `scripts/verify.ps1` is green end to end.

## Files

- `crates/parley-core/src/content/{mod.rs,syllabus.rs,validate.rs,tokenise.rs,forms.rs,wiktextract.rs,items.rs,pack.rs,lock.rs,numbers.rs}`
- `crates/parley-cli/src/cmd/{mod.rs,content.rs,forms.rs,pack.rs}`; `crates/parley-cli/src/lib.rs` (dispatch)
- `crates/parley-cli/tests/golden.rs` (`exit.txt`, `{tmp}`), `crates/parley-cli/tests/m00_scaffold.rs` (the shell helper)
- `schemas/syllabus.schema.json`, `schemas/item.schema.json`
- `scripts/verify`, `scripts/verify.ps1`: add the `parley content validate content` step
- `content/**` as above; `tests/fixtures/syllabus/` (`valid-es.yaml`, `bad-V*.yaml`, the sibling
  `forms.tsv`, `numbers.yaml`, `exclude.txt`, `expected-items.json`, `expected-forms.tsv`),
  `tests/fixtures/tatoeba-mini/`, `tests/fixtures/forms-mini.jsonl` (all hand-written, CC0, each
  directory with a README of intended outcomes)

## Interfaces

```rust
pub enum Lang { Es, Fr, De }   // as_str(): "es" | "fr" | "de"; iso3(): "spa" | "fra" | "deu"; FromStr
pub struct ContentError(pub String);                                        // Display = the message
pub fn load_syllabus(path: &Path) -> Result<Syllabus, Vec<ContentError>>;   // V01 messages only
pub fn parse_syllabus(file: &str, text: &str) -> Result<Syllabus, Vec<ContentError>>;
pub struct Context<'a> { pub file: &'a str, pub folder: Option<&'a str>, pub forms: &'a Forms,
    pub numbers: &'a [String], pub exclude: &'a [String], pub lock: Option<&'a IdsLock> }
pub fn validate(s: &Syllabus, ctx: &Context) -> Report;     // Report { errors: Vec<String>, warnings: Vec<String> }; Report::ok()
pub fn tokenise(lang: Lang, text: &str, forms: &Forms) -> Vec<Token>;  // Token { surface, norm, kind: Word | Number | Name, lemma_keys: Vec<String> }
pub struct FormRow { pub form: String, pub lemma: String, pub pos: String, pub gender: Option<Gender>, pub tags: String }
impl Forms { pub fn load_tsv(r: impl Read) -> Result<Forms, ContentError>; pub fn from_rows(rows: Vec<FormRow>) -> Forms;
  pub fn rows(&self) -> &[FormRow]; pub fn lemmas_of(&self, form: &str) -> Vec<&str>; pub fn gender_of(&self, noun: &str) -> Option<Gender>; }
pub fn read_wiktextract(r: impl BufRead, lang: Lang, lemmas: Option<&[String]>) -> Result<Extract, ContentError>;
                                                             // Extract { rows: Vec<FormRow>, found: usize, missing: Vec<String> }
pub fn write_forms_tsv(w: &mut impl Write, source: &str, sha256: &str, date: &str, rows: &[FormRow]) -> io::Result<()>;
pub fn load_numbers(text: &str) -> Result<Vec<String>, ContentError>;     // the 101 number words, tokenised into the word set
pub fn load_exclude(text: &str) -> Vec<String>;
pub fn derive_items(s: &Syllabus, forms: &Forms) -> Vec<Item>;
impl IdsLock { pub fn parse(text: &str) -> Result<IdsLock, ContentError>; pub fn to_text(&self) -> String; pub fn is_active(&self, id) -> bool; pub fn is_retired(&self, id) -> bool; }
pub fn lock_items(s: &Syllabus, forms: &Forms, lock: &IdsLock) -> (IdsLock, LockReport);   // LockReport { added: Vec<String>, retired: Vec<String> }
pub struct PackInput { pub lang: Lang, pub tatoeba_dir: PathBuf, pub wiktextract: PathBuf, pub syllabus: PathBuf,
    pub exclude: Option<PathBuf>, pub built_at: String }
pub fn build_pack(input: &PackInput, out: &Path) -> Result<PackReport, PackError>;
pub fn pack_info(path: &Path) -> Result<PackInfo, PackError>;
pub fn verify_pack(path: &Path, expected_sha256: &str) -> Result<(), PackError>;
pub fn parse_packs_lock(text: &str) -> Result<PacksLock, ContentError>;
```

`PackError::ChecksumMismatch` displays "The download was damaged. Try again."; `PackError::NotFound(file)`
displays `{file} not found`; `PackError::Content(errors)` carries validation errors; `PackError::Io(message)`
and `PackError::Sqlite(message)` carry the underlying text without any absolute path.

CLI (every command prints one JSON object on stdout; exit 0 on success, 1 on content errors, 2 on
usage errors, which print one line on stderr and nothing on stdout: `error: {path} not found`,
`error: {flag} is required. Run parley --help.` or `error: unknown argument {arg}. Run parley --help.`;
`{path}` is the path as typed, with `/` between a directory argument and the file inside it):

    parley content validate <file-or-dir> [--forms <forms.tsv>] [--json]
        → {"ok": bool, "errors": [string], "warnings": [string]}
        Without --json: every error and warning on its own line, then `ok` when there is no error.
        A directory: every `<dir>/<lang>/syllabus.yaml` (lang in es, fr, de), in that order.
        A file: that syllabus alone. In both cases the forms table is `--forms` if given, else
        `forms.tsv` beside the syllabus if present, else empty; `numbers.yaml`, `exclude.txt` and
        `ids.lock` are read from beside the syllabus when present. V02 runs when the syllabus's
        folder is named es, fr or de; V20 and V21 run when `ids.lock` exists beside it.
        Errors are ordered by file, then by rule number, then in document order.
    parley content lock --lang <es|fr|de> [--content <dir>]          (default dir: content)
        → {"added": [item_id], "retired": [item_id]}
        Reads <dir>/<lang>/syllabus.yaml and <dir>/<lang>/ids.lock (created when missing), writes
        ids.lock back with every id of the syllabus active and every other active id retired; never
        deletes a line. Refuses (exit 1, the validate output) a syllabus with errors other than V20
        and V21.
    parley forms extract --lang <l> --wiktextract <file.jsonl> (--syllabus <file> | --lemma <lemma>...)
                         [--date <YYYY-MM-DD>] --out <forms.tsv>
        → {"lemmas": n, "rows": n, "missing": [lemma]}
        `lemmas` counts the requested lemmas that have at least one entry; `missing` lists the others,
        in the order requested. `--date` defaults to today's date (the tool reads the clock; the core
        receives the string).
    parley pack build --lang <l> --tatoeba-dir <dir> --wiktextract <file.jsonl> --syllabus <file>
                      [--exclude <file>] [--built-at <YYYY-MM-DD>] --out <file.sqlite3>
        reads <dir>/<iso3>_sentences_detailed.tsv, <dir>/eng_sentences_detailed.tsv and
        <dir>/<iso3>-eng_links.tsv (decompressed first; checked in that order, the first missing one
        is the usage error). The exclude list is `--exclude` if given, else `exclude.txt` beside the
        syllabus, else empty; number words come from `numbers.yaml` beside the syllabus. `--built-at`
        defaults to today's date. An existing output file is replaced.
        → {"lang": "es", "kept": n, "dropped": {"no_translation": n, "too_short": n, "too_long": n,
           "excluded_lemma": n, "unknown_word": n, "duplicate": n}}
    parley pack info <file.sqlite3>
        → {"schema": "parley.pack/1", "lang": "es", "built_at": "2026-10-01", "tool_version": "0.1.0",
           "licence": "CC BY 2.0 FR", "sentences": n, "translations": n, "forms": n,
           "ranked_lemmas": n, "bytes": n, "sha256": "<hex of the file>",
           "content_sha256": "<hex>", "source_sha256": {"spa_sentences_detailed.tsv": "<hex>", ...}}
        `content_sha256` is recomputed from the tables (and equals the stored `meta` value);
        `source_sha256` has one entry per input file, keyed by file name.

`ids.lock` (TSV, sorted by item id in byte order, LF line ends, no header):
`item_id<TAB>active|retired<TAB>syllabus version first seen`, e.g. `es.a1.u01.s01#e:2	retired	1`.

Syllabus example (`rank` values are filled by module 05 from the pack's frequency table). This is
`tests/fixtures/syllabus/valid-es.yaml`, verbatim:

```yaml
schema: parley.syllabus/1
lang: es
name: Spanish for English speakers
variety: Spain Spanish; usted and tú both taught
version: 1
function_words: [el, la, los, las, un, una, y, o, pero, de, a, en, con, sin, por, para, no, sí,
  yo, tú, él, ella, usted, nosotros, ellos, ellas, ustedes, me, te, se, lo, le, que, qué]
names: [Ana, Luis, Carmen, Pablo, Madrid, Sevilla]
units:
  - id: es.a1.u01
    title: Hello
    cefr: A1
    skills:
      - id: es.a1.u01.s01
        kind: function            # grammar | vocab | function
        title: Greetings
        cefr: A1
        can_do: I can say hello and goodbye and ask how someone is.
        requires: []
        grammar_note: >-
          Questions and exclamations open with an upside-down mark: ¿Qué tal? ¡Hola!
          Say buenos días until lunch, buenas tardes until dark, then buenas noches.
        lemmas:
          - {lemma: hola, pos: intj, en: [hello, hi], rank: null}
          - {lemma: adiós, pos: intj, en: [goodbye, bye], rank: null}
          - {lemma: bueno, pos: adj, en: [good], rank: null}
          - {lemma: día, pos: noun, gender: m, en: [day], rank: null}
          - {lemma: noche, pos: noun, gender: f, en: [night], rank: null}
          - {lemma: tal, pos: adv, en: [such], rank: null}
          - {lemma: gracias, pos: intj, en: [thank you, thanks], rank: null}
        examples:
          - {id: 1, target: "Hola, ¿qué tal?", en: ["Hi, how are you?", "Hello, how are you?"]}
          - {id: 2, target: "Buenos días.", en: ["Good morning."]}
          - {id: 3, target: "Buenas noches, Ana.", en: ["Good night, Ana.", "Good evening, Ana."]}
          - {id: 4, target: "Adiós, gracias.", en: ["Goodbye, thank you.", "Bye, thanks."]}
```

Fields: `schema` (const `parley.syllabus/1`), `lang` (es | fr | de), `name`, `variety`, `version`
(integer), `function_words` and `names` (arrays of strings), `units[]` with `id`, `title`, `cefr`
(A1 | A2), `skills[]` with `id`, `kind` (grammar | vocab | function), `title`, `cefr`, `can_do`,
`requires` (array of skill ids), optional `grammar_note`, `lemmas[]` and `examples[]`. A lemma has
`lemma`, `pos` (noun | verb | adj | adv | intj | pron | prep | conj | det | num | expr), optional
`gender` (m | f | n), `en` (array of strings), `rank` (integer or null), optional `separable` (a
German prefix) and optional `article` (overrides the article of a vocabulary item, e.g. `el` for
`agua`). An example has `id` (integer), `target` and `en` (array of strings). The schema checks
types, required fields, enumerations and the `schema` constant only; every count, shape and wording
rule is one of V03 to V21, so each has its own message.

Forms TSV (`content/<lang>/forms.tsv` and the pack's `forms` table): the first line is the comment
`# source: {file name}; sha256: {hex}; date: {YYYY-MM-DD}` (a hand-written table says
`# source: hand-written; sha256: -; date: {date}`), then one row per form, no column-name line,
`form<TAB>lemma<TAB>pos<TAB>gender<TAB>tags`, e.g. `buenas	bueno	adj		feminine plural`,
`noches	noche	noun	f	plural`. The loader skips blank lines and `#` lines and refuses a line
without exactly five columns: `forms.tsv: line {n}: expected 5 tab-separated columns`.

Wiktextract input (`parley forms extract`, `parley pack build`): one JSON object per line with
`word`, `pos`, `lang_code`, optional `tags`, `forms` (array of `{form, tags}`) and `senses` (array
with optional `tags`). An entry is used when `lang_code` is the language and, when a lemma list is
given, `word` is on it. Its gender is the first `masculine`, `feminine` or `neuter` found in, in
order, the entry's `tags`, the tags of a `forms` entry tagged `canonical`, then the `senses`
entries' `tags`; the TSV `gender` column carries it for nouns (`m`, `f`, `n`) and is empty for
other parts of speech. Rows: one for the headword with tags `canonical`, then one per `forms` entry
whose `form` differs from the headword, with that entry's tags joined by single spaces in the order
given. Rows are sorted by (form, lemma, pos) in byte order; a repeated (form, lemma, pos) keeps the
first tags. The field mapping is confirmed against a real extract at module 05, which downloads one.

Item (`schemas/item.schema.json`; `expected-items.json` is an array of these, in syllabus order:
for each unit, each skill, its lemma items in lemma order, then its example items in example order):

```json
{"id": "es.a1.u01.s01#l:día", "lang": "es", "skill": "es.a1.u01.s01", "kind": "vocab",
 "target": "el día", "en": ["day"], "accepted": {"to_target": ["el día"], "to_en": ["day"]},
 "lemmas": ["día"], "cefr": "A1"}
```

Pack (`<lang>-<YYYY-MM-DD>.sqlite3`): `meta(key, value)` with `schema` = `parley.pack/1`, `lang`,
`built_at`, `tool_version`, `source_sha256.<file name>` (one per input file: the three Tatoeba files,
the wiktextract file, the syllabus and the exclude file when given), `licence` = `CC BY 2.0 FR`,
`attribution` (one sentence crediting Tatoeba and its contributors under CC BY 2.0 FR), and
`content_sha256`;
`sentences(id INTEGER PRIMARY KEY, text, norm, tokens INTEGER, max_rank INTEGER, author)`;
`translations(sentence_id, en_id, en_text, en_author, PRIMARY KEY(sentence_id, en_id))`;
`sentence_lemmas(lemma, sentence_id, PRIMARY KEY(lemma, sentence_id)) WITHOUT ROWID`;
`forms(form, lemma, pos, gender, tags, PRIMARY KEY(form, lemma, pos)) WITHOUT ROWID`;
`freq(lemma PRIMARY KEY, rank INTEGER, count REAL)`. `content_sha256` is SHA-256 over every row
of every table in primary-key order, as TSV lines: tables in the order `meta`, `sentences`,
`translations`, `sentence_lemmas`, `forms`, `freq`; a row is its columns in table order joined by
TAB and ended by LF; integers in decimal, `count` as a whole number, empty text for an empty
value; the `meta` rows `built_at` and `content_sha256` are left out, so two builds from the same
inputs on different days hash the same.

`packs.lock.toml`: `schema = "parley.packs/1"` and per language a table `[es]` with `version`,
`url` (this repository's release asset), `sha256`, `bytes`, `content_sha256`; a table missing a
key: `packs.lock.toml: [es] needs sha256`. At this module the file carries `schema` only; module 05
fills the languages.

`numbers.yaml`: `schema: parley.numbers/1`, `lang`, and `words`, a list of exactly 101 entries,
index = the number (`numbers.yaml: words has {n} entries; 0 to 100 need 101`). Every word of
every entry (`treinta y uno` gives `treinta`, `y`, `uno`) is a number word for V12 and for the
pack builder. `exclude.txt`: one lemma per line; blank lines and `#` comments are ignored.

## Out of scope

- A1 beyond unit 2 for Spanish, A2, frequency ranks, scenarios (module 05); publishing packs
  (maintainer, module 14); FSRS state (02); grading (03).

## Design notes

- Rules: **V01** YAML or schema (`{file}: {pointer}: {message}`, the pointer a JSON Pointer such as
  `/units/0/skills/1/lemmas/2/pos`, `/` for the root; messages: `expected {a string | an integer |
  a number | a boolean | an array | an object | null}` (several joined by ` or `) for a wrong
  type, `{name} is required` for a missing field, `unknown field {name}`, `must be one of {a, b}`
  for an enumeration, `must be {value}` for a constant, else the schema library's own text; a
  file that is not YAML gives `{file}: {parser message}`; a V01 failure stops the other rules for
  that file); **V02** `lang` equals the folder
  (`{file}: lang is "fr" but the folder is "es"`); **V03** id shape `<lang>.<a1|a2>.uNN[.sNN]`
  with the syllabus's own `lang`, and a skill id is its unit's id plus `.sNN`
  (`{id}: ids look like es.a1.u01.s01`); **V04** unique ids (`duplicate id {id}`); **V05** units
  never go from A2 back to A1 (`unit {id} is A1 after an A2 unit`); **V06** `requires` names earlier
  skills only (`{skill}: requires {other}, which is not an earlier skill`); **V07** `can_do` starts
  "I can " and ends with a full stop (`{skill}: can_do must start with "I can" and end with a full
  stop`); **V08** grammar note at most 120 words, counted as whitespace-separated words
  (`{skill}: grammar_note has {n} words; the limit is 120`); **V09** 3 to 20 lemmas (`{skill}: {n}
  lemmas; a skill has 3 to 20`); **V10** 4 to 15 examples (`{skill}: {n} examples; a skill has 4 to
  15`); **V11** nouns have a gender (es, fr: m or f; de: m, f or n) (`{skill}: noun {lemma} needs a
  gender (m or f)` or `(m, f or n)`); **V12** every example word is a function word, a name, a
  number, or a form of a lemma of this or an earlier skill (`{skill}: example {n} uses "{word}",
  which no earlier skill teaches`, `{word}` the surface form, one error per offending word in
  document order); **V13** every example uses a lemma of its skill (`{skill}: example {n} uses none
  of this skill's lemmas`); **V14** no repeated example target anywhere in the syllabus, compared
  by normalised tokens (`{skill}: example {n} repeats {other} example {m}`, `{other}` the skill of
  the earlier example); **V15** at least one English translation (`{skill}: example {n} needs at
  least one English translation`); **V16** no lemma from `exclude.txt` (`{skill}: {lemma} is on the
  exclude list`); **V17** warning only: a lemma taught in two skills (`warning: {lemma} is taught
  in {a} and {b}`, one warning per later skill against the first); **V18** all text is NFC
  (`{skill}: example {n} is not in Unicode NFC; save the file as NFC`); **V19** example ids are
  positive integers, unique within their skill (`{skill}: example id {n} is used twice`); **V20** no
  example id that `ids.lock` marks retired (`{item_id} was retired; give the example a new id`);
  **V21** every item id is active in `ids.lock` and every active id is in the syllabus
  (`{item_id} is not in ids.lock; run parley content lock --lang {lang}` and `{item_id} is in
  ids.lock but not in the syllabus; run parley content lock --lang {lang} to retire it`). In
  messages, `{n}` is the example's `id`. A retired lemma item that reappears in the syllabus is
  reactivated by `content lock` (listed under `added`); only example ids are never reused.
- Taught forms (V12, V13, the pack builder): a word matches a lemma when `lemmas_of` maps its
  surface or its normalised form to that lemma, when it equals the lemma's own citation form, or
  when it is one of the words of a multi-word lemma (`por favor` teaches `por` and `favor`). A
  German lemma marked `separable: auf` also allows its base verb (the lemma without the prefix,
  `stehen`) and the particle `auf` on its own (`aufstehen` allows `stehe … auf`).
- Tokeniser: UAX #29 words (`unicode-segmentation`); `norm` is the surface in NFC and lower case;
  digits become `Number`; French elisions (`l' d' j' m' t' s' n' c' qu' jusqu' lorsqu' puisqu'`,
  typographic ’ read as ') split after the apostrophe into a token whose surface keeps the
  apostrophe and whose lemma keys are the full word (`le`, `de`, `je`, `me`, `te`, `se`, `ne`, `ce`,
  `que`, `jusque`, `lorsque`, `puisque`; `l'` gives `le` and `la`); `aujourd'hui` is kept whole;
  French inverted pronouns (`-je -tu -il -elle -on -nous -vous -ils -elles -ce`) are separate words
  already under UAX #29, and a `t` between two hyphens (`a-t-il`) is dropped; Spanish `al` and
  `del` and German `am im ins zum zur vom beim` stay one token with two lemma keys (`a`+`el`,
  `de`+`el`, `an`+`dem`, `in`+`dem`, `in`+`das`, `zu`+`dem`, `zu`+`der`, `von`+`dem`, `bei`+`dem`);
  every other word's lemma keys are `lemmas_of(surface)` and `lemmas_of(norm)` merged, sorted and
  deduplicated; a capitalised word not at sentence start, unknown to the forms table, is a `Name`.
  A word is at sentence start when, scanning back from it over spaces and opening punctuation
  (`¿ ¡ " « ( ‘ “`), the text begins or the previous character is `.`, `!` or `?`.
- Item ids: `<skill>#l:<lemma>` and `<skill>#e:<id>`, where `<id>` is the example's own `id`, so
  deleting or reordering examples never renumbers the others. Moving a lemma to another skill changes
  its id; `content lock` then retires the old id and module 02 keeps its cards as retired. A
  vocabulary noun's target carries its definite article (`el día`; es `el`/`la`, fr `le`/`la` or
  `l'` before a vowel or `h`, de `der`/`die`/`das`; a lemma's `article` field overrides); every other
  vocabulary item's target is the lemma itself. A vocabulary item's `en` and `to_en` are the `en`
  list and `to_target` is its target; an example item's `en` and `to_en` are the example's `en`
  list and `to_target` is the example's `target`. An example in a `grammar` skill is a `pattern`
  item; in a `vocab` or `function` skill, a `phrase` item. An item's `lemmas` lists, in token order
  without repeats, the syllabus lemmas its target uses (a vocabulary item: its own lemma); `cefr` is
  the skill's.
- `content lock` input and output: the lock holds both lemma and example ids. `added` lists the
  syllabus ids that were not active (new or reactivated), `retired` the active ids no longer in
  the syllabus; both sorted in byte order. A new line records the syllabus `version` as first seen;
  a retired line keeps its version.
- Pack filters, in order, each a report reason: `no_translation` (no link to an English sentence
  that exists in the English file), `too_short` (< 3 words), `too_long` (> 12 words or > 120
  characters, counted as Unicode scalar values), `excluded_lemma` (a word whose normalised form or
  any lemma key is on the exclude list), `unknown_word` (a word that is not a function word, not a
  name, not a number word or `Number`, and has no lemma key), `duplicate` (same `norm` as an earlier
  kept sentence, where `norm` is the tokens' normalised forms joined by single spaces; the lowest id
  is kept). Function words and names are the syllabus's lists; a `Name` token counts as a name.
  Words are tokens of any kind. `tokens` is the word count; `sentence_lemmas` has every lemma key of
  every token; `freq` counts, per lemma, the kept sentences that contain it, and ranks by count
  descending then lemma ascending (byte order), starting at 1; `max_rank` is the largest rank among
  a sentence's lemmas, 0 when it has none. The `forms` table is the whole extract for the language.
- Tatoeba inputs, verified on the per-language export listing on 2026-10-27:
  `per_language/<iso3>/<iso3>_sentences_detailed.tsv.bz2` (id, lang, text, username, added,
  modified), `per_language/eng/eng_sentences_detailed.tsv.bz2` and the pair links
  `per_language/<iso3>/<iso3>-eng_links.tsv.bz2` (sentence id, translation id). Tatoeba also
  publishes a CC0 subset (`<iso3>_sentences_CC0.tsv`); packs use the full CC BY 2.0 FR set with
  authors kept for attribution. A username of `\N` (Tatoeba's marker for a sentence without an
  owner) is stored as empty text.
- Frequency source: counts over the Tatoeba sentences (CC BY 2.0 FR), the default, is the choice:
  it needs no further download, and every pack carries its own ranks; FrequencyWords (CC BY-SA 4.0)
  stays the alternative if module 05 finds the Tatoeba ranks too noisy for A1 selection.
- `forms.tsv` is course content, not a test fixture: it is committed because the validator in
  `verify` and offline grading need it, under the content licence with Wiktionary credited
  (ADR-0003, CC BY-SA 4.0). The seed tables of this module are hand-written for the syllabus lemmas
  only and checked against Wiktionary; module 05 regenerates them with `parley forms extract`.
- Content files and packs are read by path (`load_syllabus`, `build_pack`, `verify_pack`): they
  are inputs and build outputs, not learner data, and SQLite opens its file itself. The `Fs` trait
  of `env.rs` stays the only way to the learner's data directory. `build_pack` receives `built_at`
  as text; only the command-line tool reads the clock.
- Paths in messages and JSON use `/` as the separator and the path as typed, so the same input
  gives the same output on every platform.

Decisions made during implementation:

- **YAML crate**: `serde_yaml_ng` 0.10 (MIT) instead of `serde-saphyr`. The syllabus is read
  into an untyped JSON value first, so the schema check can name every problem, and in that
  pass `serde-saphyr` applies YAML 1.1 booleans with no option to turn them off: the function
  word `no` became `false` (and `yes`, `on`, `off` likewise), failing the schema before any
  rule ran. `serde_yaml_ng` follows YAML 1.2, keeps them as strings, parses the example
  syllabus, and gives one-line parser messages (`did not find expected '-' indicator at line 4
  column 4, while parsing a block collection at line 3 column 3`).
- **V01 details**: enumerated fields and `schema` use `enum` and `const` alone (no `type`), so a
  wrong value gives one message; `rank` is required (integer or null) and `gender`,
  `grammar_note`, `separable` and `article` are optional, as the Interfaces list them; several
  allowed types are joined by ` or ` in the order string, integer, number, boolean, array,
  object, null (`expected an integer or null`); `additionalProperties` gives one `unknown field
  {name}` per field; messages keep the schema library's order; a value the schema accepts but
  the typed structure does not (a negative `version`) is `{file}: /: {message}`. A UTF-8
  byte-order mark is dropped; a file that is not UTF-8 is `{file}: not valid UTF-8`.
- **V03**: `uNN` and `sNN` are exactly two digits; the message text is the constant
  `ids look like es.a1.u01.s01` for every language.
- **V12**: a name is an entry of the syllabus's `names` list (compared as written or in lower
  case); a capitalised unknown word is not enough, so a misspelt word is reported even when
  capitalised. The pack builder, as specified, also accepts `Name` tokens. A function word
  matches a token's normalised form, its surface, or any of its lemma keys (so `al` and `l'`
  pass through `a`, `el`, `le`, `la`). Numbers are `Number` tokens and the words of
  `numbers.yaml`.
- **V17** warnings carry the `warning: ` prefix in the string itself; **V18** checks an
  example's `target` and `en`; **V19** also reports a non-positive id as `{skill}: example id
  {n} must be a positive integer`; an id **V20** reports is left out of **V21**; V21 lists the
  syllabus ids missing from the lock in document order, then the active lock ids missing from
  the syllabus in byte order.
- **Items**: French `l'` applies before `a e i o u`, their accented forms, `œ`, `æ` and `h`; an
  `article` ending in an apostrophe joins the lemma without a space; the override applies to
  nouns. An example item's `lemmas` holds, for each token in order, every syllabus lemma it
  matches (in syllabus order), without repeats. The taught-forms rule lives in one place
  (`TaughtForms`) shared by V12, V13 and item derivation.
- **`content lock`** refuses with the validate report computed without the lock, so V20 and
  V21 do not appear in it, and always rewrites `ids.lock` (creating it when missing).
- **Pack builder**: the links file is read in either orientation; a Tatoeba row needs at least
  three columns (the author is empty when the column is absent or `\N`); `source_sha256` lists
  the three Tatoeba files, the wiktextract file, the syllabus and the exclude file whenever one
  was read (given or found beside the syllabus); `numbers.yaml` is read but, as the Interfaces
  list it, not recorded. Building and `pack info` both compute `content_sha256` by reading the
  tables back in primary-key order, and `pack info` refuses a pack whose stored hash differs
  from the tables. An existing output file and its `-journal` are removed first; `pack info`
  opens the file read-only. The attribution sentence is "Sentences and translations come from
  Tatoeba (tatoeba.org) and its contributors, under CC BY 2.0 FR."
- **Command line**: `--date` and `--built-at` default to today's date in UTC and must be
  `YYYY-MM-DD`; `--lang` outside es, fr, de gives `error: --lang must be es, fr or de. Run
  parley --help.`; `content`, `forms` or `pack` without a subcommand names the choices
  (`error: content validate or content lock is required. Run parley --help.`); a directory with
  none of the three syllabi is `error: {dir}/es/syllabus.yaml not found`; a malformed
  `forms.tsv`, `numbers.yaml` or `ids.lock` beside a syllabus is reported in the validate
  report (exit 1) with the loader's message; a read, write or SQLite failure prints `error:
  {message}` on stderr and exits 1. `forms extract` with `--syllabus` requests the syllabus's
  lemmas in document order without repeats.
- **`cargo run -q -p parley-cli -- …`**: the crate ships `parley` and `parley-fixtures`, so
  `cargo run` needs `default-run = "parley"` in its manifest for the documented commands and
  the `verify` step to run; module 01 sets it.
- **Golden harness**: `{tmp}` is a fresh temporary directory per case (forward slashes) and
  `exit.txt` the expected exit code (trimmed, default 0); stdout must still be one JSON
  object. The module 00 shell helper falls back to Git for Windows' `usr\bin\sh.exe`, found one
  or two directories above the `git` on the PATH.
- **Dependencies**: `serde_yaml_ng`, `jsonschema` (default features off, so no network
  resolver; `parley-core` has no `reqwest` in its tree), `rusqlite` with bundled SQLite,
  `unicode-segmentation`, `unicode-normalization`; `toml` becomes a normal dependency of
  `parley-core` for `packs.lock.toml`; `parley-cli` adds `serde` and, for tests, `rusqlite`;
  `parley-core` adds `tempfile` for tests. `deny.toml` gains `MIT-0` (MIT No Attribution, OSI
  approved), which `borrow-or-share`, a transitive dependency of `jsonschema`, carries.
- **Loader messages**: `forms.tsv: line {n}: gender must be m, f, n or empty`; `ids.lock: line
  {n}: expected 3 tab-separated columns`, `ids.lock: line {n}: status must be active or
  retired`, `ids.lock: line {n}: version must be an integer` and `ids.lock: line {n}: duplicate
  id {id}`; `numbers.yaml: schema must be parley.numbers/1` and `numbers.yaml: words has {n}
  entries; 0 to 100 need 101`. `{n}` is the physical line number, counting from 1.
- **Paths in messages**: a trailing `/` or `\` on a path argument is dropped, so `content/`
  prints as `content`; everything else stays as typed.
- **`scripts/verify.ps1`**: a step whose tool cannot be launched sets no exit code; the script
  now fails that step with exit code 1 instead of passing it.
- **Acceptance test 15** adds a fifth example before deleting example 2, because a skill needs
  four examples (V10); the deletion then exercises V21 and `content lock` as the bullet says.

## Acceptance tests

Golden cases in `tests/cases/01-content-model-and-syllabus/`; code tests in
`crates/parley-core/tests/m01_content_model_and_syllabus.rs` (the library) and
`crates/parley-cli/tests/m01_content_model_and_syllabus.rs` (the binary: exit codes, stderr, files
written). Expected values come from how each fixture was written (its README), from the example
syllabus above, and from UAX #29.

1. `parley content validate tests/fixtures/syllabus/valid-es.yaml --json` → `{"ok":true,"errors":[],"warnings":[]}`.
2. One fixture per rule, `bad-V01.yaml` to `bad-V21.yaml`, each → exactly its message from Design
   notes with the fixture's ids filled in (for V01: `version: "one"` gives
   `tests/fixtures/syllabus/bad-V01.yaml: /version: expected an integer`; for V08:
   `es.a1.u01.s01: grammar_note has 121 words; the limit is 120`); exit code 1, or 0 for V17. The
   V02, V20 and V21 fixtures are directories (`bad-V02/es/syllabus.yaml` with `lang: fr`;
   `bad-V20/es/` and `bad-V21/es/` with an `ids.lock`) validated with the directory form.
3. Tokenise fr `L'homme est aujourd'hui à Paris.` → surfaces `L'`, `homme`, `est`, `aujourd'hui`,
   `à`, `Paris` with `Paris` a Name; fr `Est-ce que tu viens ?` → `Est`, `ce`, `que`, `tu`, `viens`.
4. Tokenise es `Voy al mercado del pueblo.` → lemma keys `ir`, `a`+`el`, `mercado`, `de`+`el`,
   `pueblo` (with forms-mini); de `Ich gehe zum Bahnhof.` → `zum` gives `zu`+`dem`; `Straße` stays
   one token; `Tengo 3 gatos.` → `3` is a Number.
5. With forms-mini: `lemmas_of("buenas")` = `["bueno"]`; `lemmas_of("fue")` = `["ir","ser"]`;
   `gender_of("mesa")` = `f`; `gender_of("Tisch")` = `m`.
6. A de fixture skill allowing `aufstehen` (separable `auf`) accepts example `Ich stehe um sieben auf.`
   under V12; without that lemma it fails V12 on `stehe`.
7. `derive_items(valid-es)` → the items in `expected-items.json` (ids, kinds, targets `el día`, `la
   noche`, accepted answers), in syllabus order.
8. `parley pack build --lang es --tatoeba-dir tests/fixtures/tatoeba-mini --wiktextract
   tests/fixtures/forms-mini.jsonl --syllabus tests/fixtures/syllabus/valid-es.yaml --built-at
   2026-10-01 --out <tmp>/es.sqlite3` → `{"lang":"es","kept":22,"dropped":{"no_translation":5,"too_short":1,"too_long":3,"excluded_lemma":2,"unknown_word":5,"duplicate":2}}`
   (the fixture README assigns each of the 40 sentences its fate; the exclude list is the
   `exclude.txt` beside `valid-es.yaml`); sentence 1001 is kept with English 2001 and author `fixture_a`.
9. Two builds from the same inputs give the same `content_sha256` in `parley pack info`; its output
   has every key of the Interfaces format, `licence` = `CC BY 2.0 FR`, `sentences` = 22, and one
   `source_sha256` entry per input file.
10. `verify_pack` on a copy with one changed byte → `ChecksumMismatch` displaying "The download was
    damaged. Try again."; on the original → `Ok`.
11. `parley content validate content` → exit 0 with no errors (the seed content).
12. `packs.lock.toml` with a missing `sha256` → `packs.lock.toml: [es] needs sha256`.
13. `parley forms extract --lang es --wiktextract tests/fixtures/forms-mini.jsonl --lemma bueno
    --lemma noche --lemma ir --date 2026-10-01 --out <tmp>/forms.tsv` → `{"lemmas":3,"rows":<as in the README>,"missing":[]}`
    and exactly the bytes of `expected-forms.tsv` (written from the fixture's entries), whose header
    line carries the fixture file's SHA-256; `--lemma zzz` → `"missing":["zzz"]`.
14. `content/LICENSE` names the licence ADR-0003 accepted (CC BY-SA 4.0); `content/ATTRIBUTION.md`
    credits Tatoeba (CC BY 2.0 FR) and Wiktionary (CC BY-SA).
15. Stable ids: deleting example 2 of `valid-es` leaves items `#e:1`, `#e:3` and `#e:4` with the same
    ids; V21 then reports `#e:2` until `parley content lock --lang es` prints `{"added":[],"retired":
    ["es.a1.u01.s01#e:2"]}`; a new example with `id: 2` → V20; with `id: 5` → V21 until locked, then valid.
16. `parley pack build` without `--out` → exit 2 and `error: --out is required. Run parley --help.`;
    with a missing Tatoeba file → exit 2 and
    `error: tests/fixtures/tatoeba-mini/spa-eng_links.tsv not found`.
17. The golden harness honours `exit.txt` and `{tmp}` (a case under this module exits 1 by design
    and one writes into `{tmp}`); `cargo test -p parley-cli --test m00_scaffold t08` passes from a
    PowerShell whose PATH has `git` but neither `sh` nor `bash`.

## Status

Done (2026-10-28)
