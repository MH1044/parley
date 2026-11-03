//! The learner's settings, `settings.json` in the data directory (docs/ARCHITECTURE.md §4;
//! docs/modules/04-app-shell-and-offline-course.md, Design notes).
//!
//! A missing file gives the defaults. A file that is not UTF-8 JSON, names another schema, or
//! fails `schemas/settings.schema.json` (unknown keys included) also gives the defaults, and the
//! app says so once with [`SETTINGS_NOTICE`]; the next save overwrites it. Every key but `schema`
//! may be left out and takes its default, so a hand-edited file keeps working. Settings never
//! hold a key.
//!
//! A change arrives as a patch: an object of settings keys, nested objects merged key by key,
//! `null` setting null, `schema` ignored. Every value is checked against the rules below before
//! anything changes, and a refusal names the setting by its dotted path (`audio.voice_source`).

use std::sync::OnceLock;

use jsonschema::Validator;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, ErrorCode};
use crate::grading::AccentLeniency;
use crate::srs::SrsParams;

/// The schema tag of `settings.json`.
pub const SETTINGS_SCHEMA: &str = "parley.settings/1";

/// The notice of a settings file that could not be read.
pub const SETTINGS_NOTICE: &str =
    "Your settings file could not be read, so Parley started with default settings.";

/// The file name in the data directory.
pub const SETTINGS_FILE: &str = "settings.json";

const SCHEMA_TEXT: &str = include_str!("../../../schemas/settings.schema.json");

/// The compiled `schemas/settings.schema.json`.
pub fn settings_validator() -> &'static Validator {
    static VALIDATOR: OnceLock<Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| {
        let schema: Value =
            serde_json::from_str(SCHEMA_TEXT).expect("the settings schema is valid JSON");
        jsonschema::validator_for(&schema).expect("the settings schema is a valid JSON Schema")
    })
}

