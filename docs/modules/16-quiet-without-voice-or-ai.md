# 16: Quiet without voice or AI

## Goal

Parley without a voice for the course language and without AI is calm and complete, not a wall of
errors and dead ends. With no voice, nothing offers to play, nothing tries to, and the path says
once, calmly, how to add one. An AI feature first says in one line that it uses AI and what it
costs in dollars, with **Set it up** and **Not now**, and the full consent dialog still comes before
any request. **Explain** works without AI, from the skill's notes.

In testing, with no French voice installed, every prompt and every result sheet carried a red
"No French voice is installed…" line beside Play and Slow buttons that could only fail; and
**Explain**, **Check with AI** and **Read a story** each opened the same long privacy and payment
panel, whose **Not now** left the learner where they were with nothing gained.

## Depends on

06 (consent dialog, AI status), 08 (Explain, Check with AI), 09 (voices, autoplay, Settings >
Audio), 10 (speaking checks), 15 (the path's Next card, beside which the voice note sits).
`docs/PUBLIK-CONTRACT.md` §4 and §9 and `docs/PUBLIK-INTEGRATION.md` §4 define what the consent
dialog shows and when (invariant I12); this module keeps that dialog exactly and puts a short
panel in front of it.

## Scope

1. **No voice.** A course language has a voice when an OS voice for it is installed or natural
   voices are on with a natural voice configured for it (module 09's `natural_voice`). Without
   one: **Play** and **Slow** are hidden everywhere (exercises, the result sheet, the speak
   exercise's **Play the voice**, the skill page's words); autoplay makes no attempt; no no-voice
   line appears on a prompt or a result sheet; listening exercises stay skipped as today (module
   09's composer switch).
2. **The voice note.** On the path, under the Next card, one note while the course language has no
   voice: "No French voice yet, so listening exercises are off." with **How to add a voice**
   (opens Settings > Audio, which keeps module 09's full platform help) and **Don't show again**
   (remembered per language). It goes by itself once a voice exists.
3. **Autoplay is silent on failure.** With a voice, a failed autoplay shows nothing; a failure
   after the learner presses **Play** or **Slow** shows module 09's message as today.
4. **The short AI panel.** Every AI feature control, while it cannot run because no provider is
   chosen or consent is missing, opens first a short panel (a dialog named **Uses AI**): the
   feature's one line with its dollar figure from the feature map (Messages), **Set it up**, **Not
   now** and a **Details** link. **Set it up** and **Details** open module 06's full *Set up AI features*
   dialog in its place; **Not now** closes it, cancels the action and sends nothing.
5. **What opens what.** The short panel: **Check with AI**; **Read a story** while it is AI-only
   (module 19 makes it a written story and adds **Fresh story (AI)**, which keeps the panel); the
   **Natural voice** switch in Settings > Audio; a speaking check that answers `needs_consent`;
   later, the Conversation, Photo and Your media tabs (modules 11 and 12). The full dialog
   directly: the path card's **Set up AI**, **Use publik API** in Settings > AI, and the link
   "More detailed explanations: set up AI (optional)" under an offline explanation, since each of
   them already says it sets AI up.
6. **Explain without AI.** With no provider chosen, **Explain** (offered after `almost` or `wrong`,
   as module 08 says) shows the notes explanation with no call and no panel: "A correct answer:
   {answer}", "{target}: {meaning}" for a vocabulary item, the grammar note of the exercise's skill
   under "From {skill}", and the small line "More detailed explanations: set up AI (optional)".
   With a provider chosen, **Explain** asks the AI as module 08 says; when that fails, the notes
   explanation shows under the error.
7. **AI marks.** AI-only controls carry a small "AI" mark (text, not colour alone; accessible name
   "{label}, uses AI") whenever AI is not set up (decided 2026-11-18; no remembered flag). No
   screen, timer or exercise opens the panel or the dialog by itself: only
   a press of a marked control, or of a control that says it sets AI up.

## Files

- `crates/parley-core/src/app/{audio.rs,course.rs,lesson.rs}` (`voice_available`, the path and
  lesson flags), `crates/parley-core/src/ai/explain.rs` (`explain_notes`, the notes value),
  `crates/parley-core/src/settings.rs` and `schemas/settings.schema.json` (`audio.voice_note`),
  `crates/parley-core/src/api.rs`
- `app/src-tauri/src/commands/lesson.rs` (`explain_notes`), `app/src-tauri/src/lib.rs`
- `app/src/components/{AiPanel,AiMark,VoiceNote,PlayButton,ResultSheet}.tsx`,
  `app/src/components/AiSetupDialog.tsx` (opened from the panel; its content unchanged),
  `app/src/screens/{Path,Lesson,Story,Settings}/*`, `app/src/api/commands.ts`,
  `app/src/strings/en.json`
- Docs: `docs/PUBLIK-INTEGRATION.md` §4 (the panel before the dialog; Explain and the story node no
  longer open the dialog by themselves), ARCHITECTURE §4 (`audio.voice_note`) and §5
  (`explain_notes`), `docs/modules/06-…`, `08-…` and `09-…` (UI sections, with a pointer here),
  `README.md`, `docs/HANDBOOK.md`
- Tests: `crates/parley-core/tests/m16_quiet_without_voice_or_ai.rs`, `app/src/test/m16/*.test.tsx`

## Interfaces

Planning level, made exact when the module starts.

- `Parley::voice_available(lang) -> bool`: module 09's `listen_available` without the **Can't
  listen now** time. `api::PathView` and `api::LessonStarted` gain `voice: bool` for the course
  language.
- Settings gain `audio.voice_note: {es, fr, de}` booleans, default true (show the note); **Don't
  show again** sets the course's to false with `settings_set`.
- New command `explain_notes(session_id, exercise_id) -> ExplainNotes { answer, meaning:
  Option<String>, skill_title, grammar_note: Option<String> }`: the same refusals as module 08's
  `explain_begin` (`not_found`, and `invalid_input` "Explanations are available after an answer
  that was almost right or wrong."), never a client, never a call. Appended to `COMMANDS`.
- `explain_answer` is unchanged; the interface calls it only when a provider is chosen.
- No core change for the panel: it is interface only and calls no command.

## UI

- **Voice**: `PlayButton` renders nothing when `voice` is false; the lesson makes no `tts_play` or
  `tts_prefetch` call for autoplay when `voice` is false, and ignores an autoplay failure when it
  is true. The path's `VoiceNote` (a region named **Listening**) shows when `voice` is false and
  `audio.voice_note.<lang>` is true.
- **Short panel** (`AiPanel`, `role="dialog"`, named **Uses AI**): the line, **Set it up**
  (focused), **Not now**, **Details**. Esc is **Not now** (module 04's dialog rules). It comes last
  on its screen like the other dialogs, above everything (`--layer-dialog`).
- **Full dialog**: module 06's, unchanged (title, data-path sentence, `DISCLOSURE` verbatim,
  "Everything else in Parley works without AI.", the three equal buttons, **Not now**); its
  **Continue** still runs the action that opened the panel.
- **Explain** with no provider: "A correct answer: {answer}", the meaning line, "From {skill}" and
  the note, then "More detailed explanations: set up AI (optional)" as a link button that opens the
  full dialog. With a provider that failed: module 08's error as `AiBanner` shows it, then the
  notes explanation (its last line replaced by the error's own way out).
- **Marks** (`AiMark`): the text "AI" after the control's label; the control's accessible name
  "{label}, uses AI".

## Messages

| Where | Text |
|---|---|
| Path note | "No {Language} voice yet, so listening exercises are off."; **How to add a voice**; **Don't show again** |
| Short panel | the feature's line (below); **Set it up**; **Not now**; **Details** |
| Explain without AI | "A correct answer: {answer}"; "{target}: {meaning}"; "From {skill}"; "More detailed explanations: set up AI (optional)" |
| Marks | "AI"; accessible name "{label}, uses AI" |
| `explain_notes` errors | module 08's "This exercise is no longer available." and "Explanations are available after an answer that was almost right or wrong." |

The panel's lines (decided 2026-11-18), one per feature, in dollars as the contract's copy rule
asks, each from the cost per action of the feature map in PUBLIK-INTEGRATION §5: **Check with AI**
"Checking an answer uses AI: about $0.0002 with publik API, free with a local model." (P3,
$0.00015); **Read a story** while it is AI-only, and later **Fresh story (AI)**, "A fresh story uses
AI: about $0.01 with publik API, free with a local model." (P2, $0.0097); **Natural voice** "Natural
voices use AI: about $0.0005 a sentence with publik API." (P5, $0.00054); a speaking check "Checking
your speaking uses AI: about $0.0005 a try with publik API, or free with the local speech model."
(P6, $0.00054). Modules 11 and 12 add their tabs' lines the same way from rows P7 to P11.

## Out of scope

- Installing or downloading a voice (the OS does it; module 09's help says how); a bundled voice.
- Any change to the consent dialog's content, to when a request is made, or to the providers
  (module 06); new AI features.
- Hearts, energy, lesson limits, ads, paywalls or social features (I8); consent never at launch or
  in onboarding (I12): the short panel, like the dialog, appears only at an AI action the learner
  takes; nothing leaves the machine except as modules 06 to 10 allow.

## Design notes

- **How the panel and consent fit.** The contract (§4 step 4, §9 "Before setup") requires the
  data-path sentence and `DISCLOSURE` verbatim, with **Continue with publik API**, **Use my own
  key** and **Use a local model**, before provisioning; PUBLIK-INTEGRATION §4 requires that dialog
  the first time the learner triggers an AI action, never at launch or in onboarding (I12). The
  short panel does not replace it: the panel makes no command call, chooses no provider and
  accepts nothing; **Set it up** and **Details** both lead to the full dialog, and only that
  dialog's **Continue with publik API** (`ai_consent_accept`) provisions, while an own key and a
  local model are still chosen in Settings > AI. So the full disclosure still appears before the
  first AI request of every kind, at the action the learner took, and the panel is an extra step in
  front of it, never a way round it. PUBLIK-INTEGRATION §4 is edited to say so and to list which
  controls open the panel first.
- **Why one line first.** The full dialog is the right place to decide, but as the first answer to
  "Explain" it read as a wall of privacy and payment text. The panel answers the question the
  learner has at that moment (does this cost money, can I skip it) and keeps the full text one
  press away.
- **Explain without AI.** The grammar note is the skill's own (`grammar_note`, at most 120 words,
  module 01), written for exactly that skill; the item is the exercise's, so a review explains each
  exercise from its own skill. A skill without a note shows the answer and meaning only. No AI
  request, no consent and no cache are involved, so it works offline (I3) and never spends.
- **AI marks (maintainer decision, 2026-11-18).** The mark shows whenever AI is not set up (a
  provider unset or no consent), read from `ai_status`, which includes every moment after **Not
  now**; nothing about a **Not now** is remembered and no setting is added. Marking only after a
  first **Not now** was not chosen, since it needed a remembered flag and told the learner nothing
  before their first press.
- **Panel copy (maintainer decision, 2026-11-18).** Each feature's panel names its own cost in
  dollars, from the feature map's cost per action rounded to one significant figure, and says when
  a local model makes it free; a generic "This uses AI, which costs a little." was not chosen, since
  the figures exist and the copy rule asks Parley to talk in dollars. When a cost in §5 is measured
  again, the line follows it.
- **When the panel opens.** The interface knows AI is not set up from `ai_status` (`provider`
  `unset`, or `needs_consent`), so it opens the panel before calling the feature's command; a
  command that still answers `needs_consent` (which is decided before any request, module 06)
  opens the panel the same way.
- **Not now** in the panel, like the dialog's, cancels the action: an unchecked answer stays
  unchecked (**I was right** and **Continue** still work), a story is not written, the natural-voice
  switch stays on System voice.
- **No voice.** "No voice" is decided per course language, so a learner of German with only an
  English voice installed gets the note; a natural voice configured with credit problems still
  counts as a voice, and its failure is module 09's fallback and message, shown only after a press.
  The note is not shown during onboarding (the path does not exist yet).
- Modules 11 and 12 are Planned with their tabs as consent triggers; their specs gain a line that
  those tabs open the short panel first, as Scope 5 says.

## Acceptance tests

Core tests in `crates/parley-core/tests/m16_quiet_without_voice_or_ai.rs` (fake `OsVoices`, a fake
`AiClient` that records requests, `FailingNet`); UI tests in `app/src/test/m16/`. Contract tests 1
to 18 keep passing; module 06's test 18 (the dialog's exact text) keeps passing unchanged.

1. `voice_available("fr")` with OS voices `es-ES` only → false; with `fr-FR` → true; with no OS
   voice, `audio.voice_source` natural and a configured voice → true. `path_get` and `lesson_start`
   carry the same value.
2. UI, `voice` false: a `pick_meaning` and a result sheet show no **Play** or **Slow** button, no
   no-voice text, and with autoplay on make no `tts_play` or `tts_prefetch` call.
3. UI, `voice` true: an autoplay whose `tts_play` rejects shows nothing; a press of **Play** whose
   `tts_play` rejects with `no_voice` shows its message.
4. UI: the path note shows "No French voice yet, so listening exercises are off."; **How to add a
   voice** opens Settings > Audio; **Don't show again** calls `settings_set` with
   `{"audio": {"voice_note": {"fr": false}}}` and the note goes; with `voice` true it never shows.
5. Settings: `audio.voice_note` defaults to true for each language and refuses a non-boolean with
   "Invalid value for audio.voice_note.fr.".
6. UI: with `ai.provider` unset, **Check with AI** opens the dialog named **Uses AI** with "Checking
   an answer uses AI: about $0.0002 with publik API, free with a local model." and its buttons, and
   no command is called; **Not now** closes it, `exercise_check` is never called and
   the sheet still offers **I was right**.
7. UI: **Set it up** (and, separately, **Details**) replaces the panel with *Set up AI features*,
   which shows the data-path sentence and `DISCLOSURE` verbatim; its **Continue with publik API**
   then **Continue** calls `exercise_check` once (I12, consent before the request).
8. UI: the path card's **Set up AI** opens *Set up AI features* directly, with no panel.
9. UI: with `ai.provider` unset, **Explain** after a wrong answer calls `explain_notes`, never
   `explain_answer`, and shows "A correct answer: …", the skill's note and "More detailed
   explanations: set up AI (optional)", which opens the full dialog.
10. Core: `explain_notes` for a wrong `pick_meaning` of a vocabulary item gives the answer, the
    meaning and the skill's note; for a `correct` answer "Explanations are available after an answer
    that was almost right or wrong."; for an unknown exercise "This exercise is no longer
    available."; the fake client records no request (I3).
11. UI: with a provider chosen, **Explain** whose `explain_answer` fails with `offline` shows
    "Explanations need a connection or a local model." and the notes explanation.
12. UI: with `ai.provider` unset, **Check with AI**, **Read a story** and **Natural voice** carry the
    "AI" mark and the accessible name "{label}, uses AI"; with a provider chosen, none does.
13. UI: after **Not now**, three further unchecked answers in the same lesson open no panel and no
    dialog by themselves, and nothing is saved (`settings_set` is not called); the marks are the
    same before and after **Not now**; launch and every onboarding step open neither (I12).
14. UI: the **Natural voice** switch with no provider opens the panel with "Natural voices use AI:
    about $0.0005 a sentence with publik API."; **Not now** leaves **System voice** selected and
    calls no AI command; **Read a story** (while AI-only) opens it with the fresh-story line.
15. The copy-rule test passes over the new strings (dollars, never "credits"), and each panel line's
    figure is its feature-map cost rounded to one significant figure.
16. `COMMANDS` in `api.rs` equals the list in `commands.ts` and every name is registered.

## Status

Planned
