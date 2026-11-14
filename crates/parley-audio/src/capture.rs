//! The microphone (docs/modules/10-speaking-and-pronunciation.md): [`CpalInput`], the default
//! input through cpal, and [`Microphone`], which turns any [`InputDevice`] into the core's
//! [`Capture`]: mono, a level every 50 ms, a limit, then 16 kHz, cut, trimmed and written as WAV.
//!
//! cpal's input stream may not move between threads on every platform, so one thread owns it and
//! receives opens and closes over a channel, as the playback thread owns the output. The device
//! is opened at `start`, not when the app starts: a computer with no microphone starts normally.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Data, SampleFormat};
use parley_core::audio::{Capture, CaptureError, Take};

use crate::resample::{TAKE_RATE_HZ, resample_to_16k};
use crate::trim::trim;
use crate::wav::wav_16k_mono;

/// The rate and channel count of the chunks a device delivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputFormat {
    pub rate_hz: u32,
    pub channels: u16,
}

/// Where the chunks of a take go: called from any thread, with the chunk's format.
pub type InputSink = Box<dyn FnMut(InputFormat, &[i16]) + Send>;

/// An input device: the real microphone, or a fake in tests.
pub trait InputDevice: Send + Sync {
    fn available(&self) -> bool;
    /// Delivers interleaved 16-bit chunks with their format until `close`, from any thread.
    fn open(&self, sink: InputSink) -> Result<(), CaptureError>;
    fn close(&self);
}

/// Each frame's mean, rounded toward zero; a trailing partial frame is dropped.
pub fn downmix(interleaved: &[i16], channels: u16) -> Vec<i16> {
    let channels = usize::from(channels.max(1));
    if channels == 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|s| i32::from(*s)).sum();
            // Integer division rounds toward zero; the mean of i16 values is an i16.
            (sum / channels as i32) as i16
        })
        .collect()
}

/// 20 log10(rms ÷ 32,768) clamped to −60..0; −60 for silence or an empty window. RMS, not peak,
/// so the meter follows loudness: a full-scale square reads 0 dBFS, silence the meter's floor.
pub fn level_dbfs(window: &[i16]) -> f32 {
    if window.is_empty() {
        return -60.0;
    }
    let energy: f64 = window.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    let rms = (energy / window.len() as f64).sqrt();
    if rms <= 0.0 {
        return -60.0;
    }
    (20.0 * (rms / 32_768.0).log10()).clamp(-60.0, 0.0) as f32
}

/// A float sample as 16 bits: round(clamp(x, −1, 1) × 32,767).
fn float_to_i16(x: f64) -> i16 {
    (x.clamp(-1.0, 1.0) * 32_767.0).round() as i16
}

/// A chunk in any of cpal's integer or float formats as 16-bit samples: floats scaled, unsigned
/// formats moved to zero (u16 as x − 32,768), wider integers shifted to 16 bits. False for a
/// format that is not sampled audio (DSD).
pub(crate) fn to_i16(data: &Data, out: &mut Vec<i16>) -> bool {
    fn put<T: cpal::SizedSample>(
        data: &Data,
        out: &mut Vec<i16>,
        convert: impl Fn(T) -> i16,
    ) -> bool {
        match data.as_slice::<T>() {
            Some(samples) => {
                out.extend(samples.iter().map(|s| convert(*s)));
                true
            }
            None => false,
        }
    }
    match data.sample_format() {
        SampleFormat::I8 => put(data, out, |x: i8| i16::from(x) << 8),
        SampleFormat::I16 => put(data, out, |x: i16| x),
        SampleFormat::I24 => put(data, out, |x: cpal::I24| (x.inner() >> 8) as i16),
        SampleFormat::I32 => put(data, out, |x: i32| (x >> 16) as i16),
        SampleFormat::I64 => put(data, out, |x: i64| (x >> 48) as i16),
        SampleFormat::U8 => put(data, out, |x: u8| (i16::from(x) - 128) << 8),
        SampleFormat::U16 => put(data, out, |x: u16| (i32::from(x) - 32_768) as i16),
        SampleFormat::U24 => put(data, out, |x: cpal::U24| {
            ((x.inner() - (1 << 23)) >> 8) as i16
        }),
        SampleFormat::U32 => put(data, out, |x: u32| {
            ((i64::from(x) - (1 << 31)) >> 16) as i16
        }),
        SampleFormat::U64 => put(data, out, |x: u64| ((x >> 48) as i32 - 32_768) as i16),
        SampleFormat::F32 => put(data, out, |x: f32| float_to_i16(f64::from(x))),
        SampleFormat::F64 => put(data, out, float_to_i16),
        _ => false,
    }
}

