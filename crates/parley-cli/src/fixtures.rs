//! The fixture generator behind the `parley-fixtures` binary (docs/TESTING.md §3 and §4).
//!
//! Every fixture is built from integer maths only, never from the platform's `sin` or `exp`, so
//! the bytes are identical on Windows and Linux. `manifest.json` records the SHA-256 and size of
//! every file; the committed copy under `tests/fixtures/generated/` is the specification, and
//! `--check` proves a directory still matches it.
//!
//! Module 00 writes one fixture, `audio/tone_gaps_a.wav`; module 02 adds
//! `syllabus-mini/xx/syllabus.yaml`; later modules add theirs here.

use std::collections::BTreeMap;
use std::fmt::Write as _;
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

/// Where the mini syllabus lives inside the fixture directory. The folder is `xx`, not a course
/// language, so the validator's folder rule (V02) does not run on it.
pub const SYLLABUS_MINI: &str = "syllabus-mini/xx/syllabus.yaml";

/// The mini syllabus has 8 units of 10 skills: units 1 to 4 at A1, units 5 to 8 at A2.
pub const MINI_UNITS: usize = 8;
pub const MINI_SKILLS_PER_UNIT: usize = 10;
/// Every skill teaches three lemmas and has four examples.
pub const MINI_LEMMAS_PER_SKILL: usize = 3;
pub const MINI_EXAMPLES_PER_SKILL: usize = 4;

const MINI_VOWELS: [char; 5] = ['a', 'e', 'i', 'o', 'u'];
const MINI_CONSONANTS: [char; 10] = ['b', 'd', 'f', 'g', 'l', 'm', 'n', 'p', 'r', 't'];

/// Lemma number `n` of the mini syllabus (`n` = 3 × skill index + position, below 250): a
/// pseudo-word of five lower-case ASCII letters, `w` + vowel + consonant + vowel + `k`, such as
/// `wabak`. Spanish has no native word that starts with `w` and ends with `k`, so none of them is
/// a Spanish word, and each `n` gives a different word.
pub fn mini_lemma(n: usize) -> String {
    assert!(n < 250, "the mini syllabus has fewer than 250 lemmas");
    let mut word = String::with_capacity(5);
    word.push('w');
    word.push(MINI_VOWELS[n / 50]);
    word.push(MINI_CONSONANTS[(n / 5) % 10]);
    word.push(MINI_VOWELS[n % 5]);
    word.push('k');
    word
}

/// `wabak` → `Wabak`.
fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// The skill id at a skill index (0 to 79): `es.a1.u01.s01` … `es.a2.u08.s10`, where the index
/// is 10 × (unit − 1) + (skill − 1).
pub fn mini_skill_id(index: usize) -> String {
    let unit = index / MINI_SKILLS_PER_UNIT + 1;
    let skill = index % MINI_SKILLS_PER_UNIT + 1;
    let level = if unit <= 4 { "a1" } else { "a2" };
    format!("es.{level}.u{unit:02}.s{skill:02}")
}

/// The four example sentences of the skill at `index`, as lemma numbers in order. Each uses the
/// skill's own lemmas; from the second skill on, the fourth also uses the first lemma of the
/// skill before it, so examples are made of this and earlier skills' lemmas. Every example's
/// word sequence is different from every other's in the syllabus (V14).
fn mini_examples(index: usize) -> Vec<Vec<usize>> {
    let own = |k: usize| MINI_LEMMAS_PER_SKILL * index + k;
    let fourth = if index == 0 {
        vec![own(2), own(1), own(0)]
    } else {
        vec![own(0), MINI_LEMMAS_PER_SKILL * (index - 1)]
    };
    vec![
        vec![own(0), own(1)],
        vec![own(1), own(2)],
        vec![own(2), own(0), own(1)],
        fourth,
    ]
}

/// A sentence from lemma numbers: the first word capitalised, a full stop at the end.
fn mini_sentence(words: &[usize], word: impl Fn(usize) -> String) -> String {
    let text: Vec<String> = words.iter().map(|&n| word(n)).collect();
    format!("{}.", capitalised(&text.join(" ")))
}

