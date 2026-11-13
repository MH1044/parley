//! The operating system's voices (ADR-0014): `tts` on Windows (WinRT, OneCore voices), macOS
//! (AVFoundation) and Android; Speech Dispatcher's `spd-say` command on Linux.
//!
//! `tts`'s handle is reference-counted without atomics, so one thread owns it and receives
//! commands over a channel; its end-of-utterance callback announces the end of speech. On Linux
//! the `tts` backend would link `libspeechd` and need `libclang` to build, so the `spd-say`
//! command is used instead: Speech Dispatcher stays optional at run time (docs/ARCHITECTURE.md
//! §10), and a computer without it simply has no voices.

use std::sync::Arc;

use parley_core::audio::{AudioError, OsVoice, OsVoices, RateRange};

/// Called when an utterance ends by itself.
type OnEnd = Arc<dyn Fn() + Send + Sync>;

/// The rates a backend that reports none is given: half, normal and double speed.
const DEFAULT_RATES: RateRange = RateRange {
    min: 0.5,
    normal: 1.0,
    max: 2.0,
};

/// [`OsVoices`] of this computer.
pub struct SystemVoices {
    backend: backend::Backend,
}

impl std::fmt::Debug for SystemVoices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemVoices").finish_non_exhaustive()
    }
}

impl SystemVoices {
    /// The voices of this computer; `on_end` runs when an utterance ends by itself. Nothing
    /// blocks here: the speech engine starts on its own thread, and a computer without one has
    /// no voices.
    pub fn new(on_end: Arc<dyn Fn() + Send + Sync>) -> SystemVoices {
        SystemVoices {
            backend: backend::Backend::new(on_end),
        }
    }
}

impl OsVoices for SystemVoices {
    fn list(&self) -> Vec<OsVoice> {
        self.backend.list()
    }

    fn rates(&self) -> RateRange {
        self.backend.rates()
    }

    fn speak(&self, text: &str, voice: &str, rate: f32) -> Result<(), AudioError> {
        self.backend.speak(text, voice, rate)
    }

    fn stop(&self) {
        self.backend.stop();
    }
}

/// `spd-say -r` for a rate multiplier (0.5, 1.0, 2.0): round(100 × log2(rate)), clamped to
/// −100..100; 0 for a rate that is not a positive number.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn spd_rate(rate: f32) -> i32 {
    if !(rate.is_finite() && rate > 0.0) {
        return 0;
    }
    (100.0 * f64::from(rate).log2())
        .round()
        .clamp(-100.0, 100.0) as i32
}

/// The voices of `spd-say -L`: a header line (NAME, LANGUAGE, VARIANT), then one voice per line
/// in columns; a name may hold spaces, so the last two words are the language and the variant.
/// A name listed twice (several variants) is one voice.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_spd_voices(listing: &str) -> Vec<OsVoice> {
    let mut voices: Vec<OsVoice> = Vec::new();
    for line in listing.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.len() < 3 || words == ["NAME", "LANGUAGE", "VARIANT"] {
            continue;
        }
        let name = words[..words.len() - 2].join(" ");
        if voices.iter().any(|voice| voice.id == name) {
            continue;
        }
        voices.push(OsVoice {
            id: name.clone(),
            name,
            lang_tag: words[words.len() - 2].to_string(),
        });
    }
    voices
}

#[cfg(not(target_os = "linux"))]
mod backend {
    //! `tts` on one thread that owns its handle.

    use std::sync::mpsc::{self, Receiver, Sender};

    use parley_core::audio::{AudioError, OsVoice, RateRange};
    use tts::Tts;

    use super::{DEFAULT_RATES, OnEnd};

    enum Command {
        List(Sender<Vec<OsVoice>>),
        Rates(Sender<RateRange>),
        Speak {
            text: String,
            voice: String,
            rate: f32,
            started: Sender<Result<(), AudioError>>,
        },
        Stop,
    }

    pub(super) struct Backend {
        commands: Sender<Command>,
    }

    fn failed(error: tts::Error) -> AudioError {
        AudioError::Backend(error.to_string())
    }

    impl Backend {
        pub(super) fn new(on_end: OnEnd) -> Backend {
            let (commands, receiver) = mpsc::channel();
            // A thread that cannot be started leaves a computer with no voices.
            let _ = std::thread::Builder::new()
                .name("parley-voices".into())
                .spawn(move || serve(receiver, on_end));
            Backend { commands }
        }

