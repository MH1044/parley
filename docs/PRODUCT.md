# Parley: product

## 1. Summary

Parley is a free, open source app for learning Spanish, French or German from English by reading,
listening, speaking and holding short conversations, with spaced repetition underneath. It
replaces Duolingo Super and covers what people pay Speak, Pimsleur and Language Reactor for,
without hearts, energy, ads or a subscription.

## 2. Why this exists

**The paid products.**

- Duolingo Super: $16.99 a month or $119.99 a year; the free tier limits lessons with an energy
  system on mobile and shows ads; Video Call moved from Max into Super in 2026, and Max is being
  phased out for new subscriptions
  ([languageappguide.com, updated 6 September 2026](https://languageappguide.com/pricing/duolingo-cost/)).
- Speak (speaking practice with AI): Premium $17.99 a month or $83.99 a year; Premium Plus $39.99
  a month ([speakshark.com, 27 August 2026](https://speakshark.com/blog/speak-app-pricing-per-month-2026)).
- Pimsleur (audio lessons): $19.95 a month for one language, $20.95 for all
  ([speakfluentreviews.com, June 2026](https://speakfluentreviews.com/pimsleur-cost/)).
- Language Reactor Pro (learning from films and videos): about $5 a month billed yearly, about $9
  monthly ([tubevocab.com, 10 May 2026](https://www.tubevocab.com/blog/en/language-reactor-review)).
- ELSA Speak is in the same speaking-coach market; its price was not checked for this plan.

**The open source state.** LibreLingo (AGPL-3.0, about 2,600 stars), the best-known open course
platform, was archived in June 2026 ([repository](https://github.com/kantord/LibreLingo)). Anki
(AGPL-3.0) is an excellent flashcard tool but not a course: no syllabus, no speaking, no
conversation. In the [Publik catalogue](https://publikhq.com/apps), the only listing in the
category, Microstudy, says it replaces Duolingo but is Dutch exam-preparation quizzes and teaches no
language (a review of every listing on 29 September 2026 rated it weak, 1 out of 5 for coverage).

**The gap.** Nobody offers, for free and in the open, a structured course with a real scheduler,
honest grading, speaking checks and conversation practice, working offline at its core. The AI
parts Duolingo puts behind Super (conversation, explanations) cost a daily learner about $2 to $3
a month of Publik use, and nothing when they use a local model.

## 3. Who it is for

- **The daily commuter**: 15 minutes a day on a laptop or phone; wants a streak that forgives a
  bad day, lessons that never run out, and no paywall.
- **The traveller**: three months before a trip; wants to order, ask directions and check into a
  hotel out loud, and to hear whether they said it right.
- **The returning learner**: did some school French; wants a placement test that skips what they
  know and a way to learn from podcasts and videos they already own.

## 4. v1 features

Each feature names the module that builds it. "Offline" means with networking disabled.

**F1 Start a course offline** (01, 04, 05, 15). As a learner I pick Spanish, French or German and start
at once. Given a fresh install and no network, when I finish onboarding, then the path shows Unit 1
and the first skill's page, then its first exercise, opens within 90 seconds of first launch. Given
a network, the course pack download shows its size first and verifies its SHA-256; a mismatch shows
"The download was damaged. Try again." and keeps the app usable without the pack.

**F2 Placement test** (02, 04, 18). As a returning learner I take a 15-question adaptive test (it
ends sooner for a complete beginner) that shows how far along it is ("Question 4 of 15") and asks
about each skill's words, verbs and phrases in four exercise types it grades offline. Given my
answers, when the test ends, then I see my score, a CEFR estimate and the unit I start at ("You got
7 of 15. You're around A1. You'll start at Unit 3: Food and drink."), and I choose
**Start at Unit 3** or **Start at Unit 1**. The units before the start are marked tested out; they
stay open for lessons and their words come back in reviews, so a wrong guess is caught. The test
never needs AI. "Start from the beginning" skips it.

**F3 Learning path** (02, 04, 15, 19). Units and skills in syllabus order, each unit labelled with
its CEFR level and a one-line banner about the course's recurring person and place, each skill
locked, available, learning, practised or mastered (rules in module 02; mastered shown as a
badge), with a mastery percentage and its can-do statement. A Next card at the top says what
comes next ("Next: Some and any, lesson 2 of about 4") with one **Continue**, and the current
skill is marked and scrolled into view. Tapping a skill opens its page; a locked one says which
skill opens it.

**F4 Lessons** (03, 04, 15, 17; listening 09, speaking 10, stories 07 and 19). A lesson is 12
exercises (setting 8 to 20) mixing up to 5 new items with due ones; a skill's first lesson opens on
the skill's page (can-do line, grammar note, words) with **Start lesson (about 2 min)**, and later
lessons link the page as **Notes** from a header that shows the skill and progress ("Some and any ·
4 of 12"). Exercise types: pick the meaning; translate into the target language; translate into
English; listen and type; speak the sentence; fill the gap (showing the English of the whole
sentence, with the first letter as a hint on request); match pairs; and, as separate sessions,
graded stories and conversations. Every typed exercise can be answered by typing or with tiles,
switched in the lesson and remembered. Types are interleaved, never more than three of one in a row;
a word the lesson does not introduce is the focus of at most two exercises in it; once a skill's
words are all introduced, its lessons bring back the weakest words of earlier skills too. A lesson
ends with the XP, the score, the words learned, the words to watch with **Practise these now**, and
**Next lesson** as the main button beside **Back to my path**. Given no provider, every lesson is
built from the syllabus and the sentence pack. There is no limit on lessons per day.

**F5 Forgiving, honest grading** (03). Given an answer that differs only in case, punctuation or
spacing, it is correct. Given a missing or wrong accent (or "ss" for "ß", "ae" for "ä"), it is
"Almost: mind the accent" with the accented word shown, counted as correct but scheduled sooner
(setting: lenient, strict or ignore). Given one slip in a word of four letters or more that does not
make another real word, it is "Typo" with the fix shown. A free translation that is not in the
accepted list goes to the AI judge (F13) or, with no provider, offers **I was right**.

**F6 Reviews** (02, 04). Every item has two FSRS cards, receptive and productive. Given due cards,
the Review button shows their count and a review session serves the most at-risk first. Desired
retention defaults to 90% (setting 80% to 95%). Every review is logged; card state can be rebuilt
from the log.

**F7 Streak, freezes, XP, daily goal, weekly record** (03, 04, 15). A day counts towards the streak
when I finish at least one lesson, review or conversation. Every 7 days of streak earns a freeze
(at most 2 banked); a missed day uses one automatically, keeping the streak without adding to it.
Freezes are never sold. XP: 10 per lesson plus 1 per correct answer, and 5 more for a lesson with
no mistakes, explained in one line when I choose my goal; the daily goal is 10, 20, 30 or 50 XP,
shown on the path as a ring that fills and stops at "20 of 20 · goal reached"; the first time a
day's sessions reach it, the lesson end says "Goal reached: 20 of 20 XP. 3 days in a row."; a
skill reaching Mastered and an earned freeze are marked at the lesson end; beating my best week
shows "New weekly record!". No leagues.

**F8 Stats** (04). Streak calendar, XP for the last 30 days, this week against my record, words
known (cards with predicted recall of 90% or more), mastery per skill, the CEFR estimate, and my
measured recall rate over the last 30 days.

**F9 Backup and restore** (04). Export everything I own (progress, items, reviews, conversations,
settings without keys) as one JSON file; import replaces the current data after a confirmation and
rejects a file from another app with "This is not a Parley backup."

**F10 AI setup** (06, 16). When I use an AI feature before AI is set up, a short panel says that
it uses AI and what it costs, with **Set it up** and **Not now**; **Set it up** then explains what
Parley sends and offers publik API, my own key, or a local model, equally. Features that need AI
are marked as such while AI is not set up. Given publik API at $0.00, I see the balance line
and **Sign in with publik**. Settings shows all three providers, the balance and a
**Check connection** button. My key never reaches the interface.

**F11 Fresh AI lessons and course packs** (07). Given a provider, lessons are written fresh for my
skill and known words. Every exercise passes schema, language, vocabulary (90% of words known),
answer and duplicate checks before I see it; failures are replaced from the course pack. I can
export the lessons I have as a course pack and import one from a friend; imported exercises are
checked the same way.

**F12 Graded stories** (07, 19). Every unit ends with a written story that needs no AI: 3 or 4
short paragraphs in the words taught so far plus a few new words, every word translated with a
tap and each paragraph's English on request, then 3 or 4 questions in English; the stories follow
the course's recurring person and place. Stories not yet checked by a native speaker are marked
Beta. Given a provider, **Fresh story (AI)** writes a new one with my words; given a failed check,
I see "The AI answer could not be used." and **Try again**.

**F13 AI judge** (08). Given a free translation not matched offline, publik-fast returns correct,
almost or wrong, a corrected answer and a short explanation within 4 seconds (p95). A correct
verdict is remembered, so the same answer is accepted offline next time.

**F14 Explain my answer** (08, 16). After an almost or wrong verdict, **Explain** gives at most 80
words of plain English about my mistake, cached so a second tap costs nothing. Without AI, it
shows the correct answer, the word's meaning and the skill's grammar note, with no cost.

**F15 Listening** (09, 16). Sentences are read by an OS voice (free, offline) or a natural voice
through the provider, with a slow mode at 0.75× speed. Given no voice for my language, Parley
hides the play buttons, skips listening exercises, and says once on the path, calmly, how to add
a voice. Natural audio is cached, so each sentence is paid for once.

**F16 Speaking checks** (10). I hold a button (or Space) and say the sentence. Parley transcribes it
(provider or local Whisper) and shows each word as right, missed or different, with a score and a
pass mark (80% by default). I can replay my take next to the voice. **Can't speak now** skips
speaking exercises for an hour. Phoneme-level scoring is not in v1.

**F17 Conversation role-play** (11). I pick a scenario (café, directions, shop, hotel, doctor, job
interview), each with a CEFR level and a goals checklist. Each turn I speak or type; the partner
answers in the target language at my level, corrects my turn when needed, and ticks goals. Replies
stream and are spoken. A turn cap (12 by default) and the running cost are always visible. At the
end, a summary turns my mistakes into new review items.

**F18 Photo to vocabulary** (12). I pick a photo; Parley lists up to 12 objects with the word,
article, gender and plural; I untick wrong ones and save the rest to "My photos". The photo is
resized and stripped of metadata before sending and is not kept.

**F19 Your own media** (12). I import an audio or video file I own; Parley transcribes it (provider
or local Whisper), shows an interactive transcript, translates a sentence when I tap it, plays from
that point, and saves sentences to reviews. Long files ask for confirmation with the call count and
estimated cost first.

**F20 Course content to A2** (01, 05). A1 and A2 syllabi for Spanish, French and German as open
data in `content/<lang>/syllabus.yaml`, each skill with a can-do statement, lemmas with frequency
ranks, a short grammar note and example sentences; sentence packs built from Tatoeba with
attribution. Adding another target language is content work (a syllabus, forms, a pack, tokeniser
rules and an article table); the AI pipeline works for any pair the model handles.

**F21 Android** (13). The same app on Android phones, word bank by default, Android voices,
microphone permission asked when first needed; transcription through the provider.

**F22 Installers and updates** (14). Windows installer first, then macOS and Linux packages, built
by a tagged release; an optional update check that sends nothing but the request.

**F23 Warm feedback and reminders** (20). A right answer gets varied praise that names what I did
when Parley knows it ("Right, and the article too."); a wrong one says what the answer is and that
the word will come back ("Not quite. La viande is meat. You'll see it again soon."). Quiet sounds,
which I can turn off, and small motion, none when my system asks for reduced motion, mark answers,
the end of a lesson and the daily goal. An optional daily reminder, off by default, shows one
notification at the time I choose when today's goal is not met, while Parley is open or
minimised, and never mentions the streak.

## 5. Screens and flows

**First run.** (1) Welcome: "Learn by speaking. No hearts, no ads, nothing to buy." [Start]. (2)
Choose a language: three cards, "Parley teaches from English". (3) Course pack: "Download the
Spanish course pack (8.2 MB)?" [Download] [Skip for now]; offline: "You're offline. Parley works
without the pack, with fewer example sentences." [Continue]. (4) Daily goal: 10, 20 (selected), 30,
50 XP, with one line on XP ("XP are points: 1 for each right answer and 10 for each lesson you
finish, 5 more with no mistakes. 20 XP is about one lesson, 2 to 3 minutes."). (5) Where to start:
[Take the placement test (about 5 minutes)] [Start from the beginning]. (6) Placement ("Question 4
of 15"), then "You got 7 of 15. You're around A1. You'll start at Unit 3: Food and drink." [Start
at Unit 3] [Start at Unit 1].
No AI setup appears in onboarding.

| Screen | Purpose and main elements | Empty | Loading | Error | Offline |
|---|---|---|---|---|---|
| Path (home) | a Next card with Continue; units with CEFR labels and banners, skill nodes with the current one marked and mastered ones badged; top bar with streak, freezes, the daily goal ring, reviews due, AI chip; buttons Review, Read a story, Fresh story (AI); a one-time note when the course language has no voice; nav to Conversation, Photo, Your media, Stats, Settings | never empty | skeleton nodes | "Parley could not open its data file." [Restore a backup] [Show details] | AI chip reads "Offline" |
| Lesson player | the skill and a progress bar ("Some and any · 4 of 12"), Notes, exercise, Type or Tiles, Check (Enter), the first-letter hint, "Can't listen now", "Can't speak now", quit with confirmation | n/a | "Writing a fresh lesson…", after 5 s [Use the course pack instead] | a notice when exercises came from the pack | lesson from the pack, notice |
| Result sheet | verdict with a line that names what I did (Correct), Almost: mind the accent, Typo, or Not quite with the answer and "You'll see it again soon."; my answer against the corrected one with differences highlighted; Explain (the skill's notes without AI); I was right; Continue | n/a | "Checking…" (judge) | "Not checked." + reason (section 9 of PUBLIK-INTEGRATION) | "You're offline, so Parley can't check free answers." |
| Lesson end | XP, score, streak, skill mastered, goal reached, freeze earned, weekly record, words learned, words to watch with Practise these now, "1 more lesson to master …", Next lesson, Back to my path | n/a | n/a | n/a | works |
| Story | title, Beta label when unreviewed, paragraphs with a translation on every word, Show in English, questions | n/a | "Writing a story…" (Fresh story (AI) only) | "The AI answer could not be used." [Try again] (AI only) | the written story works |
| Reviews | the player over due cards | "Nothing to review. Next review: tomorrow at 09:00." | as player | as player | works |
| Stats | calendar, XP chart, week against record, words known, mastery, CEFR, recall rate | "Finish a lesson to see your stats." | skeleton | "Stats could not be computed." | works |
| Conversation list | scenarios with level and goals | n/a | n/a | n/a | "Conversations need a connection or a local model." |
| Conversation | turns, provisional streaming text, hold-to-talk, type box, goals, "Turn 4 of 12", "This conversation: $0.012", translation per reply, replay, End | opening line from the scenario | "…" while waiting | withdrawn reply, "The AI answer could not be used." [Try again] | unavailable, with the line above |
| Conversation summary | summary, mistakes, proposed items with ticks, Save | "No mistakes to review. Well done." | "Summarising…" | [Summarise again]; corrections still saved | n/a |
| Photo | pick a photo, preview, Find words, list with ticks, gender colour, Save | "Pick a photo of things around you." | "Looking at your photo…" | "The AI answer could not be used." | "Photo words need a connection or a local vision model." |
| Your media | library; import; plan with duration, chunks, cost; progress; transcript with tap to translate, play from here, save | "Import an audio or video file you own." | per-chunk progress | stopped job with [Continue] | local model or paused |
| Settings | Learning (with the daily reminder), Audio (with answer sounds), Speaking, AI (three providers, balance line, Check connection, toggles), Data (backup, restore, clear caches, delete everything), About (licences, Tatoeba credit, unreviewed units and stories, update check) | n/a | n/a | inline messages | AI section shows "Offline" |
| AI panel | the feature's own cost line in dollars (e.g. "Checking an answer uses AI: about $0.0002 with publik API, free with a local model."), Set it up, Not now, Details | n/a | n/a | n/a | n/a |
| AI setup dialog | data-path sentence, DISCLOSURE, three buttons, Not now; then the first-run card | n/a | "Setting up…" | contract messages | "publik API is unreachable right now. Use your own key for now." |

Keyboard: Enter checks and continues (and starts the next lesson from a lesson end), 1 to 4 pick
options, 1 to 9 add tiles and Backspace removes one, Space holds to talk, Esc quits a lesson after
confirmation. Every control has an accessible name; colour never carries meaning alone.

## 6. Non-goals

- Hearts, energy, lesson limits, ads, gems, paid streak repairs; reminders that warn about losing a
  streak, or more than one reminder a day.
- Leagues, friends, leaderboards or any server of our own.
- Accounts or sync in v1 (folder sync is a later idea).
- Phoneme-level pronunciation scoring; video avatars.
- Teaching from a language other than English in v1; courses beyond A2 in v1.
- iOS in v1 (ADR-0010).
- Free-form AI tutoring outside the syllabus: every generated exercise is anchored to syllabus items.

## 7. Success measures

| Measure | Target | How checked |
|---|---|---|
| First launch to first exercise, offline | ≤ 90 s | manual checklist, timed |
| Offline grading of one answer | ≤ 5 ms | benchmark in module 03 |
| Template lesson build | ≤ 200 ms | benchmark in module 03 |
| AI lesson, end to end | p95 ≤ 20 s, else templates | gate G1 (module 07) |
| AI exercises passing validators on publik-balanced | ≥ 80% | gate G1 |
| Judge latency and agreement | p95 ≤ 4 s; ≥ 90% agreement with the labelled set; ≤ 3% false accepts | gate G2 (module 08) |
| Role-play first streamed word | p50 ≤ 2.5 s | gate G5 (module 11) |
| Placement accuracy on simulated learners | ±1 skill deterministic; ±4 skills in ≥ 90% of noisy runs on the 64-skill courses (15 questions) | modules 02 and 18 tests |
| Measured 30-day recall | within 5 points of desired retention | tester review logs, before 1.0 |
| Cost per daily learner, recommended configuration | ≤ $3 per 30 days | maintainer's measured run (module 11) |
| Content examples attested in Tatoeba | ≥ 60% | coverage report (module 05) |
| Written stories | one per unit for es, fr and de, every word glossed | `parley content validate` (module 19) |

## 8. Later

iOS; folder sync between devices; reminders while Parley is closed (scheduled system
notifications, once module 14's installers give Parley its identity); audio for the written
stories; more target languages (Italian, Portuguese) and B1; interface languages other than
English; Anki deck import and export; a shared gallery of course packs; subtitles for streaming
video in the browser; phoneme-level feedback; handwriting and non-Latin scripts.

## 9. Listing draft

| Field | Value |
|---|---|
| Name | Parley (6 characters) |
| Summary | Learn Spanish, French or German by speaking. No hearts, no ads, works offline. (78 characters) |
| Category | Education |
| alternativeTo | Duolingo, Speak, Pimsleur |
| Platforms | Windows, macOS, Linux; Android after module 13 |
| Licence | MIT OR Apache-2.0 (code); course content per ADR-0003 (CC BY-SA 4.0 recommended); sentence packs CC BY 2.0 FR (Tatoeba) |
| Publik API products | ai.chat, ai.transcribe, ai.speech |

Publik lists an app that needs a service Publik covers only if the app runs on publik API. Parley
does: publik API is its default provider for chat, transcription and speech, and the learner's own
key and a local model stay as the alternatives. The maintainer's one-time setup run
(`docs/PUBLIK-INTEGRATION.md` §2) derives the listing from this draft.
