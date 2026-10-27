# ADR-0005: Grading order: exact, accent, typo, cached judgement, then the AI judge

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

Grading must be forgiving about case, punctuation, spacing, a missed accent and a single slip in a
long word, honest about everything else, instant and offline for the great majority of answers, and
never dependent on an AI call to accept an answer that a rule can accept. A free translation that
no rule can match is the one case where a judgement is worth paying a fraction of a cent for
(`docs/PRODUCT.md` F5, F13).

## Decision

Every answer is graded in this order, stopping at the first verdict: exact match after
normalisation (NFC, lowercase, punctuation removed, whitespace collapsed); accent match (almost,
with the accented word shown; lenient, strict or ignore by setting); typo match (one slip per
differing word of four or more letters that is not itself a known form); a cached judgement for the
same sentence and answer; and only then, only for free translations and only with a provider ready,
the AI judge on `publik-fast`. With no provider the learner can say "I was right", which is
remembered.

## Consequences

- Grading one answer offline takes under 5 ms (module 03 benchmark) and never needs a connection.
- A correct AI verdict is stored as an accepted answer, so the same answer is accepted offline next
  time, and the judge is called at most once per submission.
- The rules are deterministic and testable by hand, which is what the acceptance tests in module 03
  do.
- Cost: a two-pass normaliser and an edit-distance check are more code than "ask the model".

## Alternatives considered

- **AI first**: cost on every answer, latency on every answer, and nothing works offline. Rejected.
- **Exact match only**: punishes a missing accent as a wrong answer, which teaches learners to stop
  trying.
