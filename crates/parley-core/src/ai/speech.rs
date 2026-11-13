//! P5, natural voice (docs/PUBLIK-INTEGRATION.md §6.7; docs/modules/09-listening-and-voices.md):
//! `content/voices.toml`, the choice between the OS voice and a natural one, the cache key and
//! request of one sentence, the validation of the audio that comes back, and the measured cost
//! line.
//!
//! A sentence is cached as a file, `cache/audio/<key>.mp3`, under the SHA-256 of the canonical
//! JSON of {feature, v, provider, model, input: {text_nfc, voice, speed}}, so identical input never
//! spends twice (invariant I5). Audio that fails §6.7 is never stored (I2).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::json;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use super::cache::{cache_key, cache_key_json, idempotency_key};
use super::client::{AiStatus, AudioReply, ProviderKind, SpeechRequest};
use super::money::format_dollars;
use crate::audio::AudioDecoder;
use crate::content::Lang;
use crate::settings::{PerLanguage, Settings};

/// The cache key's feature (version [`SPEECH_VERSION`]).
pub const SPEECH_FEATURE: &str = "speech";
/// The cache key's version of [`SPEECH_FEATURE`].
const SPEECH_VERSION: u32 = 1;
/// The request's timeout.
pub const SPEECH_TIMEOUT_S: u64 = 20;
/// The notice of a play that fell back to the OS voice.
pub const NATURAL_UNAVAILABLE: &str = "Natural voice unavailable, using the system voice.";
/// The setup run's measured charge of one sentence (tts-1, alloy), the cost line's figure until
/// this install has measured its own.
pub const MEASURED_SPEECH_MICROS: i64 = 540;

/// The format the request asks for, and the format the cache keeps.
const RESPONSE_FORMAT: &str = "mp3";

/// `content/voices.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoicesConfig {
    pub model: String,
    pub speed_honoured: bool,
    pub voices: BTreeMap<Lang, String>,
}

/// A non-empty string, trimmed.
fn text_value(value: &toml::Value, what: &str) -> Result<String, String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("{what} must be a non-empty string"))
}

/// Reads `content/voices.toml`: `model` (required), `speed` (a boolean, false when left out) and
/// one table per course language with its `voice`. Anything else is refused, so a typo is caught
/// when the file is edited rather than ignored.
pub fn parse_voices_toml(text: &str) -> Result<VoicesConfig, String> {
    let table: toml::Table = text.parse().map_err(|e| format!("not TOML: {e}"))?;
    let mut model = None;
    let mut speed_honoured = false;
    let mut voices = BTreeMap::new();
    for (key, value) in &table {
        match key.as_str() {
            "model" => model = Some(text_value(value, "model")?),
            "speed" => {
                speed_honoured = value
                    .as_bool()
                    .ok_or_else(|| "speed must be true or false".to_string())?;
            }
            code => {
                let lang: Lang = code
                    .parse()
                    .map_err(|_| format!("unknown key {code}; use model, speed, es, fr or de"))?;
                let section = value
                    .as_table()
                    .ok_or_else(|| format!("[{code}] must be a table"))?;
                if let Some(extra) = section.keys().find(|k| k.as_str() != "voice") {
                    return Err(format!("unknown key {extra} in [{code}]; use voice"));
                }
                let voice = section
                    .get("voice")
                    .ok_or_else(|| format!("[{code}] has no voice"))?;
                voices.insert(lang, text_value(voice, &format!("[{code}] voice"))?);
            }
        }
    }
    Ok(VoicesConfig {
        model: model.ok_or_else(|| "model is missing".to_string())?,
        speed_honoured,
        voices,
    })
}

const BUNDLED_VOICES: &str = include_str!("../../../../content/voices.toml");

/// The bundled `content/voices.toml`, parsed once (a unit test proves it parses).
pub fn bundled_voices() -> VoicesConfig {
    static PARSED: OnceLock<VoicesConfig> = OnceLock::new();
    PARSED
        .get_or_init(|| {
            parse_voices_toml(BUNDLED_VOICES).expect("content/voices.toml is a valid voices file")
        })
        .clone()
}

/// Where a sentence is spoken from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceSource {
    Os,
    Natural,
}

/// The voice `per` names for `lang`, when it names one.
pub(crate) fn per_language(per: &PerLanguage, lang: Lang) -> Option<&str> {
    match lang {
        Lang::Es => per.es.as_deref(),
        Lang::Fr => per.fr.as_deref(),
        Lang::De => per.de.as_deref(),
    }
    .map(str::trim)
    .filter(|voice| !voice.is_empty())
}

