# ADR-0008: AI client: the reference publik module, one OpenAI-compatible client, no retries

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

Parley makes AI calls through three providers the learner chooses between: publik API (per-install
keys, a balance, a 402 on an empty balance), the learner's own OpenAI-compatible key, and a local
model server. The publik contract (`docs/PUBLIK-CONTRACT.md`) asks every app to copy its reference
module verbatim so future changes merge easily, to keep the key out of the interface, to send tier
names rather than vendor model names, and never to retry a publik call automatically because usage
is settled even when the client gives up. Android has no `curl`, and role-play needs streaming.

## Decision

The reference `publik.rs` is copied verbatim into `crates/parley-publik` and changed only in the
marked adaptations: `reqwest` with rustls as the transport on every platform (the key only ever in
the `Authorization` header), no `device_name` unless the learner opts in, and the status and error
mapping kept in separate files. One OpenAI-compatible client, implementing the `AiClient` trait of
`parley-core`, serves all three providers; the core has no HTTP code of its own. Every response is
cached by SHA-256 of its input, validated against a JSON Schema and the feature's rules before it is
stored or shown, and never retried on publik API (own-key and local calls retry twice with backoff).
Prompts are versioned files under `prompts/`, loaded at build time and never edited in place.

## Consequences

- One code path for three providers; tests run against a fake server for all of them.
- Identical input never spends twice, and a failed validation stores nothing.
- A publik call that times out is offered as **Try again**, never retried silently.
- Cost: `reqwest` and rustls in the dependency tree, and the discipline of keeping the copied
  module close to the reference.

## Alternatives considered

- **Vendor SDKs**: one per provider, vendor model names in code, and no publik tier mapping.
- **Calls from the web view**: the key would reach the interface and could be logged by it.
- **The reference module's `curl` helper**: not available on Android; no streaming.
