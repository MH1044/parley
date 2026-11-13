# P5 (natural voice) reply fixtures

Hand-written descriptions of replies to `POST /audio/speech` (`docs/PUBLIK-INTEGRATION.md` §6.7),
one per case the module 09 tests need (`docs/modules/09-listening-and-voices.md`, Acceptance tests).
The scripted AI client of `crates/parley-core/tests/m09_listening_and_voices.rs` turns each file into
the `AudioReply` or the `AiError` that `AiClient::speech` returns. Nothing here is sent anywhere.

## Format

A reply that arrived:

    {"status": 200, "content_type": "audio/wav", "charge_micros": 600, "duration_ms": 1500}

- `status` is 200: the reply came back and is checked against §6.7.
- `content_type` is the reply's `Content-Type` header as received.
- `charge_micros` is the reply's `x-publik-charge-micros`, or `null` when it carries none.
- `duration_ms` makes the body: a RIFF WAVE file, 16 kHz mono 16-bit PCM, with the 44-byte header
  written field by field and then 32 bytes for each millisecond of an integer square wave (period
  40 samples, levels +8000 and -8000). Its data size divided by its byte rate (32,000 bytes a
  second) is exactly `duration_ms`, so the duration a WAV decoder measures is true by construction.
- `body`, in place of `duration_ms`, is a body sent as the UTF-8 bytes of the text.

A call that failed:

    {"status": 402, "error": "needs_credit", "message": "...", "top_up_url": "..."}
    {"error": "offline"}

`error` names the `AiError` the client returns: `needs_credit` (with the `message` and
`top_up_url` of the 402 body in `tests/fixtures/http/402-claimed.json`), `offline`, `timeout`
(publik API did not answer in time), or `provider` (with `status` as the code and `message`).

The charges are picked by hand, each different, so a test can tell which reply a cost came from.

## Files

| File | Reply | Charge (micros) | What it is |
|---|---|---|---|
| `valid.json` | `audio/wav`, 1,500 ms | 600 | a valid clip for every sentence the tests read (two words or more at speed 1.0 allow up to 3,000 ms) |
| `valid-640.json` | `audio/wav`, 1,500 ms | 640 | the same clip charged 640, for the cost line of 600 and 640 |
| `valid-type-parameters.json` | `Audio/WAV; codecs=1`, 1,500 ms | 600 | valid: the type is compared without regard to case and may carry parameters |
| `text-html.json` | `text/html; charset=utf-8`, 1,500 ms | 580 | the bytes are a valid clip, so only the `audio/*` rule fails |
| `not-audio.json` | `audio/mpeg`, an HTML page as the body | 560 | the type is right, so only the rule that the bytes decode fails |
| `clip-50ms.json` | `audio/wav`, 50 ms | 520 | shorter than the 200 ms every clip needs |
| `clip-9s.json` | `audio/wav`, 9,000 ms | 690 | the longest a six-word sentence at speed 1.0 allows: 3 x 500 x 6 / 1.0 = 9,000 ms |
| `clip-10s.json` | `audio/wav`, 10,000 ms | 700 | 1,000 ms longer than a six-word sentence at speed 1.0 allows |
| `402.json` | 402 `needs_credit` | none | not enough balance, with the one top-up link |
| `offline.json` | no reply | none | no connection |
| `timeout.json` | no reply | none | publik API did not answer in 20 s |
| `500.json` | 500 `provider` | none | another error of the provider |

The six-word sentence is "Quiero un café con leche, gracias." (Quiero, un, café, con, leche,
gracias as UAX #29 words); the five-word one, "Quiero un café con leche.", allows 7,500 ms at speed
1.0 and 10,000 ms at 0.75.
