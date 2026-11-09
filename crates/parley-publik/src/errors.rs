//! Error mapping (docs/PUBLIK-CONTRACT.md §8; docs/PUBLIK-INTEGRATION.md §9; adaptation 5): an HTTP
//! status and body, or a request that got no answer, become the one [`AiError`] the learner sees.
//! Stop, say one thing, offer one way out.

use parley_core::ai::{AiError, NO_TOKEN_MESSAGE, ProviderKind};
use parley_core::env::NetError;
use serde_json::Value;

use crate::publik::top_up_url_from_402;

/// The body's JSON, or `{}`.
pub fn body_json(body: &[u8]) -> Value {
    serde_json::from_slice(body).unwrap_or(Value::Object(Default::default()))
}

/// `error.message` of an OpenAI-style error body.
pub fn error_message(body: &Value) -> Option<String> {
    body["error"]["message"]
        .as_str()
        .or_else(|| body["message"].as_str())
        .filter(|m| !m.trim().is_empty())
        .map(str::to_string)
}

/// The canonical reason of an HTTP status ("Internal Server Error").
pub fn reason(code: u16) -> String {
    reqwest::StatusCode::from_u16(code)
        .ok()
        .and_then(|s| s.canonical_reason())
        .unwrap_or("Unknown status")
        .to_string()
}

/// A 401, or a 403 whose error is `install_revoked`: the install's key no longer works.
pub fn is_revoked(code: u16, body: &Value) -> bool {
    let kind = |field: &str| body["error"][field].as_str() == Some("install_revoked");
    code == 401 || (code == 403 && (kind("type") || kind("code")))
}

/// A publik API error response (contract §8): 402 needs credit with its one link, 401 and 403
/// `install_revoked` disconnect, 429 shows its message, 503 is busy, anything else names its code.
/// `fallback_top_up_url` is the status's link, for a 402 whose body names none.
pub fn publik_error(code: u16, body: &[u8], fallback_top_up_url: Option<String>) -> AiError {
    let json = body_json(body);
    match code {
        402 => {
            let (message, top_up_url) = top_up_url_from_402(&json);
            AiError::NeedsCredit {
                message,
                top_up_url: top_up_url.or(fallback_top_up_url).unwrap_or_default(),
            }
        }
        _ if is_revoked(code, &json) => AiError::Disconnected,
        429 => AiError::RateLimited {
            message: error_message(&json)
                .unwrap_or_else(|| "publik API is busy; try again in a moment".to_string()),
        },
        503 => AiError::Busy,
        _ => AiError::Provider {
            provider: ProviderKind::Publik,
            code,
            message: error_message(&json).unwrap_or_else(|| reason(code)),
        },
    }
}

/// An error response of the learner's own provider or the local server.
pub fn provider_error(kind: ProviderKind, code: u16, body: &[u8]) -> AiError {
    AiError::Provider {
        provider: kind,
        code,
        message: error_message(&body_json(body)).unwrap_or_else(|| reason(code)),
    }
}

/// A request that got no answer: a timeout names the provider; a local server that does not answer
/// is down; otherwise the learner is offline.
pub fn net_error(kind: ProviderKind, error: &NetError, base_url: &str) -> AiError {
    match (error, kind) {
        (NetError::Timeout, _) => AiError::Timeout { provider: kind },
        (_, ProviderKind::Local) => AiError::LocalDown {
            base_url: base_url.to_string(),
        },
        _ => AiError::Offline,
    }
}

/// `text` with every occurrence of `key` replaced by `…`. A server can echo the key it was sent
/// (an "invalid key pk_…" message), and its text reaches the interface (invariant I1).
pub fn redact(text: &str, key: Option<&str>) -> String {
    match key.map(str::trim) {
        Some(key) if !key.is_empty() => text.replace(key, "…"),
        _ => text.to_string(),
    }
}

/// The error with every text that came from a server's body cleaned of `key`.
pub fn redact_error(error: AiError, key: Option<&str>) -> AiError {
    match error {
        AiError::NeedsCredit {
            message,
            top_up_url,
        } => AiError::NeedsCredit {
            message: redact(&message, key),
            top_up_url: redact(&top_up_url, key),
        },
        AiError::RateLimited { message } => AiError::RateLimited {
            message: redact(&message, key),
        },
        AiError::Provider {
            provider,
            code,
            message,
        } => AiError::Provider {
            provider,
            code,
            message: redact(&message, key),
        },
        AiError::Setup { message } => AiError::Setup {
            message: redact(&message, key),
        },
        other => other,
    }
}

