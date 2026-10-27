# ADR-0001: Stack: Tauri 2, Rust core, React and TypeScript interface

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

Parley must work offline at its core, keep a per-install API key out of the interface, ship as a
small installer on Windows first and then macOS and Linux, and reach Android later with the same
course, progress format and features (`docs/PRODUCT.md` F21, F22). The learning logic (scheduling,
grading, content validation, every AI call and its validation) is the load-bearing part and must be
testable without a window.

## Decision

A Tauri 2 desktop app: the logic in a Rust library crate (`crates/parley-core`) with the AI client
and audio in sibling crates, a thin Tauri shell that only marshals commands and events, and a React
and TypeScript interface built with Vite. Android is reached through Tauri mobile with the same Rust
core. The app identifier is `org.parley.app` as a placeholder until a domain is settled.

## Consequences

- One language for everything that matters, with a test suite that runs on both CI runners without
  a display. The interface is unit-tested with Vitest and launch-tested end to end.
- The key never reaches the web view: the interface only sees a status object.
- Installers stay small (the system web view is used), and the Android build shares the core.
- Costs: Tauri's Linux and Android prerequisites, two toolchains (Rust and Node) for every
  contributor, and a web view that differs slightly per platform.

## Alternatives considered

- **Electron**: large installers, and no shared Rust core on Android.
- **Flutter**: a second language for the core, or a bridge to Rust for every call.
- **A web app**: key custody in a browser, and no credible offline story for the whole course.
