//! Acceptance tests of module 10 in parley-audio (docs/modules/10-speaking-and-pronunciation.md,
//! Acceptance tests 1, 2, 3 (trim and stop), 4, 17 and the levels of 19): resampling to 16 kHz and
//! mixing to mono, trimming silence, the take's limit, the 16-bit WAV it is written as, the level of
//! every 50 ms of input, and local Whisper on a generated tone.
//!
//! References: the spec's Interfaces and Design notes (the resampled length (len x 16,000 + rate / 2)
//! / rate; trim after Rabiner and Sambur 1975 as the spec simplifies it to energy: 20 ms frames of
//! 320 samples, the floor at index n / 10 of the sorted levels, threshold min(max(floor + 10 dB,
//! -50 dBFS), -30 dBFS), runs of under 3 frames ignored, at least 15 speech frames, 150 ms kept
//! before and 300 ms after; levels as 20 log10(rms / 32,768) clamped to -60..0), the RIFF WAVE
//! header layout (44 bytes, little-endian fields), and signals true by construction: integer
//! square waves ("tone": period 40 samples at +/-16,000; "full scale": +/-32,767) and digital
//! silence, built here sample by sample with no platform `sin` or `exp` and nothing random. A
//! square wave of period 40 has a whole number of periods in every 20 ms frame and every 50 ms
//! window at the rates used, so its RMS is exactly its level.
//!
//! The microphone is a fake `InputDevice` that feeds scripted chunks from its own thread, as a
//! real device does, and records how often it was opened and closed. A test waits for the feed to
//! end before it stops the capture.
//!
//! Where the spec leaves a detail open, these tests assume that a chunk delivered before `stop` is
//! part of the take whatever thread delivered it, and that `Microphone::stop` both closes the
//! device and leaves it ready for the next `start`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use parley_audio::{
    InputDevice, InputFormat, Microphone, downmix, level_dbfs, resample_to_16k, trim, wav_16k_mono,
};
use parley_core::audio::{Capture, CaptureError, Take};

/// The take's sample rate.
const RATE: u32 = 16_000;
/// The tone's level and period.
const TONE: i16 = 16_000;
const PERIOD: usize = 40;
/// Full scale.
const FULL: i16 = 32_767;
/// One 20 ms trim frame at 16 kHz.
const FRAME: usize = 320;
/// The exercise limit: 15 s.
const LIMIT_MS: u32 = 15_000;

// ---------------------------------------------------------------------------------------------
// Signals.
// ---------------------------------------------------------------------------------------------

/// `len` samples of an integer square wave: `period / 2` at +level, then `period / 2` at -level.
fn square(len: usize, level: i16, period: usize) -> Vec<i16> {
    (0..len)
        .map(|i| {
            if i % period < period / 2 {
                level
            } else {
                -level
            }
        })
        .collect()
}

/// The tone of the spec: period 40 at +/-16,000.
fn tone(len: usize) -> Vec<i16> {
    square(len, TONE, PERIOD)
}

fn silence(len: usize) -> Vec<i16> {
    vec![0; len]
}

fn concat(parts: &[Vec<i16>]) -> Vec<i16> {
    parts.iter().flatten().copied().collect()
}

/// Every channel of each frame carries the same sample.
fn interleave(mono: &[i16], channels: u16) -> Vec<i16> {
    mono.iter()
        .flat_map(|&s| std::iter::repeat_n(s, usize::from(channels)))
        .collect()
}

/// Sign changes between successive non-zero samples (a zero takes no side).
fn zero_crossings(samples: &[i16]) -> usize {
    let mut last: Option<bool> = None;
    let mut count = 0;
    for &sample in samples {
        if sample == 0 {
            continue;
        }
        let positive = sample > 0;
        if last.is_some_and(|before| before != positive) {
            count += 1;
        }
        last = Some(positive);
    }
    count
}

