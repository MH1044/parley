# 20: Warmth

## Goal

Parley feels kind and alive while staying calm. A right answer gets varied praise that names what
the learner did when Parley knows it; a wrong one says what the answer is and that the word will
come back. Quiet sounds and small motion mark a right answer, a wrong one, the end of a lesson and
the daily goal, and both can be turned off (motion follows the system's reduced-motion setting). An
optional daily reminder, off by default, sends at most one desktop notification a day when the goal
is not met yet, with no streak pressure.

In testing, a right answer showed a flat "Correct" every time, a wrong one "Wrong / Correct answer:
meat", and nothing sounded or moved for a right answer, a finished lesson or a mastered skill; the
one message the learner called both specific and kind was "Almost: mind the accent". With no
reminder, a day the learner forgot was a day lost.

## Depends on

03 (grades, the noun and accent rules, card state), 04 (the result sheet, Settings), 09 (Settings >
Audio, autoplay), 15 (the lesson end's moments and the goal ring this module gives sound and
motion to), 16 (no sound or speech attempt without a voice).

## Scope

1. **Praise that names what was done.** A `correct` answer in a lesson, review or practice session
   gets one line chosen by the rules in Design notes (the word came back after a miss; a tricky
   card; the article right; the accents right; otherwise one of five plain lines). Selection is
   deterministic from the session and the exercise, never from unseeded randomness.
2. **Kinder wrong answers.** The result's heading reads **Not quite** instead of **Wrong**, followed
   by what the answer is ("La viande is meat.", "Meat is la viande.", "The missing word is viande.",
   "A correct answer: Je mange de la viande.") and, when the answer reviewed a card, "You'll see it
   again soon." "Almost: mind the accent: {words}", the article messages and "Typo: {words}" stay
   exactly as module 03 writes them.
3. **Sounds.** A quiet tick for a right answer (correct, almost or typo), a soft low tone for a wrong
   one, a short chime at the end of a lesson, review, practice or story, and a brighter chime instead
   when that session reached the daily goal or mastered a skill; nothing for an unchecked answer.
   Tones are generated in the web view at run time with integer maths (Design notes); no sound
   file and no new licence.
4. **Answer sounds switch.** Settings > Audio gains **Answer sounds** (`audio.answer_sounds`), on
   by default. With no voice for the course (module 16) the answer sounds still play: they need
   speakers, not a voice.
5. **Motion.** A short colour pulse on the result's verdict, the goal ring filling to its new value
   at the lesson end and on the path, and one pulse when it closes; none when the system asks for
   reduced motion (`app/src/index.css` already turns animations off then).
6. **Daily reminder.** Settings > Learning gains **Remind me each day at {time}** (`reminder.enabled`,
   default off; `reminder.time`, default 19:00). At that time, if today's XP is below the goal, one
   desktop notification; never more than one a day; the wording never mentions the streak. In this
   module the reminder comes only while Parley is running, the window open or minimised; reminders
   while Parley is closed (scheduled system notifications) are later work, after module 14
   (Design notes).
7. A decision record `docs/adr/0016-reminders.md` for the notification dependency, which has one
   maintainer, and its exit (in scope here, as CONTRIBUTING.md asks).

## Files

- `crates/parley-core/src/grading/feedback.rs` (new: the praise and wrong-answer selection),
  `crates/parley-core/src/exercise/run.rs` (feedback on the grade; what the run already missed),
  `crates/parley-core/src/game/reminder.rs` (new: `reminder_due`), `crates/parley-core/src/settings.rs`
  and `schemas/settings.schema.json` (`audio.answer_sounds`, `reminder`), `crates/parley-core/src/api.rs`
- `app/src-tauri/src/reminder.rs` (new: the timer and the notification), `app/src-tauri/src/lib.rs`
  (the plugin registered, called from Rust only), `Cargo.toml` and `app/src-tauri/Cargo.toml`
  (`tauri-plugin-notification`), `THIRD_PARTY.md`, `deny.toml` if the gate needs a licence entry
- `app/src/sound/{tones.ts,play.ts}` (new), `app/src/components/{ResultSheet,GoalRing,LessonEnd}.tsx`,
  `app/src/screens/Settings/{AudioSection,LearningSection}.tsx`, `app/src/index.css`,
  `app/src/strings/en.json`, `app/src/api/commands.ts`
- Docs: `docs/adr/0016-reminders.md` (new), ARCHITECTURE §1 (the plugin), §4 (settings) and §9 (the
  ADR), `docs/modules/03-…`, `04-…` and `09-…` (pointers here), `SECURITY.md` (a notification is
  local; the network list is unchanged), `README.md`, `docs/HANDBOOK.md`
- Tests: `crates/parley-core/tests/m20_warmth.rs`, `app/src-tauri/tests/m20_warmth.rs`,
  `app/src/test/m20/*.test.tsx`; module 04 and 08 UI tests that read **Wrong** change to **Not
  quite** (named in the commit body)

## Interfaces

Planning level, made exact when the module starts.

- `grading::Grade` gains `feedback: Option<Feedback>` (left out of the JSON when None, so module 03's
  golden grades are unchanged): `Feedback { kind, variant, target: Option<String>, meaning:
  Option<String>, again_soon: bool }` with `kind` one of `came_back`, `tricky`, `article`, `accents`,
  `plain` (right) or `meaning`, `word`, `gap`, `answer`, `pairs` (wrong). The runner sets it; the
  interface turns it into the en.json line `feedback.{kind}` (plain: `feedback.plain.{variant}`).
  `parley grade` (no run, no card) gives no feedback.
- Settings gain `audio.answer_sounds: bool` and `reminder: { enabled: bool, time: "HH:MM" }`.
- `game::reminder_due(now_local, reminder, today_xp, goal_xp, last_reminded: Option<LocalDate>) ->
  bool`; the last reminder's date is kept in the store's `meta` table (`reminder_last_date`), so a
  restart the same evening does not remind again.
- The desktop crate runs a timer thread that checks once a minute and sends the notification through
  `tauri-plugin-notification` from Rust; the web view gets no notification permission
  (`capabilities/default.json` stays `core:default`). No new command.
- `app/src/sound/tones.ts` exports `tick()`, `tone()`, `chime()` and `goalChime()`, each returning
  16-bit samples at 48,000 Hz; `play.ts` plays them through Web Audio and is replaced by a recorder
  in tests.

## UI

- **Result sheet**: the region **Result** keeps the verdict word for screen readers first
  (**Correct**, **Almost**, **Typo**, **Not quite**, **Not checked**); a right answer shows its
  praise line as the heading; a wrong one **Not quite** and its lines; module 03's message, the
  attribution and module 08's lines as before. The verdict pulses once (unless reduced motion).
- **Sounds**: played when the grade arrives (tick or tone) and when the lesson end shows (chime or
  goal chime), only with **Answer sounds** on; the result sheet's autoplay of the answer (module 09)
  starts after the answer sound ends, so they never overlap.
- **Settings > Audio**: **Answer sounds** (a switch) under **Play prompts aloud**.
- **Settings > Learning**: **Remind me each day at** (a switch) and a time field; under them "Parley
  reminds you only while it is open or minimised."
- **The notification**: title "Parley", body "A few minutes of {Language} today? You're at {x} of
  {goal} XP."; nothing in it can be clicked through to anything but Parley's own window.

## Messages

Right (`feedback.*`): "You got it this time." (came back); "Nice, that one was tricky." (tricky);
"Right, and the article too." (article); "Right, accents and all." (accents); plain, by variant:
"Correct.", "That's right.", "Well done.", "Yes, exactly.", "Nicely done.".
Wrong: **Not quite**; "{Target} is {meaning}." (`meaning`, a word shown in the course language);
"{Meaning} is {target}." (`word`, a word asked from English); "The missing word is {answer}."
(`gap`); "A correct answer: {answer}" (`answer`, sentences); "Here are the pairs:" with the pairs
(`pairs`); "You'll see it again soon." (when a card was reviewed).
Settings: **Answer sounds**; **Remind me each day at**; "Parley reminds you only while it is open or
minimised."; "Invalid value for reminder.time." (a time that is not HH:MM, as module 04's settings
messages).
Notification: "Parley"; "A few minutes of {Language} today? You're at {x} of {goal} XP."

