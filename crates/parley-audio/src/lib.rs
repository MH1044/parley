//! Parley's audio crate: device I/O and signal work. Nothing in it knows about lessons.
//!
//! Modules 09, 10 and 12 fill it in: playback and OS voices, microphone capture, resampling,
//! silence trimming, WAV writing, local transcription behind the `local-whisper` feature, and media
//! decoding (docs/ARCHITECTURE.md §2).
//!
//! Nothing in this crate exists yet at module 00.
