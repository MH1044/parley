//! The one AI client every feature calls (docs/modules/06-publik-and-providers.md, Interfaces;
//! docs/PUBLIK-INTEGRATION.md §2 adaptation 4).
//!
//! [`AiClient`] is implemented in the `parley-publik` crate for the three providers (publik API, the
//! learner's own OpenAI-compatible key, a local model server); the core has no HTTP code of its own.
//! Requests and replies here carry no key: the key lives in the client, and [`AiStatus`], the only
//! thing the interface receives about AI, has no key field (invariant I1).

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{AppError, ErrorCode};

/// Which provider answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Publik,
    OwnKey,
    Local,
}

impl ProviderKind {
    /// "publik", "own_key" or "local": the value in a cache key and in `ai.provider`.
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::Publik => "publik",
            ProviderKind::OwnKey => "own_key",
            ProviderKind::Local => "local",
        }
    }
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One chat message: a role and its content, a JSON string or an array of OpenAI content parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: Value,
}

/// An OpenAI Chat Completions request.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest {
    /// From [`AiClient::chat_model`] of the prompt's tier.
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
    pub temperature: f64,
    pub timeout_s: u64,
    /// Sent as `x-publik-idempotency-key`, on publik API calls only.
    pub idempotency_key: Option<String>,
}

/// The wire form of a chat request, in the key order of docs/PUBLIK-INTEGRATION.md §6 (model,
/// max_tokens, temperature, stream, messages), so the bytes sent do not depend on how a JSON
/// object orders its keys.
#[derive(Serialize)]
struct WireChat<'a> {
    model: &'a str,
    max_tokens: u32,
    temperature: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    messages: &'a [ChatMessage],
}

/// A temperature as JSON: a whole number as an integer (`0`, not `0.0`), anything else as it is.
fn temperature_json(temperature: f64) -> Value {
    if temperature.is_finite() && temperature.fract() == 0.0 && temperature.abs() < 1e15 {
        json!(temperature as i64)
    } else {
        json!(temperature)
    }
}

impl ChatRequest {
    fn wire(&self, stream: bool) -> WireChat<'_> {
        WireChat {
            model: &self.model,
            max_tokens: self.max_tokens,
            temperature: temperature_json(self.temperature),
            stream: stream.then_some(true),
            messages: &self.messages,
        }
    }

    /// {"model", "max_tokens", "temperature", "messages"} (+ "stream": true when streaming). A whole
    /// temperature is written as a JSON integer, so P0's body is exactly PUBLIK-INTEGRATION §6.2's.
    pub fn body(&self, stream: bool) -> Value {
        serde_json::to_value(self.wire(stream)).expect("a chat request serialises")
    }

    /// The bytes sent: [`ChatRequest::body`] written compactly in the documented key order.
    pub fn body_bytes(&self, stream: bool) -> Vec<u8> {
        serde_json::to_vec(&self.wire(stream)).expect("a chat request serialises")
    }
}

/// A chat answer: the assistant's text and, on publik API, the call's charge and request id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatReply {
    pub content: String,
    pub charge_micros: Option<i64>,
    pub request_id: Option<String>,
}

/// An `/audio/transcriptions` request (multipart: `file`, `model`, `language`, `response_format`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscribeRequest {
    /// Exactly the id the caller gives (on publik API, an id `GET /models` lists), never a tier.
    pub model: String,
    pub audio: Vec<u8>,
    pub file_name: String,
    pub mime: String,
    pub language: String,
    pub response_format: String,
    pub timeout_s: u64,
    pub idempotency_key: Option<String>,
}

/// A transcription answer: the response body as JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptReply {
    pub body: Value,
    pub charge_micros: Option<i64>,
}

/// An `/audio/speech` request.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeechRequest {
    /// Exactly the id the caller gives, never a tier.
    pub model: String,
    pub input: String,
    pub voice: String,
    pub response_format: String,
    pub speed: f64,
    pub timeout_s: u64,
    pub idempotency_key: Option<String>,
}

/// Synthesised audio as returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioReply {
    pub bytes: Vec<u8>,
    pub content_type: String,
    pub charge_micros: Option<i64>,
}

/// A model a local server offers, with its size when the server says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalModel {
    pub name: String,
    pub bytes: Option<u64>,
}

/// What the interface knows about AI (docs/modules/06-publik-and-providers.md, the AiStatus table).
/// It never holds a key.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AiStatus {
    /// `ai.provider`: `unset`, `publik`, `own_key` or `local`.
    pub provider: String,
    pub needs_consent: bool,
    pub connected: bool,
    pub claim_state: Option<String>,
    pub balance_micros: Option<i64>,
    pub balance_line: Option<String>,
    pub cost_sentence: Option<String>,
    pub claim_url: Option<String>,
    pub add_credit_url: Option<String>,
    pub top_up_url: Option<String>,
    pub needs_credit: bool,
    pub last_charge_micros: Option<i64>,
    pub last_request_id: Option<String>,
    pub starter_remaining_micros: Option<i64>,
    pub week_used_micros: Option<i64>,
    pub week_budget_micros: Option<i64>,
    pub week_resets_at: Option<String>,
    pub own_key_stored: bool,
    pub local_model: Option<String>,
}

