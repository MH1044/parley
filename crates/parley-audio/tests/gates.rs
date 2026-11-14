//! Gate G4, the speaking loopback (docs/TESTING.md §6; docs/modules/10-speaking-and-pronunciation.md,
//! Design notes and Acceptance test 18): sentences of the course content are synthesised by an OS
//! voice, decoded, resampled to 16 kHz, trimmed, written as a 16 kHz mono WAV, transcribed by local
//! Whisper and scored against the sentence at pass mark 0.8, as a learner's take would be.
//!
//! - `gate_g4_speaking_loopback`: the first 60 sentences of each language with the `base` model;
//!   ignored, run by `scripts/gates/run --whisper`. It has no threshold: the first full run's means
//!   are recorded in `scripts/gates/thresholds.toml` (`[g4]`, with date and version).
//! - `g4_loopback_scaled`: the first 5 sentences of each language with `base`, else `tiny`, in
//!   every `cargo test`.
//!
//! The sentences are the `target` of each example of `content/<lang>/syllabus.yaml`, in syllabus
//! order, that has 3 to 8 UAX #29 words. Speech is synthesised by gate-only code with no new
//! dependency, and the text always goes to the synthesiser through a file: on Windows, PowerShell's
//! System.Speech `SpeechSynthesizer` with the first enabled installed voice whose culture's
//! two-letter name is the language; on Linux, `espeak-ng -v <lang> -w <out> -f <text file>`; on
//! macOS, `say` with a voice whose locale starts `<lang>_`. Each gate prints one line per language,
//! "G4 {lang}: mean {score} over {n} sentences ({model})", or one line naming why it was skipped: a
//! build without the `local-whisper` feature, an absent model file, or no voice for a language
//! (that language only). A gate that is skipped passes.

#[cfg(not(feature = "local-whisper"))]
const NO_WHISPER: &str =
    "G4 skipped: this build has no local Whisper; build with --features local-whisper";

/// Acceptance 18: "Gate G4: mean loopback score recorded at first run": 60 sentences per language,
/// `base`.
#[test]
#[ignore = "quality gate; run by scripts/gates/run --whisper"]
fn gate_g4_speaking_loopback() {
    #[cfg(feature = "local-whisper")]
    loopback::run(60, &["base"]);
    #[cfg(not(feature = "local-whisper"))]
    println!("{NO_WHISPER}");
}

/// Acceptance 18: "in verify, 5 sentences when a voice and the model exist, else skipped with a
/// message."
#[test]
fn g4_loopback_scaled() {
    #[cfg(feature = "local-whisper")]
    loopback::run(5, &["base", "tiny"]);
    #[cfg(not(feature = "local-whisper"))]
    println!("{NO_WHISPER}");
}

#[cfg(feature = "local-whisper")]
mod loopback {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use parley_audio::{WhisperLocal, decode_mono_i16, resample_to_16k, trim, wav_16k_mono};
    use parley_core::ai::speech_bounds_ms;
    use parley_core::api::language_name;
    use parley_core::audio::LocalTranscriber;
    use parley_core::content::numbers::{Numbers, load_number_words};
    use parley_core::content::{Lang, parse_syllabus};
    use parley_core::speech::score;

    /// The pass mark the gate scores at.
    const PASS: f64 = 0.8;

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