        /// Sends a command and waits for its answer; none when the thread is gone.
        fn ask<T>(&self, command: impl FnOnce(Sender<T>) -> Command) -> Option<T> {
            let (reply, answer) = mpsc::channel();
            self.commands.send(command(reply)).ok()?;
            answer.recv().ok()
        }

        pub(super) fn list(&self) -> Vec<OsVoice> {
            self.ask(Command::List).unwrap_or_default()
        }

        pub(super) fn rates(&self) -> RateRange {
            self.ask(Command::Rates).unwrap_or(DEFAULT_RATES)
        }

        pub(super) fn speak(&self, text: &str, voice: &str, rate: f32) -> Result<(), AudioError> {
            self.ask(|started| Command::Speak {
                text: text.to_string(),
                voice: voice.to_string(),
                rate,
                started,
            })
            .unwrap_or_else(|| Err(AudioError::Backend("the speech engine stopped".into())))
        }

        pub(super) fn stop(&self) {
            let _ = self.commands.send(Command::Stop);
        }
    }

    fn voices(tts: &Tts) -> Vec<OsVoice> {
        tts.voices()
            .unwrap_or_default()
            .into_iter()
            .map(|voice| OsVoice {
                id: voice.id(),
                name: voice.name(),
                lang_tag: voice.language().as_str().to_string(),
            })
            .collect()
    }

    fn speak(tts: &mut Tts, text: &str, voice_id: &str, rate: f32) -> Result<(), AudioError> {
        let voice = tts
            .voices()
            .map_err(failed)?
            .into_iter()
            .find(|voice| voice.id() == voice_id)
            .ok_or_else(|| AudioError::Backend("that voice is not installed".into()))?;
        tts.set_voice(&voice).map_err(failed)?;
        if tts.supported_features().rate {
            let rate = rate.max(tts.min_rate()).min(tts.max_rate());
            tts.set_rate(rate).map_err(failed)?;
        }
        // Interrupts whatever is being said.
        tts.speak(text, true).map_err(failed)?;
        Ok(())
    }

