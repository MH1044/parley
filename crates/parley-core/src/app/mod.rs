//! The service every Tauri command calls (docs/modules/04-app-shell-and-offline-course.md): one
//! [`Parley`] per app, behind a mutex, holding the bundled course, the learner's store and
//! settings, and the lessons and placement tests in progress.
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
use crate::backup::{Backup, BackupError, parse_backup};
use crate::content::{Forms, Item, Lang, Syllabus, derive_items, load_numbers, parse_syllabus};
use crate::env::{Clock, Fs, FsError, Net, Rng};
use crate::error::{AppError, ErrorCode};
use crate::exercise::LessonRun;
use crate::settings::{
    AiSettings, SETTINGS_FILE, SETTINGS_NOTICE, Settings, apply_patch, parse_settings,
};
use crate::srs::Placement;
use crate::store::{Store, StoreError};

mod course;
mod judge;
mod lesson;
mod pack;
mod placement;
mod stats;
mod story;

pub use judge::{ExplainJob, ExplainOutcome, Explained, JudgeBegin, JudgeJob, JudgeOutcome};
pub use lesson::{AiAccess, JobOutcome, LessonBegin, LessonJob};
pub use pack::{InstalledPack, LockedPack, PackInstallError, PackSentence, attributions};

/// The learner's data file, under the data directory.
pub const STORE_FILE: &str = "parley.sqlite3";
/// The file an import writes before it replaces the data file.
pub const IMPORT_FILE: &str = "parley.sqlite3.import";
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
    /// Every item of the syllabus (`derive_items`), with its accepted answers.
    pub items: Vec<Item>,
}

impl Course {
    fn load(files: &CourseFiles) -> Option<Course> {
        let syllabus = parse_syllabus("syllabus.yaml", &files.syllabus_yaml).ok()?;
        let forms = Forms::load_tsv(files.forms_tsv.as_bytes()).ok()?;
        let numbers = match &files.numbers_yaml {
            Some(text) => load_numbers(text).ok()?,
            None => Vec::new(),
        };
        let items = derive_items(&syllabus, &forms);
        Some(Course {
            syllabus,
            forms,
            numbers,
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
        }
    }

    /// A course language the app ships: "Unknown value {v} for lang." otherwise.
    pub(crate) fn lang(&self, lang: &str) -> Result<Lang, AppError> {
        lang.parse::<Lang>()
            .ok()
            .filter(|l| self.content.contains_key(l))
            .ok_or_else(|| unknown_value(lang, "lang"))
    }

    /// The first command of a launch: brings every started course up to date with the bundled
    /// syllabus and says whether onboarding is done (a course is active and started).
    pub fn app_start(&mut self) -> Result<Launch, AppError> {
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

    /// Deletes the store, the settings file, the pack files and the AI cache, then opens a fresh
    /// store. `confirm_text` must be `delete` after trimming.
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
        // The store is closed before its file is removed. Every removal is attempted even when
        // one fails, and a store is always opened again afterwards (a fresh one, or the old file
        // when it could not be removed), so the app keeps working; the first failure is reported.
        self.store = Err("The data file is being replaced.".to_string());
        self.runs.clear();
        self.story_runs.clear();
        self.pending.clear();
        self.placements.clear();
        let fs = &*self.env.fs;
        let mut paths: Vec<String> = [
            STORE_FILE,
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

    /// Removes a cache: `ai` and `all` remove `cache/ai.sqlite3` (module 09 adds `audio`).
    pub fn cache_clear(&mut self, kind: &str) -> Result<CacheCleared, AppError> {
        let files = match kind {
            "ai" | "all" => vec![AI_CACHE_FILE],
            "audio" => Vec::new(),
            other => return Err(unknown_value(other, "kind")),
        };
        let mut files_removed = 0;
        for file in files {
            if remove_file(&*self.env.fs, file)? {
                files_removed += 1;
            }
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
    /// was. Works when the current data file cannot be opened.
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
        self.runs.clear();
        self.story_runs.clear();
        self.pending.clear();
        self.placements.clear();
        let renamed = fs.rename(Path::new(IMPORT_FILE), Path::new(STORE_FILE));
        // The store is open again whatever happens next: the new file, or the old one when the
        // rename failed.
        self.store = Store::open(&self.config.data_dir.join(STORE_FILE)).map_err(|e| e.to_string());
        renamed.map_err(|_| cannot_write())?;
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
}