/// `syllabus-mini/xx/syllabus.yaml`: 80 synthetic skills for ordering, unlocking and placement
/// without real content (docs/modules/02-srs-and-learner-model.md, Design notes). `lang: es`;
/// units `es.a1.u01` … `es.a1.u04` (A1) and `es.a2.u05` … `es.a2.u08` (A2); every skill of kind
/// `vocab` with `requires: []`, three lemmas from [`mini_lemma`] (part of speech `verb`, so no
/// gender or article is involved) and four examples from [`mini_examples`]. No function words,
/// names or number words, and no forms table beside it: every example word is a lemma's own
/// citation form. `parley content validate` reports it with no error and no warning.
pub fn syllabus_mini() -> String {
    let mut y = String::new();
    y.push_str("schema: parley.syllabus/1\n");
    y.push_str("lang: es\n");
    y.push_str("name: Mini syllabus for tests\n");
    y.push_str("variety: pseudo-words in 80 skills; not a real course\n");
    y.push_str("version: 1\n");
    y.push_str("function_words: []\n");
    y.push_str("names: []\n");
    y.push_str("units:\n");
    for unit in 1..=MINI_UNITS {
        let (level, cefr) = if unit <= 4 {
            ("a1", "A1")
        } else {
            ("a2", "A2")
        };
        let _ = writeln!(y, "  - id: es.{level}.u{unit:02}");
        let _ = writeln!(y, "    title: Unit {unit}");
        let _ = writeln!(y, "    cefr: {cefr}");
        y.push_str("    skills:\n");
        for skill in 1..=MINI_SKILLS_PER_UNIT {
            let index = MINI_SKILLS_PER_UNIT * (unit - 1) + (skill - 1);
            let _ = writeln!(y, "      - id: {}", mini_skill_id(index));
            y.push_str("        kind: vocab\n");
            let _ = writeln!(y, "        title: Skill {unit}.{skill}");
            let _ = writeln!(y, "        cefr: {cefr}");
            let _ = writeln!(
                y,
                "        can_do: I can use the words of skill {unit}.{skill}."
            );
            y.push_str("        requires: []\n");
            y.push_str("        lemmas:\n");
            for k in 0..MINI_LEMMAS_PER_SKILL {
                let n = MINI_LEMMAS_PER_SKILL * index + k;
                let _ = writeln!(
                    y,
                    "          - {{lemma: {}, pos: verb, en: [word {n}], rank: null}}",
                    mini_lemma(n)
                );
            }
            y.push_str("        examples:\n");
            for (i, words) in mini_examples(index).iter().enumerate() {
                let target = mini_sentence(words, mini_lemma);
                let en = mini_sentence(words, |n| format!("word {n}"));
                let _ = writeln!(
                    y,
                    "          - {{id: {}, target: \"{target}\", en: [\"{en}\"]}}",
                    i + 1
                );
            }
        }
    }
    y
}

