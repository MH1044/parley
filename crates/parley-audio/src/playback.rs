//! The speakers, through rodio (docs/PUBLIK-INTEGRATION.md §7): MP3 and WAV bytes as they are, or
//! stretched by WSOLA for slow mode.
//!
//! rodio's output stream may not move between threads on every platform, so one thread owns it
//! and receives plays and stops over a channel. The stream is opened at the first play, not at
//! start: a computer with no audio device starts normally, and only a play reports the missing
//! device. Each play replaces the one before; the end of a clip is announced once, and never for
//! a clip that was stopped or replaced.

use std::io::Cursor;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use parley_core::audio::{AudioError, Player};
use rodio::buffer::SamplesBuffer;
use rodio::source::EmptyCallback;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink};

use crate::decode::decode_mono_i16;
use crate::wsola::wsola;

/// Called when a clip ends by itself.
type OnEnd = Arc<dyn Fn() + Send + Sync>;

enum Command {
    Play {
        audio: Vec<u8>,
        stretch: f32,
        started: Sender<Result<(), AudioError>>,
    },
    Stop,
}

/// [`Player`] through rodio: a thread owning the output stream, opened at the first play.
pub struct RodioPlayer {
    commands: Sender<Command>,
}

impl std::fmt::Debug for RodioPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RodioPlayer").finish_non_exhaustive()
    }
}

impl RodioPlayer {
    /// Starts the playback thread; `on_end` runs (on the audio thread) when a clip ends by
    /// itself.
    pub fn new(on_end: Arc<dyn Fn() + Send + Sync>) -> RodioPlayer {
        let (commands, receiver) = mpsc::channel();
        // A thread that cannot be started leaves a player whose every play reports no device.
        let _ = std::thread::Builder::new()
            .name("parley-playback".into())
            .spawn(move || serve(receiver, on_end));
        RodioPlayer { commands }
    }
}

impl Player for RodioPlayer {
    fn play(&self, audio: &[u8], stretch: f32) -> Result<(), AudioError> {
        let (started, result) = mpsc::channel();
        self.commands
            .send(Command::Play {
                audio: audio.to_vec(),
                stretch,
                started,
            })
            .map_err(|_| AudioError::NoDevice)?;
        result.recv().unwrap_or(Err(AudioError::NoDevice))
    }

    fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }
}

/// A clip ready to play: decoded as it is, or stretched.
enum Clip {
    AsIs(Decoder<Cursor<Vec<u8>>>),
    Stretched(SamplesBuffer),
}

/// Decodes `audio`, stretching it by `stretch` unless that is 1 (or not a usable factor).
fn prepare(audio: Vec<u8>, stretch: f32) -> Result<Clip, AudioError> {
    let as_is = !stretch.is_finite() || stretch <= 0.0 || (stretch - 1.0).abs() < 1e-6;
    if as_is {
        return Decoder::new(Cursor::new(audio))
            .map(Clip::AsIs)
            .map_err(|_| AudioError::Decode);
    }
    let (samples, rate) = decode_mono_i16(&audio).ok_or(AudioError::Decode)?;
    let stretched = wsola(&samples, rate, stretch);
    let rate = NonZero::new(rate).ok_or(AudioError::Decode)?;
    let mono = NonZero::new(1).ok_or(AudioError::Decode)?;
    let data: Vec<rodio::Sample> = stretched
        .iter()
        .map(|s| rodio::Sample::from(*s) / 32_768.0)
        .collect();
    Ok(Clip::Stretched(SamplesBuffer::new(mono, rate, data)))
}

/// Starts `audio` on the stream (opening it the first time), followed by the end callback of
/// play number `number`.
fn start(
    sink: &mut Option<MixerDeviceSink>,
    audio: Vec<u8>,
    stretch: f32,
    number: u64,
    current: &Arc<AtomicU64>,
    on_end: &OnEnd,
) -> Result<rodio::Player, AudioError> {
    let clip = prepare(audio, stretch)?;
    if sink.is_none() {
        let mut opened =
            DeviceSinkBuilder::open_default_sink().map_err(|_| AudioError::NoDevice)?;
        opened.log_on_drop(false);
        *sink = Some(opened);
    }
    let Some(sink) = sink.as_ref() else {
        return Err(AudioError::NoDevice);
    };
    let player = rodio::Player::connect_new(sink.mixer());
    match clip {
        Clip::AsIs(decoder) => player.append(decoder),
        Clip::Stretched(buffer) => player.append(buffer),
    }
    let current = current.clone();
    let on_end = on_end.clone();
    player.append(EmptyCallback::new(Box::new(move || {
        if current.load(Ordering::SeqCst) == number {
            on_end();
        }
    })));
    Ok(player)
}

/// The playback thread: owns the stream and the clip playing, until the player is dropped.
fn serve(commands: Receiver<Command>, on_end: OnEnd) {
    let mut sink: Option<MixerDeviceSink> = None;
    let mut playing: Option<rodio::Player> = None;
    // The number of the latest play or stop; a clip announces its end only while it is current.
    let current = Arc::new(AtomicU64::new(0));
    for command in commands {
        let number = current.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(player) = playing.take() {
            player.stop();
        }
        if let Command::Play {
            audio,
            stretch,
            started,
        } = command
        {
            let result = match start(&mut sink, audio, stretch, number, &current, &on_end) {
                Ok(player) => {
                    playing = Some(player);
                    Ok(())
                }
                Err(error) => Err(error),
            };
            let _ = started.send(result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::tests::wav;

    #[test]
    fn clips_are_decoded_as_they_are_or_stretched() {
        let samples: Vec<i16> = (0..16_000)
            .map(|i| if i % 40 < 20 { 4_000 } else { -4_000 })
            .collect();
        let bytes = wav(&samples, 1, 16_000);
        assert!(matches!(prepare(bytes.clone(), 1.0), Ok(Clip::AsIs(_))));
        assert!(matches!(
            prepare(bytes.clone(), f32::NAN),
            Ok(Clip::AsIs(_))
        ));
        match prepare(bytes, 1.0 / 0.75) {
            Ok(Clip::Stretched(buffer)) => {
                use rodio::Source;
                let ms = buffer.total_duration().unwrap().as_millis();
                assert!((1_330..=1_336).contains(&ms), "{ms} ms");
            }
            _ => panic!("a stretched clip"),
        }
        assert_eq!(
            prepare(b"not audio".to_vec(), 1.0).err(),
            Some(AudioError::Decode)
        );
        assert_eq!(
            prepare(b"not audio".to_vec(), 1.5).err(),
            Some(AudioError::Decode)
        );
    }
}
