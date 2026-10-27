# 12: Photo vocabulary and your media

## Goal

A learner can turn a photo of things around them into a personal vocabulary deck (word, article,
gender, plural), and can learn from audio or video files they own: Parley transcribes the file,
shows an interactive transcript, translates a sentence on tap, plays from that point, and saves
sentences to reviews. Long files ask for confirmation with the number of calls and the estimated
cost first.

## Depends on

06, 10. Feature-map rows **P9** (photo), **P10** (media transcription) and **P11** (sentence
translation); §6.11 to §6.13, §7 and the batch rule of §8 in `docs/PUBLIK-INTEGRATION.md`.

## Scope

- Photo preparation with `image`: EXIF orientation applied, long side at most 1568 px (Lanczos3),
  baseline JPEG quality 85 from raw pixels, stepping down by 10 (minimum 45) until under 2.9 MB.
- `photo_vocab` (P9 with `prompts/photo_vocab.v1.md`, validation V1 to V6), the Photo screen with
  ticks and gender colours, `photo_save` into `<lang>.personal.photo`. The photo is never stored.
- Live check at the start of implementation: that `publik-balanced` accepts an image part (else the contract's Gemini-dialect
  fallback), and whether the call is billed as chat (record for the developer-share line).
- Media import with `symphonia`: probe, the 3-hour limit, decode to 16 kHz mono, the chunk plan
  (cuts at the quietest 200 ms window between 90 and 110 s after the previous cut).
- `media_import` returns the plan with the batch confirmation when there are more than 10 chunks and
  a metered provider; `media_transcribe` runs chunks one at a time with `media://progress`, stops on
  an error keeping finished chunks, and resumes with **Continue**; local Whisper as an alternative.
- Transcript: sentences from segments (`verbose_json`) or interpolated times labelled "approximate
  timing"; UAX #29 sentence boundaries inside segments.
- `media_translate` (P11, `prompts/translate.v1.md`, cached) and **Translate all** with the batch
  confirmation above 10 calls; **Play from here** from the original file; `media_save_sentence`
  into `<lang>.personal.media`.
- Migration `0004_media.sql` (`media` with the local-only `path`, `media_sentences`); the backup
  (module 04) gains optional `media` and `media_sentences` arrays, and `path` is never in a backup.

## Files

- `crates/parley-core/src/ai/{photo.rs,translate.rs,media.rs}`, `crates/parley-audio/src/{media_decode.rs,chunk.rs}`,
  `crates/parley-core/src/image_prep.rs`
- `prompts/{photo_vocab,translate}.v1.md`, `schemas/ai/{photo_vocab,translate}.v1.schema.json`
- `app/src-tauri/src/commands/{photo.rs,media.rs}`; `app/src/screens/{Photo,Media}/*.tsx`
- `tests/fixtures/ai/{P9,P10,P11}/`, `crates/parley-core/tests/m12_photo_vocabulary_and_your_media.rs`,
  `crates/parley-audio/tests/m12_photo_vocabulary_and_your_media.rs`

## Interfaces

```rust
pub fn prepare_photo(bytes: &[u8], limit_bytes: usize) -> Result<PreparedJpeg, ImageError>; // { jpeg, width, height, sha256 }
pub fn photo_words(client: &dyn AiClient, cache: &Cache, lang: Lang, img: &PreparedJpeg, forms: &Forms) -> Result<Vec<PhotoWord>, AiError>;
pub fn plan_media(decoded: &[i16]) -> MediaPlan;  // chunk boundaries in samples
pub fn transcribe_media(client: &dyn AiClient, cache: &Cache, store: &mut Store, media_id: &str,
    on_progress: &mut dyn FnMut(u32, u32)) -> Result<(), AiError>;
pub fn translate_sentence(client: &dyn AiClient, cache: &Cache, lang: Lang, sentence: &str, previous: &str) -> Result<String, AiError>;
```

