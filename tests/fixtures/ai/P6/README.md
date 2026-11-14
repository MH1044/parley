# P6 (speaking check) reply fixtures

Hand-written descriptions of replies to `POST /audio/transcriptions` (`docs/PUBLIK-INTEGRATION.md`
§6.8), one per case the module 10 tests need (`docs/modules/10-speaking-and-pronunciation.md`,
Acceptance tests and Design notes). The scripted AI client of
`crates/parley-core/tests/m10_speaking_and_pronunciation.rs` turns each file into the
`TranscriptReply` or the `AiError` that `AiClient::transcribe` returns. Nothing here is sent
anywhere, and no file holds a recording: the takes the tests check are generated tones.

## Format

A reply that arrived:

    {"status": 200, "body": {"text": "Yo es estudiante."}, "charge_micros": 538}

- `status` is 200: the reply came back and is checked against `schemas/ai/transcribe.v1.schema.json`
  (§6.8: an object whose `text` is a string of at most 2,000 characters).
- `body` is the reply's JSON body as received, before any check.
- `charge_micros` is the reply's `x-publik-charge-micros`.

A call that failed:

    {"status": 402, "error": "needs_credit", "message": "...", "top_up_url": "..."}
    {"error": "offline"}

`error` names the `AiError` the client returns: `needs_credit` (with the `message` and
`top_up_url` of the 402 body in `tests/fixtures/http/402-claimed.json`), `offline` (no
connection), `timeout` (publik API did not answer in time), or `invalid` (a 200 whose body the
client could not read as JSON at all: the client's own `AiError::Invalid`).

The charges are picked by hand, each different, so a test can tell which reply a cost came from;
`valid.json` carries the 538 micros the setup run of 2026-11-12 measured for one take.

## Files

| File | Reply | Charge (micros) | What it is |
|---|---|---|---|
| `valid.json` | `{"text": "Yo es estudiante."}` | 538 | a usable transcript; against "Yo soy estudiante." it scores 2 of 3 words |
| `empty.json` | `{"text": ""}` | 520 | valid, but no words were heard: the take is too quiet and the card is not rated |
| `blank-audio.json` | `{"text": "[BLANK_AUDIO]"}` | 530 | whisper.cpp's marker for silence; with `[...]` spans removed, no words are left |
| `not-a-string.json` | `{"text": 42}` | 600 | fails the schema: neither scored nor held, and the next check of the take is attempt 2 |
| `402.json` | 402 `needs_credit` | none | not enough balance, with the one top-up link |
| `offline.json` | no reply | none | no connection: the local model checks the take when it is downloaded |
| `timeout.json` | no reply | none | publik API did not answer in 30 s |
| `unreadable.json` | a 200 the client could not read | none | counts like `not-a-string.json`: the next check of the take is attempt 2 |