/// A cpal error as a capture error: a refusal by the operating system (its text says "denied",
/// or carries Windows' E_ACCESSDENIED, 0x80070005) is `Permission`; anything else `Backend`.
pub(crate) fn classify(error: &cpal::Error) -> CaptureError {
    let text = error.to_string();
    let lower = text.to_ascii_lowercase();
    if error.kind() == cpal::ErrorKind::PermissionDenied
        || lower.contains("denied")
        || lower.contains("0x80070005")
    {
        CaptureError::Permission
    } else {
        CaptureError::Backend(text)
    }
}

enum Command {
    Available(Sender<bool>),
    Open {
        sink: InputSink,
        opened: Sender<Result<(), CaptureError>>,
    },
    Close(Sender<()>),
}

/// The default input device through cpal: a thread owning the stream, opened at each take.
pub struct CpalInput {
    commands: Sender<Command>,
}

impl std::fmt::Debug for CpalInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CpalInput").finish_non_exhaustive()
    }
}

impl CpalInput {
    /// Starts the capture thread; nothing is opened until a take starts.
    pub fn new() -> CpalInput {
        let (commands, receiver) = mpsc::channel();
        // A thread that cannot be started leaves an input that reports no device.
        let _ = std::thread::Builder::new()
            .name("parley-capture".into())
            .spawn(move || serve(receiver));
        CpalInput { commands }
    }
}

impl Default for CpalInput {
    fn default() -> Self {
        CpalInput::new()
    }
}

/// Opens the default input in its default configuration and starts it; every callback's samples
/// go to `sink` as 16-bit samples with the stream's format.
fn open_stream(mut sink: InputSink) -> Result<cpal::Stream, CaptureError> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or(CaptureError::NoDevice)?;
    let config = device.default_input_config().map_err(|e| classify(&e))?;
    let format = InputFormat {
        rate_hz: config.sample_rate(),
        channels: config.channels(),
    };
    let mut samples: Vec<i16> = Vec::new();
    let stream = device
        .build_input_stream_raw(
            config.config(),
            config.sample_format(),
            move |data: &Data, _: &cpal::InputCallbackInfo| {
                samples.clear();
                if to_i16(data, &mut samples) {
                    sink(format, &samples);
                }
            },
            // A device that fails mid-take delivers no more input; stop reports what was heard.
            |_| {},
            None,
        )
        .map_err(|e| classify(&e))?;
    stream.play().map_err(|e| classify(&e))?;
    Ok(stream)
}

/// The capture thread: owns the stream of the take in progress, until the input is dropped.
fn serve(commands: Receiver<Command>) {
    // Held only so that the stream lives, and runs, until the next open or close drops it.
    let mut stream: Option<cpal::Stream> = None;
    for command in commands {
        match command {
            Command::Available(reply) => {
                let present = cpal::default_host().default_input_device().is_some();
                let _ = reply.send(present);
            }
            Command::Open { sink, opened } => {
                drop(stream.take());
                let result = open_stream(sink).map(|opened_stream| {
                    let _ = stream.insert(opened_stream);
                });
                let _ = opened.send(result);
            }
            Command::Close(closed) => {
                // Dropping the stream stops its callbacks before the reply goes back.
                drop(stream.take());
                let _ = closed.send(());
            }
        }
    }
}

impl InputDevice for CpalInput {
    fn available(&self) -> bool {
        let (reply, answer) = mpsc::channel();
        self.commands.send(Command::Available(reply)).is_ok() && answer.recv().unwrap_or(false)
    }

    fn open(&self, sink: InputSink) -> Result<(), CaptureError> {
        let (opened, result) = mpsc::channel();
        self.commands
            .send(Command::Open { sink, opened })
            .map_err(|_| CaptureError::Backend("the capture thread is not running".into()))?;
        result
            .recv()
            .unwrap_or_else(|_| Err(CaptureError::Backend("the capture thread stopped".into())))
    }

    fn close(&self) {
        let (closed, done) = mpsc::channel();
        if self.commands.send(Command::Close(closed)).is_ok() {
            let _ = done.recv();
        }
    }
}

/// The take being recorded, shared between `start`, `stop` and the device's sink.
#[derive(Default)]
struct Recorder {
    /// The number of the current take; a sink of an earlier one adds nothing.
    take: u64,
    recording: bool,
    max_ms: u32,
    format: Option<InputFormat>,
    /// Mono samples at the device's rate, at most the limit.
    frames: Vec<i16>,
}

