//! The fixture generator behind the `parley-fixtures` binary (docs/TESTING.md §3 and §4).
//!
//! Every fixture is built from integer maths only, never from the platform's `sin` or `exp`, so
//! the bytes are identical on Windows and Linux. `manifest.json` records the SHA-256 and size of
//! every file; the committed copy under `tests/fixtures/generated/` is the specification, and
//! `--check` proves a directory still matches it.
//!
//! Module 00 writes one fixture, `audio/tone_gaps_a.wav`; later modules add theirs here.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use sha2::{Digest, Sha256};

/// Sample rate of every audio fixture: 16 kHz mono, what transcription takes.
pub const SAMPLE_RATE: u32 = 16_000;

/// The manifest's file name inside a fixture directory.
pub const MANIFEST: &str = "manifest.json";

/// The manifest's schema tag.
pub const SCHEMA: &str = "parley.fixtures/1";

/// `audio/tone_gaps_a.wav`: 16,000 samples of tone, 8,000 of silence, 16,000 of tone.
///
/// The tone is a square of period 40 samples: within each period, samples 0 to 19 are +16000 and
/// 20 to 39 are −16000. Because 16,000 and 24,000 are multiples of 40, the second tone starts at
/// the same phase as the first. Trim points and chunk boundaries are therefore known exactly:
/// sample 0 is 16000, sample 20 is −16000, samples 16,000 to 23,999 are 0, and there are 40,000
/// samples in all (docs/modules/00-scaffold.md, Design notes).
pub fn tone_gaps_a() -> Vec<i16> {
    (0..40_000u32)
        .map(|i| {
            if (16_000..24_000).contains(&i) {
                0
            } else if i % 40 < 20 {
                16_000
            } else {
                -16_000
            }
        })
        .collect()
}

/// Encodes samples as a 16 kHz, mono, 16-bit PCM WAV file (a 44-byte RIFF header, then the
/// little-endian samples).
pub fn wav_bytes(samples: &[i16]) -> Vec<u8> {
    let data_len = u32::try_from(samples.len() * 2).expect("fixture fits in a WAV file");
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // bytes per second
    out.extend_from_slice(&2u16.to_le_bytes()); // bytes per sample frame
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

/// Every generated fixture, as (path relative to the fixture directory, bytes).
pub fn files() -> Vec<(String, Vec<u8>)> {
    vec![(
        "audio/tone_gaps_a.wav".to_string(),
        wav_bytes(&tone_gaps_a()),
    )]
}

/// What the manifest records about one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub bytes: u64,
    pub sha256: String,
}

/// File path (forward slashes, relative to the fixture directory) to its entry, sorted by path.
pub type Manifest = BTreeMap<String, Entry>;

/// Lower-case hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// The manifest of the given files.
pub fn manifest_of(files: &[(String, Vec<u8>)]) -> Manifest {
    files
        .iter()
        .map(|(name, bytes)| {
            (
                name.clone(),
                Entry {
                    bytes: bytes.len() as u64,
                    sha256: sha256_hex(bytes),
                },
            )
        })
        .collect()
}

/// The manifest as the JSON text written to `manifest.json` (keys sorted, LF line ends, one
/// trailing newline), so two generators produce byte-identical files.
pub fn manifest_json(manifest: &Manifest) -> String {
    let files: serde_json::Map<String, serde_json::Value> = manifest
        .iter()
        .map(|(name, entry)| {
            (
                name.clone(),
                serde_json::json!({ "bytes": entry.bytes, "sha256": entry.sha256 }),
            )
        })
        .collect();
    let value = serde_json::json!({ "schema": SCHEMA, "files": files });
    let mut text = serde_json::to_string_pretty(&value).expect("manifest serialises");
    text.push('\n');
    text
}

/// Parses `manifest.json`.
pub fn parse_manifest(text: &str) -> Result<Manifest, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("manifest.json is not JSON: {e}"))?;
    if value.get("schema").and_then(|s| s.as_str()) != Some(SCHEMA) {
        return Err(format!("manifest.json does not have schema {SCHEMA}"));
    }
    let files = value
        .get("files")
        .and_then(|f| f.as_object())
        .ok_or("manifest.json has no files object")?;
    let mut manifest = Manifest::new();
    for (name, entry) in files {
        let bytes = entry
            .get("bytes")
            .and_then(|b| b.as_u64())
            .ok_or_else(|| format!("{name}: bytes is missing"))?;
        let sha256 = entry
            .get("sha256")
            .and_then(|s| s.as_str())
            .ok_or_else(|| format!("{name}: sha256 is missing"))?;
        manifest.insert(
            name.clone(),
            Entry {
                bytes,
                sha256: sha256.to_string(),
            },
        );
    }
    Ok(manifest)
}

/// Writes every fixture and `manifest.json` into `dir`, creating it if needed.
pub fn write(dir: &Path) -> io::Result<Manifest> {
    let files = files();
    for (name, bytes) in &files {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, bytes)?;
    }
    let manifest = manifest_of(&files);
    fs::create_dir_all(dir)?;
    fs::write(dir.join(MANIFEST), manifest_json(&manifest))?;
    Ok(manifest)
}

/// Compares every file named in `dir/manifest.json` with the manifest's size and SHA-256.
/// Returns the names of the fixtures that differ or are missing; empty means the directory matches.
pub fn check(dir: &Path) -> io::Result<Vec<String>> {
    let text = fs::read_to_string(dir.join(MANIFEST))?;
    let manifest = parse_manifest(&text).map_err(io::Error::other)?;
    let mut differing = Vec::new();
    for (name, entry) in &manifest {
        let matches = match fs::read(dir.join(name)) {
            Ok(bytes) => bytes.len() as u64 == entry.bytes && sha256_hex(&bytes) == entry.sha256,
            Err(_) => false,
        };
        if !matches {
            differing.push(name.clone());
        }
    }
    Ok(differing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_has_the_specified_shape() {
        let samples = tone_gaps_a();
        assert_eq!(samples.len(), 40_000);
        assert_eq!(samples[0], 16_000);
        assert_eq!(samples[19], 16_000);
        assert_eq!(samples[20], -16_000);
        assert_eq!(samples[39], -16_000);
        assert_eq!(samples[40], 16_000);
        assert!(samples[16_000..24_000].iter().all(|&s| s == 0));
        assert_eq!(samples[24_000], 16_000);
        assert_eq!(samples[39_999], -16_000);
    }

    #[test]
    fn wav_header_is_16k_mono_16bit() {
        let wav = wav_bytes(&[1, -1]);
        assert_eq!(wav.len(), 48);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1);
        assert_eq!(
            u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
            16_000
        );
        assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16);
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(&wav[44..48], &[1, 0, 0xff, 0xff]);
    }

    #[test]
    fn sha256_of_empty_input_is_the_known_value() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn manifest_round_trips_through_json() {
        let manifest = manifest_of(&files());
        let parsed = parse_manifest(&manifest_json(&manifest)).unwrap();
        assert_eq!(parsed, manifest);
    }
}