    fn read(relative: &str) -> String {
        let path = repo_root().join(relative);
        fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()))
    }

    /// UAX #29 words: `speech_bounds_ms` allows 3 x 500 ms a word at speed 1.0 and counts words as
    /// UAX #29 does, so its upper bound divided by 1,500 is the count.
    fn uax29_words(text: &str) -> u64 {
        speech_bounds_ms(text, 1.0).1 / 1_500
    }

    /// The first `count` example sentences of the language with 3 to 8 UAX #29 words, in syllabus
    /// order.
    fn sentences(lang: Lang, count: usize) -> Vec<String> {
        let file = format!("content/{}/syllabus.yaml", lang.as_str());
        let syllabus = parse_syllabus("syllabus.yaml", &read(&file))
            .unwrap_or_else(|e| panic!("{file} parses: {e:?}"));
        syllabus
            .units
            .iter()
            .flat_map(|unit| &unit.skills)
            .flat_map(|skill| &skill.examples)
            .map(|example| example.target.clone())
            .filter(|target| (3..=8).contains(&uax29_words(target)))
            .take(count)
            .collect()
    }

    /// A folder of its own under the system's temporary folder, empty.
    fn scratch(lang: Lang, count: usize) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "parley-g4-{}-{}-{count}",
            std::process::id(),
            lang.as_str()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir)
            .unwrap_or_else(|e| panic!("{} could not be made: {e}", dir.display()));
        dir
    }

    fn text_file(dir: &Path, i: usize) -> PathBuf {
        dir.join(format!("s{i}.txt"))
    }

    fn wav_file(dir: &Path, i: usize) -> PathBuf {
        dir.join(format!("s{i}.wav"))
    }

    /// Windows: one PowerShell run speaks every sentence of the folder with the first enabled
    /// installed voice of the language, or exits 3 when there is none. Paths and the count arrive
    /// as environment variables, the texts as UTF-8 files.
    const POWERSHELL: &str = r#"$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Speech
