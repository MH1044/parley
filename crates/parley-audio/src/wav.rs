//! The file of a take: RIFF WAVE, 16-bit PCM, one channel at 16 kHz (docs/PUBLIK-INTEGRATION.md
//! §7), written with hound. With one channel and 16 bits hound writes the plain PCM format chunk,
//! so the header is the classic 44 bytes and a second of speech is 32,000 bytes after it.

use std::io::Cursor;

use hound::{SampleFormat, WavSpec, WavWriter};

use crate::resample::TAKE_RATE_HZ;

/// The 44-byte header (PCM, 1 channel, 16,000 Hz, byte rate 32,000, align 2, 16 bits), then the
/// samples, little-endian.
pub fn wav_16k_mono(samples: &[i16]) -> Vec<u8> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: TAKE_RATE_HZ,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::with_capacity(44 + samples.len() * 2));
    {
        // Writing to memory fails only past hound's 4 GB limit, hours beyond any take.
        let mut writer = WavWriter::new(&mut cursor, spec).expect("a WAV header writes to memory");
        for sample in samples {
            writer
                .write_sample(*sample)
                .expect("a take's samples write to memory");
        }
        writer.finalize().expect("a WAV file is finished in memory");
    }
    cursor.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le16(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([bytes[at], bytes[at + 1]])
    }

    fn le32(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    }

    #[test]
    fn the_header_is_the_44_byte_pcm_one() {
        let wav = wav_16k_mono(&[1, -2, 32_767]);
        assert_eq!(wav.len(), 44 + 6);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(le32(&wav, 4), 36 + 6);
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(le32(&wav, 16), 16);
        assert_eq!(le16(&wav, 20), 1, "PCM");
        assert_eq!(le16(&wav, 22), 1, "one channel");
        assert_eq!(le32(&wav, 24), 16_000);
        assert_eq!(le32(&wav, 28), 32_000);
        assert_eq!(le16(&wav, 32), 2);
        assert_eq!(le16(&wav, 34), 16);
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(le32(&wav, 40), 6);
        assert_eq!(&wav[44..], [1, 0, 0xfe, 0xff, 0xff, 0x7f]);
        assert_eq!(wav_16k_mono(&vec![0; 240_000]).len(), 480_044);
    }
}
