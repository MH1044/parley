//! Local transcription through whisper.cpp (`whisper-rs`), behind the feature `local-whisper`
//! (docs/modules/10-speaking-and-pronunciation.md, Design notes).
//!
//! One model context is kept loaded and reloaded only when the chosen model changes. Decoding is
//! greedy at temperature 0 with no fallback, in the course language, with no translation, no
//! timestamps and no context from earlier takes, and non-speech tokens suppressed, so the same
//! take gives the same text. A take shorter than 1 s is padded with silence to 1 s.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once, PoisonError};

use parley_core::audio::LocalTranscriber;
use parley_core::content::Lang;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::resample::TAKE_RATE_HZ;

/// The most threads a transcription uses.
const MAX_THREADS: usize = 4;

/// [`LocalTranscriber`] through whisper.cpp.
pub struct WhisperLocal {
    loaded: Mutex<Option<(PathBuf, WhisperContext)>>,
}

impl std::fmt::Debug for WhisperLocal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WhisperLocal").finish_non_exhaustive()
    }
}

impl WhisperLocal {
    /// No model is loaded until the first transcription. whisper.cpp's own logging is routed
    /// away from the console.
    pub fn new() -> WhisperLocal {
        static HOOKS: Once = Once::new();
        HOOKS.call_once(whisper_rs::install_logging_hooks);
        WhisperLocal {
            loaded: Mutex::new(None),
        }
    }
}

impl Default for WhisperLocal {
    fn default() -> Self {
        WhisperLocal::new()
    }
}

/// The samples of a 16 kHz mono 16-bit WAV, as floats in −1..1.
fn read_take(wav: &[u8]) -> Result<Vec<f32>, String> {
    let mut reader = hound::WavReader::new(Cursor::new(wav)).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    if spec.sample_rate != TAKE_RATE_HZ
        || spec.channels != 1
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err("the take is not 16 kHz mono 16-bit PCM".to_string());
    }
    reader
        .samples::<i16>()
        .map(|sample| {
            sample
                .map(|s| f32::from(s) / 32_768.0)
                .map_err(|e| e.to_string())
        })
        .collect()
}

/// min(4, the available parallelism).
fn threads() -> i32 {
    let available = std::thread::available_parallelism().map_or(1, usize::from);
    available.clamp(1, MAX_THREADS) as i32
}

impl LocalTranscriber for WhisperLocal {
    fn transcribe(&self, model: &Path, wav: &[u8], lang: Lang) -> Result<String, String> {
        let mut audio = read_take(wav)?;
        let one_second = TAKE_RATE_HZ as usize;
        if audio.len() < one_second {
            audio.resize(one_second, 0.0);
        }
        let mut loaded = self.loaded.lock().unwrap_or_else(PoisonError::into_inner);
        if loaded.as_ref().map(|(path, _)| path.as_path()) != Some(model) {
            // The model in memory is dropped before the next one loads.
            *loaded = None;
            let context =
                WhisperContext::new_with_params(model, WhisperContextParameters::default())
                    .map_err(|e| e.to_string())?;
            *loaded = Some((model.to_path_buf(), context));
        }
        let Some((_, context)) = loaded.as_ref() else {
            return Err("no model is loaded".to_string());
        };
        let mut state = context.create_state().map_err(|e| e.to_string())?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(lang.as_str()));
        params.set_translate(false);
        params.set_no_context(true);
        params.set_no_timestamps(true);
        params.set_temperature(0.0);
        params.set_temperature_inc(0.0);
        params.set_suppress_nst(true);
        params.set_n_threads(threads());
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        state.full(params, &audio).map_err(|e| e.to_string())?;
        let mut text = String::new();
        for segment in state.as_iter() {
            text.push_str(&segment.to_str_lossy().map_err(|e| e.to_string())?);
        }
        Ok(text.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_must_be_16_khz_mono_pcm() {
        let wav = crate::wav::wav_16k_mono(&[0, 16_384, -32_768]);
        assert_eq!(read_take(&wav).unwrap(), [0.0, 0.5, -1.0]);
        assert!(read_take(b"not a wav").is_err());
        assert!((1..=4).contains(&threads()));
    }

    #[test]
    fn a_missing_model_is_an_error_not_a_panic() {
        let whisper = WhisperLocal::new();
        let wav = crate::wav::wav_16k_mono(&[0; 1_600]);
        let missing = Path::new("no/such/ggml-model.bin");
        assert!(whisper.transcribe(missing, &wav, Lang::Es).is_err());
    }
}
