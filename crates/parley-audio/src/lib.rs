//! Parley's audio crate: device I/O and signal work. Nothing in it knows about lessons.
//!
//! Module 09 fills in listening: [`SystemVoices`] (the OS voices, through `tts` on Windows, macOS
//! and Android and Speech Dispatcher's `spd-say` on Linux), [`RodioPlayer`] (the speakers),
//! [`MediaDecoder`] (the duration of a clip, for the checks of natural-voice audio), and
//! [`wsola`] (time stretching for slow mode). Each implements a trait of `parley_core::audio`, so
//! the core is tested with fakes and never touches a device. Modules 10 and 12 add microphone
//! capture, resampling, silence trimming, WAV writing, local transcription and media decoding
//! (docs/ARCHITECTURE.md §2).

#![forbid(unsafe_code)]

mod decode;
mod os_tts;
mod playback;
mod wsola;

pub use decode::{MediaDecoder, decode_mono_i16};
pub use os_tts::SystemVoices;
pub use playback::RodioPlayer;
pub use wsola::wsola;
