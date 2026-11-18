//! The service every Tauri command calls (docs/modules/04-app-shell-and-offline-course.md): one
//! [`Parley`] per app, behind a mutex, holding the bundled course, the learner's store and
//! settings, the lessons and placement tests in progress, from module 09 the audio it plays
//! through, and from module 10 the microphone and the takes it records.
//!
//! The Tauri commands only marshal: each one calls one method here and passes its result or
//! [`AppError`] back. Everything the service needs from the outside world arrives in [`Env`]
//! (clock, randomness, files, network), so the whole app runs in tests with fakes, and offline
//! with [`crate::env::FailingNet`] (invariant I3).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;

use crate::api::{self, AppInfo, CacheCleared, ExportSummary, ImportReport, Launch};
use crate::audio::{Audio, Speaking};
use crate::backup::{Backup, BackupError, parse_backup};
use crate::content::{
    Forms, Item, Lang, Numbers, Syllabus, derive_items, load_number_words, load_numbers,
    parse_syllabus,
};
use crate::env::{Clock, Fs, FsError, Net, Rng};
use crate::error::{AppError, ErrorCode};
use crate::exercise::LessonRun;
use crate::settings::{
    AiSettings, SETTINGS_FILE, SETTINGS_NOTICE, Settings, apply_patch, parse_settings,
};
use crate::srs::Placement;
use crate::store::{Store, StoreError};

mod audio;
mod course;
mod judge;
mod lesson;
mod pack;
mod placement;
mod speak;
mod stats;
mod story;

pub use audio::{PrefetchBegin, SpeechJob, SpeechOutcome, SpeechWait, TtsBegin};
pub use judge::{ExplainJob, ExplainOutcome, Explained, JudgeBegin, JudgeJob, JudgeOutcome};
pub use lesson::{AiAccess, JobOutcome, LessonBegin, LessonJob};
pub use pack::{InstalledPack, LockedPack, PackInstallError, PackSentence, attributions};
pub use speak::{
    DownloadBegin, DownloadOutcome, ModelDownload, SpeakBegin, TranscribeJob, TranscribeOutcome,
};

/// The learner's data file, under the data directory.
pub const STORE_FILE: &str = "parley.sqlite3";
/// The file an import writes before it replaces the data file.
pub const IMPORT_FILE: &str = "parley.sqlite3.import";
/// Where Delete everything moves the data file before it removes anything else.
pub const DELETING_FILE: &str = "parley.sqlite3.deleting";
/// The AI response cache (module 06 onwards).
pub const AI_CACHE_FILE: &str = "cache/ai.sqlite3";

/// The bundled content of one language: the text of `syllabus.yaml`, `forms.tsv` and, when the
/// course has one, `numbers.yaml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseFiles {
    pub syllabus_yaml: String,
    pub forms_tsv: String,
    pub numbers_yaml: Option<String>,
}

/// Where the app keeps its data and what it ships with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Absolute; SQLite files (the store, packs) are opened under it, and every other file goes
    /// through [`Env::fs`] relative to it.
    pub data_dir: PathBuf,
    /// The bundled content of every language.
    pub courses: BTreeMap<Lang, CourseFiles>,
    /// The text of `content/packs.lock.toml`.
    pub packs_lock: String,
}

/// The outside world, injected.
pub struct Env {
    pub clock: Box<dyn Clock + Send + Sync>,
    pub rng: Box<dyn Rng + Send>,
    pub fs: Box<dyn Fs + Send + Sync>,
    pub net: Box<dyn Net + Send + Sync>,
}

/// A bundled course, parsed once.
pub(crate) struct Course {
    pub syllabus: Syllabus,
    pub forms: Forms,
    pub numbers: Vec<String>,
    /// The words for 0 to 100, entry n the number n (module 10's speaking checks spell digits
    /// with them); empty without a numbers file, and digits then stay as written.
    pub number_words: Numbers,
    /// Every item of the syllabus (`derive_items`), with its accepted answers.
    pub items: Vec<Item>,
}

impl Course {
    fn load(files: &CourseFiles) -> Option<Course> {
        let syllabus = parse_syllabus("syllabus.yaml", &files.syllabus_yaml).ok()?;
        let forms = Forms::load_tsv(files.forms_tsv.as_bytes()).ok()?;
        let (numbers, number_words) = match &files.numbers_yaml {
            Some(text) => (load_numbers(text).ok()?, load_number_words(text).ok()?),
            None => (Vec::new(), Numbers::default()),
        };
        let items = derive_items(&syllabus, &forms);
        Some(Course {
            syllabus,
            forms,
            numbers,
            number_words,
            items,
        })
    }
}

/// A lesson or review in progress.
pub(crate) struct ActiveRun {
    pub lang: Lang,
    pub run: LessonRun,
    /// The skill of a learn-mode lesson, whose next lesson may be prepared when it finishes.
    pub skill_id: Option<String>,
    /// A 402 stopped automatic judging for the rest of this run (module 08).
    pub judging_stopped: bool,
}

/// A placement test in progress and its current question.
pub(crate) struct ActivePlacement {
    pub lang: Lang,
    pub placement: Placement,
    pub question: api::PlacementQuestion,
    /// What the test has asked about each skill, by skill id.
    pub asked: HashMap<String, placement::Asked>,
}