/// A setting's text, when it holds some.
fn some_text(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// The model and voice a natural request for `lang` uses: on publik, voices.toml's model and
/// `audio.natural_voice.<lang>` or voices.toml's voice; on own_key, `ai.own_key.speech_model` and
/// `audio.natural_voice.<lang>`, `ai.own_key.speech_voice` or voices.toml's voice, in that order.
pub fn natural_voice(
    settings: &Settings,
    natural: &VoicesConfig,
    lang: Lang,
) -> Option<(String, String)> {
    let chosen = per_language(&settings.audio.natural_voice, lang);
    let bundled = natural.voices.get(&lang).map(String::as_str);
    let (model, voice) = match settings.ai.provider.as_str() {
        "publik" => (
            Some(natural.model.as_str()).filter(|m| !m.trim().is_empty()),
            chosen.or(bundled),
        ),
        "own_key" => (
            some_text(&settings.ai.own_key.speech_model),
            chosen
                .or(some_text(&settings.ai.own_key.speech_voice))
                .or(bundled),
        ),
        _ => (None, None),
    };
    Some((model?.trim().to_string(), voice?.trim().to_string()))
}

/// Natural when `audio.voice_source = natural`, `status` is Some (a client was built: provider
/// chosen and consent given), its `needs_credit` is false, and a natural voice is configured for
/// `lang`: on `publik`, `voices.toml` has a voice for it; on `own_key`, `ai.own_key.speech_model`
/// is set; `local` never. Otherwise Os.
pub fn choose_source(
    settings: &Settings,
    status: Option<&AiStatus>,
    natural: &VoicesConfig,
    lang: Lang,
) -> VoiceSource {
    let ready = settings.audio.voice_source == "natural"
        && status.is_some_and(|status| !status.needs_credit)
        && natural_voice(settings, natural, lang).is_some();
    if ready {
        VoiceSource::Natural
    } else {
        VoiceSource::Os
    }
}

/// The canonical cache-key JSON (§6.1) with input {"text_nfc", "voice", "speed"}, and its SHA-256.
pub fn speech_key_json(
    provider: ProviderKind,
    model: &str,
    text: &str,
    voice: &str,
    speed: f64,
) -> String {
    let text_nfc: String = text.nfc().collect();
    cache_key_json(
        SPEECH_FEATURE,
        SPEECH_VERSION,
        provider,
        model,
        &json!({ "text_nfc": text_nfc, "voice": voice, "speed": speed }),
    )
}

/// The SHA-256 of [`speech_key_json`]: the name of the sentence's cached file.
pub fn speech_key(
    provider: ProviderKind,
    model: &str,
    text: &str,
    voice: &str,
    speed: f64,
) -> String {
    cache_key(&speech_key_json(provider, model, text, voice, speed))
}

/// The request of §6.7: the text NFC, `response_format` "mp3", timeout 20 s, the idempotency key of
/// the key JSON at attempt 1.
pub fn speech_request(
    provider: ProviderKind,
    model: &str,
    text: &str,
    voice: &str,
    speed: f64,
) -> SpeechRequest {
    let key_json = speech_key_json(provider, model, text, voice, speed);
    SpeechRequest {
        model: model.to_string(),
        input: text.nfc().collect(),
        voice: voice.to_string(),
        response_format: RESPONSE_FORMAT.to_string(),
        speed,
        timeout_s: SPEECH_TIMEOUT_S,
        idempotency_key: Some(idempotency_key(&key_json, 1)),
    }
}

/// `cache/audio/<key>.mp3`.
pub fn audio_file(key: &str) -> String {
    format!("cache/audio/{key}.{RESPONSE_FORMAT}")
}

/// (200, 3 × 500 × words ÷ speed) in ms, words counted as UAX #29 words.
pub fn speech_bounds_ms(text: &str, speed: f64) -> (u64, u64) {
    let words = text.unicode_words().count() as f64;
    let speed = if speed.is_finite() && speed > 0.0 {
        speed
    } else {
        1.0
    };
    (200, (3.0 * 500.0 * words / speed).round() as u64)
}

/// §6.7: Content-Type `audio/*` (case-insensitive, parameters allowed), the bytes decode, and the
/// duration lies within `speech_bounds_ms`.
pub fn speech_valid(
    reply: &AudioReply,
    text: &str,
    speed: f64,
    decoder: &dyn AudioDecoder,
) -> bool {
    let audio = reply
        .content_type
        .trim()
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("audio/"));
    if !audio {
        return false;
    }
    let (min, max) = speech_bounds_ms(text, speed);
    decoder
        .duration_ms(&reply.bytes)
        .is_some_and(|ms| (min..=max).contains(&ms))
}

