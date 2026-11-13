//! Acceptance tests of module 09 in parley-audio (docs/modules/09-listening-and-voices.md,
//! Acceptance tests 11 and 20): WSOLA time stretching makes a square wave 1 / 0.75 times as long and
//! keeps its pitch, and the media decoder measures, refuses and mixes generated WAV clips.
//!
//! References: the spec (WSOLA after Verhelst and Roelands 1993, output length = input x factor;
//! `MediaDecoder` and `decode_mono_i16` in its parley-audio Interfaces) and signals true by
//! construction: integer square waves and constant levels written into RIFF WAVE files whose 44-byte
//! header is written here field by field, so every expected duration, rate and level follows from
//! the bytes. No platform `sin` or `exp` is used, and nothing is random.
//!
//! Where the spec leaves a detail open, these tests assume that parley-audio depends on parley-core,
//! whose `AudioDecoder` trait `MediaDecoder` implements (so `duration_ms` is called through it); that
//! `decode_mono_i16` mixes the channels by their mean and returns the file's own sample rate; and
//! that decoded samples may pass through a float conversion, so decoded levels are compared within 2
//! of the levels written.

use parley_audio::{MediaDecoder, decode_mono_i16, wsola};
use parley_core::audio::AudioDecoder;

/// The sample rate of the generated signals.
const RATE: u32 = 16_000;
/// The square wave's period in samples: 400 Hz at 16 kHz.
const PERIOD: usize = 40;
/// The square wave's level.
const LEVEL: i16 = 8_000;