/// The fields of a 44-byte RIFF WAVE header and the samples after it, each checked against the
/// layout: "RIFF", size, "WAVE", "fmt ", 16, format, channels, rate, byte rate, align, bits,
/// "data", data size.
struct Wav {
    format: u16,
    channels: u16,
    rate: u32,
    byte_rate: u32,
    align: u16,
    bits: u16,
    samples: Vec<i16>,
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn read_wav(bytes: &[u8]) -> Wav {
    assert!(bytes.len() >= 44, "a WAV file has a 44-byte header");
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(u32_at(bytes, 4) as usize, bytes.len() - 8, "the RIFF size");
    assert_eq!(&bytes[8..12], b"WAVE");
    assert_eq!(&bytes[12..16], b"fmt ");
    assert_eq!(u32_at(bytes, 16), 16, "the fmt chunk of PCM is 16 bytes");
    assert_eq!(&bytes[36..40], b"data");
    let data = u32_at(bytes, 40) as usize;
    assert_eq!(data, bytes.len() - 44, "the data size");
    Wav {
        format: u16_at(bytes, 20),
        channels: u16_at(bytes, 22),
        rate: u32_at(bytes, 24),
        byte_rate: u32_at(bytes, 28),
        align: u16_at(bytes, 32),
        bits: u16_at(bytes, 34),
        samples: bytes[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| i16::from_le_bytes(*pair))
            .collect(),
    }
}

/// A WAV of 16 kHz mono 16-bit PCM, as the spec's Interfaces give it.
#[track_caller]
fn assert_16k_mono(wav: &Wav) {
    assert_eq!(
        (
            wav.format,
            wav.channels,
            wav.rate,
            wav.byte_rate,
            wav.align,
            wav.bits
        ),
        (1, 1, 16_000, 32_000, 2, 16),
        "PCM, 1 channel, 16,000 Hz, byte rate 32,000, align 2, 16 bits"
    );
}

#[track_caller]
fn close(actual: f32, expected: f32, tolerance: f32) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} where {expected} (within {tolerance}) was expected"
    );
}

// ---------------------------------------------------------------------------------------------
// A fake microphone.
// ---------------------------------------------------------------------------------------------

type Chunk = (InputFormat, Vec<i16>);

/// `interleaved` cut into chunks of `frames` frames of `format`.
fn chunked(format: InputFormat, interleaved: &[i16], frames: usize) -> Vec<Chunk> {
    interleaved
        .chunks(frames * usize::from(format.channels))
        .map(|chunk| (format, chunk.to_vec()))
        .collect()
}

fn mono(rate_hz: u32) -> InputFormat {
    InputFormat {
        rate_hz,
        channels: 1,
    }
}

/// An input device that, on each `open`, feeds the next scripted list of chunks from a thread of
/// its own until the list ends or the device is closed. Clones share their state, so the test
/// keeps one while the microphone owns another.
#[derive(Clone)]
struct FakeDevice {
    available: bool,
    open_error: Option<CaptureError>,
    feeds: Arc<Mutex<VecDeque<Vec<Chunk>>>>,
    feeder: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
    /// Bumped by every open and close; a feed stops when it is no longer its own.
    generation: Arc<AtomicUsize>,
    opens: Arc<AtomicUsize>,
    closes: Arc<AtomicUsize>,
}