$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
$voice = $synth.GetInstalledVoices() | Where-Object { $_.Enabled -and $_.VoiceInfo.Culture.TwoLetterISOLanguageName -eq $env:PARLEY_G4_LANG } | Select-Object -First 1
if ($null -eq $voice) { exit 3 }
$synth.SelectVoice($voice.VoiceInfo.Name)
$dir = $env:PARLEY_G4_DIR
for ($i = 0; $i -lt [int]$env:PARLEY_G4_COUNT; $i++) {
  $text = [IO.File]::ReadAllText((Join-Path $dir "s$i.txt"), [Text.Encoding]::UTF8)
  $synth.SetOutputToWaveFile((Join-Path $dir "s$i.wav"))
  $synth.Speak($text)
}
$synth.SetOutputToNull()
$synth.Dispose()
exit 0
"#;

    /// Writes `s{i}.wav` for every `s{i}.txt` in `dir`, or says why it could not.
    fn synthesise(lang: Lang, dir: &Path, count: usize) -> Result<(), String> {
        let language = language_name(lang);
        if cfg!(target_os = "windows") {
            let script = dir.join("speak.ps1");
            fs::write(&script, POWERSHELL).map_err(|e| e.to_string())?;
            let status = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                ])
                .arg(&script)
                .env("PARLEY_G4_LANG", lang.as_str())
                .env("PARLEY_G4_DIR", dir)
                .env("PARLEY_G4_COUNT", count.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|e| format!("PowerShell could not start: {e}"))?;
            match status.code() {
                Some(0) => Ok(()),
                Some(3) => Err(format!("no {language} voice is installed")),
                _ => Err(format!("System.Speech could not speak {language}")),
            }
        } else if cfg!(target_os = "macos") {
            let voice =
                mac_voice(lang).ok_or_else(|| format!("no {language} voice is installed"))?;
            for i in 0..count {
                let status = Command::new("say")
                    .arg("-v")
                    .arg(&voice)
                    .args(["--file-format=WAVE", "--data-format=LEI16@16000", "-o"])
                    .arg(wav_file(dir, i))
                    .arg("-f")
                    .arg(text_file(dir, i))
                    .status()
                    .map_err(|e| format!("say could not start: {e}"))?;
                if !status.success() {
                    return Err(format!("say could not speak {language}"));
                }
            }
            Ok(())
        } else {
            for i in 0..count {
                let status = Command::new("espeak-ng")
                    .args(["-v", lang.as_str(), "-w"])
                    .arg(wav_file(dir, i))
                    .arg("-f")
                    .arg(text_file(dir, i))
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map_err(|_| format!("no {language} voice: espeak-ng is not installed"))?;
                if !status.success() {
                    return Err(format!("no {language} voice in espeak-ng"));
                }
            }
            Ok(())
        }
    }

    /// The first voice `say -v '?'` lists whose locale starts `<lang>_`. Each line is the name
    /// (which may hold spaces), the locale, then `#` and a sample sentence.
    fn mac_voice(lang: Lang) -> Option<String> {
        let output = Command::new("say").args(["-v", "?"]).output().ok()?;
        let listing = String::from_utf8_lossy(&output.stdout).into_owned();
        let prefix = format!("{}_", lang.as_str());
        listing.lines().find_map(|line| {
            let described = line.split('#').next()?.trim_end();
            let (name, locale) = described.rsplit_once(char::is_whitespace)?;
            locale.starts_with(&prefix).then(|| name.trim().to_string())
        })
    }

    /// The loopback score of one synthesised sentence: too quiet scores 0.
    fn loopback_score(
        whisper: &WhisperLocal,
        model: &Path,
        lang: Lang,
        sentence: &str,
        wav: &Path,
        numbers: &Numbers,
    ) -> f64 {
        let bytes = fs::read(wav).unwrap_or_else(|e| panic!("{}: {e}", wav.display()));
        let (samples, rate) = decode_mono_i16(&bytes)
            .unwrap_or_else(|| panic!("the synthesised {} decodes", wav.display()));
        let at_16k = resample_to_16k(&samples, rate);
        let Some(kept) = trim(&at_16k) else {
            return 0.0;
        };
        let take = wav_16k_mono(&at_16k[kept.start..kept.end]);
        let heard = whisper
            .transcribe(model, &take, lang)
            .unwrap_or_else(|e| panic!("local Whisper failed on \"{sentence}\": {e}"));
        score(lang, sentence, &heard, numbers, PASS).score
    }

    /// Runs the loopback over the first `count` sentences of each language with the first of
    /// `models` found in `models/whisper/`.
    pub fn run(count: usize, models: &[&str]) {
        let root = repo_root();
        let found = models.iter().find_map(|name| {
            let path = root
                .join("models")
                .join("whisper")
                .join(format!("ggml-{name}.bin"));
            path.is_file().then(|| (name.to_string(), path))
        });
        let Some((model_name, model)) = found else {
            let paths: Vec<String> = models
                .iter()
                .map(|name| format!("models/whisper/ggml-{name}.bin"))
                .collect();
            let smallest = models[models.len() - 1];
            println!(
                "G4 skipped: no Whisper model at {}; run parley whisper download {smallest}",
                paths.join(" or ")
            );
            return;
        };
        let whisper = WhisperLocal::new();
        for lang in Lang::ALL {
            let chosen = sentences(lang, count);
            assert!(
                !chosen.is_empty(),
                "{lang} has example sentences of 3 to 8 words"
            );
            let numbers =
                load_number_words(&read(&format!("content/{}/numbers.yaml", lang.as_str())))
                    .unwrap_or_else(|e| panic!("content/{lang}/numbers.yaml loads: {e:?}"));
            let dir = scratch(lang, count);
            for (i, sentence) in chosen.iter().enumerate() {
                fs::write(text_file(&dir, i), sentence)
                    .unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            }
            if let Err(reason) = synthesise(lang, &dir, chosen.len()) {
                println!("G4 {lang} skipped: {reason}");
                let _ = fs::remove_dir_all(dir);
                continue;
            }
            let scores: Vec<f64> = chosen
                .iter()
                .enumerate()
                .map(|(i, sentence)| {
                    loopback_score(
                        &whisper,
                        &model,
                        lang,
                        sentence,
                        &wav_file(&dir, i),
                        &numbers,
                    )
                })
                .collect();
            let _ = fs::remove_dir_all(dir);
            assert!(
                scores.iter().all(|s| (0.0..=1.0).contains(s)),
                "every score lies between 0 and 1: {scores:?}"
            );
            let mean = scores.iter().sum::<f64>() / scores.len() as f64;
            println!(
                "G4 {lang}: mean {mean:.3} over {} sentences ({model_name})",
                scores.len()
            );
        }
    }
}