/// The core's [`Capture`] over an input device.
pub struct Microphone<D: InputDevice> {
    device: D,
    recorder: Arc<Mutex<Recorder>>,
}

impl Microphone<CpalInput> {
    /// The computer's default input.
    pub fn system() -> Microphone<CpalInput> {
        Microphone::new(CpalInput::new())
    }
}

impl<D: InputDevice> Microphone<D> {
    pub fn new(device: D) -> Microphone<D> {
        Microphone {
            device,
            recorder: Arc::new(Mutex::new(Recorder::default())),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Recorder> {
        self.recorder.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// ⌈max_ms × rate ÷ 1000⌉ frames.
fn limit_frames(max_ms: u32, rate_hz: u32) -> usize {
    let frames = (u64::from(max_ms) * u64::from(rate_hz)).div_ceil(1000);
    usize::try_from(frames).unwrap_or(usize::MAX)
}

/// The sink of take number `take`: mixes each chunk to mono, keeps up to the limit, reports the
/// level of every whole 50 ms window (carried across chunks) and the limit once. The callbacks
/// run after the recorder is released.
fn sink_for(
    recorder: Arc<Mutex<Recorder>>,
    take: u64,
    mut on_level: Box<dyn FnMut(f32) + Send>,
    on_limit: Box<dyn FnOnce() + Send>,
) -> InputSink {
    let mut on_limit = Some(on_limit);
    let mut window: Vec<i16> = Vec::new();
    Box::new(move |format: InputFormat, chunk: &[i16]| {
        let mono = downmix(chunk, format.channels);
        let full = {
            let mut rec = recorder.lock().unwrap_or_else(PoisonError::into_inner);
            if rec.take != take || !rec.recording {
                return;
            }
            let format = *rec.format.get_or_insert(format);
            let limit = limit_frames(rec.max_ms, format.rate_hz);
            let room = limit.saturating_sub(rec.frames.len());
            rec.frames.extend_from_slice(&mono[..room.min(mono.len())]);
            rec.frames.len() >= limit
        };
        let window_len = (format.rate_hz / 20).max(1) as usize;
        for sample in mono {
            window.push(sample);
            if window.len() == window_len {
                on_level(level_dbfs(&window));
                window.clear();
            }
        }
        if full && let Some(limit) = on_limit.take() {
            limit();
        }
    })
}

impl<D: InputDevice> Capture for Microphone<D> {
    fn has_device(&self) -> bool {
        self.device.available()
    }

    fn start(
        &self,
        max_ms: u32,
        on_level: Box<dyn FnMut(f32) + Send>,
        on_limit: Box<dyn FnOnce() + Send>,
    ) -> Result<(), CaptureError> {
        let was_recording = self.lock().recording;
        if was_recording {
            // A capture in progress is discarded.
            self.lock().recording = false;
            self.device.close();
        }
        if !self.device.available() {
            return Err(CaptureError::NoDevice);
        }
        let take = {
            let mut rec = self.lock();
            rec.take += 1;
            rec.recording = true;
            rec.max_ms = max_ms;
            rec.format = None;
            rec.frames.clear();
            rec.take
        };
        // The device may deliver chunks before `open` returns, so nothing is locked here.
        let sink = sink_for(self.recorder.clone(), take, on_level, on_limit);
        if let Err(e) = self.device.open(sink) {
            self.lock().recording = false;
            return Err(e);
        }
        Ok(())
    }

    fn stop(&self) -> Result<Take, CaptureError> {
        let (format, frames, max_ms) = {
            let mut rec = self.lock();
            if !rec.recording {
                return Err(CaptureError::NotRecording);
            }
            rec.recording = false;
            rec.take += 1;
            (
                rec.format.take(),
                std::mem::take(&mut rec.frames),
                rec.max_ms,
            )
        };
        self.device.close();
        let Some(format) = format else {
            return Err(CaptureError::TooQuiet);
        };
        let mut samples = resample_to_16k(&frames, format.rate_hz);
        let limit = usize::try_from(u64::from(max_ms) * u64::from(TAKE_RATE_HZ) / 1000)
            .unwrap_or(usize::MAX);
        samples.truncate(limit);
        let kept = trim(&samples).ok_or(CaptureError::TooQuiet)?;
        let take = &samples[kept.start..kept.end];
        let duration_ms =
            (take.len() as u64 * 1000 + u64::from(TAKE_RATE_HZ) / 2) / u64::from(TAKE_RATE_HZ);
        Ok(Take {
            wav: wav_16k_mono(take),
            duration_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A device that feeds its chunks to the sink as soon as it is opened.
    struct Feeding {
        format: InputFormat,
        chunks: Vec<Vec<i16>>,
        closes: AtomicUsize,
    }

    impl InputDevice for Feeding {
        fn available(&self) -> bool {
            true
        }
        fn open(&self, mut sink: InputSink) -> Result<(), CaptureError> {
            for chunk in &self.chunks {
                sink(self.format, chunk);
            }
            Ok(())
        }
        fn close(&self) {
            self.closes.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn tone(len: usize, amplitude: i16) -> Vec<i16> {
        (0..len)
            .map(|i| if i % 40 < 20 { amplitude } else { -amplitude })
            .collect()
    }

    fn mic(rate_hz: u32, channels: u16, samples: Vec<i16>, chunk: usize) -> Microphone<Feeding> {
        Microphone::new(Feeding {
            format: InputFormat { rate_hz, channels },
            chunks: samples.chunks(chunk).map(<[i16]>::to_vec).collect(),
            closes: AtomicUsize::new(0),
        })
    }

    #[test]
    fn mixing_levels_and_conversions() {
        assert_eq!(downmix(&[1000, -1000, 3, 4, 5], 2), [0, 3]);
        assert_eq!(downmix(&[-3, 0], 2), [-1]);
        assert_eq!(downmix(&[7, 8], 1), [7, 8]);
        assert_eq!(level_dbfs(&[]), -60.0);
        assert_eq!(level_dbfs(&[0; 800]), -60.0);
        assert!(level_dbfs(&tone(800, 32_767)).abs() < 0.1);
        assert!((level_dbfs(&tone(800, 16_384)) + 6.02).abs() < 0.01);
        assert_eq!(level_dbfs(&[1; 10]), -60.0, "clamped at the floor");
        assert_eq!(float_to_i16(1.5), 32_767);
        assert_eq!(float_to_i16(-1.0), -32_767);
        assert_eq!(float_to_i16(0.5), 16_384);
        assert_eq!(limit_frames(15_000, 44_100), 661_500);
        assert_eq!(limit_frames(1, 44_100), 45);
    }

    #[test]
    fn permission_errors_are_told_apart() {
        let denied = cpal::Error::with_message(cpal::ErrorKind::BackendError, "Access is denied.");
        assert_eq!(classify(&denied), CaptureError::Permission);
        let hresult =
            cpal::Error::with_message(cpal::ErrorKind::BackendError, "HRESULT 0x80070005");
        assert_eq!(classify(&hresult), CaptureError::Permission);
        assert_eq!(
            classify(&cpal::Error::new(cpal::ErrorKind::PermissionDenied)),
            CaptureError::Permission
        );
        let other = cpal::Error::with_message(cpal::ErrorKind::DeviceBusy, "in use");
        assert_eq!(classify(&other), CaptureError::Backend("in use".into()));
    }

    #[test]
    fn a_take_is_limited_resampled_trimmed_and_written() {
        let levels = Arc::new(Mutex::new(Vec::new()));
        let limits = Arc::new(AtomicUsize::new(0));
        let mic = mic(48_000, 1, tone(48_000 * 16, 16_000), 480);
        let (l, n) = (levels.clone(), limits.clone());
        mic.start(
            15_000,
            Box::new(move |db| l.lock().unwrap().push(db)),
            Box::new(move || {
                n.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .unwrap();
        assert_eq!(limits.load(Ordering::SeqCst), 1);
        assert_eq!(levels.lock().unwrap().len(), 16 * 20);
        let take = mic.stop().unwrap();
        assert_eq!(take.wav.len(), 480_044);
        assert_eq!(take.duration_ms, 15_000);
        assert_eq!(mic.stop(), Err(CaptureError::NotRecording));
        assert_eq!(mic.device.closes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn silence_is_too_quiet_and_a_new_start_discards_the_take() {
        let quiet = mic(16_000, 2, vec![0; 32_000], 320);
        quiet
            .start(15_000, Box::new(|_| {}), Box::new(|| {}))
            .unwrap();
        assert_eq!(quiet.stop(), Err(CaptureError::TooQuiet));
        let loud = mic(16_000, 1, tone(16_000, 16_000), 160);
        loud.start(15_000, Box::new(|_| {}), Box::new(|| {}))
            .unwrap();
        loud.start(15_000, Box::new(|_| {}), Box::new(|| {}))
            .unwrap();
        assert_eq!(loud.device.closes.load(Ordering::SeqCst), 1);
        let take = loud.stop().unwrap();
        assert_eq!(take.duration_ms, 1_000, "one take, not two");
    }
}
