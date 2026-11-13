//! Audio in the core (docs/modules/09-listening-and-voices.md): the traits the desktop app's audio
//! objects implement (OS voices, the player, the decoder that measures a clip), and the pure
//! helpers around them. No device code lives here: the `parley-audio` crate implements the traits
//! with `tts`, Speech Dispatcher and `rodio`, and the core's defaults ([`NoVoices`], [`NoPlayer`],
//! [`WavDecoder`]) need no device at all, so every test runs with fakes.

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ai::{VoicesConfig, bundled_voices};
use crate::api::language_name;
use crate::content::Lang;

/// An installed OS voice. `lang_tag` is its BCP 47 tag as the backend reports it (`es-ES`, `es_MX`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsVoice {
    pub id: String,
    pub name: String,
    pub lang_tag: String,
}

/// A backend's speaking rates (WinRT: 0.5, 1.0, 6.0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateRange {
    pub min: f32,
    pub normal: f32,
    pub max: f32,
}

/// Why a sound could not be played.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    /// No output device could be opened.
    NoDevice,
    /// The OS voice or the audio backend failed; the backend's own words, for logs only.
    Backend(String),
    /// The bytes are not audio the player can decode.
    Decode,
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::NoDevice => f.write_str("no audio output device"),
            AudioError::Backend(message) => write!(f, "audio backend: {message}"),
            AudioError::Decode => f.write_str("the audio does not decode"),
        }
    }
}

impl std::error::Error for AudioError {}

/// The operating system's voices.
pub trait OsVoices: Send + Sync {
    fn list(&self) -> Vec<OsVoice>;
    fn rates(&self) -> RateRange;
    /// Starts speaking and returns; a play in progress is interrupted.
    fn speak(&self, text: &str, voice: &str, rate: f32) -> Result<(), AudioError>;
    fn stop(&self);
}

/// The speakers.
pub trait Player: Send + Sync {
    /// Starts playing MP3 or WAV bytes and returns; a play in progress is interrupted. `stretch`
    /// is the length factor of WSOLA (1.0 plays as is; 1 ÷ 0.75 is slow mode stretched locally).
    fn play(&self, audio: &[u8], stretch: f32) -> Result<(), AudioError>;
    fn stop(&self);
}

/// Measures a clip.
pub trait AudioDecoder: Send + Sync {
    /// The clip's duration, or None when the bytes do not decode.
    fn duration_ms(&self, audio: &[u8]) -> Option<u64>;
}

/// RIFF WAVE with PCM data only: duration = data bytes ÷ byte rate (pure parsing, for tests and
/// as the core's default).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WavDecoder;

/// No OS voices (the default until the app sets its own; also Linux without Speech Dispatcher).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NoVoices;

/// Plays nothing and succeeds.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NoPlayer;

fn le_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn le_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// WAVE_FORMAT_PCM, and WAVE_FORMAT_EXTENSIBLE (whose sub-format then names PCM).
const FORMAT_PCM: u16 = 1;
const FORMAT_EXTENSIBLE: u16 = 0xFFFE;

impl AudioDecoder for WavDecoder {
    /// Walks the RIFF chunks: `fmt ` must come first and name PCM with a byte rate above zero;
    /// the `data` chunk's length (no more than the bytes present, for a file whose header was
    /// written before its length was known) over the byte rate, rounded to the nearest ms.
    fn duration_ms(&self, audio: &[u8]) -> Option<u64> {
        if audio.get(0..4)? != b"RIFF" || audio.get(8..12)? != b"WAVE" {
            return None;
        }
        let mut at = 12;
        let mut byte_rate: Option<u32> = None;
        while at + 8 <= audio.len() {
            let id = &audio[at..at + 4];
            let size = le_u32(audio, at + 4)? as usize;
            let body = at + 8;
            match id {
                b"fmt " => {
                    let format = le_u16(audio, body)?;
                    let pcm = match format {
                        FORMAT_PCM => true,
                        // The sub-format GUID starts with the format code, 24 bytes into the body.
                        FORMAT_EXTENSIBLE => le_u16(audio, body + 24)? == FORMAT_PCM,
                        _ => false,
                    };
                    if !pcm {
                        return None;
                    }
                    byte_rate = Some(le_u32(audio, body + 8)?).filter(|rate| *rate > 0);
                }
                b"data" => {
                    let rate = u64::from(byte_rate?);
                    let present = audio.len() - body;
                    let data = size.min(present) as u64;
                    return Some((data * 1000 + rate / 2) / rate);
                }
                _ => {}
            }
            // Chunks are padded to an even length.
            at = body.checked_add(size)?.checked_add(size % 2)?;
        }
        None
    }
}