    fn serve(commands: Receiver<Command>, on_end: OnEnd) {
        // The handle never leaves this thread.
        let mut tts = Tts::default().ok();
        if let Some(engine) = &tts
            && engine.supported_features().utterance_callbacks
        {
            let _ = engine.on_utterance_end(Some(Box::new(move |_| on_end())));
        }
        for command in commands {
            match command {
                Command::List(reply) => {
                    let _ = reply.send(tts.as_ref().map(voices).unwrap_or_default());
                }
                Command::Rates(reply) => {
                    let rates = tts.as_ref().map_or(DEFAULT_RATES, |engine| RateRange {
                        min: engine.min_rate(),
                        normal: engine.normal_rate(),
                        max: engine.max_rate(),
                    });
                    let _ = reply.send(rates);
                }
                Command::Speak {
                    text,
                    voice,
                    rate,
                    started,
                } => {
                    let result = match tts.as_mut() {
                        Some(engine) => speak(engine, &text, &voice, rate),
                        None => Err(AudioError::Backend("no speech engine".into())),
                    };
                    let _ = started.send(result);
                }
                Command::Stop => {
                    if let Some(engine) = tts.as_mut() {
                        let _ = engine.stop();
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod backend {
    //! Speech Dispatcher's `spd-say` command: `-L` lists the voices, `--wait` speaks and returns
    //! when the utterance ends (or is cancelled), `--cancel` stops what is being said.

    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use parley_core::audio::{AudioError, OsVoice, RateRange};

    use super::{DEFAULT_RATES, OnEnd, parse_spd_voices, spd_rate};

    const SPD_SAY: &str = "spd-say";
    /// How long `spd-say -L` may take (it may start the Speech Dispatcher server first).
    const LIST_TIMEOUT: Duration = Duration::from_secs(5);

    pub(super) struct Backend {
        on_end: OnEnd,
        /// The voices, listed once.
        voices: Mutex<Option<Vec<OsVoice>>>,
        /// The number of the latest utterance or stop; an utterance announces its end only while
        /// it is current.
        current: Arc<AtomicU64>,
    }

    fn quiet(command: &mut Command) -> &mut Command {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
    }

    /// `spd-say -L`, or no voices when the command is missing, fails or takes too long.
    fn listed() -> Vec<OsVoice> {
        let (reply, answer) = mpsc::channel();
        std::thread::spawn(move || {
            let output = Command::new(SPD_SAY)
                .arg("-L")
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output();
            let _ = reply.send(output);
        });
        match answer.recv_timeout(LIST_TIMEOUT) {
            Ok(Ok(output)) if output.status.success() => {
                parse_spd_voices(&String::from_utf8_lossy(&output.stdout))
            }
            _ => Vec::new(),
        }
    }

    impl Backend {
        pub(super) fn new(on_end: OnEnd) -> Backend {
            Backend {
                on_end,
                voices: Mutex::new(None),
                current: Arc::new(AtomicU64::new(0)),
            }
        }

        pub(super) fn list(&self) -> Vec<OsVoice> {
            let mut voices = self.voices.lock().unwrap_or_else(PoisonError::into_inner);
            voices.get_or_insert_with(listed).clone()
        }

        pub(super) fn rates(&self) -> RateRange {
            DEFAULT_RATES
        }

        pub(super) fn speak(&self, text: &str, voice: &str, rate: f32) -> Result<(), AudioError> {
            let number = self.current.fetch_add(1, Ordering::SeqCst) + 1;
            let language = self
                .list()
                .into_iter()
                .find(|listed| listed.id == voice)
                .map(|listed| listed.lang_tag);
            let mut command = Command::new(SPD_SAY);
            // Cancel what is being said, speak, and wait for the end of this utterance.
            command
                .arg("--cancel")
                .arg("--wait")
                .arg("-r")
                .arg(spd_rate(rate).to_string());
            if let Some(language) = language {
                command.arg("-l").arg(language);
            }
            command.arg("-y").arg(voice).arg("--").arg(text);
            let mut child = quiet(&mut command)
                .spawn()
                .map_err(|e| AudioError::Backend(e.to_string()))?;
            let current = self.current.clone();
            let on_end = self.on_end.clone();
            std::thread::spawn(move || {
                let ended = child.wait().is_ok_and(|status| status.success());
                if ended && current.load(Ordering::SeqCst) == number {
                    on_end();
                }
            });
            Ok(())
        }

        pub(super) fn stop(&self) {
            self.current.fetch_add(1, Ordering::SeqCst);
            // Waited for on its own thread, so the caller never waits for Speech Dispatcher.
            std::thread::spawn(|| {
                let _ = quiet(Command::new(SPD_SAY).arg("--cancel")).status();
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spd_say_rates_are_a_hundred_times_the_log2_of_the_multiplier() {
        assert_eq!(spd_rate(1.0), 0);
        assert_eq!(spd_rate(0.5), -100);
        assert_eq!(spd_rate(2.0), 100);
        assert_eq!(spd_rate(0.75), -42);
        assert_eq!(spd_rate(0.25), -100, "clamped");
        assert_eq!(spd_rate(6.0), 100, "clamped");
        assert_eq!(spd_rate(0.0), 0);
        assert_eq!(spd_rate(f32::NAN), 0);
    }

    #[test]
    fn the_spd_say_listing_is_read_by_columns() {
        let listing = "                     NAME                 LANGUAGE                  VARIANT\n\
                       \x20               Afrikaans                       af                     none\n\
                       \x20         Spanish (Spain)                       es                     none\n\
                       \x20         Spanish (Spain)                       es                    klatt\n\
                       \x20                 es-419                    es-419                     none\n\
                       \n\
                       broken line\n";
        let voices = parse_spd_voices(listing);
        assert_eq!(
            voices,
            vec![
                OsVoice {
                    id: "Afrikaans".into(),
                    name: "Afrikaans".into(),
                    lang_tag: "af".into()
                },
                OsVoice {
                    id: "Spanish (Spain)".into(),
                    name: "Spanish (Spain)".into(),
                    lang_tag: "es".into()
                },
                OsVoice {
                    id: "es-419".into(),
                    name: "es-419".into(),
                    lang_tag: "es-419".into()
                },
            ]
        );
        assert!(parse_spd_voices("").is_empty());
    }
}
