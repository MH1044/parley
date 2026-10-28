# Attribution

The course content in this directory is licensed under CC BY-SA 4.0 (see `LICENSE`). It
builds on two open projects.

## Tatoeba

Sentence packs (`<lang>-<date>.sqlite3`, pinned in `packs.lock.toml`) are built from the
exports of the Tatoeba project, a collaborative collection of sentences and translations.

- Project: <https://tatoeba.org/>
- Licence: Creative Commons Attribution 2.0 France (CC BY 2.0 FR),
  <https://creativecommons.org/licenses/by/2.0/fr/>

Every sentence and every English translation in a pack keeps the Tatoeba user name of its
author, and each pack carries an attribution line in its `meta` table. The packs are a
separate work from the syllabus files and keep this licence.

## Wiktionary through wiktextract

Word-form tables (`<lang>/forms.tsv`) map inflected forms to their lemma. Their source is
Wiktionary, read through the machine-readable extracts that the wiktextract project
publishes.

- Wiktionary: <https://www.wiktionary.org/>
- Extracts: <https://kaikki.org/dictionary/>
- Licence: Creative Commons Attribution-ShareAlike 4.0 (CC BY-SA 4.0),
  <https://creativecommons.org/licenses/by-sa/4.0/>, and the GNU Free Documentation
  License, <https://www.gnu.org/licenses/fdl-1.3.html>

The forms tables of this first release are hand-written for the syllabus lemmas and
checked against Wiktionary; as the course grows they are regenerated from the extracts
with `parley forms extract`. The header line of each table names its source.
