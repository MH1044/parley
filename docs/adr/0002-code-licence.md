# ADR-0002: Code licence: MIT OR Apache-2.0

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

Parley is open source from the first commit. Its core (a scheduler, a grader, a syllabus
validator, an AI validation pipeline) is the kind of code other projects may want to reuse, and the
Rust ecosystem's convention is dual MIT OR Apache-2.0, which keeps the code compatible with both
permissive and patent-granting uses. The dependencies the stack needs are permissive, and the
licence gate (`deny.toml`, `scripts/check-npm-licences.mjs`) keeps copyleft code out of the build.

## Decision

The code is licensed under MIT OR Apache-2.0, at the user's option (`LICENSE-MIT`,
`LICENSE-APACHE`), with the copyright line "Copyright (c) 2026 The Parley contributors". The
licence gate allows only permissive licences (MIT, Apache-2.0 with and without the LLVM exception,
BSD-2-Clause, BSD-3-Clause, ISC, Zlib, 0BSD, Unicode-3.0, Unicode-DFS-2016, MPL-2.0, Unlicense,
CC0-1.0, BSL-1.0). The course content has its own licence (ADR-0003).

## Consequences

- Anyone can reuse the core in a permissive or proprietary project; contributions are accepted
  under the same terms.
- The clean-room rule in `CONTRIBUTING.md` applies: the source of GPL, AGPL, SSPL, BUSL or
  source-available projects is never open while writing the corresponding part of Parley.
- No copyleft dependency can ever be added, whatever it would save.

## Alternatives considered

- **GPL-3.0 or AGPL-3.0**: would bar permissive reuse of the core and would have let the project
  study LibreLingo and Anki (both AGPL-3.0) directly; reuse of the core was judged more valuable.
- **MIT alone**: no explicit patent grant; the dual licence costs nothing and is the convention.
