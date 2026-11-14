//! A take resampled to 16 kHz, the rate every transcriber takes (docs/PUBLIK-INTEGRATION.md §7),
//! with rubato's band-limited (FFT) resampler.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

/// The rate of a take.
pub const TAKE_RATE_HZ: u32 = 16_000;

/// The frames rubato is given at a time; any size works for a whole clip.
const CHUNK: usize = 1024;

/// The length of `len` samples at `rate_hz` once at 16 kHz: (len × 16,000 + rate ÷ 2) ÷ rate,
/// in integers.
fn length_at_16k(len: usize, rate_hz: u32) -> usize {
    let rate = u128::from(rate_hz);
    usize::try_from((len as u128 * u128::from(TAKE_RATE_HZ) + rate / 2) / rate).unwrap_or(0)
}

/// A float sample back to 16 bits: rounded, clamped to the 16-bit range.
fn to_i16(sample: f64) -> i16 {
    (sample * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16
}

/// The nearest earlier input sample for each output sample: used only if rubato refused the
/// rates, which it does only for a rate of zero.
fn nearest(mono: &[i16], rate_hz: u32, len: usize) -> Vec<i16> {
    (0..len)
        .map(|i| {
            let at = (i as u128 * u128::from(rate_hz) / u128::from(TAKE_RATE_HZ)) as usize;
            mono.get(at).copied().unwrap_or(0)
        })
        .collect()
}

/// Exactly (len × 16,000 + rate ÷ 2) ÷ rate samples (integers); 16 kHz is copied. The resampler's
/// cut-off sits below 8 kHz, the new Nyquist frequency, so nothing above it folds back into the
/// speech band; its start-up delay is removed, so the output lines up with the input, and the
/// output is then cut or padded with zeros to the exact length. A rate of zero gives nothing.
pub fn resample_to_16k(mono: &[i16], rate_hz: u32) -> Vec<i16> {
    if rate_hz == 0 {
        return Vec::new();
    }
    if rate_hz == TAKE_RATE_HZ {
        return mono.to_vec();
    }
    let len = length_at_16k(mono.len(), rate_hz);
    if mono.is_empty() || len == 0 {
        return vec![0; len];
    }
    let input: Vec<f64> = mono.iter().map(|s| f64::from(*s) / 32_768.0).collect();
    let resampled = Fft::<f64>::new(
        rate_hz as usize,
        TAKE_RATE_HZ as usize,
        CHUNK,
        1,
        FixedSync::Both,
    )
    .ok()
    .and_then(|mut resampler| {
        let adapter = InterleavedSlice::new(&input[..], 1, input.len()).ok()?;
        resampler.process_all(&adapter, input.len(), None).ok()
    })
    .map(|output| output.take_data());
    match resampled {
        Some(samples) => {
            let mut out: Vec<i16> = samples.into_iter().map(to_i16).collect();
            out.resize(len, 0);
            out
        }
        None => nearest(mono, rate_hz, len),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(len: usize, period: usize, amplitude: i16) -> Vec<i16> {
        (0..len)
            .map(|i| {
                if i % period < period / 2 {
                    amplitude
                } else {
                    -amplitude
                }
            })
            .collect()
    }

    fn zero_crossings(samples: &[i16]) -> usize {
        let signs: Vec<bool> = samples
            .iter()
            .filter(|s| **s != 0)
            .map(|s| *s > 0)
            .collect();
        signs.windows(2).filter(|w| w[0] != w[1]).count()
    }

    #[test]
    fn the_length_is_exact_and_the_pitch_is_kept() {
        let tone = square(48_000, 48, 16_000);
        let out = resample_to_16k(&tone, 48_000);
        assert_eq!(out.len(), 16_000);
        let crossings = zero_crossings(&out) as f64;
        assert!((crossings - 2_000.0).abs() <= 40.0, "{crossings}");
        assert_eq!(
            resample_to_16k(&square(44_100, 40, 16_000), 44_100).len(),
            16_000
        );
        assert_eq!(resample_to_16k(&square(1_001, 40, 1), 44_100).len(), 363);
        assert_eq!(resample_to_16k(&square(8_000, 16, 1), 8_000).len(), 16_000);
    }

    #[test]
    fn sixteen_khz_is_copied_and_empty_stays_empty() {
        let tone = square(320, 40, 1_000);
        assert_eq!(resample_to_16k(&tone, 16_000), tone);
        assert!(resample_to_16k(&[], 48_000).is_empty());
        assert!(resample_to_16k(&tone, 0).is_empty());
        assert_eq!(length_at_16k(3, 48_000), 1);
        assert_eq!(nearest(&[1, 2, 3, 4, 5, 6], 48_000, 2), [1, 4]);
    }
}
