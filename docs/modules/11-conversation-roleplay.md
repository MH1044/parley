# 11: Conversation role-play

## Goal

A learner can hold a short spoken or typed conversation in a real-life scenario (café, shop,
directions, hotel, doctor, job interview) with a partner who answers at their level, corrects
their mistakes, ticks goals as they reach them, and speaks each reply. The reply streams as it is
written. A turn cap and the running cost are always visible, and the end-of-session summary turns
mistakes into new review items.

## Depends on

06, 09, 10. Feature-map rows **P7** (role-play turn) and **P8** (summary), plus P5 and P6 for voice;
§6.9 and §6.10 of `docs/PUBLIK-INTEGRATION.md`.

## Scope

- `prompts/roleplay_turn.v1.md`, `prompts/roleplay_summary.v1.md` with the exact text of §6.9 and
  §6.10; schemas `schemas/ai/roleplay_turn.v1.schema.json` and `roleplay_summary.v1.schema.json`.
- Migration `0003_conversations.sql` (`conversations`, `conversation_turns`); the personal skill
  `<lang>.personal.conversation` created on first use.
- Commands `scenario_list`, `conversation_start` (opening from the scenario, spoken, no AI call),
  `conversation_transcribe`, `conversation_turn`, `conversation_end`.
- Turns through `AiClient::chat_stream`: the message list of §6.9, incremental extraction of
  `reply`, `conversation://delta`, validation R1 to R5, `conversation://withdrawn` on failure, the
  reply spoken with `tts_play` only after validation.
- Words outside the known lemmas and the syllabus underlined; **Add to reviews** creates an item.
- Goals checklist, "Turn {n} of {cap}", the closing rule on the last turn, `end`.
- The cost line "This conversation: {amount}" from the summed charges of P5, P6 and P7 calls.
- Summary (P8): validation M1 to M5, items saved as new cards, the offline fallback from in-turn
  corrections, **Summarise again**.
- The backup (module 04) gains optional `conversations` and `conversation_turns` arrays.
- Resuming an unfinished conversation; XP (2 per learner turn, 5 per goal); the day counts for the
  streak.
- Gates G5 (role-play latency and validity) and G8 (daily cost).

## Files

- `crates/parley-core/src/ai/roleplay.rs`, `crates/parley-core/src/store/migrations/0003_conversations.sql`
- `prompts/roleplay_{turn,summary}.v1.md`, `schemas/ai/roleplay_{turn,summary}.v1.schema.json`
- `app/src-tauri/src/commands/conversation.rs`; `app/src/screens/Conversation/{List,Chat,Summary}.tsx`
- `tests/fixtures/ai/P7/*.json` (including SSE chunk files), `tests/fixtures/ai/P8/*.json`
- `crates/parley-core/tests/m11_conversation_roleplay.rs`

## Interfaces

```rust
pub fn start(store: &mut Store, scenario: &Scenario, lang: Lang, now: i64) -> ConversationState;
pub fn turn(client: &dyn AiClient, cache: &Cache, store: &mut Store, id: &str, text: &str,
    spoken: bool, on_delta: &mut dyn FnMut(&str)) -> Result<TurnResult, AiError>;
// TurnResult { reply, reply_en, correction, goals_done, end, new_words, turn, cap, cost_micros }
pub fn end(client: &dyn AiClient, cache: &Cache, store: &mut Store, id: &str) -> Result<ConversationSummary, AiError>;
pub struct ReplyExtractor { .. } impl ReplyExtractor { pub fn push(&mut self, chunk: &str) -> Option<String>; }
```

Messages: "This conversation: {amount}"; "Turn {n} of {cap}"; "This conversation has reached its
turn limit. End it to see your summary." (`turn_cap_reached`); "The AI answer could not be used."
with **Try again**; "Conversations need a connection or a local model."; "Summarising…"; **Summarise
again**; "No mistakes to review. Well done."; "Continue: {scenario}, turn {n}".