/// The app's state and every command's logic.
pub struct Parley {
    pub(crate) config: Config,
    pub(crate) env: Env,
    pub(crate) content: BTreeMap<Lang, Arc<Course>>,
    /// The open store, or the store's message when the data file could not be opened.
    pub(crate) store: Result<Store, String>,
    pub(crate) settings: Settings,
    /// The settings file could not be read; cleared by the next save.
    pub(crate) settings_notice: bool,
    pub(crate) runs: HashMap<String, ActiveRun>,
    /// Stories being read (module 07).
    pub(crate) story_runs: HashMap<String, story::StoryRun>,
    /// The fresh lesson or story each skill is waiting for, by a number its job carries; a job
    /// whose number is no longer here was superseded (Use the course pack instead).
    pub(crate) pending: HashMap<String, u64>,
    pub(crate) next_job: u64,
    pub(crate) placements: HashMap<String, ActivePlacement>,
    /// The speakers, OS voices and decoder (module 09).
    pub(crate) audio: Audio,
    /// **Can't listen now** and **Can't speak now**: until when each is on.
    pub(crate) skips: audio::Skips,
    /// The natural sentences being fetched, by cache key, so each is asked for once (module 09).
    pub(crate) flights: audio::Flights,
    /// The microphone, local Whisper and model registry (module 10).
    pub(crate) speaking: Speaking,
    /// The takes recorded while the app runs (module 10).
    pub(crate) takes: speak::Takes,
    /// Valid transcripts of takes, by the key of docs/PUBLIK-INTEGRATION.md §6.1; in memory only.
    pub(crate) transcripts: HashMap<String, String>,
    /// Transcription replies that could not be used, by key, so a Try again is a new attempt.
    pub(crate) failed_attempts: HashMap<String, u32>,
    /// The local models being downloaded.
    pub(crate) downloads: speak::Downloads,
}

// Messages (docs/modules/04-app-shell-and-offline-course.md, Interfaces).

/// "Unknown value {v} for {key}."
pub(crate) fn unknown_value(value: &str, key: &str) -> AppError {
    AppError::new(
        ErrorCode::InvalidInput,
        format!("Unknown value {value} for {key}."),
    )
}

pub(crate) fn not_started() -> AppError {
    AppError::new(ErrorCode::NotFound, "This course has not been started.")
}

pub(crate) fn no_such_skill() -> AppError {
    AppError::new(ErrorCode::NotFound, "This skill is not in the course.")
}

pub(crate) fn not_available() -> AppError {
    AppError::new(ErrorCode::NotFound, "This exercise is no longer available.")
}

pub(crate) fn cannot_read() -> AppError {
    AppError::new(ErrorCode::NotFound, "Parley could not read that file.")
}

pub(crate) fn cannot_write() -> AppError {
    AppError::new(ErrorCode::Storage, "Parley could not write that file.")
}

/// Delete everything could not move the data file aside, so it removed nothing.
fn delete_in_use() -> AppError {
    AppError::new(
        ErrorCode::Busy,
        "Parley could not delete your progress because another program is using it. Close it \
         and try again.",
    )
}

/// Restore could not put the new data file in place of the old, so nothing changed.
fn restore_in_use() -> AppError {
    AppError::new(
        ErrorCode::Busy,
        "Parley could not replace your progress because another program is using it. Close it \
         and try again.",
    )
}

/// Whether a Delete everything that answered `error` left every file as it was: the word was not
/// typed (`invalid_input`), or another program has the data file open (`busy`). Any other
/// failure came after the data file was moved aside, so the progress is gone and whatever else
/// the delete clears (the AI status, in the desktop app) goes with it.
pub fn nothing_was_deleted(error: &AppError) -> bool {
    matches!(error.code, ErrorCode::InvalidInput | ErrorCode::Busy)
}

/// The data file cannot be opened or used; the details are the store's message.
pub(crate) fn data_file_error(details: &str) -> AppError {
    AppError::new(ErrorCode::Storage, "Parley could not open its data file.")
        .with_action(json!({ "details": details }))
}

impl From<StoreError> for AppError {
    fn from(e: StoreError) -> Self {
        data_file_error(&e.to_string())
    }
}

/// The open store, or the data-file error.
pub(crate) fn open_store(store: &mut Result<Store, String>) -> Result<&mut Store, AppError> {
    store.as_mut().map_err(|details| data_file_error(details))
}

/// A session id: the prefix, `-` and 16 lower-case hex digits from the injected generator.
pub(crate) fn session_id(prefix: &str, rng: &mut dyn Rng) -> String {
    format!("{prefix}-{:016x}", rng.next_u64())
}

/// Removes a file; a file that is not there counts as removed. True when one was removed.
fn remove_file(fs: &dyn Fs, path: &str) -> Result<bool, AppError> {
    match fs.remove(Path::new(path)) {
        Ok(()) => Ok(true),
        Err(FsError::NotFound(_)) => Ok(false),
        Err(FsError::Io(_)) => Err(cannot_write()),
    }
}

/// The settings in `settings.json`, and whether the notice shows: a missing file gives the
/// defaults quietly; a file that cannot be read or parsed gives the defaults and the notice.
fn load_settings(fs: &dyn Fs) -> (Settings, bool) {
    match fs.read(Path::new(SETTINGS_FILE)) {
        Ok(bytes) => match parse_settings(&bytes) {
            Ok(settings) => (settings, false),
            Err(_) => (Settings::default(), true),
        },
        Err(FsError::NotFound(_)) => (Settings::default(), false),
        Err(FsError::Io(_)) => (Settings::default(), true),
    }
}