/// A voice per course language.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PerLanguage {
    pub es: Option<String>,
    pub fr: Option<String>,
    pub de: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    /// `os` or `natural`.
    pub voice_source: String,
    pub os_voice: PerLanguage,
    pub natural_voice: PerLanguage,
    pub slow_speed: f64,
    pub autoplay: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        AudioSettings {
            voice_source: "os".into(),
            os_voice: PerLanguage::default(),
            natural_voice: PerLanguage::default(),
            slow_speed: 0.75,
            autoplay: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpeakingSettings {
    pub enabled: bool,
    /// `provider` or `local`.
    pub transcriber: String,
    pub pass_threshold: f64,
    pub keep_recordings: bool,
}

impl Default for SpeakingSettings {
    fn default() -> Self {
        SpeakingSettings {
            enabled: true,
            transcriber: "provider".into(),
            pass_threshold: 0.8,
            keep_recordings: false,
        }
    }
}

/// The learner's own OpenAI-compatible endpoint. The key itself is never here.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OwnKeySettings {
    pub base_url: Option<String>,
    pub chat_model: Option<String>,
    pub transcribe_model: Option<String>,
    pub speech_model: Option<String>,
    pub speech_voice: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalSettings {
    pub base_url: String,
    pub chat_model: Option<String>,
}

impl Default for LocalSettings {
    fn default() -> Self {
        LocalSettings {
            base_url: "http://localhost:11434/v1".into(),
            chat_model: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    /// `unset`, `publik`, `own_key` or `local`.
    pub provider: String,
    pub fresh_lessons: bool,
    pub prepare_next_lesson: bool,
    pub stories: bool,
    pub share_device_name: bool,
    pub own_key: OwnKeySettings,
    pub local: LocalSettings,
}

impl Default for AiSettings {
    fn default() -> Self {
        AiSettings {
            provider: "unset".into(),
            fresh_lessons: true,
            prepare_next_lesson: true,
            stories: true,
            share_device_name: false,
            own_key: OwnKeySettings::default(),
            local: LocalSettings::default(),
        }
    }
}

/// `settings.json`, field for field and in its key order. Enumerated values are kept as the
/// strings the file holds; [`apply_patch`] and the schema allow only the listed ones.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Always [`SETTINGS_SCHEMA`].
    pub schema: String,
    /// `es`, `fr`, `de`, or none before a course is chosen.
    pub active_course: Option<String>,
    /// 10, 20, 30 or 50.
    pub daily_goal_xp: u32,
    /// `lenient`, `strict` or `ignore`.
    pub accent_leniency: String,
    /// `typing` or `word_bank`.
    pub answer_input: String,
    /// 0.80 to 0.95.
    pub desired_retention: f64,
    /// 1 to 10.
    pub new_items_per_lesson: u32,
    /// 8 to 20.
    pub lesson_length: u32,
    pub audio: AudioSettings,
    pub speaking: SpeakingSettings,
    pub ai: AiSettings,
    pub update_check: bool,
}

impl Default for Settings {
    /// docs/ARCHITECTURE.md §4, with no course chosen yet.
    fn default() -> Self {
        Settings {
            schema: SETTINGS_SCHEMA.into(),
            active_course: None,
            daily_goal_xp: 20,
            accent_leniency: "lenient".into(),
            answer_input: "typing".into(),
            desired_retention: SrsParams::DEFAULT_RETENTION,
            new_items_per_lesson: 5,
            lesson_length: 12,
            audio: AudioSettings::default(),
            speaking: SpeakingSettings::default(),
            ai: AiSettings::default(),
            update_check: true,
        }
    }
}

impl Settings {
    /// The accent rule grading uses.
    pub fn accent(&self) -> AccentLeniency {
        match self.accent_leniency.as_str() {
            "strict" => AccentLeniency::Strict,
            "ignore" => AccentLeniency::Ignore,
            _ => AccentLeniency::Lenient,
        }
    }

    /// The scheduler's parameters.
    pub fn srs(&self) -> SrsParams {
        SrsParams::with_retention(self.desired_retention)
    }

    /// The file's text: pretty-printed with two spaces and a final newline.
    pub fn to_file_text(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).expect("settings serialise");
        text.push('\n');
        text
    }
}

/// Reads a settings file. The error says why it was refused, for logs only; the learner sees
/// [`SETTINGS_NOTICE`].
pub fn parse_settings(bytes: &[u8]) -> Result<Settings, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "not UTF-8".to_string())?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let value: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    settings_from_value(value)
}

