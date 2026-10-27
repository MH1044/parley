# ADR-0003: Course content licence

- **Status:** Accepted (2026-10-27)
- **Date:** 2026-10-27

## Context

The course (`content/<lang>/syllabus.yaml` with can-do statements, lemmas, grammar notes and
example sentences, plus the forms tables, frequency ranks, numbers and scenarios) is open data and
needs a licence of its own, separate from the code (ADR-0002). Two of its planned sources carry
share-alike terms: word forms come from Wiktionary through wiktextract (CC BY-SA 4.0 and GFDL),
and the best frequency lists found are CC BY-SA 4.0 (FrequencyWords); no CC0 or CC BY frequency
list was found on 2026-09-29 (`docs/ARCHITECTURE.md` §1). Sentence packs are built from Tatoeba
(CC BY 2.0 FR) and are a separate artifact with its own attribution.

## Decision

**CC BY-SA 4.0** for everything under `content/`, recorded in `content/LICENSE` and credited in
`content/ATTRIBUTION.md`. The forms tables may be derived from Wiktionary through wiktextract, and
the frequency ranks from any CC BY-SA source or from Tatoeba counts. Anyone reusing the course must
share alike and credit Parley, Tatoeba and Wiktionary. The code stays MIT OR Apache-2.0
(ADR-0002): content and code are separate works, and sentence packs built from Tatoeba keep their
own CC BY 2.0 FR attribution.

## Consequences

- The forms tables and ranks come from the best open sources; `content/ATTRIBUTION.md` credits
  Tatoeba and Wiktionary and names the licence of each.
- Course content written for Parley (syllabi, grammar notes, examples, numbers, scenarios) is
  published under CC BY-SA 4.0 from the first commit that adds it (module 01).
- Fixtures under `tests/fixtures/` stay CC0 and hand-written, so the tests never depend on
  share-alike data.

## Alternatives considered

- **CC0**: maximal reuse, but it forbids the CC BY-SA sources (Wiktionary forms, FrequencyWords),
  so the inflection tables would have to be hand-written for every lemma (several thousand rows per
  language) and the frequency ranks counted from Tatoeba only. Rejected for the cost of authoring
  forms by hand.
- **CC BY 4.0**: incompatible with building on CC BY-SA sources for the same reason.
