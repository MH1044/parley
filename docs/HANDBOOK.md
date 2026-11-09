# Parley handbook

The user guide. It grows with the app: each module adds the pages for what a learner can now do,
and every example in it has been run.

## Version 0.1.0

Building from source (see `README.md`) gives:

- the desktop app, which teaches offline: onboarding with a placement test, the learning path,
  lessons and reviews, the streak and stats, settings, and backups (below), and lets you set up AI
  when you choose to: publik API, your own key or a local model, with a connection check;
- the `parley` command-line tool, which answers `parley --version --json` with
  `{"name":"parley","version":"0.1.0"}` and refuses an unknown argument with
  `error: unknown argument <arg>. Run parley --help.`;
- the course content tools below;
- the scheduler and the placement test, which the command line can already show;
- the offline lesson engine: grading, template lessons, XP and streaks, which the command line
  can already show too.

### The desktop app

Build the front end, then run the app (the front end is embedded at compile time):

```
npm --prefix app run build
cargo run -p parley-app
```

Everything below works with no network and no AI. Parley asks about AI only when you choose
**Set up AI** (see *AI* below), never at launch or during onboarding.

**First launch.** One step at a time: *Welcome to Parley* ("Learn by speaking. No hearts, no ads,
nothing to buy.", **Start**); *Choose a language* (Spanish, French or German, taught from
English); *Course pack*, which offers the language's sentence pack with its size first
("Download the Spanish course pack (246.8 MB)?"; for a language with no pack in
`content/packs.lock.toml` it says "No course pack is published for Spanish yet. Parley works
without it, with fewer example sentences." instead); *Daily goal* (10, 20, 30 or
50 XP a day, 20 by default); and *Where to start*: **Take the placement test (about 5 minutes)**
or **Start from the beginning**. The placement test asks you to translate a word of one skill
after another into English (**I don't know** is always fine) and ends with where you start, such
as "You're around A1. You'll start at Unit 2: …"; the skills before it count as tested out and
come back in your reviews.

**The path.** Your streak ("3 day streak") and today's XP ("12 of 20 XP today"), then
**Continue** (the next lesson of the first skill you can learn), **Review (n)** with the number of
cards due, **Stats** and **Settings**. Before any AI is set up, a card says "Parley works without
AI. Set it up for fresh lessons, answer checks and conversations." with **Set up AI**; once a
provider is chosen, a chip says which ("AI: publik API" with the balance line under it, "AI: your
own key" or "AI: local model"). Below them every unit ("Unit 1: …") and its skills with their
state: locked, available, learning, practised or mastered. A skill opens its detail: what you will
be able to do, its words, and **Start lesson**; a locked skill says which skill to finish first.

**Lessons.** "Exercise 3 of 12" at the top, then one of: *What does el día mean?* (four choices),
*Translate into Spanish* or *Translate into English* (typed, or built from a word bank when the
settings say so), *Fill the gap* with an English hint, and *Match the pairs* (a word, then its
meaning). **Check** shows the result: Correct, Almost, Typo, Wrong or Not checked, the reason,
the correct answer and, for a sentence from the course pack, its Tatoeba author ("Sentence by
… on Tatoeba"). Not checked means a free translation Parley cannot decide offline: **I was
right** accepts it from then on. The keyboard does it all: Enter checks and continues, 1 to 4
choose an option, Esc asks "Quit this lesson?". The summary shows the XP earned, how many answers
were correct and the streak. There is no limit on lessons.

**Reviews.** **Review** takes up to 20 due cards. With nothing due it says when the next review
is ("Nothing to review. Next review: tomorrow at 09:00."), or, before your first lesson, "Nothing
to review yet. Finish a lesson to start your reviews."

**Stats.** The streak and freezes, the last 30 days, this week's XP and your best earlier week,
words known (vocabulary you would recall now with at least 90% likelihood), the share of reviews
recalled in the last 30 days, your level from the placement test, and every skill's state. Before
your first lesson: "Finish a lesson to see your stats."

**Settings.** *Learning*: the daily goal, how accents are graded (an accent slip is almost right,
wrong, or ignored), typing or word bank, exercises per lesson, new words per lesson and the target
recall (80% to 95%). *AI*: the three providers (see *AI* below). *Data*: **Export a backup** writes one JSON file (`parley.backup/1`, laid out
in `docs/modules/04-app-shell-and-offline-course.md`) with your settings and progress, never a
key, a cache or a pack; **Restore a backup** replaces all your progress on this computer with a
backup's, after a warning; **Clear caches**; and **Delete everything**, once you type `delete`,
which starts Parley afresh: your progress, settings, packs, AI answers and the publik API status
are deleted. Two things stay, because they are not in the data folder: publik API's credential for
this computer (it holds this computer's publik balance; **Disconnect** in Settings > AI forgets its
key) and your own key in the operating system's keychain (**Remove my key** deletes it). *About*: the version, the folder your data is in, and the credits:
example sentences from Tatoeba (CC BY 2.0 FR), word forms from Wiktionary (CC BY-SA 4.0).

**AI.** **Set up AI** opens *Set up AI features*: what Parley sends through publik's servers
(only what the AI feature you use needs; your progress, reviews and other recordings stay on this
computer), publik's own disclosure word for word, "Everything else in Parley works without AI.",
and three equal choices. **Continue with publik API** sets this computer up with publik API (a key
made for this install, no key to paste) and shows the first-run card: the balance line ("$0.00 ·
link this computer for $0.05 of free use"), what a use costs, and **Sign in with publik**, which
opens publik's page in your browser to link this computer (once linked, **Add a plan or pack**).
**Use my own key** and **Use a local model** open Settings > AI. **Not now** closes the dialog and
sends nothing. If publik API cannot set this computer up, the dialog says why ("publik API is
unreachable right now. Use your own key for now.") and the other two choices stay.

*Settings > AI* always shows **publik API**, **Your own key** and **Local model**, in that order,
with "In use" on the chosen one. publik API: the balance line, **Sign in with publik** or **Add a
plan or pack**, **Use publik API** when another provider is chosen, and **Disconnect** (forgets
this computer's key) or **Reconnect**. Your own key: **Base URL** (any OpenAI-compatible API,
starting with `https://` or `http://`), **Key** (kept in the operating system's keychain and never
shown again), **Chat model**, and optional transcription and speech models and voice; **Save my
key** chooses it, **Remove my key** deletes it from the keychain. Local model: **Server address**
(`http://localhost:11434/v1`, Ollama's, by default; it must be on this computer), **Find models**
lists the chat models it offers, largest first with their sizes ("llama3.1:8b 4.7 GB"), and **Use
this model** chooses one (without a choice, the largest is used). **Check connection** makes one
tiny call to the chosen provider and says "Connected. This check cost $0.000017." (on publik API,
with what it cost) or "Connected."; a problem shows one message and one way out: with no publik
balance left, the message and one link (**Sign in with publik** until this computer is linked,
then **Add a plan or pack**) "or use your own key or a local model in Settings"; a disconnected
publik API, **Reconnect**; a busy provider or an answer that could not be used, **Try again**.
Only the provider you chose is ever contacted, and publik API calls are never repeated by
themselves, so nothing is paid for twice.

**Your data.** Everything is in the app data folder (`org.parley.app` under `AppData\Roaming` on
Windows, under `~/.local/share` on Linux): `parley.sqlite3` (your progress), `settings.json`,
once downloaded, `packs/`, and, once AI is used, `cache/ai.sqlite3` (AI answers, so the same
question is never paid for twice; **Clear caches** removes it) and `publik-status.json` (the
publik API balance and status, never a key). publik API's own key for this computer is in publik's
credential file (`%LOCALAPPDATA%\publik\apps\parley.json` on Windows,
`~/.config/publik/apps/parley.json` on Linux), and your own key in the operating system's
keychain. A settings file that cannot be read gives the default settings and
says so once. A data file that cannot be opened shows "Parley could not open its data file." with
**Restore a backup** and **Show details**.

### The command line

Every command prints one JSON object on standard output and exits 0 on success, 1 on a content
error (the JSON report is still printed) or 2 on a usage error, which prints one line on standard
error, such as `error: --out is required. Run parley --help.` or
`error: content/es/syllabus.yaml not found`. In the examples `parley` stands for
`cargo run -q -p parley-cli --`.

One command is for the maintainer only and prints plain lines instead of JSON: `parley publik
proof` (also `scripts/publik-proof`) makes one real publik API connection check, which costs a
fraction of a cent, and prints the reply, the charge, every `x-publik-*` header and the model ids
publik API lists, never the key. Without `PUBLIK_PROOF=1` in the environment it sends nothing:

```
parley publik proof
This makes one real publik API call. Run it with PUBLIK_PROOF=1.
```

and exits 1. A built desktop app started with `PUBLIK_SELF_TEST=1` sets itself up with publik API
(that is a real request), prints one line, `publik self-test: ok parley 0.1.0 (install 5f0c8a52)`
or `publik self-test: failed: <why>`, and exits without opening a window.

### The course files

The course is open data under `content/`, licensed CC BY-SA 4.0 (`content/LICENSE`). Each
language (`es`, `fr`, `de`) has a folder with:

- `syllabus.yaml`: units, skills, the lemmas each skill teaches and its example sentences with
  English translations. `docs/modules/01-content-model-and-syllabus.md` shows a complete one.
- `ids.lock`: every item id ever published, `active` or `retired`, with the syllabus version it
  was first seen in. An id is never renumbered or reused, so a learner's progress survives
  every edit of the syllabus.
- `forms.tsv`: inflected form to lemma (`buenas` to `bueno`, `noches` to `noche`), derived from
  Wiktionary.
- `numbers.yaml`: the words for 0 to 100.
- `exclude.txt`: lemmas never taught or shown, one per line.
- `checklist.yaml`: the grammar points (`a1.g.present_regular`) and communicative functions
  (`a1.f.greet`) of A1 and A2, in the project's own words, each with the skills that teach it.
- `review.yaml`: every unit, `unreviewed` or `reviewed` by a native speaker, with the day the
  status was last set; no names.

`content/scenarios/` holds the role-play scenarios (`cafe.yaml`, `shop.yaml`, …): the roles, the
goals, a turn cap and, per language, the setting, the partner's opening line and the skills it
builds on. `content/packs.lock.toml` pins the published sentence packs (URL, SHA-256, size) per
language.

Units still marked `unreviewed` ship as a labelled beta: they are on the path like any other,
and the release notes say the course awaits native-speaker review.

### Checking a syllabus: `parley content validate`

```
parley content validate content
```

checks every `content/<lang>/syllabus.yaml` (es, fr, de, in that order) with the files beside
it, and prints every error and warning on its own line, then `ok` when there is no error:

```
ok
```

A single file works too, and `--json` gives the report as one object:

```
parley content validate content/es/syllabus.yaml --json
{"ok":true,"errors":[],"warnings":[]}
```

The forms table is `--forms <forms.tsv>` if given, else `forms.tsv` beside the syllabus, else
empty. The rules, in the order they are reported: the file is YAML and matches the schema
(`content/es/syllabus.yaml: /version: expected an integer`); `lang` matches the folder; ids look
like `es.a1.u01.s01` and are unique; units never go from A2 back to A1; `requires` names earlier
skills; `can_do` reads "I can ... ."; a grammar note has at most 120 words; a skill has 3 to 20
lemmas and 4 to 15 examples; nouns have a gender; every word of every example is a function word,
a name, a number or a form of a lemma this or an earlier skill teaches; every example uses one of
its skill's lemmas; no sentence repeats another; every example has an English translation; no
lemma is on the exclude list; text is Unicode NFC; example ids are unique; the id lock agrees
with the syllabus; and, in a course folder (one with `review.yaml` beside the syllabus), every
single-word lemma has its frequency rank (`es.a1.u01.s01: gato needs a rank`). A lemma taught
twice is a warning, not an error, and so is an A1 lemma ranked above 3,000
(`warning: mesa has rank 3500; check that it belongs at A1`). For example, the example
syllabus in `tests/fixtures/syllabus/valid-es.yaml` checked against a forms table that holds only
its header line:

```
parley content validate tests/fixtures/syllabus/valid-es.yaml --forms empty.tsv
es.a1.u01.s01: example 2 uses "Buenos", which no earlier skill teaches
es.a1.u01.s01: example 2 uses "días", which no earlier skill teaches
es.a1.u01.s01: example 3 uses "Buenas", which no earlier skill teaches
es.a1.u01.s01: example 3 uses "noches", which no earlier skill teaches
es.a1.u01.s01: example 2 uses none of this skill's lemmas
es.a1.u01.s01: example 3 uses none of this skill's lemmas
```

and the exit code is 1.

A directory is checked one language at a time: the syllabus, then its `checklist.yaml` (every
entry once, every skill it names existing at the entry's level) and its `review.yaml` (every
unit of the syllabus exactly once), then every scenario in `<dir>/scenarios/`, in the order of
their file names. For example, a fixture course whose checklist repeats an entry, names a skill
that does not exist and maps an A1 entry to an A2 skill:

```
parley content validate tests/fixtures/course05/bad-CL
tests/fixtures/course05/bad-CL/es/checklist.yaml: duplicate entry a1.f.greet
tests/fixtures/course05/bad-CL/es/checklist.yaml: a1.g.ser_present: skill es.a1.u03.s01 does not exist
tests/fixtures/course05/bad-CL/es/checklist.yaml: a1.f.directions: skill es.a2.u01.s01 is A2, but the entry is A1
```

A review file says `unit es.a1.u01 is listed twice`, `unit es.a1.u05 is not in the syllabus` or
`unit es.a2.u01 is missing` in the same way.

### Recording item ids: `parley content lock`

Every lemma gives a vocabulary item (`es.a1.u01.s01#l:día`, shown as `el día`) and every
example a phrase or pattern item (`es.a1.u01.s01#e:2`). After adding or removing any of them:

```
parley content lock --lang es
{"added":["es.a1.u01.s01#e:1","es.a1.u01.s01#e:2","es.a1.u01.s01#e:3","es.a1.u01.s01#e:4","es.a1.u01.s01#l:adiós","es.a1.u01.s01#l:bueno","es.a1.u01.s01#l:día","es.a1.u01.s01#l:gracias","es.a1.u01.s01#l:hola","es.a1.u01.s01#l:noche","es.a1.u01.s01#l:tal"],"retired":[]}
```

rewrites `content/es/ids.lock` (`--content <dir>` names another content directory). Ids that
left the syllabus are retired, never deleted; an example id that was retired cannot come back
(`es.a1.u01.s01#e:2 was retired; give the example a new id`), and until the lock is run the
validator says what to do (`es.a1.u01.s01#e:5 is not in ids.lock; run parley content lock
--lang es`). The command refuses a syllabus with any other error and prints the validate report
instead.

### Building a forms table: `parley forms extract`

A forms table comes from a wiktextract extract of Wiktionary (one JSON object per line,
downloaded into `data-cache/`, never committed):

```
parley forms extract --lang es --wiktextract tests/fixtures/forms-mini.jsonl --syllabus tests/fixtures/syllabus/valid-es.yaml --date 2026-10-01 --out forms.tsv
{"lemmas":4,"rows":9,"missing":["adiós","tal","gracias"]}
```

(This run used the small extract under `tests/fixtures/`; for the course the command is
`parley forms extract --lang es --wiktextract data-cache/es.jsonl --syllabus content/es/syllabus.yaml --out content/es/forms.tsv`
with a real extract.) `lemmas` counts the syllabus lemmas the extract has an entry for, `missing` lists the others.
`--lemma <lemma>` (repeatable) extracts the given lemmas instead; `--date <YYYY-MM-DD>` sets the
date in the table's header line, which also carries the extract's SHA-256. The inflection
table's own labels that real extracts list among the forms (`irregular`, `es-conj`, `7 strong`,
or the `haben` of a German verb table) are left out:

```
# source: forms-mini.jsonl; sha256: bdd2092585e56451edb8e59e844bfca7a0659c3cc5cf51c3716430dea3b33919; date: 2026-10-01
buena	bueno	adj		feminine singular
buenas	bueno	adj		feminine plural
bueno	bueno	adj		canonical
```

### Building a sentence pack: `parley pack build` and `parley pack info`

A sentence pack is one SQLite file of real sentences with English translations and authors,
built from Tatoeba's per-language exports (`spa_sentences_detailed.tsv`,
`eng_sentences_detailed.tsv` and `spa-eng_links.tsv`, decompressed into one directory) and the
same wiktextract extract:

```
parley pack build --lang es --tatoeba-dir data-cache/tatoeba --wiktextract data-cache/es.jsonl --syllabus content/es/syllabus.yaml --built-at 2026-10-01 --out es-2026-10-01.sqlite3
{"lang":"es","kept":22,"dropped":{"no_translation":5,"too_short":1,"too_long":3,"excluded_lemma":2,"unknown_word":5,"duplicate":2}}
```

A sentence is kept when it has an English translation, has 3 to 12 words and at most 120
characters, uses no excluded lemma, uses only words the syllabus or the forms table knows, and
is not a repeat; the report counts each reason. `--built-at` defaults to today. The pack also
carries a frequency rank per lemma and a content hash that does not depend on the build date:

```
parley pack info es-2026-10-01.sqlite3
{"schema":"parley.pack/1","lang":"es","built_at":"2026-10-01","tool_version":"0.1.0","licence":"CC BY 2.0 FR","sentences":22,"translations":22,"forms":84,"ranked_lemmas":24,"bytes":40960,"sha256":"9a70fcbacdf93d720b30426889fed2fcc6e08a856b7e1efed12388474d642f18","content_sha256":"3432d02523e78633e0734da96581de249dd44dc6f63e6997071226d15838ce4e","source_sha256":{"eng_sentences_detailed.tsv":"a40da67ec84151c7159414cbc30282bfd1a3d4e5aa1a221da63484c69c6c3476","exclude.txt":"6f1eae4edd46053462f81e0e46c2fb74674efb6af63c7b4836ea4ad1b58d27b1","forms-mini.jsonl":"bdd2092585e56451edb8e59e844bfca7a0659c3cc5cf51c3716430dea3b33919","spa-eng_links.tsv":"9616d127644056171c47ac37a3ad1df19a065a73f35076d9400a2712b49e7152","spa_sentences_detailed.tsv":"3be194b5d65d2818cd4c54f9a9050a424c155dcd589bd908314f8a82bfd1393e","valid-es.yaml":"b7cee1a6f0b0fce23d4b18c580078e5cc8a8f9629fb5c93a19ee56f09d8d2f03"}}
```

(This run used the small test inputs under `tests/fixtures/`; a real export gives thousands of
sentences.) The app downloads a published pack from the URL in `content/packs.lock.toml` and
refuses one whose SHA-256 differs with "The download was damaged. Try again."

### Checking scenarios: `parley content scenarios`

```
parley content scenarios tests/fixtures/scenarios/bad-SC06.yaml --content tests/fixtures/scenarios/content
{"ok":false,"errors":["bad-SC06: es: opening uses \"pongo\", which the listed skills do not teach","bad-SC06: es: opening uses \"Café\", which the listed skills do not teach"],"warnings":[]}
```

checks one scenario, or every `*.yaml` of a folder, against the syllabi of the `--content`
folder (default `content`) and exits 1 on any error. The rules, in order: the file is YAML and
matches `schemas/scenario.schema.json`; its `id` is its file name; it has 3 to 6 goals and a
turn cap of 6 to 20; every skill it lists for a language exists in that language's syllabus;
and every word of the opening is a function word, a name, a number or a form of a lemma of a
skill up to the latest listed one, so a learner who has done those skills can read it. Here
`pongo` and `café` are taught only by later skills. A valid scenario gives
`{"ok":true,"errors":[],"warnings":[]}`.

### A course report: `parley content report`

```
parley content report --lang es --content tests/fixtures/course05/course --pack es-2026-11-05.sqlite3 --json
{"lang":"es","units":{"A1":2,"A2":1},"skills":4,"skills_per_unit":{"min":1,"max":2},"lemmas":{"A1":18,"A2":5},"examples":16,"attested_share":0.5625,"a1_rank_le_2000_share":0.8235,"checklist_uncovered":["a1.g.plural_nouns","a2.f.directions"],"unreviewed_units":["es.a1.u02","es.a2.u01"]}
```

(The pack is the small one of the previous section, built with `--built-at 2026-11-05`.) It
counts the units per level, the skills, the smallest and largest unit, the distinct lemmas of A1
and those A2 adds, and the examples; `attested_share` is the share of examples that are, word
for word, sentences of the pack, and `a1_rank_le_2000_share` the share of single-word A1 lemmas
among the pack's 2,000 most frequent. Both are null without `--pack`. `checklist_uncovered`
lists the checklist entries no skill teaches yet, and `unreviewed_units` the units awaiting
native-speaker review. A course with errors gives the validate report instead, with exit code 1.

### Frequency ranks: `parley freq check`

A lemma's `rank` is its rank in the pack's frequency table. `freq check` compares them:

```
parley freq check --lang es --pack es-2026-11-05.sqlite3 --content tests/fixtures/course05/course
{"lang":"es","lemmas":23,"differs":[{"skill":"es.a1.u01.s02","lemma":"casa","rank":9,"pack_rank":7},{"skill":"es.a2.u01.s01","lemma":"amigo","rank":30,"pack_rank":9}],"missing":[{"skill":"es.a1.u01.s01","lemma":"adiós"},{"skill":"es.a1.u01.s02","lemma":"perro"},{"skill":"es.a1.u02.s01","lemma":"leche"}]}
```

`lemmas` counts the single-word lemmas checked (a lemma with a space has no rank), `differs`
those whose rank is not the pack's, and `missing` those the pack never ranks: such a word cannot
be taught, so the author picks another or a multi-word lemma. The exit code is 1 until both
lists are empty; the command reports, and the author edits the ranks.

### Installing a pack for development: `parley pack install`

The app downloads a pack from the release URL in `content/packs.lock.toml` and checks its
SHA-256, but until the packs are published as release assets, **Download** says "The course pack
could not be downloaded. Try again later." (offline: "You're offline. Parley works without the
pack, with fewer example sentences."). A pack built with `parley pack build` and pinned in
`content/packs.lock.toml` can be installed by hand instead:

1. Start the course in the app (choose the language in onboarding), then close the app.
2. Install the pack into the app data folder:

```
parley pack install data-cache/packs/es-2026-11-05.sqlite3 --data-dir <app data folder>
{"lang":"es","version":"2026-11-05","path":"packs/es-2026-11-05.sqlite3"}
```

The app data folder is `%APPDATA%\org.parley.app` on Windows (`AppData\Roaming\org.parley.app`
in your user folder), `~/Library/Application Support/org.parley.app` on macOS and
`~/.local/share/org.parley.app` on Linux; Settings > About shows it too.

The file must be exactly the one the lock pins for its language (`--lock <file>` names another
lock), else `error: This pack does not match packs.lock.toml.` and nothing is written; without a
started course, `error: Start the Spanish course in Parley first.` The pack is installed the way
a download is: written beside its place, then moved into it, with the course's previous pack
removed. The next launch shows the pack as installed, and lessons take example sentences from
it.

### Reviews and the scheduler: `parley srs replay`

Every item becomes two cards: a receptive one (you see Spanish and recognise it) and a
productive one (you produce it). A card is scheduled by FSRS (the `fsrs` crate's default FSRS-6
parameters, no fuzz): its stability is the number of days after which you would recall it 90%
of the time, its difficulty runs from 1 to 10, and the next review is due after the rounded
interval at your desired retention (0.80 to 0.95, default 0.90; at 0.90 the interval is the
stability), at least 1 day and at most 365. A graded answer becomes a rating: "I was right" is
Good (3); wrong is Again (1); almost (an accent, a typo, an article) is Hard (2); correct with a
hint is Hard; correct without a hint in a pick-the-meaning or match-the-pairs exercise within
2.5 seconds is Easy (4); any other correct answer is Good. Again on a card you knew is a lapse.

Every review is appended to the review log in your data file and never changed, and every card
is exactly what replaying its log gives. `parley srs replay` shows it for a log file. This one,
`review-log.json`, holds four reviews of `hola` on two days:

```
{"schema": "parley.review-log/1", "entries": [
  {"item_id": "es.a1.u01.s01#l:hola", "facet": "receptive", "reviewed_at": 1793610000000, "rating": 3,
   "exercise_type": "pick_meaning", "verdict": "correct", "response_ms": 3100, "session_id": "lesson-1"},
  {"item_id": "es.a1.u01.s01#l:hola", "facet": "productive", "reviewed_at": 1793610060000, "rating": 2,
   "exercise_type": "translate", "verdict": "almost", "response_ms": 5200, "session_id": "lesson-1"},
  {"item_id": "es.a1.u01.s01#l:hola", "facet": "receptive", "reviewed_at": 1793782800000, "rating": 4,
   "exercise_type": "pick_meaning", "verdict": "correct", "response_ms": 1400, "session_id": "review-1"},
  {"item_id": "es.a1.u01.s01#l:hola", "facet": "productive", "reviewed_at": 1793782860000, "rating": 1,
   "exercise_type": "translate", "verdict": "wrong", "response_ms": null, "session_id": "review-1"}
]}
```

```
parley srs replay review-log.json
{"cards":[{"item_id":"es.a1.u01.s01#l:hola","facet":"receptive","state":"review","stability":18.521770477294922,"difficulty":1.0,"due":1795424400000,"last_review":1793782800000,"reps":2,"lapses":0},{"item_id":"es.a1.u01.s01#l:hola","facet":"productive","state":"relearning","stability":0.40717509388923645,"difficulty":8.378631591796875,"due":1793869260000,"last_review":1793782860000,"reps":2,"lapses":1}]}
```

Times are milliseconds since 1970 (UTC): the receptive card, Good and then Easy two days later,
is due in 19 days; the productive card, Hard and then Again, has lapsed and is due the next day.
In an entry, `response_ms` may be `null` or left out, and `elapsed_days`, `stability_after` and
`difficulty_after` (which your data file records) may be left out: they are recomputed. A lower
desired retention leaves stability and difficulty as they are and spaces reviews further apart:

```
parley srs replay review-log.json --retention 0.8
{"cards":[{"item_id":"es.a1.u01.s01#l:hola","facet":"receptive","state":"review","stability":18.521770477294922,"difficulty":1.0,"due":1799053200000,"last_review":1793782800000,"reps":2,"lapses":0},{"item_id":"es.a1.u01.s01#l:hola","facet":"productive","state":"relearning","stability":0.40717509388923645,"difficulty":8.378631591796875,"due":1793869260000,"last_review":1793782860000,"reps":2,"lapses":1}]}
```

A retention outside the range is a usage error,
`error: --retention must be a number from 0.80 to 0.95. Run parley --help.`; a file that is not
a review log exits 1 with what is wrong, for example
`error: settings.json: schema must be parley.review-log/1`.

The app uses the same cards for the review queue (the due cards you are most likely to have
forgotten first), for each skill's mastery (the average chance of recall over both cards of all
its items) and for the learning path: a skill is available once the skills it builds on are
practised or tested out, learning once you have started it, practised once every item has been
reviewed, and mastered when it is practised and its mastery is at least 85%.

### The placement test: `parley placement simulate`

The placement test walks up and down the course's skills: a right answer moves it up, a wrong
one down, and every change of direction halves the step. It stops after 20 questions, after 12
or more once the direction has changed 8 times, or early after 4 wrong answers in a row at the
first skill. The estimate is the average of the turning points after the first two (or of the
last four questions when there are fewer than three); the skills below it are tested out, and
the label is "Pre-A1" below the fourth skill, otherwise "around" and the estimate's CEFR level.

`parley placement simulate` runs it against a simulated learner who answers correctly exactly
below skill index `--boundary`. On the Spanish course, a learner who knows its first five skills:

```
parley placement simulate --syllabus content/es/syllabus.yaml --boundary 5
{"skills":8,"questions":[2,4,6,5,4,5,4,5,4,5,4,5],"answers":[true,true,false,false,true,false,true,false,true,false,true,false],"reversals":[6,4,5,4,5,4,5,4,5],"estimate":5,"label":"around A1","tested_out":["es.a1.u01.s01","es.a1.u01.s02","es.a1.u01.s03","es.a1.u01.s04","es.a1.u02.s01"]}
```

A beginner stops early:

```
parley placement simulate --syllabus content/es/syllabus.yaml --boundary 0
{"skills":8,"questions":[2,0,0,0,0],"answers":[false,false,false,false,false],"reversals":[],"estimate":0,"label":"Pre-A1","tested_out":[]}
```

`--slip <p>` flips each answer with probability `p` (default 0), drawn from a SplitMix64
generator seeded with `--seed <s>` (default 0), so a run is the same on every machine:

```
parley placement simulate --syllabus content/es/syllabus.yaml --boundary 5 --slip 0.2 --seed 7
{"skills":8,"questions":[2,4,3,4,5,4,5,4,5,6,5,6],"answers":[true,false,true,true,false,true,false,true,true,false,true,false],"reversals":[4,3,5,4,5,4,6,5,6],"estimate":5,"label":"around A1","tested_out":["es.a1.u01.s01","es.a1.u01.s02","es.a1.u01.s03","es.a1.u01.s04","es.a1.u02.s01"]}
```

`--boundary` and `--seed` must be whole numbers and `--slip` a number from 0 to 1
(`error: --boundary must be a whole number. Run parley --help.`); a syllabus that does not load
exits 1 with the report `parley content validate --json` prints.

### Grading an answer: `parley grade`

An answer is compared after normalising both sides: case, punctuation, typographic apostrophes
and extra spaces never count (`¿Qué  TAL?` is `qué tal`). Then, in order:

- **exact**: it matches an accepted answer: correct.
- **article** (vocabulary nouns): the noun without its article is almost ("Almost: remember the
  article: el día"); the noun with another article is wrong ("Día is masculine: el día.").
- **accent**: it differs only in accents, or in German writes `ae`, `oe`, `ue`, `ss` or drops the
  umlaut: almost with the default setting ("Almost: mind the accent: días"), wrong with
  `strict`, correct with a note with `ignore`.
- **typo**: one wrong, missing, extra or swapped letter in a word of 4 letters or more, in at
  most one word in four (at least one), and not a different real word (`pero` for `perro` is
  wrong): a typo, rated like almost ("Typo: leche").
- **a stored judgement** of this very answer.
- otherwise a free translation is **unchecked**: an AI judge decides it later, or you press "I was
  right", and the same answer is then correct next time, offline. Every other type is wrong.

Almost and typo rate the card Hard, wrong Again. `parley grade` shows it for one answer, with
nothing stored. `--type` is `translate_to_target`, `translate_to_en`, `listen_type`, `speak` or
`fill_gap`; `--expected` can be repeated; `--accent` is `lenient` (default), `strict` or
`ignore`; `--noun` turns on the article rule; the forms table is `content/<lang>/forms.tsv`
unless `--forms` names another.

```
parley grade --lang es --type listen_type --expected "Buenos días." --answer "Buenos dias"
{"verdict":"almost","tags":["accent"],"message":"Almost: mind the accent: días","corrected":"Buenos días.","words":["días"],"rating":2}
parley grade --lang es --type listen_type --expected "Buenos días." --answer "Buenos dias" --accent ignore
{"verdict":"correct","tags":["accent"],"message":"Note the accent: días","corrected":"Buenos días.","words":["días"],"rating":3}
parley grade --lang es --type translate_to_target --expected "el día" --answer "la día" --noun
{"verdict":"wrong","tags":["article"],"message":"Día is masculine: el día.","corrected":"el día","words":["el día"],"rating":1}
parley grade --lang es --type fill_gap --expected "Quiero un café con leche." --answer "quiero un cafe con lecje"
{"verdict":"typo","tags":["typo","accent"],"message":"Typo: café, leche","corrected":"Quiero un café con leche.","words":["café","leche"],"rating":2}
parley grade --lang de --type speak --expected "Das Mädchen" --answer "das Maedchen"
{"verdict":"almost","tags":["accent"],"message":"Almost: mind the accent: Mädchen","corrected":"Das Mädchen","words":["Mädchen"],"rating":2}
parley grade --lang es --type translate_to_en --expected "Good morning." --answer "Morning!"
{"verdict":"unchecked","tags":[],"message":null,"corrected":null,"words":[],"rating":null}
```

An answer with no words exits 1 with `error: Type an answer first.`; a type that is not typed is
a usage error (`error: --type must be translate_to_target, translate_to_en, listen_type, speak or
fill_gap. Run parley --help.`).

### A template lesson: `parley lesson template`

A lesson plans up to 12 exercises: the skill's next 5 new items, each introduced with a
recognition exercise; a match-the-pairs round when there are 4 or more words; your due cards
alternating with a production exercise for each new item; then the skill's other cards, the ones
you are most likely to have forgotten first. Every exercise uses only words the course has taught
by then, has at most 8 words at A1 (12 at A2), and practises its item; an exercise that cannot be
built that way falls back to a simpler type. There is no limit on lessons.

`parley lesson template` builds the template lesson of a skill offline, as the app does without
AI, for a learner who has done its earlier lessons (`--seq`, default 1); `--seed` seeds the
generator (default 0), so a seed always gives the same lesson:

```
parley lesson template --lang es --skill es.a1.u01.s01 --seed 1
{"schema":"parley.lesson/1","lang":"es","skill_id":"es.a1.u01.s01","seq":1,"origin":"template","exercises":[{"id":"e1","item_id":"es.a1.u01.s01#l:hola","facet":"receptive","origin":"template","type":"pick_meaning","prompt_target":"hola","options":["afternoon","night","day","hello"],"answer_index":3},{"id":"e2","item_id":"es.a1.u01.s01#l:adiós","facet":"receptive","origin":"template","type":"pick_meaning","prompt_target":"adiós","options":["very","day","good","goodbye"],"answer_index":3},{"id":"e3","item_id":"es.a1.u01.s01#l:bueno","facet":"receptive","origin":"template","type":"pick_meaning","prompt_target":"bueno","options":["very","day","such","good"],"answer_index":3},{"id":"e4","item_id":"es.a1.u01.s01#l:día","facet":"receptive","origin":"template","type":"pick_meaning","prompt_target":"el día","options":["afternoon","day","good","thank you"],"answer_index":1},{"id":"e5","item_id":"es.a1.u01.s01#l:tarde","facet":"receptive","origin":"template","type":"pick_meaning","prompt_target":"la tarde","options":["goodbye","day","very","afternoon"],"answer_index":3},{"id":"e6","item_id":"es.a1.u01.s01#l:hola","facet":"receptive","origin":"template","type":"match_pairs","pairs":[["adiós","goodbye"],["bueno","good"],["hola","hello"],["el día","day"]]},{"id":"e7","item_id":"es.a1.u01.s01#l:hola","facet":"productive","origin":"template","type":"fill_gap","sentence":"___, ¿qué tal?","answer":"Hola","full_sentence":"Hola, ¿qué tal?","hint_en":"hello"},{"id":"e8","item_id":"es.a1.u01.s01#l:adiós","facet":"productive","origin":"template","type":"fill_gap","sentence":"___, gracias.","answer":"Adiós","full_sentence":"Adiós, gracias.","hint_en":"goodbye"},{"id":"e9","item_id":"es.a1.u01.s01#l:bueno","facet":"productive","origin":"template","type":"fill_gap","sentence":"___ días.","answer":"Buenos","full_sentence":"Buenos días.","hint_en":"good"},{"id":"e10","item_id":"es.a1.u01.s01#l:día","facet":"productive","origin":"template","type":"fill_gap","sentence":"Hola, buenos ___.","answer":"días","full_sentence":"Hola, buenos días.","hint_en":"day"},{"id":"e11","item_id":"es.a1.u01.s01#l:tarde","facet":"productive","origin":"template","type":"fill_gap","sentence":"Buenas ___, Luis.","answer":"tardes","full_sentence":"Buenas tardes, Luis.","hint_en":"afternoon"}]}
```

`--content <dir>` reads another course folder (default `content`). A skill the syllabus does not
have exits 1 (`error: unknown skill es.a1.u99.s01`); `--seq` must be a whole number from 1.

### XP, streaks and freezes: `parley streak simulate`

Each correct, typo or almost answer earns 1 XP; finishing a lesson earns 10 more, and 5 more
again with no wrong answer; finishing a review earns 5. Nothing is ever taken away. The daily goal
(20 XP by default) is met when the day's XP reaches it.

A day with a finished lesson or review keeps your streak going. Every 7 days of streak earn a
freeze, and up to 2 can be banked; a missed day uses one (the streak stays) or, without one, the
streak starts again. Freezes are only earned, never bought. Until you practise today, your streak
is yesterday's. A session counts on the day it started, where you are. When a week's XP first
passes your best earlier week, you see "New weekly record!", once that week.

`parley streak simulate` replays a list of sessions. This `days.json` has one session on Monday
26 October, then one a day from Monday 2 to Sunday 8 November, none on the 9th, and one on the
10th, all at 19:00 in a time zone one hour east of UTC:

```
{"today": "2026-11-10", "utc_offset_minutes": 60, "goal_xp": 20, "sessions": [
  {"started_at": 1793037600000, "xp": 20},
  {"started_at": 1793642400000, "xp": 15},
  {"started_at": 1793728800000, "xp": 15},
  {"started_at": 1793815200000, "xp": 15},
  {"started_at": 1793901600000, "xp": 15},
  {"started_at": 1793988000000, "xp": 15},
  {"started_at": 1794074400000, "xp": 15},
  {"started_at": 1794160800000, "xp": 15},
  {"started_at": 1794333600000, "xp": 15}
]}
```

```
parley streak simulate days.json
{"length":8,"freezes":0,"active_today":true,"frozen":["2026-11-09"],"events":[{"local_date":"2026-11-08","kind":"earned","streak_len":7},{"local_date":"2026-11-09","kind":"used","streak_len":7}],"days":[{"local_date":"2026-10-26","xp":20,"sessions":1},{"local_date":"2026-11-02","xp":15,"sessions":1},{"local_date":"2026-11-03","xp":15,"sessions":1},{"local_date":"2026-11-04","xp":15,"sessions":1},{"local_date":"2026-11-05","xp":15,"sessions":1},{"local_date":"2026-11-06","xp":15,"sessions":1},{"local_date":"2026-11-07","xp":15,"sessions":1},{"local_date":"2026-11-08","xp":15,"sessions":1},{"local_date":"2026-11-10","xp":15,"sessions":1}],"records":[{"local_date":"2026-11-03","message":"New weekly record!"}]}
```

The missed days after 26 October reset the streak; the seven days from 2 November earn a freeze,
which covers the 9th, so the streak is 8 on the 10th. The second session of the new week passes
the 20 XP of the week before: a weekly record. `utc_offset_minutes` defaults to 0 and `goal_xp`
to 20; a file that is not a days file exits 1 with what is wrong.

## Coming next

In the order of `docs/ROADMAP.md`: fresh AI lessons and stories, then AI answer checks and
explanations, through the provider set up in Settings > AI. The sentence packs are published as
release assets before the first release; until then the app's Download button cannot fetch them,
and `parley pack install` puts a locally built pack in place.
