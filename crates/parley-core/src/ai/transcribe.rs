//! P6, the speaking check (docs/PUBLIK-INTEGRATION.md §6.8; docs/modules/10-speaking-and-
//! pronunciation.md): the transcription model, the key and request of one take, the check of the
//! transcript that comes back, the measured cost line, and how a failed call shows.
//!
//! A take's transcript is keyed like any AI answer (§6.1), with input {audio_sha256, language},
//! but it is held in memory only, while the app runs: the takes themselves are removed when the
//! lesson ends, so nothing about a take is written to `cache/ai.sqlite3` except, for the cost
//! line, the amount it was charged.

use std::sync::OnceLock;

use jsonschema::Validator;
use serde_json::{Value, json};

use super::cache::{cache_key, cache_key_json, idempotency_key, sha256_hex};
use super::client::{AiError, ProviderKind, TranscribeRequest};
use super::money::format_dollars;
use super::schema::compile;
use crate::content::Lang;
use crate::error::{AppError, ErrorCode};
use crate::settings::Settings;

/// The cache key's feature (version [`TRANSCRIBE_VERSION`]).
pub const TRANSCRIBE_FEATURE: &str = "transcribe";
/// The cache key's version of [`TRANSCRIBE_FEATURE`].
const TRANSCRIBE_VERSION: u32 = 1;
/// The request's timeout.
pub const TRANSCRIBE_TIMEOUT_S: u64 = 30;
/// The id the setup run of 2026-11-12 confirmed for /audio/transcriptions on publik API.
pub const PUBLIK_TRANSCRIBE_MODEL: &str = "gpt-4o-mini-transcribe";
/// The setup run's measured charge of one take; the cost line's figure until the install has its
/// own.
pub const MEASURED_TRANSCRIBE_MICROS: i64 = 538;
/// The notice of a check the local model answered because the provider could not be reached.
pub const OFFLINE_LOCAL_NOTICE: &str = "You're offline, so the local model checked this.";

/// The file name, type and format of every take sent.
const FILE_NAME: &str = "take.wav";
const MIME: &str = "audio/wav";
const RESPONSE_FORMAT: &str = "json";

const TRANSCRIBE_SCHEMA: &str = include_str!("../../../../schemas/ai/transcribe.v1.schema.json");

/// `schemas/ai/transcribe.v1.schema.json`, compiled once.
fn validator() -> &'static Validator {
    static VALIDATOR: OnceLock<Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| {
        compile(TRANSCRIBE_SCHEMA).expect("schemas/ai/transcribe.v1.schema.json compiles")
    })
}