impl OsVoices for NoVoices {
    fn list(&self) -> Vec<OsVoice> {
        Vec::new()
    }

    fn rates(&self) -> RateRange {
        RateRange {
            min: 0.5,
            normal: 1.0,
            max: 2.0,
        }
    }

    fn speak(&self, _text: &str, _voice: &str, _rate: f32) -> Result<(), AudioError> {
        Err(AudioError::Backend("no OS voices".to_string()))
    }

    fn stop(&self) {}
}

impl Player for NoPlayer {
    fn play(&self, _audio: &[u8], _stretch: f32) -> Result<(), AudioError> {
        Ok(())
    }

    fn stop(&self) {}
}

/// What the service plays through and with.
#[derive(Clone)]
pub struct Audio {
    pub os: Arc<dyn OsVoices>,
    pub player: Arc<dyn Player>,
    pub decoder: Arc<dyn AudioDecoder>,
    /// `content/voices.toml`, parsed (tests may replace it).
    pub natural: VoicesConfig,
}

impl Default for Audio {
    /// No OS voices, a player that plays nothing, the WAV decoder and the bundled voices.toml.
    fn default() -> Self {
        Audio {
            os: Arc::new(NoVoices),
            player: Arc::new(NoPlayer),
            decoder: Arc::new(WavDecoder),
            natural: bundled_voices(),
        }
    }
}

impl fmt::Debug for Audio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Audio")
            .field("natural", &self.natural)
            .finish_non_exhaustive()
    }
}

/// The voices whose tag's primary subtag is `lang` (case-insensitive; `-` or `_`), in list order.
pub fn voices_for(voices: &[OsVoice], lang: Lang) -> Vec<OsVoice> {
    voices
        .iter()
        .filter(|voice| {
            voice
                .lang_tag
                .split(['-', '_'])
                .next()
                .is_some_and(|primary| primary.trim().eq_ignore_ascii_case(lang.as_str()))
        })
        .cloned()
        .collect()
}

/// normal, or normal × slow_speed clamped to [min, max] when `slow`.
pub fn os_rate(range: RateRange, slow: bool, slow_speed: f64) -> f32 {
    if !slow {
        return range.normal;
    }
    let rate = (f64::from(range.normal) * slow_speed) as f32;
    // Not `clamp`, which panics on a backend whose range is reported upside down.
    rate.max(range.min).min(range.max)
}

/// The desktop platforms a voice can be missing on; each has its own way to add one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Linux,
}

impl Platform {
    /// The platform this build is for (a compile-time fact, not a reading of the environment).
    /// Every platform other than Windows and macOS gets the Linux help.
    pub fn current() -> Platform {
        if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }
}

