//! Time stretching for slow mode when the speech provider does not honour `speed`
//! (docs/PUBLIK-INTEGRATION.md §6.7): WSOLA, waveform similarity overlap-add, after Verhelst and
//! Roelands (1993), "An overlap-add technique based on waveform similarity (WSOLA) for high
//! quality time-scale modification of speech", ICASSP-93.
//!
//! The output is built from frames of 20 ms laid down every 10 ms (50% overlap) and cross-faded
//! with a Hann window. Output frame k sits at k × 10 ms; its nominal place in the input is
//! k × 10 ms ÷ factor. Within ±10 ms of that place, the frame taken is the one most like the
//! natural continuation of the previous frame (the input that followed it), by normalised
//! cross-correlation, so the overlapping parts are in phase and the pitch is kept while the
//! length changes.

/// Frame length in ms; the hop is half of it.
const FRAME_MS: u64 = 20;
/// How far around the nominal place the search goes, in ms.
const SEARCH_MS: u64 = 10;

/// The Hann window of an even length `n`, sampled half a sample off the ends, so every value is
/// above zero and two windows half a frame apart add up to exactly 1:
/// w(i) = 0.5 − 0.5 cos(2π (i + ½) ÷ n).
fn hann(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * (i as f64 + 0.5) / n as f64).cos())
        .collect()
}

/// The search offsets nearest first: 0, −1, +1, −2, +2, …, so among equally good frames the one
/// nearest the nominal place wins.
fn offsets(radius: isize) -> impl Iterator<Item = isize> {
    std::iter::once(0).chain((1..=radius).flat_map(|d| [-d, d]))
}

/// `samples` at `rate_hz`, `factor` times as long (1 ÷ 0.75 for slow mode) and at the same pitch:
/// output length = input × `factor`, rounded. A factor of 1, an empty input or an input shorter
/// than one frame comes back as it is (the short one can only be cut or padded).
pub fn wsola(samples: &[i16], rate_hz: u32, factor: f32) -> Vec<i16> {
    let factor = f64::from(factor);
    if samples.is_empty() || rate_hz == 0 || !factor.is_finite() || factor <= 0.0 {
        return samples.to_vec();
    }
    let out_len = (samples.len() as f64 * factor).round() as usize;
    if (factor - 1.0).abs() < 1e-9 {
        return samples.to_vec();
    }
    let rate = u64::from(rate_hz);
    // An even frame of 20 ms, so the hop of 10 ms is exactly half of it.
    let frame = (((rate * FRAME_MS + 500) / 1000) as usize).max(2) & !1;
    let hop = frame / 2;
    let radius = ((rate * SEARCH_MS + 500) / 1000) as isize;
    let len = samples.len();
    if len < frame {
        let mut out = samples.to_vec();
        out.resize(out_len, 0);
        return out;
    }
    let last_start = (len - frame) as isize;
    let window = hann(frame);
    let x = |i: isize| -> f64 {
        if i >= 0 && (i as usize) < len {
            f64::from(samples[i as usize])
        } else {
            0.0
        }
    };

    let mut sum = vec![0.0f64; out_len + frame];
    let mut weight = vec![0.0f64; out_len + frame];
    let mut previous: isize = 0;
    let mut k = 0usize;
    while k * hop < out_len {
        let start = if k == 0 {
            0
        } else {
            let nominal = (k as f64 * hop as f64 / factor).round() as isize;
            // The search keeps its full width near the ends of the input, so a frame in phase
            // can always be found, and every frame is taken from real input. An input shorter
            // than two search widths is searched whole.
            let centre = if radius <= last_start - radius {
                nominal.clamp(radius, last_start - radius)
            } else {
                last_start / 2
            };
            let target = previous + hop as isize;
            let mut best = (f64::NEG_INFINITY, centre);
            for delta in offsets(radius) {
                let candidate = centre + delta;
                if candidate < 0 || candidate > last_start {
                    continue;
                }
                let (mut dot, mut energy) = (0.0f64, 0.0f64);
                for i in 0..frame as isize {
                    let c = x(candidate + i);
                    dot += x(target + i) * c;
                    energy += c * c;
                }
                let score = dot / (energy.sqrt() + 1e-9);
                if score > best.0 + 1e-9 {
                    best = (score, candidate);
                }
            }
            best.1
        };
        let at = k * hop;
        for (i, w) in window.iter().enumerate() {
            sum[at + i] += w * x(start + i as isize);
            weight[at + i] += w;
        }
        previous = start;
        k += 1;
    }
    sum.truncate(out_len);
    sum.iter()
        .zip(&weight)
        .map(|(s, w)| {
            let value = if *w > 1e-9 { s / w } else { 0.0 };
            value.round().clamp(-32_768.0, 32_767.0) as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sign changes between neighbouring samples.
    fn crossings(samples: &[i16]) -> usize {
        samples
            .windows(2)
            .filter(|pair| (pair[0] < 0) != (pair[1] < 0))
            .count()
    }

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

    #[test]
    fn hann_windows_half_a_frame_apart_add_up_to_one() {
        let w = hann(320);
        assert!(w.iter().all(|v| *v > 0.0 && *v < 1.0));
        for i in 0..160 {
            assert!((w[i] + w[i + 160] - 1.0).abs() < 1e-12, "{i}");
        }
        assert_eq!(offsets(2).collect::<Vec<_>>(), [0, -1, 1, -2, 2]);
    }

    #[test]
    fn a_square_wave_gets_longer_and_keeps_its_pitch() {
        let rate = 16_000;
        let input = square(rate as usize, 40, 8_000);
        let output = wsola(&input, rate, 1.0 / 0.75);
        let expected = (input.len() as f64 / 0.75).round() as usize;
        assert_eq!(output.len(), expected);
        let per_second_in = crossings(&input) as f64;
        let per_second_out = crossings(&output) as f64 / (output.len() as f64 / f64::from(rate));
        assert!(
            (per_second_out / per_second_in - 1.0).abs() < 0.02,
            "{per_second_in} crossings a second in, {per_second_out} out"
        );
        // Faster works the same way.
        let faster = wsola(&input, rate, 0.8);
        assert_eq!(faster.len(), 12_800);
        let per_second_fast = crossings(&faster) as f64 / 0.8;
        assert!((per_second_fast / per_second_in - 1.0).abs() < 0.02);
    }

    #[test]
    fn edge_cases_come_back_unchanged_or_padded() {
        let input = square(1_000, 40, 100);
        assert_eq!(wsola(&input, 16_000, 1.0), input);
        assert_eq!(wsola(&[], 16_000, 1.5), Vec::<i16>::new());
        assert_eq!(wsola(&input, 0, 1.5), input);
        assert_eq!(wsola(&input, 16_000, f32::NAN), input);
        assert_eq!(wsola(&input, 16_000, -1.0), input);
        let short = [1i16, 2, 3];
        assert_eq!(wsola(&short, 16_000, 2.0), vec![1, 2, 3, 0, 0, 0]);
        assert_eq!(wsola(&short, 16_000, 0.5), vec![1, 2]);
        // Longer than a frame, shorter than the search: searched whole.
        assert_eq!(wsola(&square(400, 40, 100), 16_000, 1.5).len(), 600);
    }
}
