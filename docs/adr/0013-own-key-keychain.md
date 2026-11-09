# ADR-0013: The learner's own key in the OS keychain through keyring

- **Status:** Accepted
- **Date:** 2026-11-06

## Context

The learner's own OpenAI-compatible key must live somewhere the interface, `settings.json`, a
backup, a log and the status file never reach (invariant I1; `docs/ARCHITECTURE.md` §4 puts it in
the OS keychain on desktop). Each desktop platform has its own store: the Windows Credential
Manager, the macOS Keychain, and the Secret Service on Linux (GNOME Keyring, KWallet), reached over
D-Bus.

`keyring` 4 (MIT OR Apache-2.0) wraps all three behind one `Entry` with `get_password`,
`set_password` and `delete_credential`. Since version 4 it is a thin crate over `keyring-core` and
one store crate per platform. On crates.io (checked 2026-11-06), `keyring` has two owners, but
`keyring-core`, `windows-native-keyring-store`, `apple-native-keyring-store` and
`zbus-secret-service-keyring-store` have one, the same maintainer. CONTRIBUTING.md asks for a
decision record that names the exit for a dependency with one maintainer.

## Decision

`parley-publik` uses `keyring` 4 with its default `v1` feature: Windows Credential Manager, the
macOS Keychain, and the Secret Service over `zbus` (pure Rust, so Linux builds need no system D-Bus
library). The key sits in one entry, service `org.parley.app`, user `own_key`, behind the crate's
own `Keychain` trait (`get`, `set`, `delete`); everything else, and every test, uses that trait
(`MemoryKeychain` in tests), so `keyring` appears in one small file.

The exit, if `keyring` stops being maintained or a platform store breaks: replace the three calls in
`crates/parley-publik/src/keychain.rs` with the platform APIs directly (`windows-sys` Credential
Manager functions, which the app already depends on for Windows, `security-framework` on macOS,
and `secret-service` or `zbus` on Linux), or fall back to a file in the app-private storage that
Android uses anyway (module 13), keeping the `Keychain` trait unchanged. A stored key that cannot
be read back is not lost data: the learner enters it again.

## Consequences

- One small file knows about the platform stores; the service and its tests do not.
- No `libdbus-1-dev` on Linux builds or CI runners.
- On a Linux machine with no Secret Service running, saving the key fails with a message, and the
  learner can still use publik API or a local model.
- Cost: a dependency chain whose platform crates have one maintainer, and zbus's dependency tree on
  Linux.

## Alternatives considered

- **`keyring` 3 with `sync-secret-service`**: links the system D-Bus library, so every Linux build
  and CI runner needs `libdbus-1-dev`; version 3 is no longer the maintained line.
- **The platform APIs directly, now**: three implementations to write and test for one key, where
  `keyring` already covers them; kept as the exit.
- **A file in the app data directory, encrypted with a key on disk**: no real protection, and the
  file would sit next to `settings.json` and backups.