impl Parley {
    /// Opens `parley.sqlite3` under the data directory and reads `settings.json`. Never fails: a
    /// store that cannot be opened is reported by `app_start` and every later command; a
    /// settings file that cannot be read gives the defaults and a notice.
    pub fn new(config: Config, env: Env) -> Parley {
        let content = config
            .courses
            .iter()
            .filter_map(|(lang, files)| Course::load(files).map(|course| (*lang, Arc::new(course))))
            .filter(|(lang, course)| course.syllabus.lang == *lang)
            .collect();
        let store = Store::open(&config.data_dir.join(STORE_FILE)).map_err(|e| e.to_string());
        let (settings, settings_notice) = load_settings(&*env.fs);
        Parley {
            config,
            env,
            content,
            store,
            settings,
            settings_notice,
            runs: HashMap::new(),
            story_runs: HashMap::new(),
            pending: HashMap::new(),
            next_job: 0,
            placements: HashMap::new(),
            audio: Audio::default(),
            skips: audio::Skips::new(),
            flights: audio::Flights::new(),
            speaking: Speaking::default(),
            takes: speak::Takes::new(),
            transcripts: HashMap::new(),
            failed_attempts: HashMap::new(),
            downloads: speak::Downloads::default(),
        }
    }

    /// A course language the app ships: "Unknown value {v} for lang." otherwise.
    pub(crate) fn lang(&self, lang: &str) -> Result<Lang, AppError> {
        lang.parse::<Lang>()
            .ok()
            .filter(|l| self.content.contains_key(l))
            .ok_or_else(|| unknown_value(lang, "lang"))
    }

    /// The first command of a launch: removes the takes a previous run left behind (unless **Keep
    /// my recordings** is on; module 10), brings every started course up to date with the bundled
    /// syllabus and says whether onboarding is done (a course is active and started).
    pub fn app_start(&mut self) -> Result<Launch, AppError> {
        // Removing every take at launch means a crash leaves none behind.
        self.remove_takes_unless_kept();
        let store = open_store(&mut self.store)?;
        let now = self.env.clock.now_ms();
        for row in store.courses()? {
            if let Some(course) = self.content.get(&row.lang) {
                store.start_course(&course.syllabus, &course.forms, now)?;
            }
        }
        let active = self
            .settings
            .active_course
            .as_deref()
            .and_then(|code| code.parse::<Lang>().ok());
        let onboarded = match active {
            Some(lang) => store.course(lang)?.is_some(),
            None => false,
        };
        Ok(Launch {
            onboarded,
            active_course: self.settings.active_course.clone(),
            notice: self.settings_notice.then(|| SETTINGS_NOTICE.to_string()),
        })
    }

    /// The version and the data directory.
    pub fn app_info(&self) -> AppInfo {
        api::app_info(&self.config.data_dir)
    }

