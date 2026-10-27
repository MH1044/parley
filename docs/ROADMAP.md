# Parley: roadmap

| NN | Module | Goal | Tier | Depends on | Status |
|---|---|---|---|---|---|
| 00 | scaffold | Workspace, `verify`, licence gate, hygiene test, golden harness, fixture generator, CI, ADRs, a window that opens | process | – | Done (2026-10-27) |
| 01 | content-model-and-syllabus | Syllabus format and validator, items, tokeniser, forms table, Tatoeba pack builder, seed content | load-bearing (content formats) | 00, ADR-0003 decided | Planned |
| 02 | srs-and-learner-model | FSRS cards and review log, mastery, skill states, adaptive placement, CEFR estimate | load-bearing (learner data) | 01 | Planned |
| 03 | exercise-engine-and-grading | Exercise types, normaliser, accent and typo grading, template lessons, composer, XP, streaks and freezes | load-bearing (grading) | 01, 02 | Planned |
| 04 | app-shell-and-offline-course | The whole offline app: onboarding, path, lesson player, reviews, stats, settings, backup | interface (backup is load-bearing) | 03 | Planned |
| 05 | course-content-a1-a2 | Complete A1 and A2 syllabi for es, fr, de; scenarios; content reports; packs pinned | load-bearing (what learners are taught) | 01, 04 | Planned |
| 06 | publik-and-providers | The Publik contract in full, own key, local model, consent, status, errors, the AI client | load-bearing (money path) | 04 | Planned |
| 07 | lesson-generation | Fresh AI lessons and stories with validators and template fill; course pack export and import | load-bearing (what learners see) | 03, 06 | Planned |
| 08 | judging-and-explanations | AI judge for free translations, cached and learned; Explain my answer | load-bearing (grading) | 03, 06 | Planned |
| 09 | listening-and-voices | OS voices, natural voices with cache, slow mode, listen-and-type | load-bearing (audio cost) | 04, 06 | Planned |
| 10 | speaking-and-pronunciation | Microphone capture, trimming, transcription (provider or local Whisper), word scoring, speak exercise | load-bearing (audio path, privacy) | 09 | Planned |
| 11 | conversation-roleplay | Scenarios, streamed turns, corrections, goals, turn cap, cost line, summary to reviews | load-bearing (money path) | 06, 09, 10 | Planned |
| 12 | photo-vocabulary-and-your-media | Photo to vocabulary; media import, chunked transcription, transcript, translate, save | load-bearing (privacy, batch cost) | 06, 10 | Planned |
| 13 | android | Tauri mobile build, reqwest transport, Android voices and microphone, word bank default | load-bearing (credential storage) | 12 | Planned |
| 14 | packaging-and-release | Installers, tagged releases, update check, macOS job, listing prepared, packs published | process | 13 | Planned |

The order puts the offline course first: modules 01 to 04 give a complete app that teaches, grades
and schedules with no provider at all, which is exactly what a fresh install at $0.00 has, and it
lets a person test every later AI feature by hand in the real interface (ADR-0012). Content
authoring (05) sits after the app shell so the maintainer and reviewers can read the course in the
app as it grows, and before any AI module because lesson generation and role-play are anchored on
the syllabus. The Publik module (06) comes before the first module that makes an AI call (07); its
live path needs the app token, which the maintainer's one-time Publik setup run (listing, app token
and the built-app self-test) provides once the module is done and a release build works, and until
then it is completed and tested against the fake server. Judging (08) follows lessons because it grades
their free translations. Listening (09) comes before speaking (10) because the speak exercise
replays the voice next to the learner's take, and both come before conversation (11), which uses
them for every turn. Photo and media (12) reuse the transcription and image paths. Android (13)
waits until the desktop feature set is stable, and packaging (14) is last.
