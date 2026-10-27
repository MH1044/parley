# ADR-0003: Course content licence

- **Status:** Proposed (the maintainer decides before module 01; module 01 does not start until this record is Accepted)
- **Date:** 2026-10-27

## Context

The course (`content/<lang>/syllabus.yaml` with can-do statements, lemmas, grammar notes and
example sentences, plus the forms tables, frequency ranks, numbers and scenarios) is open data and
needs a licence of its own, separate from the code (ADR-0002). Two of its planned sources carry
share-alike terms: word forms come from Wiktionary through wiktextract (CC BY-SA 4.0 and GFDL),
and the best frequency lists found are CC BY-SA 4.0 (FrequencyWords); no CC0 or CC BY frequency
list was found on 2026-09-29 (`docs/ARCHITECTURE.md` §1). Sentence packs are built from Tatoeba
(CC BY 2.0 FR) and are a separate artifact with its own attribution.

## Decision (proposed)

Recommended: **CC BY-SA 4.0** for everything under `content/`, recorded in `content/LICENSE` and
credited in `content/ATTRIBUTION.md`. With it, `forms.tsv` can be derived from Wiktionary and the
frequency ranks from any CC BY-SA source.

## Consequences

- With CC BY-SA 4.0: the forms tables and ranks come from the best open sources; anyone reusing the
  course must share alike and credit Parley, Tatoeba and Wiktionary; the code stays MIT OR
  Apache-2.0 because content and code are separate works.
- With CC0 instead: no share-alike sources may be used, so inflection tables would have to be
  hand-written for every lemma (several thousand rows per language) and frequency ranks counted from
  Tatoeba only; reuse of the course would be unrestricted.

## Alternatives considered

- **CC0**: maximal reuse, but it forbids the CC BY-SA sources (Wiktionary forms, FrequencyWords),
  so the forms tables would need hand-written rules. Rejected in the recommendation for the cost of
  authoring forms by hand.
- **CC BY 4.0**: incompatible with building on CC BY-SA sources for the same reason.
