//! Silence trimming of a 16 kHz take (docs/modules/10-speaking-and-pronunciation.md, Design
//! notes): short-time energy endpoint detection with hangover, after Rabiner and Sambur (1975),
//! simplified to energy alone.
//!
//! The take is cut into 20 ms frames; the noise floor is the level a tenth of the way up the
//! sorted frame levels; a frame is speech when it is 10 dB above the floor, between −50 and −30
//! dBFS. Short bursts (under 3 frames, 60 ms) are clicks, not speech, and a take with under 300
//! ms of speech is too quiet. The kept span starts 150 ms before the first speech and ends
//! 300 ms after the last, so soft consonants at either end are not cut off.

/// 20 ms at 16 kHz.
const FRAME: usize = 320;
/// The shortest run of speech frames that counts (60 ms).
const MIN_RUN: usize = 3;
/// The least speech a take must hold, in frames (300 ms).
const MIN_SPEECH_FRAMES: usize = 15;
/// Kept before the first speech: 150 ms.
const LEAD: usize = 2_400;
/// Kept after the last speech: 300 ms.
const TAIL: usize = 4_800;

/// The kept span of a take, `samples[start..end]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trimmed {
    pub start: usize,
    pub end: usize,
}

/// A frame's level: 20 log10(max(rms, 1) ÷ 32,768), so digital silence reads about −90 dBFS.
fn frame_level(frame: &[i16]) -> f64 {
    let energy: f64 = frame.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    let rms = (energy / frame.len().max(1) as f64).sqrt();
    20.0 * (rms.max(1.0) / 32_768.0).log10()
}

/// The span of `samples` (16 kHz) to keep, or None when the take holds under 300 ms of speech.
///
/// Whole 20 ms frames of 320 samples only; the noise floor is the level at index ⌊n ÷ 10⌋ of the
/// n levels sorted ascending; threshold = min(max(floor + 10 dB, −50 dBFS), −30 dBFS); a speech
/// frame is above the threshold; runs of under 3 frames are ignored; fewer than 15 speech frames
/// in all, or none, is too quiet; the span is from max(0, first run's start × 320 − 2,400) to
/// min(len, last run's end × 320 + 4,800). The −30 dBFS cap keeps a take with no pause, whose own
/// speech would otherwise be its noise floor, from being judged silent.
pub fn trim(samples: &[i16]) -> Option<Trimmed> {
    let (frames, _) = samples.as_chunks::<FRAME>();
    let levels: Vec<f64> = frames.iter().map(|frame| frame_level(frame)).collect();
    let n = levels.len();
    if n == 0 {
        return None;
    }
    let mut sorted = levels.clone();
    sorted.sort_by(f64::total_cmp);
    let floor = sorted[n / 10];
    let threshold = (floor + 10.0).clamp(-50.0, -30.0);
    // Runs of speech frames, as [start, end) frame indexes, of at least MIN_RUN frames.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        if levels[i] > threshold {
            let start = i;
            while i < n && levels[i] > threshold {
                i += 1;
            }
            if i - start >= MIN_RUN {
                runs.push((start, i));
            }
        } else {
            i += 1;
        }
    }
    let speech: usize = runs.iter().map(|(start, end)| end - start).sum();
    if speech < MIN_SPEECH_FRAMES {
        return None;
    }
    let first = runs.first()?.0;
    let last = runs.last()?.1;
    Some(Trimmed {
        start: (first * FRAME).saturating_sub(LEAD),
        end: (last * FRAME + TAIL).min(samples.len()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(len: usize) -> Vec<i16> {
        (0..len)
            .map(|i| if i % 40 < 20 { 16_000 } else { -16_000 })
            .collect()
    }

    fn take(parts: &[(bool, usize)]) -> Vec<i16> {
        parts
            .iter()
            .flat_map(|(loud, len)| if *loud { tone(*len) } else { vec![0; *len] })
            .collect()
    }

    #[test]
    fn a_tone_between_silences_is_kept_with_its_margins() {
        let samples = take(&[(false, 8_000), (true, 16_000), (false, 11_200)]);
        assert_eq!(
            trim(&samples),
            Some(Trimmed {
                start: 5_600,
                end: 28_800
            })
        );
        // Speech from the first sample to the last: nothing to cut, and the cap keeps it speech.
        let all = tone(16_000);
        assert_eq!(
            trim(&all),
            Some(Trimmed {
                start: 0,
                end: 16_000
            })
        );
    }

    #[test]
    fn too_little_speech_is_too_quiet() {
        assert_eq!(trim(&vec![0; 16_000]), None);
        assert_eq!(trim(&[]), None);
        assert_eq!(
            trim(&take(&[(false, 8_000), (true, 3_200), (false, 8_000)])),
            None
        );
        // Clicks of 40 ms are not speech, however many there are.
        let clicks: Vec<(bool, usize)> = (0..20)
            .flat_map(|_| [(true, 640), (false, 3_200)])
            .collect();
        assert_eq!(trim(&take(&clicks)), None);
        // 300 ms of speech is enough.
        assert!(trim(&take(&[(false, 8_000), (true, 4_800), (false, 8_000)])).is_some());
    }
}
