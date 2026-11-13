//! Decoding MP3 and WAV through rodio's decoder (Symphonia underneath): the duration of a clip,
//! which the checks of natural-voice audio need (docs/PUBLIK-INTEGRATION.md §6.7), and the clip
//! as mono 16-bit samples, which WSOLA stretches.

use std::io::Cursor;

use parley_core::audio::AudioDecoder;
use rodio::{Decoder, Source};

/// [`AudioDecoder`] for MP3 and WAV: decodes the whole clip and counts its samples, so the
/// duration is what plays, not what a header claims.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MediaDecoder;

/// A decoder over a copy of `audio`, or none when the bytes are no format rodio knows.
pub(crate) fn decoder(audio: &[u8]) -> Option<Decoder<Cursor<Vec<u8>>>> {
    Decoder::new(Cursor::new(audio.to_vec())).ok()
}

/// The length of `samples` interleaved samples of `channels` channels at `rate` Hz, in ns.
fn span_ns(samples: u128, (channels, rate): (u16, u32)) -> u128 {
    let per_second = u128::from(channels) * u128::from(rate);
    samples * 1_000_000_000 / per_second.max(1)
}

impl AudioDecoder for MediaDecoder {
    /// The decoded samples over (channels × rate), added up span by span (a stream may change its
    /// rate or channels between spans), rounded to the nearest ms; none when the bytes do not
    /// decode or decode to nothing.
    fn duration_ms(&self, audio: &[u8]) -> Option<u64> {
        let mut source = decoder(audio)?;
        let spec = |s: &Decoder<Cursor<Vec<u8>>>| (s.channels().get(), s.sample_rate().get());
        let mut current = spec(&source);
        let mut in_span: u128 = 0;
        let mut total_samples: u128 = 0;
        let mut total_ns: u128 = 0;
        loop {
            let now = spec(&source);
            if now != current {
                total_ns += span_ns(in_span, current);
                in_span = 0;
                current = now;
            }
            if source.next().is_none() {
                break;
            }
            in_span += 1;
            total_samples += 1;
        }
        if total_samples == 0 {
            return None;
        }
        total_ns += span_ns(in_span, current);
        u64::try_from((total_ns + 500_000) / 1_000_000).ok()
    }
}

/// A decoded sample as 16-bit PCM. Symphonia turns 16-bit PCM into floats by dividing by 2^15,
/// so multiplying back gives the original sample exactly.
fn to_i16(sample: rodio::Sample) -> i16 {
    (f64::from(sample) * 32_768.0)
        .round()
        .clamp(-32_768.0, 32_767.0) as i16
}

/// The clip's samples as mono 16-bit PCM and its sample rate: each frame's channels are added and
/// divided by their number (in integers, rounding toward zero). None when the bytes do not decode
/// or decode to nothing.
pub fn decode_mono_i16(audio: &[u8]) -> Option<(Vec<i16>, u32)> {
    let mut source = decoder(audio)?;
    let rate = source.sample_rate().get();
    let mut mono = Vec::new();
    let mut sum: i32 = 0;
    let mut in_frame: u16 = 0;
    loop {
        let channels = source.channels().get().max(1);
        let Some(sample) = source.next() else {
            break;
        };
        sum += i32::from(to_i16(sample));
        in_frame += 1;
        if in_frame >= channels {
            mono.push((sum / i32::from(channels)) as i16);
            sum = 0;
            in_frame = 0;
        }
    }
    if mono.is_empty() {
        return None;
    }
    Some((mono, rate))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A PCM WAV of 16-bit `samples` (interleaved) with `channels` channels at `rate` Hz.
    pub(crate) fn wav(samples: &[i16], channels: u16, rate: u32) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let block = u32::from(channels) * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * block).to_le_bytes());
        out.extend_from_slice(&(block as u16).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }

    #[test]
    fn a_wav_is_measured_to_the_millisecond() {
        let samples: Vec<i16> = (0..24_000).map(|i| ((i % 40) * 100) as i16).collect();
        assert_eq!(
            MediaDecoder.duration_ms(&wav(&samples, 1, 16_000)),
            Some(1_500)
        );
        let stereo: Vec<i16> = (0..2 * 22_050).map(|i| (i % 7) as i16).collect();
        assert_eq!(
            MediaDecoder.duration_ms(&wav(&stereo, 2, 22_050)),
            Some(1_000)
        );
    }

    #[test]
    fn bytes_that_are_not_audio_are_refused() {
        assert_eq!(MediaDecoder.duration_ms(b"<html>not audio</html>"), None);
        assert_eq!(MediaDecoder.duration_ms(b""), None);
        assert_eq!(decode_mono_i16(b"plain text, no audio at all"), None);
    }

    #[test]
    fn stereo_is_mixed_to_mono_exactly() {
        let frames: [(i16, i16); 4] = [(1_000, 3_000), (-2_000, 2_000), (32_767, 32_767), (5, 6)];
        let interleaved: Vec<i16> = frames.iter().flat_map(|(l, r)| [*l, *r]).collect();
        let (mono, rate) = decode_mono_i16(&wav(&interleaved, 2, 16_000)).unwrap();
        assert_eq!(rate, 16_000);
        assert_eq!(mono, vec![2_000, 0, 32_767, 5]);
        let samples = [0i16, 1, -1, 32_767, -32_768, 12_345];
        let (same, _) = decode_mono_i16(&wav(&samples, 1, 8_000)).unwrap();
        assert_eq!(same, samples);
    }
}