/// `samples` samples of a square wave: PERIOD / 2 at +LEVEL, then PERIOD / 2 at -LEVEL.
fn square(samples: usize) -> Vec<i16> {
    (0..samples)
        .map(|i| {
            if i % PERIOD < PERIOD / 2 {
                LEVEL
            } else {
                -LEVEL
            }
        })
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

/// A RIFF WAVE file of 16-bit PCM: `interleaved` holds one sample per channel per frame.
fn wav_bytes(channels: u16, rate: u32, interleaved: &[i16]) -> Vec<u8> {
    let data_len = u32::try_from(interleaved.len() * 2).expect("a short clip");
    let block_align = channels * 2;
    let byte_rate = rate * u32::from(block_align);
    let mut out = Vec::with_capacity(44 + interleaved.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    // Format 1: PCM.
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in interleaved {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

/// `frames` stereo frames of a constant left and right level.
fn stereo(frames: usize, left: i16, right: i16) -> Vec<i16> {
    (0..frames).flat_map(|_| [left, right]).collect()
}

const NOT_AUDIO: &[u8] = b"<!doctype html><title>Service unavailable</title>";

// ---------------------------------------------------------------------------------------------
// Acceptance test 11: WSOLA.
// ---------------------------------------------------------------------------------------------

/// Test 11: "WSOLA on a 1 s square wave of period 40 samples with factor 1/0.75 → length 1.333 s ± 2%,
/// and zero crossings per second within 2% of the input (pitch kept)."
#[test]
fn t11_wsola_makes_a_square_wave_four_thirds_as_long_and_keeps_its_pitch() {
    let input = square(RATE as usize);
    // 799 sign changes in 16,000 samples: one every 20 samples after the first 20.
    let input_crossings = zero_crossings(&input);
    assert_eq!(input_crossings, 799);
    let input_rate = input_crossings as f64;

    let output = wsola(&input, RATE, 1.0 / 0.75);
    let seconds = output.len() as f64 / f64::from(RATE);
    let expected = 4.0 / 3.0;
    assert!(
        (seconds - expected).abs() <= 0.02 * expected,
        "the output lasts {seconds} s, not 1.333 s within 2%"
    );

    let output_rate = zero_crossings(&output) as f64 / seconds;
    assert!(
        (output_rate - input_rate).abs() <= 0.02 * input_rate,
        "{output_rate} zero crossings a second, not {input_rate} within 2%: the pitch moved"
    );
}

/// 11, the factor's meaning: "1.0 plays as is", so a factor of 1 keeps the length; an empty signal
/// stays empty (output length = input x factor).
#[test]
fn t11_wsola_keeps_the_length_at_factor_one_and_an_empty_signal_empty() {
    let input = square(RATE as usize);
    let output = wsola(&input, RATE, 1.0);
    let ratio = output.len() as f64 / input.len() as f64;
    assert!(
        (ratio - 1.0).abs() <= 0.02,
        "factor 1 gives {} samples for {}",
        output.len(),
        input.len()
    );
    let output_rate = zero_crossings(&output) as f64 / (output.len() as f64 / f64::from(RATE));
    assert!(
        (output_rate - 799.0).abs() <= 0.02 * 799.0,
        "{output_rate} zero crossings a second at factor 1"
    );
    assert!(wsola(&[], RATE, 1.0 / 0.75).is_empty());
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 20: the media decoder.
// ---------------------------------------------------------------------------------------------

/// Test 20: "MediaDecoder measures a generated 1.5 s WAV as 1500 ms ± 1 ms and refuses bytes that are
/// not audio."
#[test]
fn t20_the_media_decoder_measures_a_wav_and_refuses_bytes_that_are_not_audio() {
    // 24,000 samples at 16 kHz: 48,000 data bytes at 32,000 bytes a second.
    let clip = wav_bytes(1, RATE, &square(24_000));
    assert_eq!(clip.len(), 44 + 48_000);
    let ms = MediaDecoder
        .duration_ms(&clip)
        .expect("a 16 kHz mono WAV decodes");
    assert!(ms.abs_diff(1_500) <= 1, "measured {ms} ms, not 1500 ms");

    // A stereo file at 44.1 kHz: 22,050 frames are 500 ms.
    let wide = wav_bytes(2, 44_100, &stereo(22_050, 1_000, -1_000));
    let ms = MediaDecoder
        .duration_ms(&wide)
        .expect("a 44.1 kHz stereo WAV decodes");
    assert!(ms.abs_diff(500) <= 1, "measured {ms} ms, not 500 ms");

    assert_eq!(MediaDecoder.duration_ms(NOT_AUDIO), None);
    assert_eq!(MediaDecoder.duration_ms(&[]), None);
    assert_eq!(
        MediaDecoder.duration_ms(&clip[..20]),
        None,
        "a header cut short is not audio"
    );
}

/// Test 20: "decode_mono_i16 mixes a stereo WAV to mono": left +1000 and right -1000 give silence; left
/// 2000 and right 0 give 1000 (the mean); the rate is the file's, one sample per frame.
#[test]
fn t20_decode_mono_i16_mixes_a_stereo_wav_to_mono() {
    let frames = 8_000;
    let (opposite, rate) = decode_mono_i16(&wav_bytes(2, RATE, &stereo(frames, 1_000, -1_000)))
        .expect("a stereo WAV decodes");
    assert_eq!(rate, RATE);
    assert_eq!(opposite.len(), frames, "one sample per frame");
    assert!(
        opposite.iter().all(|sample| sample.unsigned_abs() <= 2),
        "opposite channels cancel: {:?}",
        &opposite[..8]
    );

    let (one_sided, rate) = decode_mono_i16(&wav_bytes(2, RATE, &stereo(frames, 2_000, 0)))
        .expect("a stereo WAV decodes");
    assert_eq!(rate, RATE);
    assert_eq!(one_sided.len(), frames);
    assert!(
        one_sided.iter().all(|sample| sample.abs_diff(1_000) <= 2),
        "the mean of 2000 and 0 is 1000: {:?}",
        &one_sided[..8]
    );
}

/// 20, the mono case of decode_mono_i16: a mono file comes back as it was written, at its rate.
#[test]
fn t20_decode_mono_i16_returns_a_mono_wav_as_written() {
    let written = square(1_600);
    let (samples, rate) =
        decode_mono_i16(&wav_bytes(1, RATE, &written)).expect("a mono WAV decodes");
    assert_eq!(rate, RATE);
    assert_eq!(samples.len(), written.len());
    assert!(
        samples
            .iter()
            .zip(&written)
            .all(|(got, want)| got.abs_diff(*want) <= 2),
        "the samples are those written"
    );
    assert_eq!(decode_mono_i16(NOT_AUDIO), None);
}