impl FakeDevice {
    fn feeding(feeds: Vec<Vec<Chunk>>) -> FakeDevice {
        FakeDevice {
            available: true,
            open_error: None,
            feeds: Arc::new(Mutex::new(feeds.into())),
            feeder: Arc::new(Mutex::new(None)),
            generation: Arc::new(AtomicUsize::new(0)),
            opens: Arc::new(AtomicUsize::new(0)),
            closes: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Waits until the current feed has delivered every chunk (or stopped at a close).
    fn wait(&self) {
        let handle = self.feeder.lock().unwrap().take();
        if let Some(handle) = handle {
            handle.join().expect("the feed thread ends");
        }
    }

    fn opens(&self) -> usize {
        self.opens.load(Ordering::SeqCst)
    }

    fn closes(&self) -> usize {
        self.closes.load(Ordering::SeqCst)
    }
}

impl InputDevice for FakeDevice {
    fn available(&self) -> bool {
        self.available
    }

    fn open(
        &self,
        mut sink: Box<dyn FnMut(InputFormat, &[i16]) + Send>,
    ) -> Result<(), CaptureError> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = &self.open_error {
            return Err(error.clone());
        }
        let chunks = self.feeds.lock().unwrap().pop_front().unwrap_or_default();
        let generation = Arc::clone(&self.generation);
        let mine = generation.fetch_add(1, Ordering::SeqCst) + 1;
        let handle = thread::spawn(move || {
            for (format, samples) in chunks {
                if generation.load(Ordering::SeqCst) != mine {
                    break;
                }
                sink(format, &samples);
            }
        });
        *self.feeder.lock().unwrap() = Some(handle);
        Ok(())
    }

    fn close(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.closes.fetch_add(1, Ordering::SeqCst);
    }
}

/// What one capture gave.
struct Recorded {
    take: Result<Take, CaptureError>,
    levels: Vec<f32>,
    limits: usize,
    device: FakeDevice,
}

/// Starts a capture of at most `max_ms` on a device that feeds `chunks`, waits for the feed to
/// end, and stops.
fn record(chunks: Vec<Chunk>, max_ms: u32) -> Recorded {
    let device = FakeDevice::feeding(vec![chunks]);
    let mic = Microphone::new(device.clone());
    let levels = Arc::new(Mutex::new(Vec::<f32>::new()));
    let limits = Arc::new(AtomicUsize::new(0));
    let (heard, limited) = (Arc::clone(&levels), Arc::clone(&limits));
    mic.start(
        max_ms,
        Box::new(move |dbfs: f32| heard.lock().unwrap().push(dbfs)),
        Box::new(move || {
            limited.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .expect("the fake device opens");
    device.wait();
    let take = mic.stop();
    let levels = levels.lock().unwrap().clone();
    Recorded {
        take,
        levels,
        limits: limits.load(Ordering::SeqCst),
        device,
    }
}

/// 16 kHz mono input in 10 ms chunks.
fn at_16k(samples: &[i16]) -> Vec<Chunk> {
    chunked(mono(RATE), samples, 160)
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 1: resampling and mixing.
// ---------------------------------------------------------------------------------------------

/// Acceptance 1: "resample_to_16k of a 48 kHz, 1 s square wave (period 48 samples) → 16,000 ± 1
/// samples whose zero crossings are 2,000 a second ± 2%".
#[test]
fn t01_a_48_khz_square_wave_resamples_to_16000_samples_with_its_pitch() {
    let input = square(48_000, TONE, 48);
    // 1 kHz: one sign change every 24 input samples.
    assert_eq!(zero_crossings(&input), 1_999);
    let output = resample_to_16k(&input, 48_000);
    assert!(
        output.len().abs_diff(16_000) <= 1,
        "{} samples, not 16,000 +/- 1",
        output.len()
    );
    let seconds = output.len() as f64 / f64::from(RATE);
    let per_second = zero_crossings(&output) as f64 / seconds;
    assert!(
        (per_second - 2_000.0).abs() <= 0.02 * 2_000.0,
        "{per_second} zero crossings a second, not 2,000 within 2%"
    );
}

/// Acceptance 1: "44.1 kHz, 1 s → 16,000 ± 1".
#[test]
fn t01_one_second_at_44_1_khz_resamples_to_16000_samples() {
    let output = resample_to_16k(&square(44_100, TONE, PERIOD), 44_100);
    assert!(
        output.len().abs_diff(16_000) <= 1,
        "{} samples, not 16,000 +/- 1",
        output.len()
    );
}

/// Acceptance 1, Interfaces: "Exactly (len × 16,000 + rate ÷ 2) ÷ rate samples (integers); 16 kHz
/// is copied."
#[test]
fn t01_the_resampled_length_is_the_rounded_ratio_and_16_khz_is_copied() {
    // (len x 16,000 + rate / 2) / rate, worked by hand.
    for (len, rate, expected) in [
        (48_000, 48_000, 16_000),
        (44_100, 44_100, 16_000),
        (100, 48_000, 33),
        (1_000, 44_100, 363),
        (3, 8_000, 6),
        (720_000, 48_000, 240_000),
        (661_500, 44_100, 240_000),
        (0, 48_000, 0),
    ] {
        assert_eq!(
            resample_to_16k(&tone(len), rate).len(),
            expected,
            "{len} samples at {rate} Hz"
        );
    }
    let input = concat(&[silence(100), tone(1_000), square(77, -5, 7)]);
    assert_eq!(resample_to_16k(&input, RATE), input, "16 kHz is copied");
}

/// Acceptance 1: "downmix of stereo with L = +1000 and R = −1000 → mono 0".
#[test]
fn t01_downmix_of_opposite_channels_is_silence() {
    let stereo: Vec<i16> = (0..1_000).flat_map(|_| [1_000, -1_000]).collect();
    assert_eq!(downmix(&stereo, 2), vec![0; 1_000]);
}

/// Acceptance 1, Interfaces: "Each frame's mean, rounded toward zero; a trailing partial frame is
/// dropped."
#[test]
fn t01_downmix_takes_each_frames_mean_toward_zero_and_drops_a_partial_frame() {
    assert_eq!(downmix(&[3, 4], 2), [3], "3.5 rounds toward zero");
    assert_eq!(downmix(&[-3, -4], 2), [-3], "-3.5 rounds toward zero");
    assert_eq!(
        downmix(&[1, 2, 3, 4, 5], 2),
        [1, 3],
        "the lone 5 is dropped"
    );
    assert_eq!(
        downmix(&[1, 1, 2, 9, 9], 3),
        [1],
        "two samples of a frame of three"
    );
    assert_eq!(downmix(&[FULL, FULL, -32_768, -32_768], 2), [FULL, -32_768]);
    assert_eq!(downmix(&[5, -7, 0], 1), [5, -7, 0], "mono is kept");
    assert_eq!(downmix(&[], 2), Vec::<i16>::new());
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 2: trimming.
// ---------------------------------------------------------------------------------------------

/// Acceptance 2: "trim on 0.5 s silence, 1.0 s tone, 0.7 s silence (16 kHz) → start 0.35 s (sample
/// 5,600) and end 1.8 s (sample 28,800), each within one frame (320 samples)."
///
/// By hand: 110 frames, 60 silent (level 20 log10(1 / 32,768) = -90.3 dB) and 50 of tone (-6.2 dB);
/// the floor is the 12th lowest, -90.3, so the threshold is min(max(-80.3, -50), -30) = -50; the
/// one run is frames 25 to 74; 25 x 320 - 2,400 = 5,600 and 75 x 320 + 4,800 = 28,800.
#[test]
fn t02_trim_keeps_150_ms_before_and_300_ms_after_the_tone() {
    let take = concat(&[silence(8_000), tone(16_000), silence(11_200)]);
    let kept = trim(&take).expect("a second of tone is speech");
    assert!(
        kept.start.abs_diff(5_600) <= FRAME,
        "start {} is not 5,600 within a frame",
        kept.start
    );
    assert!(
        kept.end.abs_diff(28_800) <= FRAME,
        "end {} is not 28,800 within a frame",
        kept.end
    );
}

/// Acceptance 2, Design notes: the floor follows a steady background. Background +/-100 (-50.3 dB),
/// so the threshold is min(max(-40.3, -50), -30) = -40.3: the background is not speech and the
/// same points are kept.
#[test]
fn t02_trim_finds_the_tone_over_a_quiet_background() {
    let background = |len| square(len, 100, PERIOD);
    let take = concat(&[background(8_000), tone(16_000), background(11_200)]);
    let kept = trim(&take).expect("the tone is speech over a quiet background");
    assert!(kept.start.abs_diff(5_600) <= FRAME, "start {}", kept.start);
    assert!(kept.end.abs_diff(28_800) <= FRAME, "end {}", kept.end);
}

/// Acceptance 2, Design notes: "The −30 dBFS cap ...: without it a take with no pause has its own
/// speech as its noise floor and would be judged silent." A second of tone alone is kept whole:
/// start max(0, 0 - 2,400) = 0, end min(16,000, 50 x 320 + 4,800) = 16,000.
#[test]
fn t02_a_take_with_no_pause_is_kept_whole() {
    let kept = trim(&tone(16_000)).expect("a take of speech alone is not silent");
    assert_eq!((kept.start, kept.end), (0, 16_000));
}

/// Acceptance 2, Design notes: "20 ms frames of 320 samples (whole frames only)": 16,100 samples
/// are 50 whole frames and 100 samples over; the end is min(len, 50 x 320 + 4,800) = 16,100.
#[test]
fn t02_trim_reads_whole_frames_and_never_ends_past_the_take() {
    let kept = trim(&tone(16_100)).expect("speech");
    assert_eq!((kept.start, kept.end), (0, 16_100));
}

/// Acceptance 2, Design notes: "runs of under 3 frames are ignored". Two-frame bursts around a
/// twenty-frame run are not where the take starts or ends: the run is frames 30 to 49, so the
/// take is 30 x 320 - 2,400 = 7,200 to 50 x 320 + 4,800 = 20,800.
#[test]
fn t02_runs_under_three_frames_do_not_move_the_ends() {
    let take = concat(&[
        silence(25 * FRAME),
        tone(2 * FRAME),
        silence(3 * FRAME),
        tone(20 * FRAME),
        silence(3 * FRAME),
        tone(2 * FRAME),
        silence(25 * FRAME),
    ]);
    let kept = trim(&take).expect("twenty frames of speech");
    assert_eq!((kept.start, kept.end), (7_200, 20_800));
}

/// Acceptance 2, Design notes: runs of exactly 3 frames count. Five runs of three (15 frames)
/// from frame 25 to frame 47: 25 x 320 - 2,400 = 5,600 to 48 x 320 + 4,800 = 20,160.
#[test]
fn t02_five_runs_of_three_frames_are_enough_speech() {
    let mut parts = vec![silence(25 * FRAME)];
    for run in 0..5 {
        parts.push(tone(3 * FRAME));
        if run < 4 {
            parts.push(silence(2 * FRAME));
        }
    }
    parts.push(silence(25 * FRAME));
    let kept = trim(&concat(&parts)).expect("15 frames of speech in runs of 3");
    assert_eq!((kept.start, kept.end), (5_600, 20_160));
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 3: too quiet.
// ---------------------------------------------------------------------------------------------

/// Acceptance 3: "A silence-only take → trim None ...; 200 ms of tone between silences → the same".
#[test]
fn t03_trim_finds_no_speech_in_silence_or_in_200_ms_of_tone() {
    assert!(trim(&silence(32_000)).is_none(), "silence");
    assert!(trim(&[]).is_none(), "nothing");
    // 200 ms is 10 frames, under the 15 (300 ms) a take needs.
    let short = concat(&[silence(8_000), tone(3_200), silence(8_000)]);
    assert!(trim(&short).is_none(), "200 ms of tone");
}

/// Acceptance 3, Design notes: "fewer than 15 speech frames in all (300 ms) ... is too quiet": 300
/// ms of tone (15 frames, 25 to 39) is a take, kept from 5,600 to 40 x 320 + 4,800 = 17,600;
/// 280 ms (14 frames) is not.
#[test]
fn t03_a_take_needs_300_ms_of_speech() {
    let enough = concat(&[silence(8_000), tone(15 * FRAME), silence(8_000)]);
    let kept = trim(&enough).expect("300 ms of tone");
    assert_eq!((kept.start, kept.end), (5_600, 17_600));
    let short = concat(&[silence(8_000), tone(14 * FRAME), silence(8_000)]);
    assert!(trim(&short).is_none(), "280 ms of tone");
}

/// Acceptance 3: twenty frames of tone, all in runs of two, are no speech at all.
#[test]
fn t03_bursts_of_two_frames_are_no_speech() {
    let mut parts = vec![silence(25 * FRAME)];
    for _ in 0..10 {
        parts.push(tone(2 * FRAME));
        parts.push(silence(2 * FRAME));
    }
    parts.push(silence(25 * FRAME));
    assert!(trim(&concat(&parts)).is_none());
}

/// Acceptance 3: "... and stop TooQuiet; 200 ms of tone between silences → the same".
#[test]
fn t03_stop_is_too_quiet_for_silence_and_for_200_ms_of_tone() {
    let quiet = record(at_16k(&silence(32_000)), LIMIT_MS);
    assert_eq!(quiet.take, Err(CaptureError::TooQuiet));
    let short = record(
        at_16k(&concat(&[silence(8_000), tone(3_200), silence(8_000)])),
        LIMIT_MS,
    );
    assert_eq!(short.take, Err(CaptureError::TooQuiet));
}

/// Acceptance 3, Interfaces: "NotRecording with no capture in progress": before any start, and
/// after the take has been stopped.
#[test]
fn t03_stop_with_no_capture_in_progress_is_not_recording() {
    let device = FakeDevice::feeding(vec![at_16k(&tone(16_000))]);
    let mic = Microphone::new(device.clone());
    assert_eq!(mic.stop(), Err(CaptureError::NotRecording));
    mic.start(LIMIT_MS, Box::new(|_| {}), Box::new(|| {}))
        .expect("the fake device opens");
    device.wait();
    assert!(mic.stop().is_ok(), "a second of tone is a take");
    assert_eq!(mic.stop(), Err(CaptureError::NotRecording));
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 4: the limit.
// ---------------------------------------------------------------------------------------------

/// Acceptance 4: "A 16 s take of tone at 16 kHz ... → on_limit once, stopped at 15 s; the WAV is
/// 480,044 bytes, 16 kHz, mono, 16-bit, duration_ms 15,000." At 16 kHz nothing is resampled and
/// no frame is quiet, so the data is exactly the first 240,000 samples fed.
#[test]
fn t04_a_16_second_take_at_16_khz_stops_at_15_seconds() {
    let input = tone(16 * 16_000);
    let recorded = record(at_16k(&input), LIMIT_MS);
    assert_eq!(recorded.limits, 1, "on_limit runs once");
    let take = recorded.take.expect("a take of tone");
    assert_eq!(take.wav.len(), 480_044);
    assert_eq!(take.duration_ms, 15_000);
    let wav = read_wav(&take.wav);
    assert_16k_mono(&wav);
    assert_eq!(wav.samples.len(), 240_000);
    assert!(wav.samples == input[..240_000], "the first 15 s, as fed");
    assert!(recorded.device.closes() >= 1, "stop closes the device");
}

/// Acceptance 4: "... (and again at 48 kHz)": ⌈15,000 x 48,000 / 1,000⌉ = 720,000 frames are kept,
/// resampled to 240,000 samples and cut to 15,000 x 16 = 240,000.
#[test]
fn t04_a_16_second_take_at_48_khz_stops_at_15_seconds() {
    let input = tone(16 * 48_000);
    let recorded = record(chunked(mono(48_000), &input, 480), LIMIT_MS);
    assert_eq!(recorded.limits, 1, "on_limit runs once");
    let take = recorded.take.expect("a take of tone");
    assert_eq!(take.wav.len(), 480_044);
    assert_eq!(take.duration_ms, 15_000);
    let wav = read_wav(&take.wav);
    assert_16k_mono(&wav);
    assert_eq!(wav.samples.len(), 240_000);
}

/// Acceptance 4, Interfaces: the limit is a parameter (30 s is module 11's turn): with a 1 s limit,
/// 2 s of tone give one call of on_limit and a take of 16,000 samples, 32,044 bytes, 1,000 ms.
#[test]
fn t04_the_limit_is_the_given_one() {
    let recorded = record(at_16k(&tone(32_000)), 1_000);
    assert_eq!(recorded.limits, 1);
    let take = recorded.take.expect("a take of tone");
    assert_eq!((take.wav.len(), take.duration_ms), (32_044, 1_000));
}

/// Acceptance 4: a take shorter than the limit never calls on_limit.
#[test]
fn t04_a_take_under_the_limit_does_not_call_on_limit() {
    let recorded = record(at_16k(&tone(16_000)), LIMIT_MS);
    assert_eq!(recorded.limits, 0);
    assert!(recorded.take.is_ok());
}

// ---------------------------------------------------------------------------------------------
// Interfaces: the take, the WAV and the device.
// ---------------------------------------------------------------------------------------------

/// Interfaces: "stop ... resamples, cuts to max_ms × 16 samples, trims and writes the WAV;
/// duration_ms = samples × 1000 ÷ 16,000, rounded." Acceptance test 2's take through the
/// microphone keeps samples 5,600 to 28,800 exactly as fed: 23,200 samples, 1,450 ms.
#[test]
fn interfaces_stop_writes_the_trimmed_take() {
    let input = concat(&[silence(8_000), tone(16_000), silence(11_200)]);
    let take = record(at_16k(&input), LIMIT_MS)
        .take
        .expect("a second of tone is a take");
    let wav = read_wav(&take.wav);
    assert_16k_mono(&wav);
    assert!(wav.samples == input[5_600..28_800], "the trimmed samples");
    assert_eq!(take.wav.len(), 44 + 2 * 23_200);
    assert_eq!(take.duration_ms, 1_450);
}

/// Interfaces: "The 44-byte header (PCM, 1 channel, 16,000 Hz, byte rate 32,000, align 2, 16
/// bits), then data", written byte by byte here from the RIFF WAVE layout.
#[test]
fn interfaces_wav_16k_mono_writes_the_44_byte_header_then_the_samples() {
    let mut expected: Vec<u8> = Vec::new();
    expected.extend_from_slice(b"RIFF");
    expected.extend_from_slice(&42u32.to_le_bytes());
    expected.extend_from_slice(b"WAVE");
    expected.extend_from_slice(b"fmt ");
    expected.extend_from_slice(&16u32.to_le_bytes());
    expected.extend_from_slice(&1u16.to_le_bytes());
    expected.extend_from_slice(&1u16.to_le_bytes());
    expected.extend_from_slice(&16_000u32.to_le_bytes());
    expected.extend_from_slice(&32_000u32.to_le_bytes());
    expected.extend_from_slice(&2u16.to_le_bytes());
    expected.extend_from_slice(&16u16.to_le_bytes());
    expected.extend_from_slice(b"data");
    expected.extend_from_slice(&6u32.to_le_bytes());
    expected.extend_from_slice(&[0x01, 0x00, 0xFF, 0xFF, 0xFF, 0x7F]);
    assert_eq!(wav_16k_mono(&[1, -1, FULL]), expected);

    let empty = wav_16k_mono(&[]);
    assert_eq!(empty.len(), 44);
    assert_eq!(u32_at(&empty, 4), 36);
    assert_eq!(u32_at(&empty, 40), 0);
}

/// Interfaces: "20 log10(rms ÷ 32,768) clamped to −60..0; −60 for silence or an empty window."
#[test]
fn interfaces_level_dbfs_is_the_rms_in_dbfs_clamped_to_minus_60_and_0() {
    // 20 log10(32,767 / 32,768) = -0.000265.
    close(level_dbfs(&square(2_400, FULL, PERIOD)), 0.0, 0.01);
    // 20 log10(16,000 / 32,768) = -6.2266.
    close(level_dbfs(&tone(2_400)), -6.2266, 0.01);
    // 20 log10(100 / 32,768) = -50.309.
    close(level_dbfs(&square(800, 100, PERIOD)), -50.309, 0.01);
    // 20 log10(10 / 32,768) = -70.3, clamped.
    close(level_dbfs(&square(800, 10, PERIOD)), -60.0, 1e-4);
    close(level_dbfs(&silence(800)), -60.0, 1e-4);
    close(level_dbfs(&[]), -60.0, 1e-4);
    // An rms of 32,768 is 0 dBFS, never above.
    let loudest = level_dbfs(&[-32_768; 100]);
    assert!(loudest <= 0.0, "{loudest} is above 0 dBFS");
    close(loudest, 0.0, 1e-4);
}

/// Interfaces: `has_device` follows the device's `available`, and a device that refuses to open
/// passes its error on (a permission error stays Permission).
#[test]
fn interfaces_the_device_decides_has_device_and_start_errors() {
    let mut absent = FakeDevice::feeding(Vec::new());
    absent.available = false;
    assert!(!Microphone::new(absent).has_device());
    assert!(Microphone::new(FakeDevice::feeding(Vec::new())).has_device());

    let mut refused = FakeDevice::feeding(Vec::new());
    refused.open_error = Some(CaptureError::Permission);
    let mic = Microphone::new(refused);
    assert_eq!(
        mic.start(LIMIT_MS, Box::new(|_| {}), Box::new(|| {})),
        Err(CaptureError::Permission)
    );
    assert_eq!(mic.stop(), Err(CaptureError::NotRecording));
}

/// Interfaces: "A capture in progress is discarded first." The first capture hears silence; a
/// second start discards it, and the take is the second capture's second of tone.
#[test]
fn interfaces_a_new_start_discards_the_capture_in_progress() {
    let device = FakeDevice::feeding(vec![at_16k(&silence(16_000)), at_16k(&tone(16_000))]);
    let mic = Microphone::new(device.clone());
    mic.start(LIMIT_MS, Box::new(|_| {}), Box::new(|| {}))
        .expect("the first capture starts");
    device.wait();
    mic.start(LIMIT_MS, Box::new(|_| {}), Box::new(|| {}))
        .expect("the second capture starts");
    device.wait();
    let take = mic.stop().expect("the second capture is a take");
    assert_eq!(device.opens(), 2);
    assert!(device.closes() >= 1, "the first capture was closed");
    assert_eq!(take.duration_ms, 1_000, "only the second capture's second");
    assert_eq!(read_wav(&take.wav).samples, tone(16_000));
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 19: levels.
// ---------------------------------------------------------------------------------------------

/// Acceptance 19: "A fake device feeding 1 s of a full-scale square wave at 48 kHz in 10 ms chunks
/// gives exactly 20 levels, each 0 dBFS (tolerance 0.1 dB)".
#[test]
fn t19_a_second_of_full_scale_square_at_48_khz_gives_20_levels_of_0_dbfs() {
    let input = square(48_000, FULL, PERIOD);
    let recorded = record(chunked(mono(48_000), &input, 480), LIMIT_MS);
    assert_eq!(recorded.levels.len(), 20, "{:?}", recorded.levels);
    for level in &recorded.levels {
        close(*level, 0.0, 0.1);
    }
}

/// Acceptance 19: "... and 1 s of digital silence 20 levels of −60 dBFS, the meter's floor".
#[test]
fn t19_a_second_of_silence_gives_20_levels_of_minus_60_dbfs() {
    let recorded = record(chunked(mono(48_000), &silence(48_000), 480), LIMIT_MS);
    assert_eq!(recorded.levels.len(), 20, "{:?}", recorded.levels);
    for level in &recorded.levels {
        close(*level, -60.0, 1e-3);
    }
}

/// Acceptance 19, Interfaces: "a level per whole 50 ms window (rate_hz ÷ 20 frames, carried across
/// chunks)": chunks of 1,000 frames at 48 kHz and of 333 frames at 44.1 kHz still give 20 levels a
/// second, and 0.99 s gives 19 (the last window is not whole).
#[test]
fn t19_windows_are_carried_across_chunks_of_any_size() {
    let input = square(48_000, FULL, PERIOD);
    let recorded = record(chunked(mono(48_000), &input, 1_000), LIMIT_MS);
    assert_eq!(recorded.levels.len(), 20);
    let recorded = record(
        chunked(mono(44_100), &square(44_100, FULL, 30), 333),
        LIMIT_MS,
    );
    assert_eq!(
        recorded.levels.len(),
        20,
        "2,205 frames a window at 44.1 kHz"
    );
    let recorded = record(
        chunked(mono(48_000), &square(47_520, FULL, PERIOD), 480),
        LIMIT_MS,
    );
    assert_eq!(recorded.levels.len(), 19);
}

/// Acceptance 19, Design notes: "RMS, not peak, of each 50 ms window of the mixed input". Two
/// channels in phase at full scale read 0 dBFS; two in opposition mix to silence, -60 dBFS.
#[test]
fn t19_levels_are_measured_on_the_mixed_input() {
    let stereo = InputFormat {
        rate_hz: 48_000,
        channels: 2,
    };
    let in_phase = interleave(&square(48_000, FULL, PERIOD), 2);
    let recorded = record(chunked(stereo, &in_phase, 480), LIMIT_MS);
    assert_eq!(recorded.levels.len(), 20);
    for level in &recorded.levels {
        close(*level, 0.0, 0.1);
    }

    let opposed: Vec<i16> = square(48_000, FULL, PERIOD)
        .iter()
        .flat_map(|&s| [s, -s])
        .collect();
    let recorded = record(chunked(stereo, &opposed, 480), LIMIT_MS);
    assert_eq!(recorded.levels.len(), 20);
    for level in &recorded.levels {
        close(*level, -60.0, 1e-3);
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 17: local Whisper (the `local-whisper` feature).
// ---------------------------------------------------------------------------------------------

#[cfg(feature = "local-whisper")]
mod local_whisper {
    use std::fs;
    use std::path::PathBuf;

    use parley_audio::{WhisperLocal, wav_16k_mono};
    use parley_core::audio::LocalTranscriber;
    use parley_core::content::Lang;

    /// docs/TESTING.md §2: the line printed when the model is absent.
    const MISSING: &str =
        "Whisper model not found at models/whisper/ggml-tiny.bin; run parley whisper download tiny";

    /// The repository root: the first ancestor of this crate that holds the workspace Cargo.toml.
    fn repo_root() -> PathBuf {
        let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        loop {
            if let Ok(text) = fs::read_to_string(dir.join("Cargo.toml"))
                && text.contains("[workspace]")
            {
                return dir;
            }
            assert!(dir.pop(), "no workspace Cargo.toml above the crate");
        }
    }

    /// Acceptance 17: "(local-whisper job, model present) WhisperLocal transcribing the generated
    /// tone returns without error; skipped with the message of docs/TESTING.md §2 when the model is
    /// absent." The model path is absolute, as LocalTranscriber asks.
    #[test]
    fn t17_local_whisper_transcribes_a_generated_tone_without_error() {
        let model = repo_root()
            .join("models")
            .join("whisper")
            .join("ggml-tiny.bin");
        if !model.is_file() {
            println!("{MISSING}");
            return;
        }
        let samples: Vec<i16> = (0..16_000)
            .map(|i| if i % 40 < 20 { 16_000 } else { -16_000 })
            .collect();
        let heard = WhisperLocal::new().transcribe(&model, &wav_16k_mono(&samples), Lang::Es);
        assert!(heard.is_ok(), "local Whisper failed: {heard:?}");
    }
}