/// Settings from a JSON value: the schema tag, then the JSON Schema, then the typed structure.
pub fn settings_from_value(value: Value) -> Result<Settings, String> {
    match value.get("schema").and_then(Value::as_str) {
        Some(SETTINGS_SCHEMA) => {}
        Some(other) => return Err(format!("schema is {other}")),
        None => return Err("no schema".to_string()),
    }
    if !settings_validator().is_valid(&value) {
        return Err("does not match schemas/settings.schema.json".to_string());
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// What a leaf setting may hold.
enum Rule {
    /// One of these strings.
    OneOf(&'static [&'static str]),
    /// One of these strings, or null.
    OneOfOrNull(&'static [&'static str]),
    /// One of these whole numbers.
    Whole(&'static [u64]),
    /// A whole number in this range.
    WholeIn(u64, u64),
    /// A number in this range.
    NumberIn(f64, f64),
    Bool,
    Text,
    TextOrNull,
}

/// The rule of every leaf, by dotted path.
fn rule(path: &str) -> Option<Rule> {
    Some(match path {
        "active_course" => Rule::OneOfOrNull(&["es", "fr", "de"]),
        "daily_goal_xp" => Rule::Whole(&[10, 20, 30, 50]),
        "accent_leniency" => Rule::OneOf(&["lenient", "strict", "ignore"]),
        "answer_input" => Rule::OneOf(&["typing", "word_bank"]),
        "desired_retention" => Rule::NumberIn(SrsParams::MIN_RETENTION, SrsParams::MAX_RETENTION),
        "new_items_per_lesson" => Rule::WholeIn(1, 10),
        "lesson_length" => Rule::WholeIn(8, 20),
        "audio.voice_source" => Rule::OneOf(&["os", "natural"]),
        "audio.os_voice.es" | "audio.os_voice.fr" | "audio.os_voice.de" => Rule::TextOrNull,
        "audio.natural_voice.es" | "audio.natural_voice.fr" | "audio.natural_voice.de" => {
            Rule::TextOrNull
        }
        "audio.slow_speed" => Rule::NumberIn(0.5, 1.0),
        "audio.autoplay" => Rule::Bool,
        "speaking.enabled" | "speaking.keep_recordings" => Rule::Bool,
        "speaking.transcriber" => Rule::OneOf(&["provider", "local"]),
        "speaking.pass_threshold" => Rule::NumberIn(0.0, 1.0),
        "ai.provider" => Rule::OneOf(&["unset", "publik", "own_key", "local"]),
        "ai.fresh_lessons" | "ai.prepare_next_lesson" | "ai.stories" | "ai.share_device_name" => {
            Rule::Bool
        }
        "ai.own_key.base_url"
        | "ai.own_key.chat_model"
        | "ai.own_key.transcribe_model"
        | "ai.own_key.speech_model"
        | "ai.own_key.speech_voice"
        | "ai.local.chat_model" => Rule::TextOrNull,
        "ai.local.base_url" => Rule::Text,
        "update_check" => Rule::Bool,
        _ => return None,
    })
}

fn invalid_value(path: &str) -> AppError {
    AppError::new(
        ErrorCode::InvalidInput,
        format!("Invalid value for {path}."),
    )
}

fn unknown_value(value: &str, path: &str) -> AppError {
    AppError::new(
        ErrorCode::InvalidInput,
        format!("Unknown value {value} for {path}."),
    )
}

fn unknown_setting(path: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, format!("Unknown setting {path}."))
}

/// Checks one leaf value: an enumeration given an unknown string is an unknown value; anything
/// else that breaks the rule is an invalid value.
fn check(path: &str, value: &Value) -> Result<(), AppError> {
    let Some(rule) = rule(path) else {
        return Err(unknown_setting(path));
    };
    let ok = match (&rule, value) {
        (Rule::OneOf(allowed), Value::String(s))
        | (Rule::OneOfOrNull(allowed), Value::String(s)) => {
            if allowed.contains(&s.as_str()) {
                true
            } else {
                return Err(unknown_value(s, path));
            }
        }
        (Rule::OneOfOrNull(_), Value::Null) => true,
        (Rule::Whole(allowed), Value::Number(n)) => {
            n.as_u64().is_some_and(|n| allowed.contains(&n))
        }
        (Rule::WholeIn(min, max), Value::Number(n)) => {
            n.as_u64().is_some_and(|n| (*min..=*max).contains(&n))
        }
        (Rule::NumberIn(min, max), Value::Number(n)) => {
            n.as_f64().is_some_and(|n| n >= *min && n <= *max)
        }
        (Rule::Bool, Value::Bool(_)) => true,
        (Rule::Text, Value::String(_)) => true,
        (Rule::TextOrNull, Value::String(_) | Value::Null) => true,
        _ => false,
    };
    if ok { Ok(()) } else { Err(invalid_value(path)) }
}