/// A setting's text, when it holds some.
fn some_text(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// publik → [`PUBLIK_TRANSCRIBE_MODEL`]; own_key → `ai.own_key.transcribe_model` when set; else
/// None.
pub fn transcribe_model(settings: &Settings) -> Option<String> {
    match settings.ai.provider.as_str() {
        "publik" => Some(PUBLIK_TRANSCRIBE_MODEL.to_string()),
        "own_key" => some_text(&settings.ai.own_key.transcribe_model),
        _ => None,
    }
}

/// The model in a local take's key: "whisper.cpp/ggml-<name>" (provider `local`).
pub fn local_model_id(name: &str) -> String {
    format!("whisper.cpp/ggml-{name}")
}

/// Canonical key JSON (§6.1) with input {"audio_sha256", "language"}.
pub fn transcribe_key_json(
    provider: ProviderKind,
    model: &str,
    audio_sha256: &str,
    lang: Lang,
) -> String {
    cache_key_json(
        TRANSCRIBE_FEATURE,
        TRANSCRIBE_VERSION,
        provider,
        model,
        &json!({ "audio_sha256": audio_sha256, "language": lang.as_str() }),
    )
}

/// The SHA-256 of [`transcribe_key_json`]: the key a take's transcript is held under.
pub fn transcribe_key(
    provider: ProviderKind,
    model: &str,
    audio_sha256: &str,
    lang: Lang,
) -> String {
    cache_key(&transcribe_key_json(provider, model, audio_sha256, lang))
}

/// §6.8: file_name "take.wav", mime "audio/wav", the language, response_format "json",
/// temperature 0.0, timeout 30 s, and the idempotency key of the key JSON at `attempt` (1 for a
/// take not sent before; n + 1 after n replies that could not be used).
pub fn transcribe_request(
    provider: ProviderKind,
    model: &str,
    wav: &[u8],
    lang: Lang,
    attempt: u32,
) -> TranscribeRequest {
    let key_json = transcribe_key_json(provider, model, &sha256_hex(wav), lang);
    TranscribeRequest {
        model: model.to_string(),
        audio: wav.to_vec(),
        file_name: FILE_NAME.to_string(),
        mime: MIME.to_string(),
        language: lang.as_str().to_string(),
        response_format: RESPONSE_FORMAT.to_string(),
        temperature: 0.0,
        timeout_s: TRANSCRIBE_TIMEOUT_S,
        idempotency_key: Some(idempotency_key(&key_json, attempt.max(1))),
    }
}

/// The text when `body` passes `schemas/ai/transcribe.v1.schema.json` (§6.8's schema): an object
/// whose `text` is a string of at most 2,000 characters.
pub fn parse_transcript(body: &Value) -> Option<String> {
    if !validator().is_valid(body) {
        return None;
    }
    body.get("text").and_then(Value::as_str).map(str::to_string)
}

/// The transcript as the learner sees it: every `[...]` span (whisper.cpp's `[BLANK_AUDIO]`)
/// removed, white space collapsed, trimmed.
pub fn heard_text(transcript: &str) -> String {
    crate::speech::without_brackets(transcript)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// "Speaking checks cost about {format_dollars(mean)} a take (measured)." with the mean of
/// `charges` rounded half up to a whole micro, or [`MEASURED_TRANSCRIBE_MICROS`] when empty.
pub fn transcribe_cost_line(charges: &[i64]) -> String {
    let mean = if charges.is_empty() {
        MEASURED_TRANSCRIBE_MICROS
    } else {
        let n = charges.len() as i128;
        let sum: i128 = charges.iter().map(|c| i128::from(*c)).sum();
        // floor((sum + n / 2) / n) in integers: half up, also for a negative sum.
        i64::try_from((2 * sum + n).div_euclid(2 * n)).unwrap_or(i64::MAX)
    };
    format!(
        "Speaking checks cost about {} a take (measured).",
        format_dollars(mean)
    )
}

/// "Can't check speaking right now. {reason}"
fn cannot_check(code: ErrorCode, reason: &str) -> AppError {
    AppError::new(code, format!("Can't check speaking right now. {reason}"))
}

/// The error of a failed P6 call (docs/PUBLIK-INTEGRATION.md §9, the speaking row). A 402 is
/// "Can't check speaking right now." with its message and the top-up link; disconnected the same
/// with **Reconnect**; both carry `use_local`, which offers **Use the local model** when it is
/// downloaded. A 429, busy and timeout keep their messages; offline (with no local model to fall
/// back on) asks for a connection or the local model; a reply that cannot be used is the
/// validation message; any other provider error is "Can't check speaking right now." with its
/// message. Consent and key errors are module 06's.
pub fn speaking_error(e: &AiError, use_local: bool) -> AppError {
    match e {
        AiError::NeedsCredit {
            message,
            top_up_url,
        } => cannot_check(ErrorCode::NeedsCredit, message)
            .with_action(json!({ "top_up_url": top_up_url, "use_local": use_local })),
        AiError::Disconnected => cannot_check(ErrorCode::Disconnected, &e.message())
            .with_action(json!({ "use_local": use_local })),
        AiError::Offline => AppError::new(
            ErrorCode::Offline,
            "Speaking checks need a connection or the local model.",
        ),
        AiError::Provider { .. } | AiError::LocalDown { .. } | AiError::Setup { .. } => {
            cannot_check(ErrorCode::ProviderError, &e.message())
        }
        AiError::RateLimited { .. }
        | AiError::Busy
        | AiError::Timeout { .. }
        | AiError::Invalid
        | AiError::NeedsConsent
        | AiError::NoToken
        | AiError::OwnKeyMissing
        | AiError::TooLarge => e.clone().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_follows_the_provider() {
        let mut settings = Settings::default();
        assert_eq!(transcribe_model(&settings), None);
        settings.ai.provider = "publik".into();
        assert_eq!(
            transcribe_model(&settings).as_deref(),
            Some("gpt-4o-mini-transcribe")
        );
        settings.ai.provider = "own_key".into();
        assert_eq!(transcribe_model(&settings), None);
        settings.ai.own_key.transcribe_model = Some("  ".into());
        assert_eq!(transcribe_model(&settings), None);
        settings.ai.own_key.transcribe_model = Some("whisper-1".into());
        assert_eq!(transcribe_model(&settings).as_deref(), Some("whisper-1"));
        settings.ai.provider = "local".into();
        assert_eq!(transcribe_model(&settings), None);
        assert_eq!(local_model_id("base"), "whisper.cpp/ggml-base");
    }

    #[test]
    fn the_key_and_request_are_the_spec_s() {
        let wav = b"RIFF....WAVE";
        let sha = sha256_hex(wav);
        let key_json = transcribe_key_json(
            ProviderKind::Publik,
            PUBLIK_TRANSCRIBE_MODEL,
            &sha,
            Lang::Es,
        );
        assert_eq!(
            key_json,
            format!(
                r#"{{"feature":"transcribe","input":{{"audio_sha256":"{sha}","language":"es"}},"model":"gpt-4o-mini-transcribe","provider":"publik","v":1}}"#
            )
        );
        assert_eq!(
            transcribe_key(
                ProviderKind::Publik,
                PUBLIK_TRANSCRIBE_MODEL,
                &sha,
                Lang::Es
            ),
            cache_key(&key_json)
        );
        let req = transcribe_request(
            ProviderKind::Publik,
            PUBLIK_TRANSCRIBE_MODEL,
            wav,
            Lang::Es,
            1,
        );
        assert_eq!(req.audio, wav);
        assert_eq!(
            (req.file_name.as_str(), req.mime.as_str()),
            ("take.wav", "audio/wav")
        );
        assert_eq!(
            (req.language.as_str(), req.response_format.as_str()),
            ("es", "json")
        );
        assert_eq!((req.temperature, req.timeout_s), (0.0, 30));
        assert_eq!(req.idempotency_key, Some(idempotency_key(&key_json, 1)));
        let second = transcribe_request(
            ProviderKind::Publik,
            PUBLIK_TRANSCRIBE_MODEL,
            wav,
            Lang::Es,
            2,
        );
        assert_eq!(second.idempotency_key, Some(idempotency_key(&key_json, 2)));
    }

    #[test]
    fn a_transcript_must_pass_the_schema() {
        assert_eq!(
            parse_transcript(&json!({"text": "Yo es estudiante."})).as_deref(),
            Some("Yo es estudiante.")
        );
        assert_eq!(parse_transcript(&json!({"text": ""})).as_deref(), Some(""));
        assert_eq!(parse_transcript(&json!({"text": 42})), None);
        assert_eq!(parse_transcript(&json!({"words": []})), None);
        assert_eq!(parse_transcript(&json!("hola")), None);
        assert_eq!(parse_transcript(&json!({"text": "a".repeat(2001)})), None);
        assert_eq!(heard_text("  [BLANK_AUDIO]  "), "");
        assert_eq!(heard_text(" Hola [música]  mundo "), "Hola mundo");
    }

    #[test]
    fn the_cost_line_is_the_mean_or_the_measured_take() {
        assert_eq!(
            transcribe_cost_line(&[]),
            "Speaking checks cost about $0.00054 a take (measured)."
        );
        assert_eq!(
            transcribe_cost_line(&[500, 600]),
            "Speaking checks cost about $0.00055 a take (measured)."
        );
    }

    #[test]
    fn failures_show_as_the_speaking_row() {
        let credit = speaking_error(
            &AiError::NeedsCredit {
                message: "Add credit to keep going.".into(),
                top_up_url: "https://example.invalid/top-up".into(),
            },
            true,
        );
        assert_eq!(credit.code, ErrorCode::NeedsCredit);
        assert_eq!(
            credit.message,
            "Can't check speaking right now. Add credit to keep going."
        );
        assert_eq!(
            credit.action,
            Some(json!({"top_up_url": "https://example.invalid/top-up", "use_local": true}))
        );
        let gone = speaking_error(&AiError::Disconnected, false);
        assert_eq!(
            (gone.code, gone.message.as_str()),
            (
                ErrorCode::Disconnected,
                "Can't check speaking right now. publik API is disconnected."
            )
        );
        assert_eq!(gone.action, Some(json!({"use_local": false})));
        let offline = speaking_error(&AiError::Offline, false);
        assert_eq!(
            (offline.code, offline.message.as_str()),
            (
                ErrorCode::Offline,
                "Speaking checks need a connection or the local model."
            )
        );
        assert_eq!(
            speaking_error(&AiError::Invalid, false).message,
            "The AI answer could not be used."
        );
        assert_eq!(
            speaking_error(&AiError::Busy, false).message,
            "publik API is busy; try again in a moment"
        );
        let other = speaking_error(
            &AiError::Provider {
                provider: ProviderKind::Publik,
                code: 500,
                message: "Internal Server Error".into(),
            },
            false,
        );
        assert_eq!(
            (other.code, other.message.as_str()),
            (
                ErrorCode::ProviderError,
                "Can't check speaking right now. publik API 500: Internal Server Error"
            )
        );
        assert_eq!(
            speaking_error(&AiError::NeedsConsent, false).code,
            ErrorCode::NeedsConsent
        );
    }
}