/// One OpenAI-compatible client for the three providers.
pub trait AiClient: Send + Sync {
    fn chat(&self, req: ChatRequest) -> Result<ChatReply, AiError>;
    fn chat_stream(
        &self,
        req: ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<ChatReply, AiError>;
    fn transcribe(&self, req: TranscribeRequest) -> Result<TranscriptReply, AiError>;
    fn speech(&self, req: SpeechRequest) -> Result<AudioReply, AiError>;
    fn kind(&self) -> ProviderKind;
    fn status(&self) -> AiStatus;
    /// The chat `model` for a prompt tier: the tier itself on Publik (`publik-smart` becomes
    /// `publik-balanced` while `claim_state` is `anonymous`), the configured chat model on the own
    /// key, the chosen local model on Local.
    fn chat_model(&self, tier: &str) -> String;
}

/// Every request body is under this many bytes, base64 included (invariant I10). A client refuses
/// a body of this size or more with `AiError::TooLarge` before opening any connection.
pub const MAX_REQUEST_BYTES: usize = 4_000_000;

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding (RFC 4648 §4).
pub(crate) fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let sextets = [(n >> 18) & 63, (n >> 12) & 63, (n >> 6) & 63, n & 63];
        for (i, s) in sextets.iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(BASE64[*s as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The OpenAI content part of a JPEG (module 12): {"type":"image_url","image_url":{"url":"data:image/jpeg;base64,..."}}.
pub fn image_part(jpeg: &[u8]) -> Value {
    json!({
        "type": "image_url",
        "image_url": { "url": format!("data:image/jpeg;base64,{}", base64(jpeg)) }
    })
}

/// Why an AI call produced no usable answer. Every variant's text is [`AiError::message`]; none
/// carries a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum AiError {
    NeedsConsent,
    NoToken,
    OwnKeyMissing,
    NeedsCredit {
        message: String,
        top_up_url: String,
    },
    Disconnected,
    RateLimited {
        message: String,
    },
    Busy,
    Timeout {
        provider: ProviderKind,
    },
    Offline,
    Invalid,
    Provider {
        provider: ProviderKind,
        code: u16,
        message: String,
    },
    LocalDown {
        base_url: String,
    },
    /// Provisioning failures (contract §5), with their message.
    Setup {
        message: String,
    },
    TooLarge,
}

/// The message of a build without a `pat_` app token.
pub const NO_TOKEN_MESSAGE: &str =
    "This build has no publik API app token. Use your own key for now.";

impl AiError {
    /// The user-facing text (docs/modules/06-publik-and-providers.md, the AiError table).
    pub fn message(&self) -> String {
        match self {
            AiError::NeedsConsent => "AI features are not set up yet.".into(),
            AiError::NoToken => NO_TOKEN_MESSAGE.into(),
            AiError::OwnKeyMissing => "Add your own key in Settings first.".into(),
            AiError::NeedsCredit { message, .. } => message.clone(),
            AiError::Disconnected => "publik API is disconnected.".into(),
            AiError::RateLimited { message } => message.clone(),
            AiError::Busy => "publik API is busy; try again in a moment".into(),
            AiError::Timeout { provider } => match provider {
                ProviderKind::Publik => "publik API did not answer in time.".into(),
                ProviderKind::OwnKey => "Your provider did not answer in time.".into(),
                ProviderKind::Local => "The local model did not answer in time.".into(),
            },
            AiError::Offline => {
                "You're offline. AI features need a connection or a local model.".into()
            }
            AiError::Invalid => "The AI answer could not be used.".into(),
            AiError::Provider {
                provider,
                code,
                message,
            } => match provider {
                ProviderKind::Publik => format!("publik API {code}: {message}"),
                ProviderKind::OwnKey => format!("Your provider returned {code}: {message}"),
                ProviderKind::Local => format!("The local model returned {code}: {message}"),
            },
            AiError::LocalDown { base_url } => format!(
                "The local model server at {base_url} is not running. Start it, or choose another provider."
            ),
            AiError::Setup { message } => message.clone(),
            AiError::TooLarge => "This request is larger than 4 MB, so it was not sent.".into(),
        }
    }

    /// The error code the interface branches on.
    pub fn code(&self) -> ErrorCode {
        match self {
            AiError::NeedsConsent => ErrorCode::NeedsConsent,
            AiError::NoToken | AiError::OwnKeyMissing => ErrorCode::NoProvider,
            AiError::NeedsCredit { .. } => ErrorCode::NeedsCredit,
            AiError::Disconnected => ErrorCode::Disconnected,
            AiError::RateLimited { .. } => ErrorCode::RateLimited,
            AiError::Busy | AiError::Timeout { .. } => ErrorCode::Busy,
            AiError::Offline => ErrorCode::Offline,
            AiError::Invalid => ErrorCode::AiInvalid,
            AiError::Provider { .. } | AiError::LocalDown { .. } | AiError::Setup { .. } => {
                ErrorCode::ProviderError
            }
            AiError::TooLarge => ErrorCode::FileTooLarge,
        }
    }
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for AiError {}

impl From<AiError> for AppError {
    fn from(e: AiError) -> Self {
        let error = AppError::new(e.code(), e.message());
        match e {
            AiError::NeedsCredit { top_up_url, .. } => {
                error.with_action(json!({ "top_up_url": top_up_url }))
            }
            _ => error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc_4648_test_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected, "{input}");
        }
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn an_image_part_is_a_jpeg_data_url() {
        let part = image_part(&[0xff, 0xd8, 0xff]);
        assert_eq!(
            part,
            json!({"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,/9j/"}})
        );
    }

    fn ping_request() -> ChatRequest {
        ChatRequest {
            model: "publik-fast".into(),
            messages: vec![
                ChatMessage {
                    role: "system".into(),
                    content: json!(
                        "You are a connectivity check. Reply with exactly this JSON and nothing else: {\"ok\": true}"
                    ),
                },
                ChatMessage {
                    role: "user".into(),
                    content: json!("ping"),
                },
            ],
            max_tokens: 10,
            temperature: 0.0,
            timeout_s: 15,
            idempotency_key: None,
        }
    }

    #[test]
    fn the_ping_body_is_byte_for_byte_the_documented_one() {
        let expected = r#"{"model":"publik-fast","max_tokens":10,"temperature":0,"messages":[{"role":"system","content":"You are a connectivity check. Reply with exactly this JSON and nothing else: {\"ok\": true}"},{"role":"user","content":"ping"}]}"#;
        let req = ping_request();
        assert_eq!(String::from_utf8(req.body_bytes(false)).unwrap(), expected);
        assert_eq!(
            req.body(false),
            serde_json::from_str::<Value>(expected).unwrap()
        );
        let streamed = req.body(true);
        assert_eq!(streamed["stream"], json!(true));
        let mut warm = req;
        warm.temperature = 0.7;
        assert_eq!(warm.body(false)["temperature"], json!(0.7));
    }

    #[test]
    fn every_error_has_its_code_message_and_action() {
        let cases = [
            (
                AiError::NeedsConsent,
                "needs_consent",
                "AI features are not set up yet.",
            ),
            (
                AiError::NoToken,
                "no_provider",
                "This build has no publik API app token. Use your own key for now.",
            ),
            (
                AiError::OwnKeyMissing,
                "no_provider",
                "Add your own key in Settings first.",
            ),
            (
                AiError::Disconnected,
                "disconnected",
                "publik API is disconnected.",
            ),
            (
                AiError::Busy,
                "busy",
                "publik API is busy; try again in a moment",
            ),
            (
                AiError::Timeout {
                    provider: ProviderKind::Local,
                },
                "busy",
                "The local model did not answer in time.",
            ),
            (
                AiError::Offline,
                "offline",
                "You're offline. AI features need a connection or a local model.",
            ),
            (
                AiError::Invalid,
                "ai_invalid",
                "The AI answer could not be used.",
            ),
            (
                AiError::Provider {
                    provider: ProviderKind::OwnKey,
                    code: 500,
                    message: "Internal Server Error".into(),
                },
                "provider_error",
                "Your provider returned 500: Internal Server Error",
            ),
            (
                AiError::LocalDown {
                    base_url: "http://localhost:11434/v1".into(),
                },
                "provider_error",
                "The local model server at http://localhost:11434/v1 is not running. Start it, or choose another provider.",
            ),
            (
                AiError::TooLarge,
                "file_too_large",
                "This request is larger than 4 MB, so it was not sent.",
            ),
        ];
        for (error, code, message) in cases {
            let app: AppError = error.clone().into();
            assert_eq!(app.code.as_str(), code, "{error:?}");
            assert_eq!(app.message, message);
            assert_eq!(error.to_string(), message);
            assert_eq!(app.action, None);
        }
        let credit: AppError = AiError::NeedsCredit {
            message: "Add credit to keep going.".into(),
            top_up_url: "https://example.invalid/top-up".into(),
        }
        .into();
        assert_eq!(credit.code, ErrorCode::NeedsCredit);
        assert_eq!(credit.message, "Add credit to keep going.");
        assert_eq!(
            credit.action,
            Some(json!({"top_up_url": "https://example.invalid/top-up"}))
        );
    }

    #[test]
    fn provider_kinds_serialise_in_snake_case() {
        for kind in [
            ProviderKind::Publik,
            ProviderKind::OwnKey,
            ProviderKind::Local,
        ] {
            assert_eq!(
                serde_json::to_string(&kind).unwrap(),
                format!("\"{}\"", kind.as_str())
            );
        }
        let status = AiStatus::default();
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(value["claim_state"], Value::Null);
        assert_eq!(
            serde_json::from_value::<AiStatus>(value).unwrap(),
            AiStatus::default()
        );
    }
}