    /// Deletes the store, the settings file, the pack files, the AI caches (answers and
    /// natural-voice audio), the takes and the local speech models, then opens a fresh store.
    /// `confirm_text` must be `delete` after trimming. All or nothing: when the store cannot be
    /// moved aside first (another program has it open), nothing is removed and the answer is
    /// `busy`; see [`nothing_was_deleted`].
    pub fn data_delete_all(&mut self, confirm_text: &str) -> Result<(), AppError> {
        if confirm_text.trim() != "delete" {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "Type delete to confirm.",
            ));
        }
        let mut packs: Vec<String> = Vec::new();
        if let Ok(store) = &self.store
            && let Ok(rows) = store.courses()
        {
            for row in rows {
                if let Some(version) = &row.pack_version {
                    packs.push(pack::pack_file(row.lang, version));
                }
            }
        }
        if let Ok(lock) = crate::content::parse_packs_lock(&self.config.packs_lock) {
            for (code, entry) in &lock.langs {
                if let Ok(lang) = code.parse::<Lang>() {
                    packs.push(pack::pack_file(lang, &entry.version));
                }
            }
        }
        let model_files = self.model_files();
        // All or nothing. The store is closed, then its file is moved aside before anything else
        // is touched. On Windows a file another program has open (a second copy of Parley, a
        // backup or sync tool) can be neither renamed nor removed: removing file by file used to
        // take the settings and caches and leave the progress, so the next launch ran onboarding
        // over the old streak and XP. Now a store that cannot be moved aside means nothing at all
        // is removed, and the store is opened again as it was.
        self.store = Err("The data file is being replaced.".to_string());
        let fs = &*self.env.fs;
        match fs.rename(Path::new(STORE_FILE), Path::new(DELETING_FILE)) {
            Ok(()) | Err(FsError::NotFound(_)) => {}
            Err(FsError::Io(_)) => {
                self.store =
                    Store::open(&self.config.data_dir.join(STORE_FILE)).map_err(|e| e.to_string());
                return Err(delete_in_use());
            }
        }
        // From here the progress is gone. Every other removal is attempted even when one fails,
        // the moved store goes last, and a fresh store is always opened afterwards, so the app
        // keeps working and the next launch starts afresh; the first failure is reported. A moved
        // store that could not be removed is never opened again, and the next Delete everything
        // removes it.
        self.runs.clear();
        self.story_runs.clear();
        self.pending.clear();
        self.placements.clear();
        self.takes.clear();
        self.transcripts.clear();
        self.failed_attempts.clear();
        let mut paths: Vec<String> = [
            "parley.sqlite3-journal",
            IMPORT_FILE,
            SETTINGS_FILE,
            AI_CACHE_FILE,
        ]
        .iter()
        .map(|path| path.to_string())
        .collect();
        for path in &packs {
            paths.push(path.clone());
            paths.push(format!("{path}.part"));
        }
        for dir in [audio::AUDIO_DIR, speak::RECORDINGS_DIR] {
            if let Ok(names) = fs.list(Path::new(dir)) {
                for name in names {
                    paths.push(format!("{dir}/{name}"));
                }
            }
        }
        paths.extend(model_files);
        paths.push(DELETING_FILE.to_string());
        let mut first_failure: Option<AppError> = None;
        for path in &paths {
            if let Err(e) = remove_file(fs, path) {
                first_failure.get_or_insert(e);
            }
        }
        (self.settings, self.settings_notice) = load_settings(fs);
        self.store = Store::open(&self.config.data_dir.join(STORE_FILE)).map_err(|e| e.to_string());
        if let Some(failure) = first_failure {
            return Err(failure);
        }
        open_store(&mut self.store)?;
        Ok(())
    }

    /// Removes a cache and says how many files went: `ai` removes `cache/ai.sqlite3`; `audio`
    /// removes every file in `cache/audio/` and the `audio_cache` rows (module 09); `all` does
    /// both. Progress (`parley.sqlite3`) is never touched.
    pub fn cache_clear(&mut self, kind: &str) -> Result<CacheCleared, AppError> {
        let (audio, ai) = match kind {
            "ai" => (false, true),
            "audio" => (true, false),
            "all" => (true, true),
            other => return Err(unknown_value(other, "kind")),
        };
        let mut files_removed = 0;
        if audio {
            files_removed += self.clear_audio_files()?;
        }
        if ai && remove_file(&*self.env.fs, AI_CACHE_FILE)? {
            files_removed += 1;
        }
        Ok(CacheCleared { files_removed })
    }

    pub fn settings_get(&self) -> Settings {
        self.settings.clone()
    }

    /// Applies a patch, saves the result and returns it; nothing changes on a refusal.
    pub fn settings_set(&mut self, patch: &serde_json::Value) -> Result<Settings, AppError> {
        let next = apply_patch(&self.settings, patch)?;
        self.save_settings(next.clone())?;
        Ok(next)
    }

    /// Saves the AI settings an AI command changed (the provider, the own key's endpoint and
    /// models, the local server), through the same checks as `settings_set`. The own key itself is
    /// never part of them.
    pub fn save_ai_settings(&mut self, ai: &AiSettings) -> Result<Settings, AppError> {
        let ai = serde_json::to_value(ai).expect("AI settings serialise");
        self.settings_set(&json!({ "ai": ai }))
    }

    /// Writes the settings file and keeps the settings.
    pub(crate) fn save_settings(&mut self, settings: Settings) -> Result<(), AppError> {
        self.env
            .fs
            .write(Path::new(SETTINGS_FILE), settings.to_file_text().as_bytes())
            .map_err(|_| cannot_write())?;
        self.settings = settings;
        self.settings_notice = false;
        Ok(())
    }

    /// Writes a backup to `dest_path` (a path the learner picked).
    pub fn backup_export(&mut self, dest_path: &Path) -> Result<ExportSummary, AppError> {
        let store = open_store(&mut self.store)?;
        let tables = store.dump()?;
        let backup = Backup::new(&self.settings, tables, self.env.clock.now_ms());
        let text = backup.to_file_text();
        self.env
            .fs
            .write(dest_path, text.as_bytes())
            .map_err(|_| cannot_write())?;
        Ok(ExportSummary {
            bytes: text.len() as u64,
            courses: backup.courses.len(),
            cards: backup.cards.len(),
            reviews: backup.review_log.len(),
        })
    }

    /// Replaces the learner's data with a backup. The file is checked first; then a new store is
    /// written beside the data file (courses, each rebuilt from the bundled syllabus, then the
    /// other tables) and renamed over it only once complete, so a failure leaves the data as it
    /// was. Works when the current data file cannot be opened. A data file that cannot be
    /// replaced (another program has it open) answers `busy` and leaves no new file behind.
    pub fn backup_import(&mut self, src_path: &Path) -> Result<ImportReport, AppError> {
        let bytes = self.env.fs.read(src_path).map_err(|_| cannot_read())?;
        let backup = parse_backup(&bytes).map_err(|e| {
            let message = match e {
                BackupError::NotABackup => crate::backup::NOT_A_BACKUP,
                BackupError::Newer => crate::backup::NEWER_BACKUP,
            };
            AppError::new(ErrorCode::InvalidInput, message)
        })?;
        let now = self.env.clock.now_ms();
        let fs = &*self.env.fs;
        remove_file(fs, IMPORT_FILE)?;
        let report = {
            let mut fresh =
                Store::open(&self.config.data_dir.join(IMPORT_FILE)).map_err(AppError::from)?;
            let tables = backup.tables();
            let written = (|| {
                fresh.restore_courses(&tables.courses)?;
                for course in &tables.courses {
                    if let Some(bundled) = course
                        .lang
                        .parse::<Lang>()
                        .ok()
                        .and_then(|lang| self.content.get(&lang))
                    {
                        fresh.start_course(&bundled.syllabus, &bundled.forms, now)?;
                    }
                }
                fresh.restore_rows(&tables)
            })();
            match written {
                Ok(report) => report,
                Err(_) => {
                    drop(fresh);
                    remove_file(fs, IMPORT_FILE)?;
                    return Err(AppError::new(
                        ErrorCode::InvalidInput,
                        crate::backup::NOT_A_BACKUP,
                    ));
                }
            }
        };
        // The current store is closed before the new file takes its place.
        self.store = Err("The data file is being replaced.".to_string());
        let renamed = fs.rename(Path::new(IMPORT_FILE), Path::new(STORE_FILE));
        // The store is open again whatever happens next: the new file, or the old one when the
        // rename failed.
        self.store = Store::open(&self.config.data_dir.join(STORE_FILE)).map_err(|e| e.to_string());
        if renamed.is_err() {
            // On Windows a data file another program has open cannot be replaced. The progress
            // stays as it was, lessons in progress included, and the new file goes.
            let _ = remove_file(fs, IMPORT_FILE);
            return Err(restore_in_use());
        }
        self.runs.clear();
        self.story_runs.clear();
        self.pending.clear();
        self.placements.clear();
        open_store(&mut self.store)?;
        // The backup's settings apply now; a settings file that cannot be written is reported,
        // with the progress already restored.
        self.settings = backup.settings.clone();
        self.settings_notice = false;
        self.save_settings(backup.settings.clone())?;
        Ok(ImportReport {
            courses: backup.courses.len(),
            items: report.items,
            cards: report.cards,
            reviews: report.reviews,
            skipped_cards: report.skipped_cards,
        })
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! A `Parley` over the spec's example syllabus in a temporary directory, with a fixed clock,
    //! a seeded generator, the real filesystem under the directory and a scripted network.

    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::content::test_support::EXAMPLE_SYLLABUS;
    use crate::env::{HttpRequest, HttpResponse, NetError, SplitMix64};

    /// The example syllabus's forms table, as text.
    pub fn example_forms_tsv() -> String {
        let forms = crate::content::test_support::example_forms();
        let mut text = String::from("# source: test; sha256: -; date: 2026-01-01\n");
        for row in forms.rows() {
            let gender = row.gender.map_or("", |g| g.as_str());
            text.push_str(&format!(
                "{}\t{}\t{}\t{gender}\t{}\n",
                row.form, row.lemma, row.pos, row.tags
            ));
        }
        text
    }

    /// A clock the test moves: milliseconds shared with the test, and a fixed UTC offset.
    pub struct TestClock(pub Arc<AtomicI64>, pub i32);

    impl Clock for TestClock {
        fn now_ms(&self) -> i64 {
            self.0.load(Ordering::SeqCst)
        }
        fn utc_offset_minutes(&self) -> i32 {
            self.1
        }
    }

    /// Files under a root; absolute paths as they are.
    pub struct TestFs(pub PathBuf);

    impl TestFs {
        fn full(&self, path: &Path) -> PathBuf {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                self.0.join(path)
            }
        }
    }

    fn fs_error(path: &Path, e: std::io::Error) -> FsError {
        if e.kind() == std::io::ErrorKind::NotFound {
            FsError::NotFound(path.to_path_buf())
        } else {
            FsError::Io(e.to_string())
        }
    }

    impl Fs for TestFs {
        fn read(&self, path: &Path) -> Result<Vec<u8>, FsError> {
            std::fs::read(self.full(path)).map_err(|e| fs_error(path, e))
        }
        fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), FsError> {
            let full = self.full(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).map_err(|e| fs_error(path, e))?;
            }
            std::fs::write(full, bytes).map_err(|e| fs_error(path, e))
        }
        fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
            std::fs::rename(self.full(from), self.full(to)).map_err(|e| fs_error(from, e))
        }
        fn remove(&self, path: &Path) -> Result<(), FsError> {
            std::fs::remove_file(self.full(path)).map_err(|e| fs_error(path, e))
        }
        fn list(&self, dir: &Path) -> Result<Vec<String>, FsError> {
            let entries = match std::fs::read_dir(self.full(dir)) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                Err(e) => return Err(fs_error(dir, e)),
            };
            let mut names: Vec<String> = entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().is_file())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            Ok(names)
        }
    }

    /// A network that answers every request with the same response, and counts requests.
    pub struct ScriptedNet {
        pub response: Result<HttpResponse, NetError>,
        pub requests: Arc<Mutex<Vec<String>>>,
    }

    impl Net for ScriptedNet {
        fn send(&self, req: HttpRequest) -> Result<HttpResponse, NetError> {
            if let Ok(mut requests) = self.requests.lock() {
                requests.push(req.url);
            }
            self.response.clone()
        }
    }

    pub fn config(dir: &Path, packs_lock: &str) -> Config {
        let mut courses = BTreeMap::new();
        courses.insert(
            Lang::Es,
            CourseFiles {
                syllabus_yaml: EXAMPLE_SYLLABUS.to_string(),
                forms_tsv: example_forms_tsv(),
                numbers_yaml: None,
            },
        );
        Config {
            data_dir: dir.to_path_buf(),
            courses,
            packs_lock: packs_lock.to_string(),
        }
    }

    pub const NO_PACKS: &str = "schema = \"parley.packs/1\"\n";

    /// Midday UTC on 2026-01-05 (a Monday).
    pub const MONDAY_NOON: i64 = 20_458 * crate::srs::DAY_MS + crate::srs::DAY_MS / 2;

    pub fn parley_at(
        dir: &Path,
        packs_lock: &str,
        net: Box<dyn Net + Send + Sync>,
        now: Arc<AtomicI64>,
    ) -> Parley {
        Parley::new(
            config(dir, packs_lock),
            Env {
                clock: Box::new(TestClock(now, 0)),
                rng: Box::new(SplitMix64::new(7)),
                fs: Box::new(TestFs(dir.to_path_buf())),
                net,
            },
        )
    }

    pub fn parley_with(dir: &Path, packs_lock: &str, net: Box<dyn Net + Send + Sync>) -> Parley {
        parley_at(dir, packs_lock, net, Arc::new(AtomicI64::new(MONDAY_NOON)))
    }

    pub fn parley(dir: &Path) -> Parley {
        parley_with(dir, NO_PACKS, Box::new(crate::env::FailingNet))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

    use super::test_support::*;
    use super::*;
    use serde_json::json;

    #[test]
    fn a_new_directory_starts_with_onboarding_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = parley(dir.path());
        let launch = app.app_start().unwrap();
        assert_eq!(
            launch,
            Launch {
                onboarded: false,
                active_course: None,
                notice: None
            }
        );
        assert_eq!(app.settings_get(), Settings::default());
        assert_eq!(app.app_info().version, crate::api::VERSION);
        assert_eq!(
            app.course_start("it").unwrap_err().message,
            "Unknown value it for lang."
        );
        app.course_start("es").unwrap();
        let launch = app.app_start().unwrap();
        assert!(launch.onboarded);
        assert_eq!(launch.active_course.as_deref(), Some("es"));
    }

    #[test]
    fn a_corrupt_settings_file_gives_defaults_and_the_notice_until_saved() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), "{not json").unwrap();
        let mut app = parley(dir.path());
        assert_eq!(app.settings_get(), Settings::default());
        assert_eq!(
            app.app_start().unwrap().notice.as_deref(),
            Some(SETTINGS_NOTICE)
        );
        app.settings_set(&json!({"lesson_length": 10})).unwrap();
        assert_eq!(app.app_start().unwrap().notice, None);
        let again = parley(dir.path());
        assert_eq!(again.settings_get().lesson_length, 10);
    }

    #[test]
    fn a_corrupt_data_file_is_reported_and_a_backup_still_imports() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = parley(dir.path());
        app.course_start("es").unwrap();
        let backup = dir.path().join("saved").join("backup.json");
        app.backup_export(&backup).unwrap();
        drop(app);
        std::fs::write(
            dir.path().join(STORE_FILE),
            b"this is not a database at all",
        )
        .unwrap();
        let mut app = parley(dir.path());
        let err = app.app_start().unwrap_err();
        assert_eq!(err.code, ErrorCode::Storage);
        assert_eq!(err.message, "Parley could not open its data file.");
        assert!(err.action.unwrap()["details"].as_str().is_some());
        assert_eq!(app.path_get("es").unwrap_err().code, ErrorCode::Storage);
        let report = app.backup_import(&backup).unwrap();
        assert_eq!(report.courses, 1);
        assert!(app.app_start().unwrap().onboarded);
    }

    #[test]
    fn delete_everything_needs_the_word_and_clears_caches() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = parley(dir.path());
        app.course_start("es").unwrap();
        let err = app.data_delete_all("yes").unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (ErrorCode::InvalidInput, "Type delete to confirm.")
        );
        assert_eq!(app.cache_clear("ai").unwrap().files_removed, 0);
        std::fs::create_dir_all(dir.path().join("cache")).unwrap();
        std::fs::write(dir.path().join(AI_CACHE_FILE), b"x").unwrap();
        assert_eq!(app.cache_clear("all").unwrap().files_removed, 1);
        assert_eq!(
            app.cache_clear("other").unwrap_err().message,
            "Unknown value other for kind."
        );
        app.data_delete_all("  delete ").unwrap();
        let launch = app.app_start().unwrap();
        assert!(!launch.onboarded);
        assert_eq!(launch.active_course, None);
        assert!(!dir.path().join(SETTINGS_FILE).exists());
        assert_eq!(
            app.path_get("es").unwrap_err().message,
            "This course has not been started."
        );
    }

    #[test]
    fn a_failed_removal_is_reported_and_the_app_keeps_working() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = parley(dir.path());
        app.course_start("es").unwrap();
        // A folder where the AI cache file goes cannot be removed as a file.
        std::fs::create_dir_all(dir.path().join(AI_CACHE_FILE)).unwrap();
        let err = app.data_delete_all("delete").unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (ErrorCode::Storage, "Parley could not write that file.")
        );
        let launch = app.app_start().unwrap();
        assert!(!launch.onboarded, "the rest was deleted all the same");
        assert!(!dir.path().join(SETTINGS_FILE).exists());
        assert_eq!(app.settings_get(), Settings::default());
        app.course_start("es").unwrap();
        assert!(app.app_start().unwrap().onboarded);
    }

    #[test]
    fn settings_that_cannot_be_saved_after_an_import_leave_the_store_open() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = parley(dir.path());
        app.course_start("es").unwrap();
        app.settings_set(&json!({"daily_goal_xp": 50})).unwrap();
        let backup = dir.path().join("backup.json");
        app.backup_export(&backup).unwrap();
        app.data_delete_all("delete").unwrap();
        // A folder where the settings file goes cannot be written as a file.
        std::fs::create_dir_all(dir.path().join(SETTINGS_FILE)).unwrap();
        let err = app.backup_import(&backup).unwrap_err();
        assert_eq!(err.message, "Parley could not write that file.");
        assert_eq!(app.settings_get().daily_goal_xp, 50);
        assert!(app.app_start().unwrap().onboarded);
        assert!(app.path_get("es").is_ok());
    }

    #[test]
    fn a_backup_round_trips_through_delete_everything() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = parley(dir.path());
        app.course_start("es").unwrap();
        app.settings_set(&json!({"daily_goal_xp": 30})).unwrap();
        let started = app.lesson_start("es.a1.u01.s01", "learn").unwrap();
        let first = &started.exercises[0];
        app.exercise_submit(
            &started.session_id,
            &first.id,
            &crate::grading::Answer::Choice(0),
            Some(3_000),
            false,
        )
        .unwrap();
        app.lesson_finish(&started.session_id).unwrap();
        let one = dir.path().join("one.json");
        let two = dir.path().join("two.json");
        let summary = app.backup_export(&one).unwrap();
        assert_eq!((summary.courses, summary.cards, summary.reviews), (1, 2, 1));
        app.data_delete_all("delete").unwrap();
        let report = app.backup_import(&one).unwrap();
        assert_eq!(
            report,
            ImportReport {
                courses: 1,
                items: 0,
                cards: 2,
                reviews: 1,
                skipped_cards: 0
            }
        );
        assert_eq!(app.settings_get().daily_goal_xp, 30);
        app.backup_export(&two).unwrap();
        let read = |path: &Path| -> serde_json::Value {
            let mut value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            value["exported_at"] = json!(null);
            value
        };
        assert_eq!(read(&one), read(&two));
        assert_eq!(
            app.backup_import(&dir.path().join("missing.json"))
                .unwrap_err()
                .message,
            "Parley could not read that file."
        );
        let err = app.backup_export(dir.path()).unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (ErrorCode::Storage, "Parley could not write that file.")
        );
    }

    // One copy at a time, and never half a delete (the spec's Design notes and Messages).

    const DELETE_IN_USE: &str = "Parley could not delete your progress because another program \
                                 is using it. Close it and try again.";
    const RESTORE_IN_USE: &str = "Parley could not replace your progress because another program \
                                  is using it. Close it and try again.";
    const LEFTOVER: &str = "parley.sqlite3.deleting";

    /// The real files under a folder, except that, while `holding` is set, a path in `no_rename`
    /// cannot be renamed (as the source or the target) and a path in `no_remove` cannot be
    /// removed: what Windows answers while another program has the file open.
    struct HeldFs {
        files: TestFs,
        no_rename: Vec<&'static str>,
        no_remove: Vec<&'static str>,
        holding: Arc<AtomicBool>,
    }

    impl HeldFs {
        fn held(&self, list: &[&str], path: &Path) -> bool {
            self.holding.load(Ordering::SeqCst) && list.iter().any(|held| Path::new(held) == path)
        }
    }

    fn in_use() -> FsError {
        FsError::Io("the file is being used by another process".to_string())
    }

    impl Fs for HeldFs {
        fn read(&self, path: &Path) -> Result<Vec<u8>, FsError> {
            self.files.read(path)
        }
        fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), FsError> {
            self.files.write(path, bytes)
        }
        fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
            if self.held(&self.no_rename, from) || self.held(&self.no_rename, to) {
                return Err(in_use());
            }
            self.files.rename(from, to)
        }
        fn remove(&self, path: &Path) -> Result<(), FsError> {
            if self.held(&self.no_remove, path) {
                return Err(in_use());
            }
            self.files.remove(path)
        }
        fn list(&self, dir: &Path) -> Result<Vec<String>, FsError> {
            self.files.list(dir)
        }
    }

    /// The pack the lock below names for Spanish.
    const PACK: &str = "packs/es-2026-11-05.sqlite3";
    const ONE_PACK: &str = "schema = \"parley.packs/1\"\n[es]\nversion = \"2026-11-05\"\n\
        url = \"https://example.com/es.sqlite3\"\n\
        sha256 = \"0000000000000000000000000000000000000000000000000000000000000000\"\n\
        bytes = 1\ncontent_sha256 = \"x\"\n";

    fn held_parley(
        dir: &Path,
        no_rename: &[&'static str],
        no_remove: &[&'static str],
        holding: &Arc<AtomicBool>,
    ) -> Parley {
        Parley::new(
            config(dir, ONE_PACK),
            Env {
                clock: Box::new(TestClock(Arc::new(AtomicI64::new(MONDAY_NOON)), 0)),
                rng: Box::new(crate::env::SplitMix64::new(7)),
                fs: Box::new(HeldFs {
                    files: TestFs(dir.to_path_buf()),
                    no_rename: no_rename.to_vec(),
                    no_remove: no_remove.to_vec(),
                    holding: holding.clone(),
                }),
                net: Box::new(crate::env::FailingNet),
            },
        )
    }

    /// Progress worth keeping: Spanish started, a goal of 50 XP and one lesson finished today.
    fn make_progress(app: &mut Parley) -> u32 {
        app.course_start("es").unwrap();
        app.settings_set(&json!({"daily_goal_xp": 50})).unwrap();
        let started = app.lesson_start("es.a1.u01.s01", "learn").unwrap();
        let first = &started.exercises[0];
        app.exercise_submit(
            &started.session_id,
            &first.id,
            &crate::grading::Answer::Choice(0),
            Some(3_000),
            false,
        )
        .unwrap();
        app.lesson_finish(&started.session_id).unwrap();
        let path = app.path_get("es").unwrap();
        assert!(path.today_xp > 0 && path.streak > 0, "{path:?}");
        path.today_xp
    }

    /// The caches, the pack, a take and a model a learner may have beside the progress.
    const OTHER_FILES: [&str; 5] = [
        AI_CACHE_FILE,
        "cache/audio/0f3a.mp3",
        "cache/recordings/take-1.wav",
        PACK,
        "models/whisper/ggml-tiny.bin",
    ];

    fn write_other_files(dir: &Path) {
        for path in OTHER_FILES {
            let full = dir.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, path.as_bytes()).unwrap();
        }
    }

    /// Every file under `dir`, relative and with `/`, with its bytes.
    fn files_under(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(root, &path, out);
                } else {
                    let name = path.strip_prefix(root).unwrap().to_string_lossy();
                    out.insert(name.replace('\\', "/"), std::fs::read(&path).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(dir, dir, &mut out);
        out
    }

    #[test]
    fn delete_everything_with_the_data_file_in_use_removes_nothing_at_all() {
        let dir = tempfile::tempdir().unwrap();
        let holding = Arc::new(AtomicBool::new(true));
        let mut app = held_parley(dir.path(), &[STORE_FILE], &[STORE_FILE], &holding);
        let xp = make_progress(&mut app);
        write_other_files(dir.path());
        let mut before = files_under(dir.path());
        before.remove(STORE_FILE);

        let err = app.data_delete_all("delete").unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (ErrorCode::Busy, DELETE_IN_USE)
        );
        assert!(nothing_was_deleted(&err));
        let mut after = files_under(dir.path());
        assert!(
            after.remove(STORE_FILE).is_some(),
            "the data file is still there"
        );
        assert_eq!(
            after, before,
            "settings, caches, pack, take and model untouched"
        );
        assert!(app.app_start().unwrap().onboarded);
        let path = app.path_get("es").unwrap();
        assert_eq!(path.today_xp, xp, "the progress is the same");
        assert_eq!(app.settings_get().daily_goal_xp, 50);

        // Once the other program lets go, the same delete goes through.
        holding.store(false, Ordering::SeqCst);
        app.data_delete_all("delete").unwrap();
        assert!(!app.app_start().unwrap().onboarded);
        let left: Vec<String> = files_under(dir.path()).into_keys().collect();
        assert_eq!(
            left,
            vec![STORE_FILE.to_string()],
            "only the fresh data file"
        );
    }

    #[test]
    fn a_failure_after_the_data_file_is_moved_aside_still_starts_afresh() {
        let dir = tempfile::tempdir().unwrap();
        let holding = Arc::new(AtomicBool::new(true));
        let mut app = held_parley(dir.path(), &[], &[AI_CACHE_FILE], &holding);
        make_progress(&mut app);
        write_other_files(dir.path());

        let err = app.data_delete_all("delete").unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (ErrorCode::Storage, "Parley could not write that file.")
        );
        assert!(!nothing_was_deleted(&err), "the progress went");
        let left: Vec<String> = files_under(dir.path()).into_keys().collect();
        assert_eq!(
            left,
            vec![AI_CACHE_FILE.to_string(), STORE_FILE.to_string()],
            "the rest went, the moved data file with it"
        );
        assert!(!app.app_start().unwrap().onboarded);
        assert!(app.course_list().unwrap().iter().all(|c| !c.started));

        // The next launch is a first run with nothing carried over: no streak, no XP.
        drop(app);
        let mut next = held_parley(dir.path(), &[], &[AI_CACHE_FILE], &holding);
        let launch = next.app_start().unwrap();
        assert!(!launch.onboarded);
        assert_eq!(launch.active_course, None);
        next.course_start("es").unwrap();
        let path = next.path_get("es").unwrap();
        assert_eq!((path.streak, path.today_xp), (0, 0));
    }

    #[test]
    fn a_moved_aside_data_file_that_cannot_be_removed_is_never_opened_again() {
        let dir = tempfile::tempdir().unwrap();
        let holding = Arc::new(AtomicBool::new(true));
        let mut app = held_parley(dir.path(), &[], &[LEFTOVER], &holding);
        make_progress(&mut app);

        let err = app.data_delete_all("delete").unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (ErrorCode::Storage, "Parley could not write that file.")
        );
        assert!(dir.path().join(LEFTOVER).exists());
        assert!(!dir.path().join(SETTINGS_FILE).exists());
        drop(app);
        let mut next = held_parley(dir.path(), &[], &[LEFTOVER], &holding);
        assert!(!next.app_start().unwrap().onboarded);
        next.course_start("es").unwrap();
        let path = next.path_get("es").unwrap();
        assert_eq!((path.streak, path.today_xp), (0, 0));

        // The next Delete everything removes what the last one left.
        holding.store(false, Ordering::SeqCst);
        next.data_delete_all("delete").unwrap();
        assert!(!dir.path().join(LEFTOVER).exists());
    }

    #[test]
    fn restore_with_the_data_file_in_use_replaces_nothing_and_leaves_no_import_file() {
        let dir = tempfile::tempdir().unwrap();
        let holding = Arc::new(AtomicBool::new(false));
        let mut app = held_parley(dir.path(), &[STORE_FILE], &[STORE_FILE], &holding);
        app.course_start("es").unwrap();
        app.settings_set(&json!({"daily_goal_xp": 30})).unwrap();
        let backup = dir.path().join("saved").join("backup.json");
        app.backup_export(&backup).unwrap();
        let xp = make_progress(&mut app);

        holding.store(true, Ordering::SeqCst);
        let err = app.backup_import(&backup).unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (ErrorCode::Busy, RESTORE_IN_USE)
        );
        assert!(
            !dir.path().join(IMPORT_FILE).exists(),
            "no import file left"
        );
        assert_eq!(app.settings_get().daily_goal_xp, 50);
        assert!(app.app_start().unwrap().onboarded);
        assert_eq!(app.path_get("es").unwrap().today_xp, xp);

        holding.store(false, Ordering::SeqCst);
        app.backup_import(&backup).unwrap();
        assert_eq!(app.settings_get().daily_goal_xp, 30);
        assert_eq!(app.path_get("es").unwrap().today_xp, 0);
    }

    #[test]
    fn a_refused_delete_removes_nothing_either() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = parley(dir.path());
        let err = app.data_delete_all("yes").unwrap_err();
        assert!(nothing_was_deleted(&err));
        assert!(!nothing_was_deleted(&cannot_write()));
    }
}
