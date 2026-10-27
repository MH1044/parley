# ADR-0009: Default voice: OS voices by default, natural voices opt-in

- **Status:** Proposed (the maintainer decides at module 09; the module ships with OS voices as the default and does not wait)
- **Date:** 2026-10-27

## Context

Every target-language sentence can be heard. Two sources exist: the operating system's voices
(free, offline, uneven quality across platforms and languages) and natural voices through the
provider (`/audio/speech`, paid per sentence, cached so each sentence is paid for once). Speech is
the largest audio cost of a daily learner (about $0.042 a day with natural voices on, against
$0.099 for everything else, `docs/PUBLIK-INTEGRATION.md` §8) and, as a pass-through product, earns
no developer share.

## Decision (proposed)

Recommended: OS voices by default (`audio.voice_source = os`), with natural voices as an opt-in
switch in Settings > Audio that shows the measured cost per sentence. Changing the default later is
one line in the settings defaults.

## Consequences

- A fresh install hears every sentence at $0.00 and offline; a learner who wants better voices
  turns them on knowingly.
- A language with no installed OS voice shows how to install one and skips listening exercises
  until then.
- With natural voices by default instead: better audio from the first lesson, about 40% more
  spending per day, and the first AI action (a 402 at $0.00) would arrive in the first lesson.

## Alternatives considered

- **Natural voices by default**: rejected in the recommendation for cost and for making the first
  lesson depend on a provider.