/// The `no_voice` message for `lang` on `platform` (docs/modules/09-listening-and-voices.md,
/// Messages).
pub fn no_voice_message(lang: Lang, platform: Platform) -> String {
    let language = language_name(lang);
    match platform {
        Platform::Windows => format!(
            "No {language} voice is installed. On Windows, open Settings > Time & language > Speech and add a voice."
        ),
        Platform::MacOs => format!(
            "No {language} voice is installed. On macOS, open System Settings > Accessibility > Spoken Content > System voice > Manage Voices."
        ),
        Platform::Linux => format!(
            "No {language} voice is installed. On Linux, install Speech Dispatcher and a {language} voice, or turn on natural voices."
        ),
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Generated WAV bytes for the core's tests.

    /// A 16 kHz mono 16-bit PCM WAV of `ms` milliseconds of silence.
    pub fn wav_ms(ms: u32) -> Vec<u8> {
        let rate: u32 = 16_000;
        let data_len = rate / 1000 * ms * 2;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.resize(44 + data_len as usize, 0);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::wav_ms;
    use super::*;

    fn voice(id: &str, tag: &str) -> OsVoice {
        OsVoice {
            id: id.into(),
            name: id.into(),
            lang_tag: tag.into(),
        }
    }

    #[test]
    fn the_wav_decoder_measures_pcm_and_refuses_anything_else() {
        assert_eq!(WavDecoder.duration_ms(&wav_ms(1500)), Some(1500));
        assert_eq!(WavDecoder.duration_ms(&wav_ms(50)), Some(50));
        assert_eq!(WavDecoder.duration_ms(&wav_ms(0)), Some(0));
        assert_eq!(WavDecoder.duration_ms(b"<html>not audio</html>"), None);
        assert_eq!(WavDecoder.duration_ms(b""), None);
        // A data chunk claiming more than is there counts what is there.
        let mut long = wav_ms(1000);
        long[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(WavDecoder.duration_ms(&long), Some(1000));
        // IEEE float (format 3) is not PCM.
        let mut float = wav_ms(1000);
        float[20..22].copy_from_slice(&3u16.to_le_bytes());
        assert_eq!(WavDecoder.duration_ms(&float), None);
        // A byte rate of zero cannot be divided by.
        let mut zero = wav_ms(1000);
        zero[28..32].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(WavDecoder.duration_ms(&zero), None);
        // An unknown chunk before the data is skipped, padding included.
        let plain = wav_ms(500);
        let mut with_list = plain[..36].to_vec();
        with_list.extend_from_slice(b"LIST");
        with_list.extend_from_slice(&3u32.to_le_bytes());
        with_list.extend_from_slice(&[1, 2, 3, 0]);
        with_list.extend_from_slice(&plain[36..]);
        assert_eq!(WavDecoder.duration_ms(&with_list), Some(500));
    }

    #[test]
    fn voices_are_chosen_by_their_primary_subtag() {
        let voices = vec![
            voice("a", "es-ES"),
            voice("b", "fr-FR"),
            voice("c", "es_MX"),
            voice("d", "ES"),
            voice("e", "est-EE"),
            voice("f", ""),
        ];
        let ids = |lang| {
            voices_for(&voices, lang)
                .into_iter()
                .map(|v| v.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(Lang::Es), ["a", "c", "d"]);
        assert_eq!(ids(Lang::Fr), ["b"]);
        assert!(ids(Lang::De).is_empty());
    }

    #[test]
    fn the_os_rate_is_the_normal_rate_scaled_and_clamped_when_slow() {
        let winrt = RateRange {
            min: 0.5,
            normal: 1.0,
            max: 6.0,
        };
        assert_eq!(os_rate(winrt, false, 0.75), 1.0);
        assert!((os_rate(winrt, true, 0.75) - 0.75).abs() < 1e-6);
        let narrow = RateRange {
            min: 0.8,
            normal: 1.0,
            max: 2.0,
        };
        assert!((os_rate(narrow, true, 0.75) - 0.8).abs() < 1e-6);
        assert_eq!(os_rate(narrow, false, 0.5), 1.0);
        let upside_down = RateRange {
            min: 2.0,
            normal: 1.0,
            max: 0.5,
        };
        assert!(os_rate(upside_down, true, 0.75).is_finite());
    }

    #[test]
    fn each_platform_has_its_no_voice_help() {
        assert_eq!(
            no_voice_message(Lang::Es, Platform::Windows),
            "No Spanish voice is installed. On Windows, open Settings > Time & language > Speech and add a voice."
        );
        assert_eq!(
            no_voice_message(Lang::Fr, Platform::MacOs),
            "No French voice is installed. On macOS, open System Settings > Accessibility > Spoken Content > System voice > Manage Voices."
        );
        assert_eq!(
            no_voice_message(Lang::De, Platform::Linux),
            "No German voice is installed. On Linux, install Speech Dispatcher and a German voice, or turn on natural voices."
        );
        let expected = if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        };
        assert_eq!(Platform::current(), expected);
    }

    #[test]
    fn the_defaults_need_no_device() {
        let audio = Audio::default();
        assert!(audio.os.list().is_empty());
        assert!(audio.os.speak("Hola.", "x", 1.0).is_err());
        assert_eq!(audio.player.play(b"anything", 1.0), Ok(()));
        assert_eq!(audio.decoder.duration_ms(&wav_ms(200)), Some(200));
        assert_eq!(audio.natural.model, "tts-1");
        assert!(format!("{audio:?}").contains("tts-1"));
    }
}