/// Merges `patch` into `current` (both objects) at `prefix`, checking every leaf.
fn merge(
    current: &mut serde_json::Map<String, Value>,
    patch: &serde_json::Map<String, Value>,
    prefix: &str,
) -> Result<(), AppError> {
    for (key, value) in patch {
        if prefix.is_empty() && key == "schema" {
            continue;
        }
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        let Some(slot) = current.get_mut(key) else {
            return Err(unknown_setting(&path));
        };
        match slot {
            Value::Object(inner) => match value {
                Value::Object(inner_patch) => merge(inner, inner_patch, &path)?,
                _ => return Err(invalid_value(&path)),
            },
            _ => {
                check(&path, value)?;
                *slot = value.clone();
            }
        }
    }
    Ok(())
}

/// The settings after a patch, or the first refusal (keys in order): "Unknown setting {key}.",
/// "Unknown value {v} for {key}." or "Invalid value for {key}.". Nothing changes on a refusal.
pub fn apply_patch(current: &Settings, patch: &Value) -> Result<Settings, AppError> {
    let Value::Object(patch) = patch else {
        return Err(invalid_value("patch"));
    };
    let Value::Object(mut merged) = serde_json::to_value(current).expect("settings serialise")
    else {
        unreachable!("settings serialise to an object");
    };
    merge(&mut merged, patch, "")?;
    merged.insert("schema".into(), Value::String(SETTINGS_SCHEMA.into()));
    serde_json::from_value(Value::Object(merged)).map_err(|_| invalid_value("patch"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(result: Result<Settings, AppError>) -> String {
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        err.message
    }

    /// Every leaf of a value as dotted paths.
    fn leaves(value: &Value, prefix: &str, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, inner) in map {
                    let path = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    leaves(inner, &path, out);
                }
            }
            _ => out.push(prefix.to_string()),
        }
    }

    #[test]
    fn the_defaults_are_the_architecture_s_with_no_course() {
        let value = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(
            value,
            json!({
                "schema": "parley.settings/1", "active_course": null, "daily_goal_xp": 20,
                "accent_leniency": "lenient", "answer_input": "typing", "desired_retention": 0.9,
                "new_items_per_lesson": 5, "lesson_length": 12,
                "audio": { "voice_source": "os", "os_voice": { "es": null, "fr": null, "de": null },
                           "natural_voice": { "es": null, "fr": null, "de": null }, "slow_speed": 0.75, "autoplay": true },
                "speaking": { "enabled": true, "transcriber": "provider", "pass_threshold": 0.8, "keep_recordings": false },
                "ai": { "provider": "unset", "fresh_lessons": true, "prepare_next_lesson": true, "stories": true,
                        "share_device_name": false,
                        "own_key": { "base_url": null, "chat_model": null, "transcribe_model": null,
                                     "speech_model": null, "speech_voice": null },
                        "local": { "base_url": "http://localhost:11434/v1", "chat_model": null } },
                "update_check": true
            })
        );
        let text = Settings::default().to_file_text();
        assert!(text.starts_with("{\n  \"schema\": \"parley.settings/1\",\n  \"active_course\""));
        assert_eq!(
            parse_settings(text.as_bytes()).unwrap(),
            Settings::default()
        );
        let mut paths = Vec::new();
        leaves(&value, "", &mut paths);
        for path in paths.iter().filter(|p| *p != "schema") {
            assert!(rule(path).is_some(), "no rule for {path}");
            let leaf = value
                .pointer(&format!("/{}", path.replace('.', "/")))
                .unwrap();
            assert_eq!(check(path, leaf), Ok(()), "{path}");
        }
    }

    #[test]
    fn bad_files_are_refused() {
        for bad in [
            &b"\xff\xfe"[..],
            b"{",
            b"[]",
            br#"{"schema": "parley.settings/2"}"#,
            br#"{"daily_goal_xp": 20}"#,
            br#"{"schema": "parley.settings/1", "colour": "blue"}"#,
            br#"{"schema": "parley.settings/1", "lesson_length": 30}"#,
            br#"{"schema": "parley.settings/1", "audio": {"volume": 1}}"#,
        ] {
            assert!(
                parse_settings(bad).is_err(),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
        let partial =
            parse_settings(br#"{"schema": "parley.settings/1", "daily_goal_xp": 30}"#).unwrap();
        assert_eq!(partial.daily_goal_xp, 30);
        assert_eq!(partial.lesson_length, 12, "a missing key takes its default");
    }

    #[test]
    fn a_patch_merges_key_by_key() {
        let current = Settings::default();
        let next = apply_patch(
            &current,
            &json!({"schema": "ignored", "daily_goal_xp": 50, "active_course": "fr",
                    "audio": {"os_voice": {"fr": "Voice A"}}, "ai": {"own_key": {"base_url": null}}}),
        )
        .unwrap();
        assert_eq!(next.daily_goal_xp, 50);
        assert_eq!(next.active_course.as_deref(), Some("fr"));
        assert_eq!(next.audio.os_voice.fr.as_deref(), Some("Voice A"));
        assert_eq!(next.audio.slow_speed, 0.75, "the rest of audio is kept");
        assert_eq!(next.schema, SETTINGS_SCHEMA);
        let cleared = apply_patch(&next, &json!({"active_course": null})).unwrap();
        assert_eq!(cleared.active_course, None);
    }

    #[test]
    fn every_refusal_has_its_message() {
        let s = Settings::default();
        let cases = [
            (
                json!({"accent_leniency": "loose"}),
                "Unknown value loose for accent_leniency.",
            ),
            (
                json!({"lesson_length": 30}),
                "Invalid value for lesson_length.",
            ),
            (
                json!({"lesson_length": 7}),
                "Invalid value for lesson_length.",
            ),
            (
                json!({"lesson_length": 12.5}),
                "Invalid value for lesson_length.",
            ),
            (json!({"colour": "blue"}), "Unknown setting colour."),
            (
                json!({"daily_goal_xp": 15}),
                "Invalid value for daily_goal_xp.",
            ),
            (
                json!({"daily_goal_xp": "20"}),
                "Invalid value for daily_goal_xp.",
            ),
            (
                json!({"desired_retention": 0.96}),
                "Invalid value for desired_retention.",
            ),
            (
                json!({"new_items_per_lesson": 0}),
                "Invalid value for new_items_per_lesson.",
            ),
            (
                json!({"answer_input": "voice"}),
                "Unknown value voice for answer_input.",
            ),
            (
                json!({"active_course": "it"}),
                "Unknown value it for active_course.",
            ),
            (
                json!({"audio": {"voice_source": "robot"}}),
                "Unknown value robot for audio.voice_source.",
            ),
            (
                json!({"audio": {"voice_source": 3}}),
                "Invalid value for audio.voice_source.",
            ),
            (
                json!({"audio": {"pitch": 3}}),
                "Unknown setting audio.pitch.",
            ),
            (json!({"audio": null}), "Invalid value for audio."),
            (
                json!({"ai": {"local": {"base_url": null}}}),
                "Invalid value for ai.local.base_url.",
            ),
            (
                json!({"update_check": "yes"}),
                "Invalid value for update_check.",
            ),
            (json!(["daily_goal_xp"]), "Invalid value for patch."),
        ];
        for (patch, expected) in cases {
            assert_eq!(message(apply_patch(&s, &patch)), expected, "{patch}");
        }
        let edges =
            json!({"desired_retention": 0.8, "lesson_length": 20, "new_items_per_lesson": 10});
        assert!(apply_patch(&s, &edges).is_ok());
        assert!(apply_patch(&s, &json!({"desired_retention": 0.95})).is_ok());
    }

    #[test]
    fn the_accent_and_retention_follow_the_settings() {
        let mut s = Settings::default();
        assert_eq!(s.accent(), AccentLeniency::Lenient);
        s.accent_leniency = "strict".into();
        assert_eq!(s.accent(), AccentLeniency::Strict);
        s.desired_retention = 0.85;
        assert_eq!(s.srs().desired_retention, 0.85);
    }
}