Messages: "This photo is too large to send."; "This local model can't read images. Choose a vision
model in Settings > AI."; "Parley can't read this file's audio. It reads MP3, AAC or M4A, MP4,
FLAC, OGG Vorbis, WAV, and MKV with those codecs." (list verified here); "This file is longer than 3
hours. Split it first."; "This file is {n} minutes long. Transcribing it with publik API makes {c}
calls to ai.transcribe, about {amount} (estimate)." with **Transcribe**, **Use the local model**,
**Cancel**; "Confirm the transcription first."; after the first call "about {amount} a call
(measured)"; "This makes {n} calls to
publik-fast, about {amount}."; "approximate timing"; "The original file has moved. Import it again
to play it."; **Continue**; offline: "Photo words need a connection or a local vision model.",
"Transcription needs a connection or the local model.", "Translations need a connection or a local
model."

## Out of scope

- Video frames, subtitles embedded in files, streaming services, the browser (later).

## Design notes

- The estimate uses $0.006 per minute (PUBLIK-INTEGRATION §8) until module 10's measurement exists;
  23 minutes → 138,000 micros → `$0.14`.
- The image goes only in memory; the prepared JPEG's SHA-256 is the cache key input.
- Decisions are filled in here during implementation.

## Acceptance tests

Code tests in the two `m12_…` files with generated images and audio and the fake AI server; golden
cases in `tests/cases/12-photo-vocabulary-and-your-media/`. Contract tests 1 to 18 keep passing (test
15 on images in particular); this module adds tests 32 and 33 of `docs/PUBLIK-INTEGRATION.md` §11.

1. `images/exif_4000x3000.jpg` (orientation 6) → a 1176 × 1568 JPEG with no APP1 (EXIF, XMP), APP13
   (IPTC) or APP2 (ICC) segment, under 2.9 MB.
2. With `limit_bytes` set to 100,000 in the test, quality steps 85, 75, 65… until the result fits;
   if 45 still does not fit → "This photo is too large to send."
3. The P9 request has a text part and an `image_url` data URL part, and its body is under
   4,000,000 bytes.
4. The valid de fixture → `die Tasse` (f, plural `die Tassen`) and `der Tisch` (m).
5. V4: es `el agua` with gender f → kept; es `el mesa` f → dropped; fr `l'arbre` m → kept; de
   `das Tisch` m → dropped. V3: gender `n` in Spanish → dropped. V1 (not JSON) → "The AI answer
   could not be used."
6. V6: forms say `Tisch` is m; a reply `die Tisch`, f → dropped.
7. An object with confidence 0.4 is returned unticked.
8. `photo_save` of two words → two vocabulary items in `de.personal.photo` with targets `die Tasse`
   and `der Tisch`, English `cup` and `table`, and two new cards each.
9. After `photo_vocab`, no image file exists anywhere in the data directory.
10. The same photo and language again → no request.
11. A local model that rejects image parts → "This local model can't read images. Choose a vision
    model in Settings > AI."
12. A text file renamed `.mp3` → the unreadable-audio message; a fake decoder reporting 4 hours →
    "This file is longer than 3 hours. Split it first."
13. The generated 23-minute file (300 ms gaps every 106 s) → 13 chunks, each at most 110 s and
    3,520,044 bytes, cut inside the gaps (±100 ms).
14. With publik API, `media_import` returns `needs_confirmation` and "This file is 23 minutes long.
    Transcribing it with publik API makes 13 calls to ai.transcribe, about $0.14 (estimate).";
    `media_transcribe` without `confirmed` → `invalid_input` "Confirm the transcription first.";
    with it → 13 sequential requests.
15. With the local provider → no confirmation and no request to any network fake.
16. A 402 on chunk 5 → `chunks_done` 4, the job stopped with the message and one link; **Continue**
    → the next request is chunk 5.
17. `verbose_json` segments → sentence times equal segment times plus the chunk offset; with plain
    `json` → times interpolated by characters and "approximate timing".
18. A segment `Hola. ¿Qué tal? Bien.` → three sentences.
19. `media_translate` → the §6.13 request (golden); a second tap → no request; T2 (60 words for a
    6-word sentence) and T3 (Spanish output) → "The AI answer could not be used."
20. **Translate all** with 42 untranslated sentences → "This makes 42 calls to publik-fast, about
    $0.0037." (42 × $0.000087) before any request.
21. `media_save_sentence` → a phrase item in `es.personal.media` with the translation as English;
    with no translation yet it makes one P11 request first.
22. **Play from here** after the file was moved → "The original file has moved. Import it again to
    play it."
23. A backup of a database with media rows has `path` empty in every row.

## Status

Planned