/// A provisioning failure (contract §5): a build without an app token says so; every other
/// failure shows the reference module's message.
pub fn setup_error(message: String) -> AiError {
    if message == NO_TOKEN_MESSAGE {
        AiError::NoToken
    } else {
        AiError::Setup { message }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publik_statuses_map_to_the_contract_s_errors() {
        let credit = publik_error(
            402,
            br#"{"error":{"message":"Link this computer to keep going.","top_up_url":"https://example.invalid/claim"}}"#,
            Some("https://example.invalid/other".into()),
        );
        assert_eq!(
            credit,
            AiError::NeedsCredit {
                message: "Link this computer to keep going.".into(),
                top_up_url: "https://example.invalid/claim".into()
            }
        );
        let bare = publik_error(402, b"", Some("https://example.invalid/w".into()));
        assert_eq!(
            bare,
            AiError::NeedsCredit {
                message: "Not enough publik API balance.".into(),
                top_up_url: "https://example.invalid/w".into()
            }
        );
        assert_eq!(
            publik_error(401, br#"{"error":{"type":"invalid_api_key"}}"#, None),
            AiError::Disconnected
        );
        assert_eq!(
            publik_error(403, br#"{"error":{"type":"install_revoked"}}"#, None),
            AiError::Disconnected
        );
        assert_eq!(
            publik_error(
                403,
                br#"{"error":{"type":"forbidden","message":"No."}}"#,
                None
            )
            .to_string(),
            "publik API 403: No."
        );
        assert_eq!(
            publik_error(429, br#"{"error":{"message":"Slow down."}}"#, None),
            AiError::RateLimited {
                message: "Slow down.".into()
            }
        );
        assert_eq!(publik_error(503, b"", None), AiError::Busy);
        assert_eq!(
            publik_error(500, b"", None).to_string(),
            "publik API 500: Internal Server Error"
        );
        assert_eq!(
            publik_error(
                400,
                br#"{"error":{"message":"max_tokens is too large"}}"#,
                None
            )
            .to_string(),
            "publik API 400: max_tokens is too large"
        );
    }

    #[test]
    fn a_key_a_server_echoes_never_reaches_the_message() {
        assert_eq!(
            redact(
                "Incorrect API key provided: sk-own-a. Check sk-own-a.",
                Some("sk-own-a")
            ),
            "Incorrect API key provided: …. Check …."
        );
        assert_eq!(redact("no key here", None), "no key here");
        assert_eq!(redact("empty key", Some(" ")), "empty key");
        let own = redact_error(
            provider_error(
                ProviderKind::OwnKey,
                401,
                br#"{"error":{"message":"Incorrect API key provided: sk-own-a"}}"#,
            ),
            Some("sk-own-a"),
        );
        assert_eq!(
            own.to_string(),
            "Your provider returned 401: Incorrect API key provided: …"
        );
        let credit = redact_error(
            publik_error(
                402,
                br#"{"error":{"message":"pk_live_a has no balance.","top_up_url":"https://example.invalid/t"}}"#,
                None,
            ),
            Some("pk_live_a"),
        );
        assert_eq!(credit.to_string(), "… has no balance.");
        let busy = redact_error(
            publik_error(
                429,
                br#"{"error":{"message":"Slow down, pk_live_a."}}"#,
                None,
            ),
            Some("pk_live_a"),
        );
        assert_eq!(busy.to_string(), "Slow down, ….");
        let other = redact_error(
            publik_error(400, br#"{"error":{"message":"bad key pk_live_a"}}"#, None),
            Some("pk_live_a"),
        );
        assert_eq!(other.to_string(), "publik API 400: bad key …");
        let app: parley_core::error::AppError = other.into();
        assert!(!serde_json::to_string(&app).unwrap().contains("pk_live_a"));
    }

    #[test]
    fn other_providers_and_missing_answers() {
        assert_eq!(
            provider_error(
                ProviderKind::Local,
                404,
                br#"{"error":{"message":"model not found"}}"#
            )
            .to_string(),
            "The local model returned 404: model not found"
        );
        assert_eq!(
            provider_error(ProviderKind::OwnKey, 502, b"<html>").to_string(),
            "Your provider returned 502: Bad Gateway"
        );
        assert_eq!(
            net_error(ProviderKind::Publik, &NetError::Timeout, "x"),
            AiError::Timeout {
                provider: ProviderKind::Publik
            }
        );
        assert_eq!(
            net_error(ProviderKind::OwnKey, &NetError::Offline, "x"),
            AiError::Offline
        );
        assert_eq!(
            net_error(
                ProviderKind::Local,
                &NetError::Offline,
                "http://localhost:11434/v1"
            ),
            AiError::LocalDown {
                base_url: "http://localhost:11434/v1".into()
            }
        );
        assert_eq!(setup_error(NO_TOKEN_MESSAGE.into()), AiError::NoToken);
        assert_eq!(
            setup_error(
                "publik API could not set up this computer (500). Use your own key for now.".into()
            )
            .to_string(),
            "publik API could not set up this computer (500). Use your own key for now."
        );
    }
}