## Out of scope

- A mascot, leagues, shares or any server (ADR-0011); streak repair or any reminder that warns about
  losing a streak; more than one reminder a day; reminders on Android (module 13 decides them).
- Reminders while Parley is closed: scheduled system notifications are later work, after module 14
  gives Parley an installed identity; a background helper is not planned.
- Changing grading verdicts, ratings or module 03's messages; voice audio (module 09).
- Hearts, energy, lesson limits, ads, paywalls or social features (I8); consent never at launch or
  in onboarding (I12); nothing leaves the machine: a notification is shown by the operating system
  on this computer, and the network list of ARCHITECTURE §8 is unchanged.

## Design notes

- **Praise, first rule that applies** (a `correct` verdict in a lesson, review or practice):
  `came_back` when the same item was answered wrong earlier in this session; `tricky` when the card
  this answer reviewed had 2 or more lapses, or a retrievability below 0.5 just before the review;
  `article` when the exercise is productive and the grade context names a noun (module 03's `noun`)
  and the answer was exact; `accents` when the expected answer has a letter with a mark (or ß) and
  the answer matched it exactly; otherwise `plain`. `almost` and `typo` keep module 03's message
  and get no praise line. Story questions and placement get none.
- **Variant**: the first byte of SHA-256 of `{session_id}/{exercise_id}` modulo 5; when it equals
  the previous plain variant of the session, the next one (mod 5). Session ids come from the
  injected `Rng`, so a seeded test fixes every line.
- **Wrong lines**: `meaning` for `pick_meaning`, `translate_to_en` and `listen_type` of a vocabulary
  item (the target with its article, its first meaning); `word` for `translate_to_target` of a
  vocabulary item; `gap` for `fill_gap`; `answer` for sentences (the grade's `corrected`); `pairs`
  for `match_pairs`. The first letter of a line is upper case. `again_soon` is true when the runner
  reviewed a card with Again, which schedules it within a day (module 02), so the sentence is true;
  module 03's article line ("Día is masculine: el día.") is kept and followed by it.
- **Tones (maintainer decision, 2026-11-18): generated in the web view at run time** with integer
  maths, which needs no file, no dependency and no licence. A build step writing WAV files with the
  same maths, and small committed sound files with a `THIRD_PARTY.md` row, were not chosen. Triangle
  tones from an integer phase accumulator at 48,000 Hz with integer linear envelopes, so every platform
  makes the same samples. Tick: 2,000 Hz, 40 ms, peak 15% of full scale, falling to zero. Wrong: 220
  Hz, 160 ms, 10 ms rise, peak 12%. Chime: 523, 659 and 784 Hz, 110 ms each, peak 20%. Goal chime:
  the chime and 1,047 Hz. Every tone starts and ends at zero, so nothing clicks.
- **Answer sounds on by default (maintainer decision, 2026-11-18).** The tones are short and
  quiet, Parley has no other feedback sound, and the switch is one tap away in Settings > Audio for
  a learner in a shared space. A new install and a settings file without the key both start with
  the sounds on.
- **Reminder, how it runs.** A desktop app cannot show a notification while it is closed unless it
  registers something with the operating system. Options: (a) only while Parley is running, the
  window open or minimised: a timer in the app, the same on Windows, macOS and Linux, nothing
  registered with the system, and nothing when Parley is closed (Settings says so); (b) Windows
  scheduled notifications: Parley hands the next days' reminders to Windows (WinRT scheduled
  toasts) and withdraws today's as soon as the goal is met, so it works while Parley is closed; it
  needs the installed app identity the installer gives (module 14), macOS has an equivalent for a
  signed app, Linux none; (c) a small helper started at login that checks the goal and notifies:
  works everywhere, but is a second process always running that reads the learner's data, more to
  explain and to keep safe. **Maintainer decision, 2026-11-18:** (a) in this module. Scheduled
  system notifications (b) are noted as later work after module 14, when the installers give
  Parley the identity they need; (c) is not planned.
- **Reminder rules**: `reminder_due` is true when the reminder is on, the local time is at or after
  `time` and less than 10 minutes past it (a computer asleep at that time is not reminded later),
  today's XP is below the goal, and no reminder was sent today. The text gives today's figures and
  never the streak, freezes or anything lost.
- **The notification dependency.** `tauri-plugin-notification` 2.8.1 (Apache-2.0 OR MIT, the Tauri
  project), which uses `notify-rust` 4.19 (MIT OR Apache-2.0), with `tauri-winrt-notification` (MIT
  OR Apache-2.0) on Windows, `mac-notification-sys` (MIT OR Apache-2.0) on macOS and `zbus` (MIT,
  already in the tree through the keychain) on Linux; licences read on crates.io at planning,
  2026-11-18, and checked again by the licence gate at the module. `notify-rust` has one owner, so
  ADR-0016 names the exit: the platform calls directly (WinRT toasts, `UNUserNotificationCenter`, the
  freedesktop `Notify` call over `zbus`). On Windows a notification shows only for an installed app
  (a development build shows PowerShell's name and icon), which module 14's installer provides.

## Acceptance tests

Core tests in `crates/parley-core/tests/m20_warmth.rs` (seeded `SplitMix64`, fixed clock); desktop
tests in `app/src-tauri/tests/m20_warmth.rs` with a fake notifier; UI tests in `app/src/test/m20/`
with the sound player replaced by a recorder.

1. Praise rules: an item answered wrong then right in one session → `came_back`; a card with 2 lapses
   answered right → `tricky`; a `translate_to_target` of `el día` answered `el día` → `article`; a
   `listen_type` of `Buenos días.` answered with its accent → `accents`; otherwise `plain`; `almost` and `typo` → no
   feedback and module 03's messages unchanged (module 03's golden grades all pass unchanged).
2. Variants: with a fixed seed the plain variants of a 12-exercise lesson are the same on every run,
   and no two consecutive plain lines are equal.
3. Wrong lines: a wrong `pick_meaning` of `la viande` → "Not quite", "La viande is meat.", "You'll
   see it again soon."; a wrong `fill_gap` → "The missing word is viande."; a wrong story question →
   no "You'll see it again soon."
4. UI: the result of a right answer shows its praise line and keeps the accessible verdict
   **Correct**; a wrong one shows **Not quite**; "Almost: mind the accent: legumes" shows exactly.
5. Tones: each has 48 × its length in ms samples, starts and ends at 0, peaks within 1% of its level,
   and its zero crossings per second are within 2% of twice its frequency; two calls give identical
   samples.
6. UI: with **Answer sounds** on, a correct answer plays the tick, a wrong one the tone, an unchecked
   one nothing, the lesson end the chime, and a lesson end with `goal_reached_now` the goal chime;
   with it off, nothing plays; the result's autoplay is called after the tick has ended.
7. UI: with `prefers-reduced-motion: reduce`, the verdict and the ring get no animation class; without
   it they do.
8. Settings: `audio.answer_sounds` defaults to true (a new install, and a settings file without the
   key); `reminder` defaults to off at "19:00";
   `reminder.time` "7pm" → "Invalid value for reminder.time."; a good patch is saved and read back.
9. `reminder_due`: off → false; 18:59 → false; 19:00 and 19:09 with 12 of 20 XP → true; 19:10 →
   false; 20 of 20 XP → false; already reminded today → false; the next day at 19:00 → true.
10. Desktop: with the fake notifier and a clock moved past 19:00, exactly one notification a day,
    whose body is "A few minutes of French today? You're at 12 of 20 XP."; after a restart the same
    evening, none (the `meta` date).
11. No reminder string in `en.json` and no notification body contains "streak"; the copy-rule test
    passes.
12. `app/src-tauri/capabilities/default.json` still grants `core:default` only; the licence gate is
    green with the new crates and each has a `THIRD_PARTY.md` row.
13. With `FailingNet`, everything above works (I3); 100 lessons in a day each get their sounds and
    lines, and no reminder ever limits or nags beyond one a day (I8).

## Status

Planned