/// Every generated fixture, as (path relative to the fixture directory, bytes).
pub fn files() -> Vec<(String, Vec<u8>)> {
    vec![
        (
            "audio/tone_gaps_a.wav".to_string(),
            wav_bytes(&tone_gaps_a()),
        ),
        (SYLLABUS_MINI.to_string(), syllabus_mini().into_bytes()),
    ]
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

    fn parsed_mini() -> parley_core::content::Syllabus {
        parley_core::content::parse_syllabus(SYLLABUS_MINI, &syllabus_mini())
            .unwrap_or_else(|errors| panic!("the mini syllabus does not parse: {errors:?}"))
    }

    #[test]
    fn mini_syllabus_is_in_the_generated_set_and_deterministic() {
        let names: Vec<String> = files().into_iter().map(|(name, _)| name).collect();
        assert!(names.contains(&SYLLABUS_MINI.to_string()), "{names:?}");
        assert_eq!(syllabus_mini(), syllabus_mini());
        let text = syllabus_mini();
        assert!(!text.contains('\r'), "LF line ends only");
        assert!(text.ends_with('\n'));
        assert!(text.is_ascii());
    }

    #[test]
    fn mini_syllabus_has_the_specified_shape() {
        use parley_core::content::{Cefr, Lang};
        let s = parsed_mini();
        assert_eq!(s.lang, Lang::Es);
        assert_eq!(s.units.len(), 8);
        for (u, unit) in s.units.iter().enumerate() {
            let (level, cefr) = if u < 4 {
                ("a1", Cefr::A1)
            } else {
                ("a2", Cefr::A2)
            };
            assert_eq!(unit.id, format!("es.{level}.u{:02}", u + 1));
            assert_eq!(unit.cefr, cefr);
            assert_eq!(unit.skills.len(), 10);
            for (k, skill) in unit.skills.iter().enumerate() {
                let index = 10 * u + k;
                assert_eq!(skill.id, format!("{}.s{:02}", unit.id, k + 1));
                assert_eq!(skill.id, mini_skill_id(index));
                assert_eq!(skill.cefr, cefr);
                assert!(skill.requires.is_empty(), "{}", skill.id);
                assert!(skill.lemmas.len() >= 3, "{}", skill.id);
                assert!(skill.examples.len() >= 4, "{}", skill.id);
            }
        }
        assert_eq!(mini_skill_id(0), "es.a1.u01.s01");
        assert_eq!(mini_skill_id(23), "es.a1.u03.s04");
        assert_eq!(mini_skill_id(39), "es.a1.u04.s10");
        assert_eq!(mini_skill_id(40), "es.a2.u05.s01");
        assert_eq!(mini_skill_id(79), "es.a2.u08.s10");
    }

    #[test]
    fn mini_lemmas_are_distinct_letters_only_pseudo_words() {
        let s = parsed_mini();
        let mut seen = std::collections::BTreeSet::new();
        for (_, skill) in s.skills() {
            for lemma in &skill.lemmas {
                assert!(
                    lemma.lemma.chars().all(|c| c.is_ascii_lowercase()),
                    "{}",
                    lemma.lemma
                );
                assert!(
                    lemma.lemma.starts_with('w') && lemma.lemma.ends_with('k'),
                    "{}",
                    lemma.lemma
                );
                assert!(seen.insert(lemma.lemma.clone()), "{} twice", lemma.lemma);
            }
        }
        assert_eq!(seen.len(), 240);
        assert_eq!(mini_lemma(0), "wabak");
        assert_eq!(mini_lemma(1), "wabek");
        assert_eq!(mini_lemma(5), "wadak");
        assert_eq!(mini_lemma(239), "wupuk");
    }

    #[test]
    fn mini_examples_use_own_and_earlier_lemmas_only() {
        let s = parsed_mini();
        let mut taught: Vec<String> = Vec::new();
        let mut targets = std::collections::BTreeSet::new();
        for (index, (_, skill)) in s.skills().enumerate() {
            let own: Vec<String> = skill.lemmas.iter().map(|l| l.lemma.clone()).collect();
            taught.extend(own.iter().cloned());
            let mut uses_earlier = false;
            for example in &skill.examples {
                let words: Vec<String> = example
                    .target
                    .trim_end_matches('.')
                    .split(' ')
                    .map(|w| w.to_lowercase())
                    .collect();
                assert!(words.iter().all(|w| taught.contains(w)), "{example:?}");
                assert!(words.iter().any(|w| own.contains(w)), "{example:?}");
                uses_earlier |= words.iter().any(|w| !own.contains(w));
                assert!(
                    targets.insert(words.join(" ")),
                    "{} repeats a target",
                    example.target
                );
                assert!(!example.en.is_empty());
            }
            assert_eq!(uses_earlier, index > 0, "{}", skill.id);
        }
    }

    #[test]
    fn mini_syllabus_validates_with_no_error_and_no_warning() {
        use parley_core::content::{Context, Forms, validate};
        let s = parsed_mini();
        let forms = Forms::default();
        let ctx = Context {
            file: SYLLABUS_MINI,
            folder: None,
            forms: &forms,
            numbers: &[],
            exclude: &[],
            lock: None,
        };
        let report = validate(&s, &ctx);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    }
}
