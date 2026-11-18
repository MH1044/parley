# Parley: roadmap

| NN | Module | Goal | Tier | Depends on | Status |
|---|---|---|---|---|---|
| 00 | scaffold | Workspace, `verify`, licence gate, hygiene test, golden harness, fixture generator, CI, ADRs, a window that opens | process | – | Done (2026-10-27) |
| 01 | content-model-and-syllabus | Syllabus format and validator, items, tokeniser, forms table, Tatoeba pack builder, seed content | load-bearing (content formats) | 00, ADR-0003 decided | Done (2026-10-28) |
| 02 | srs-and-learner-model | FSRS cards and review log, mastery, skill states, adaptive placement, CEFR estimate | load-bearing (learner data) | 01 | Done (2026-11-03) |
| 03 | exercise-engine-and-grading | Exercise types, normaliser, accent and typo grading, template lessons, composer, XP, streaks and freezes | load-bearing (grading) | 01, 02 | Done (2026-11-03) |
| 04 | app-shell-and-offline-course | The whole offline app: onboarding, path, lesson player, reviews, stats, settings, backup | interface (backup is load-bearing) | 03 | Done (2026-11-03) |
| 05 | course-content-a1-a2 | Complete A1 and A2 syllabi for es, fr, de; scenarios; content reports; packs pinned | load-bearing (what learners are taught) | 01, 04 | Done (2026-11-06) |
| 06 | publik-and-providers | The Publik contract in full, own key, local model, consent, status, errors, the AI client | load-bearing (money path) | 04 | Done (2026-11-09) |
| 07 | lesson-generation | Fresh AI lessons and stories with validators and template fill; course pack export and import | load-bearing (what learners see) | 03, 06 | Done (2026-11-10) |
| 08 | judging-and-explanations | AI judge for free translations, cached and learned; Explain my answer | load-bearing (grading) | 03, 06 | Done (2026-11-11) |
| 09 | listening-and-voices | OS voices, natural voices with cache, slow mode, listen-and-type | load-bearing (audio cost) | 04, 06 | Done (2026-11-12) |
| 10 | speaking-and-pronunciation | Microphone capture, trimming, transcription (provider or local Whisper), word scoring, speak exercise | load-bearing (audio path, privacy) | 09 | Done (2026-11-14) |
| 15 | lesson-flow | The skill's page before its first lesson, a lesson header with the skill and progress, a lesson end with words learned, words to watch and Next lesson, the daily goal ring and goal moment, the Next card, CEFR unit labels, a mastered badge, XP explained | interface (lesson summary figures in the core) | 03, 04 | Planned |
| 16 | quiet-without-voice-or-ai | No play buttons or autoplay without a voice and one calm note on the path; a short AI panel before the consent dialog; Explain from the skill's notes without AI; AI marks | interface (the consent order, I12) | 06, 08, 09, 10, 15 | Planned |
| 17 | variety-and-recall | Interleaved exercise types, an old word at most twice a lesson, earlier skills' words in later lessons, fill the gap with the sentence's English and a first-letter hint, tiles for every typed exercise | load-bearing (composer, templates) | 03, 04, 07 | Planned |
| 18 | placement-rework | A 15-question placement test with progress, questions on a skill's words, verbs and phrases in four offline types, a score, a start at a whole unit and the choice to start at Unit 1 | load-bearing (placement) | 02, 04, 17 | Planned |
| 19 | offline-stories-and-characters | A written story per unit for Spanish, French and German with tap-to-translate and questions, validated; a recurring cast and city per course with unit banners; a Beta tag on unreviewed stories; AI stories as Fresh story (AI) | load-bearing (what learners are taught) | 05, 07, 15, 16 | Planned |
| 20 | warmth | Praise that names what was done, kinder wrong answers, quiet answer sounds and small motion, an optional daily reminder | interface (notification dependency) | 03, 09, 15, 16 | Planned |
| 11 | conversation-roleplay | Scenarios, streamed turns, corrections, goals, turn cap, cost line, summary to reviews | load-bearing (money path) | 06, 09, 10, 16 | Planned |
| 12 | photo-vocabulary-and-your-media | Photo to vocabulary; media import, chunked transcription, transcript, translate, save | load-bearing (privacy, batch cost) | 06, 10, 16 | Planned |
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

Modules 15 to 20 come before conversation (11): the table's order is the build order, and their
numbers only record when they were planned. In testing, a first-time learner of French with no AI
set up and no French voice installed found the free course hard to trust and flat: quizzes on
words nobody had shown them, a lesson end that said almost nothing, a goal counter that ran past
its goal, an error about the missing voice on every exercise, an AI panel behind Explain, Check
with AI and every story, the same six words lesson after lesson, a placement test of food words
that skipped two units, and no story to read. A fresh install at $0.00 is the app most learners
meet first (ADR-0012), so before the next AI feature it is made clear (15), quiet without a voice
or AI (16), varied (17), trustworthy at the start (18), worth reading (19) and warm (20).
Placement (18) follows 17 because its fill-the-gap questions show 17's sentence English; stories
(19) use 16's AI mark and panel for the AI story; warmth (20) gives sound and motion to 15's lesson
end and goal ring. Conversation (11) and photo and media (12) then open their tabs through 16's
panel.
