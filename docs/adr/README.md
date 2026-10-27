# Decision records

Architecture decision records (ADRs): the decisions that are expensive to make and expensive to
reverse, each with what was rejected and why. `docs/ARCHITECTURE.md` §9 summarises them.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-stack.md) | Stack: Tauri 2, Rust core, React and TypeScript interface | Accepted |
| [0002](0002-code-licence.md) | Code licence: MIT OR Apache-2.0 | Accepted |
| [0003](0003-content-licence.md) | Course content licence: CC BY-SA 4.0 | Accepted |
| [0004](0004-scheduler.md) | Scheduler: FSRS with two cards per item | Accepted |
| [0005](0005-grading-order.md) | Grading order: exact, accent, typo, cached judgement, then the AI judge | Accepted |
| [0006](0006-storage.md) | Storage: one SQLite file plus derived caches, JSON backup | Accepted |
| [0007](0007-sentence-packs.md) | Sentence packs: built from Tatoeba, published as release assets, pinned by checksum | Accepted |
| [0008](0008-ai-client.md) | AI client: the reference publik module, one OpenAI-compatible client, no retries | Accepted |
| [0009](0009-default-voice.md) | Default voice: OS voices by default, natural voices opt-in | Proposed (maintainer decides at module 09) |
| [0010](0010-ios.md) | iOS: later | Proposed (maintainer decides after module 13) |
| [0011](0011-no-server.md) | No server: streaks, XP and records are local; no leagues | Accepted |
| [0012](0012-module-order.md) | Module order: the offline app before publik API | Accepted |

## Rules

- Append-only. An accepted record is never edited; a change gets a new record that supersedes it,
  and the old one's status becomes `Superseded by ADR-NNNN`.
- Only decisions that are expensive to make and expensive to reverse. Anything else goes in the
  module spec's Design notes.
- Record what was rejected and the deciding factor for each. That section is the one people come
  back to.
- A `Proposed` record names who decides and by when. The module that depends on it waits until it
  is `Accepted`.

To add one, copy `template.md` to `NNNN-short-title.md` with the next number, fill every section,
and add a row above.
