//! Parley's audio crate: device I/O and signal work. Nothing in it knows about lessons.
//!
//! Module 09 fills in listening: [`SystemVoices`] (the OS voices, through `tts` on Windows, macOS
//! and Android and Speech Dispatcher's `spd-say` on Linux), [`RodioPlayer`] (the speakers),
//! [`MediaDecoder`] (the duration of a clip, for the checks of natural-voice audio), and
//! [`wsola`] (time stretching for slow mode). Module 10 adds speaking: [`Microphone`] over
//! [`CpalInput`] (the default input through cpal), [`downmix`], [`level_dbfs`],
//! [`resample_to_16k`] (rubato), [`trim`] and [`wav_16k_mono`] (hound), and, with the feature
//! `local-whisper`, `WhisperLocal` (whisper.cpp through whisper-rs). Each implements a trait of
//! `parley_core::audio`, so the core is tested with fakes and never touches a device. Module 12
//! adds media decoding (docs/ARCHITECTURE.md §2).

#![forbid(unsafe_code)]

mod capture;
mod decode;
mod os_tts;
mod playback;
mod resample;
mod trim;
mod wav;
#[cfg(feature = "local-whisper")]
mod whisper_local;
mod wsola;

pub use capture::{
    CpalInput, InputDevice, InputFormat, InputSink, Microphone, downmix, level_dbfs,
};
pub use decode::{MediaDecoder, decode_mono_i16};
pub use os_tts::SystemVoices;
pub use playback::RodioPlayer;
pub use resample::{TAKE_RATE_HZ, resample_to_16k};
pub use trim::{Trimmed, trim};
pub use wav::wav_16k_mono;
#[cfg(feature = "local-whisper")]
pub use whisper_local::WhisperLocal;
pub use wsola::wsola;