## Out of scope

- Video or avatars; free conversation without a scenario; phoneme feedback.

## Design notes

- The learner's text is inserted as a JSON string (§6.1). Assistant history messages carry the
  earlier reply JSON without `reply_en`, keeping the format visible to the model at low cost.
- `ReplyExtractor` handles JSON escapes split across chunks (`\"`, `\\`, `\uXXXX`).
- Decisions are filled in here during implementation.

## Acceptance tests

Golden cases in `tests/cases/11-conversation-roleplay/`; code tests in
`crates/parley-core/tests/m11_conversation_roleplay.rs`; UI in `app/src/screens/Conversation/*.test.tsx`.
Contract tests 1 to 18 keep passing; this module adds tests 30, 31 and 34 of
`docs/PUBLIK-INTEGRATION.md` §11.

1. `conversation_start` for `cafe` in Spanish makes no AI request, stores the opening "¡Buenos días!
   ¿Qué le pongo?" as turn 0, and `tts_play` receives it.
2. The first turn's request equals the §6.9 example body (golden, system prompt fully rendered).
3. The third turn's request holds the system message, the opening, two earlier exchanges (assistant
   content without `reply_en`) and the new turn, in that order.
4. SSE fixture split mid-word and mid-escape → deltas whose last value equals the final `reply`;
   the first delta is emitted before the stream ends.
5. R1: a stream ending in invalid JSON → `conversation://withdrawn`, "The AI answer could not be
   used.", no turn stored, nothing cached; the learner's text is back in the input (UI test).
6. R2: an English `reply` of 6 words → invalid. R3: a 45-word reply at A1 → invalid.
7. R4: goals done `[greet]`, reply `["order_drink", "dance"]` → stored `["greet", "order_drink"]`.
8. R5: correction `original: "una leche"` when the learner wrote `Quiero una café` → the correction
   is dropped, the reply kept.
9. A reply word in neither the known lemmas nor the syllabus is in `new_words`; **Add to reviews**
   creates an item in `es.personal.conversation` with two new cards.
10. With cap 12, the twelfth turn's system prompt contains "This is the last turn: close the
    conversation politely and set \"end\" to true."; a thirteenth `conversation_turn` →
    `turn_cap_reached` and its message. A reply with `end: true` disables the input.
11. Charges of 300 (P6), 4,900 (P7) and 600 (P5) micros → "This conversation: $0.0058".
12. Summary, valid fixture → the summary, 2 mistakes and 3 new items saved as new cards; M2 (9
    items), M3 (61-word summary) → invalid; M4 (an item equal to an existing one) and M5 (a mistake
    not in any learner line) → only that entry dropped.
13. Summary invalid → each in-turn correction becomes an item (lemmas from the forms table);
    **Summarise again** makes exactly one request.
14. A 402 on a turn → the text stays in the input, the message and one link show; the conversation
    stays resumable; after a success needs-credit clears.
15. With `FailingNet` and no local provider, the Conversation tab shows "Conversations need a
    connection or a local model." and `conversation_start` is not offered.
16. Spoken turn: `conversation_transcribe` makes one P6 request and returns the text; `conversation_turn`
    with `spoken: true` stores `input_mode = spoken`.
17. 5 learner turns and 3 goals done → 25 XP; the local date becomes active for the streak.
18. Resume after a restart: a conversation with 4 turns shows them, and the next request carries all 4
    exchanges.
19. Gate G5 (live): first delta p50 ≤ 2.5 s, ≥ 95% valid turns; gate G8 (live): a scripted daily
    learner costs ≤ $0.10 in the recommended configuration; results recorded in
    `scripts/gates/thresholds.toml` and the cost figures of PUBLIK-INTEGRATION §8 updated.
20. A backup exported after two conversations round-trips them exactly; a backup without the two
    arrays (made before this module) still imports, with no conversations.

## Status

Planned
