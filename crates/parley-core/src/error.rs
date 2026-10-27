//! The one error type every command returns (docs/ARCHITECTURE.md §5).
//!
//! It serialises as `{"code": "...", "message": "...", "action": {...} | null}`, so the UI can
//! branch on the code, show the message, and offer the one action the error carries (for example a
//! top-up link on `needs_credit`). The message is user-facing text; module specs spell each one out.

use std::fmt;

use serde::Serialize;

/// Every error code a command can return. Serialised in snake case, exactly as the UI matches them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Offline,
    NeedsConsent,
    NeedsCredit,
    Disconnected,
    RateLimited,
    Busy,
    AiInvalid,
    ProviderError,
    NoProvider,
    PackMissing,
    ChecksumMismatch,
    NoMicrophone,
    MicPermission,
    TooQuiet,
    NoVoice,
    TurnCapReached,
    FileTooLarge,
    UnsupportedMedia,
    InvalidInput,
    NotFound,
    Storage,
}

impl ErrorCode {
    /// The code as the UI sees it.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Offline => "offline",
            ErrorCode::NeedsConsent => "needs_consent",
            ErrorCode::NeedsCredit => "needs_credit",
            ErrorCode::Disconnected => "disconnected",
            ErrorCode::RateLimited => "rate_limited",
            ErrorCode::Busy => "busy",
            ErrorCode::AiInvalid => "ai_invalid",
            ErrorCode::ProviderError => "provider_error",
            ErrorCode::NoProvider => "no_provider",
            ErrorCode::PackMissing => "pack_missing",
            ErrorCode::ChecksumMismatch => "checksum_mismatch",
            ErrorCode::NoMicrophone => "no_microphone",
            ErrorCode::MicPermission => "mic_permission",
            ErrorCode::TooQuiet => "too_quiet",
            ErrorCode::NoVoice => "no_voice",
            ErrorCode::TurnCapReached => "turn_cap_reached",
            ErrorCode::FileTooLarge => "file_too_large",
            ErrorCode::UnsupportedMedia => "unsupported_media",
            ErrorCode::InvalidInput => "invalid_input",
            ErrorCode::NotFound => "not_found",
            ErrorCode::Storage => "storage",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a failed command tells the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppError {
    pub code: ErrorCode,
    /// User-facing text. Never a key, a learner's answer, a file path chosen by the learner, or a
    /// balance (docs/ARCHITECTURE.md §8).
    pub message: String,
    /// The one way out the error offers, such as `{"top_up_url": "..."}`, or nothing.
    pub action: Option<serde_json::Value>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        AppError {
            code,
            message: message.into(),
            action: None,
        }
    }

    pub fn with_action(mut self, action: serde_json::Value) -> Self {
        self.action = Some(action);
        self
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_strings_match_the_serialised_form() {
        for code in [
            ErrorCode::Offline,
            ErrorCode::NeedsCredit,
            ErrorCode::ChecksumMismatch,
            ErrorCode::TurnCapReached,
            ErrorCode::Storage,
        ] {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(json, format!("\"{}\"", code.as_str()));
        }
    }

    #[test]
    fn action_is_kept_as_given() {
        let err = AppError::new(ErrorCode::NeedsCredit, "Add credit")
            .with_action(serde_json::json!({"top_up_url": "https://example.com/top-up"}));
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(
            json,
            r#"{"code":"needs_credit","message":"Add credit","action":{"top_up_url":"https://example.com/top-up"}}"#
        );
    }
}