/// "Natural voices cost about {format_dollars(mean)} a sentence (measured)." with the mean of
/// `charges` rounded half up to a whole micro, or MEASURED_SPEECH_MICROS when `charges` is empty.
pub fn speech_cost_line(charges: &[i64]) -> String {
    let mean = if charges.is_empty() {
        MEASURED_SPEECH_MICROS
    } else {
        let n = charges.len() as i128;
        let sum: i128 = charges.iter().map(|c| i128::from(*c)).sum();
        // floor((sum + n / 2) / n) in integers: half up, also for a negative sum.
        i64::try_from((2 * sum + n).div_euclid(2 * n)).unwrap_or(i64::MAX)
    };
    format!(
        "Natural voices cost about {} a sentence (measured).",
        format_dollars(mean)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::WavDecoder;
    use crate::audio::test_support::wav_ms;

    fn publik(source: &str) -> Settings {
        let mut settings = Settings::default();
        settings.audio.voice_source = source.into();
        settings.ai.provider = "publik".into();
        settings
    }

    fn ready() -> AiStatus {
        AiStatus {
            provider: "publik".into(),
            connected: true,
            ..AiStatus::default()
        }
    }

    #[test]
    fn the_bundled_voices_file_names_tts_1_and_alloy() {
        let bundled = bundled_voices();
        assert_eq!(bundled.model, "tts-1");
        assert!(bundled.speed_honoured);
        for lang in Lang::ALL {
            assert_eq!(bundled.voices.get(&lang).map(String::as_str), Some("alloy"));
        }
    }

    #[test]
    fn a_voices_file_is_read_strictly() {
        let parsed = parse_voices_toml("model = \"m\"\n[fr]\nvoice = \" v \"\n").unwrap();
        assert_eq!(parsed.model, "m");
        assert!(!parsed.speed_honoured, "speed is false when left out");
        assert_eq!(parsed.voices.len(), 1);
        assert_eq!(parsed.voices[&Lang::Fr], "v");
        for bad in [
            "",
            "speed = true",
            "model = \"\"",
            "model = 1",
            "model = \"m\"\nspeed = \"yes\"",
            "model = \"m\"\nvoice = \"v\"",
            "model = \"m\"\n[it]\nvoice = \"v\"",
            "model = \"m\"\n[es]\nname = \"v\"",
            "model = \"m\"\n[es]\nvoice = \"v\"\npitch = 1",
            "model = \"m\"\nes = \"v\"",
            "model = ",
        ] {
            assert!(parse_voices_toml(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn natural_voices_follow_the_provider_and_the_overrides() {
        let natural = bundled_voices();
        let mut s = publik("natural");
        assert_eq!(
            natural_voice(&s, &natural, Lang::Es),
            Some(("tts-1".to_string(), "alloy".to_string()))
        );
        s.audio.natural_voice.es = Some("nova".into());
        assert_eq!(
            natural_voice(&s, &natural, Lang::Es).unwrap().1,
            "nova",
            "audio.natural_voice overrides voices.toml"
        );
        s.ai.provider = "own_key".into();
        assert_eq!(
            natural_voice(&s, &natural, Lang::Fr),
            None,
            "no speech model"
        );
        s.ai.own_key.speech_model = Some("tts-own".into());
        s.ai.own_key.speech_voice = Some("echo".into());
        assert_eq!(
            natural_voice(&s, &natural, Lang::Fr),
            Some(("tts-own".to_string(), "echo".to_string()))
        );
        assert_eq!(natural_voice(&s, &natural, Lang::Es).unwrap().1, "nova");
        s.ai.own_key.speech_voice = None;
        assert_eq!(natural_voice(&s, &natural, Lang::De).unwrap().1, "alloy");
        for provider in ["local", "unset"] {
            s.ai.provider = provider.into();
            assert_eq!(natural_voice(&s, &natural, Lang::Es), None, "{provider}");
        }
    }

    #[test]
    fn the_source_is_natural_only_when_everything_is_ready() {
        let natural = bundled_voices();
        let s = publik("natural");
        assert_eq!(
            choose_source(&s, Some(&ready()), &natural, Lang::Es),
            VoiceSource::Natural
        );
        assert_eq!(choose_source(&s, None, &natural, Lang::Es), VoiceSource::Os);
        let credit = AiStatus {
            needs_credit: true,
            ..ready()
        };
        assert_eq!(
            choose_source(&s, Some(&credit), &natural, Lang::Es),
            VoiceSource::Os
        );
        assert_eq!(
            choose_source(&publik("os"), Some(&ready()), &natural, Lang::Es),
            VoiceSource::Os
        );
        let mut missing = natural.clone();
        missing.voices.remove(&Lang::De);
        assert_eq!(
            choose_source(&s, Some(&ready()), &missing, Lang::De),
            VoiceSource::Os
        );
        let mut local = s.clone();
        local.ai.provider = "local".into();
        assert_eq!(
            choose_source(&local, Some(&ready()), &natural, Lang::Es),
            VoiceSource::Os
        );
        assert_eq!(
            serde_json::to_string(&VoiceSource::Natural).unwrap(),
            "\"natural\""
        );
    }

    #[test]
    fn the_key_and_request_are_the_documented_ones() {
        let text = "Quiero un café con leche.";
        let key_json = speech_key_json(ProviderKind::Publik, "tts-1", text, "alloy", 0.75);
        assert_eq!(
            key_json,
            r#"{"feature":"speech","input":{"speed":0.75,"text_nfc":"Quiero un café con leche.","voice":"alloy"},"model":"tts-1","provider":"publik","v":1}"#
        );
        // A decomposed é gives the same key.
        let decomposed = "Quiero un cafe\u{301} con leche.";
        assert_eq!(
            speech_key(ProviderKind::Publik, "tts-1", decomposed, "alloy", 0.75),
            cache_key(&key_json)
        );
        let req = speech_request(ProviderKind::Publik, "tts-1", decomposed, "alloy", 0.75);
        assert_eq!(req.input, text);
        assert_eq!(req.timeout_s, 20);
        assert_eq!(req.response_format, "mp3");
        assert_eq!(
            req.idempotency_key.as_deref(),
            Some(&cache_key(&key_json)[..32])
        );
        assert_eq!(
            req.body(),
            json!({"model": "tts-1", "input": text, "voice": "alloy", "response_format": "mp3", "speed": 0.75})
        );
        let base = speech_key(ProviderKind::Publik, "tts-1", text, "alloy", 1.0);
        assert_ne!(
            base,
            speech_key(ProviderKind::Publik, "tts-1", text, "nova", 1.0)
        );
        assert_ne!(
            base,
            speech_key(ProviderKind::Publik, "tts-1", text, "alloy", 0.75)
        );
        assert_ne!(
            base,
            speech_key(ProviderKind::OwnKey, "tts-1", text, "alloy", 1.0)
        );
        assert_eq!(audio_file("abc"), "cache/audio/abc.mp3");
    }

    #[test]
    fn the_duration_bound_counts_words() {
        assert_eq!(
            speech_bounds_ms("Uno dos tres cuatro cinco seis.", 1.0),
            (200, 9_000)
        );
        assert_eq!(speech_bounds_ms("Quiero un café.", 0.75), (200, 6_000));
        assert_eq!(speech_bounds_ms("¡¿?!", 1.0), (200, 0));
        assert_eq!(speech_bounds_ms("Hola.", 0.0), (200, 1_500));
    }

    #[test]
    fn audio_is_valid_by_type_decoding_and_duration() {
        let six = "Uno dos tres cuatro cinco seis.";
        let reply = |ms: u32, content_type: &str| AudioReply {
            bytes: wav_ms(ms),
            content_type: content_type.into(),
            charge_micros: None,
        };
        assert!(speech_valid(
            &reply(2_000, "audio/wav"),
            six,
            1.0,
            &WavDecoder
        ));
        assert!(speech_valid(
            &reply(2_000, " Audio/MPEG; charset=binary"),
            six,
            1.0,
            &WavDecoder
        ));
        assert!(speech_valid(
            &reply(9_000, "audio/wav"),
            six,
            1.0,
            &WavDecoder
        ));
        assert!(speech_valid(
            &reply(200, "audio/wav"),
            six,
            1.0,
            &WavDecoder
        ));
        assert!(!speech_valid(
            &reply(10_000, "audio/wav"),
            six,
            1.0,
            &WavDecoder
        ));
        assert!(!speech_valid(
            &reply(50, "audio/wav"),
            six,
            1.0,
            &WavDecoder
        ));
        assert!(!speech_valid(
            &reply(2_000, "text/html"),
            six,
            1.0,
            &WavDecoder
        ));
        assert!(!speech_valid(&reply(2_000, "audio"), six, 1.0, &WavDecoder));
        let garbage = AudioReply {
            bytes: b"<html></html>".to_vec(),
            content_type: "audio/mpeg".into(),
            charge_micros: None,
        };
        assert!(!speech_valid(&garbage, six, 1.0, &WavDecoder));
    }

    #[test]
    fn the_cost_line_is_the_rounded_mean_or_the_setup_run_s_figure() {
        assert_eq!(
            speech_cost_line(&[600, 640]),
            "Natural voices cost about $0.00062 a sentence (measured)."
        );
        assert_eq!(
            speech_cost_line(&[]),
            "Natural voices cost about $0.00054 a sentence (measured)."
        );
        // 601 and 602 average 601.5, which rounds up.
        assert_eq!(speech_cost_line(&[601, 602]), speech_cost_line(&[602]));
        assert_eq!(speech_cost_line(&[1, 2, 2]), speech_cost_line(&[2]));
    }
}
