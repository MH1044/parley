# ADR-0007: Sentence packs: built from Tatoeba, published as release assets, pinned by checksum

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

Template lessons and fill-the-gap exercises need real sentences with translations and authors.
Tatoeba's exports (CC BY 2.0 FR) provide them, but they are large, change over time, and carry
attribution requirements. Committing them would bloat the repository and freeze a snapshot without
a build record; downloading them at runtime from Tatoeba would make first launch depend on a third
party's servers and on parsing exports on the learner's machine.

## Decision

`parley pack build` turns the Tatoeba exports (and the forms table) into one SQLite pack per
language with the author of every sentence kept for attribution and a content hash recorded. Packs
are published as release assets of this repository and pinned in `content/packs.lock.toml` by URL,
size, SHA-256 and content hash. The app downloads a pack on explicit user action, shows its size
first, verifies the checksum, and keeps working without it (with fewer example sentences). Packs are
never committed; the build report and the lock file are.

## Consequences

- First launch works offline; the download is optional and verified; a damaged download is
  refused with a plain message and nothing left behind.
- Every sentence shown carries "Sentence by {author} on Tatoeba", as the licence asks.
- Rebuilding a pack is reproducible: the same inputs give the same content hash.
- Cost: a maintainer step to publish packs on a `packs-<date>` tag, and a lock file to keep current.

## Alternatives considered

- **Committing Tatoeba data**: CC BY data is downloaded by checksum and never committed in this
  project, and the size (tens of megabytes per language, growing) would weigh on every clone.
- **Parsing exports at runtime**: slow first launch, a dependency on Tatoeba's servers, and no fixed
  content to test against.
