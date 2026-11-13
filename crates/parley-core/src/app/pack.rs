//! Course packs: `pack_status`, `pack_download`, the sentences a lesson takes from an installed
//! pack, and their attribution (docs/modules/04-app-shell-and-offline-course.md, Design notes);
//! and `parley pack install`, which installs a locally built pack for development
//! (docs/modules/05-course-content-a1-a2.md).
//!
//! A download is checked against the SHA-256 in `content/packs.lock.toml` before anything is
//! written; it is written as `packs/<lang>-<version>.sqlite3.part` and renamed into place, so a
//! damaged or partial download never becomes the pack. An installed file takes the same path.
//! The app works without a pack, with fewer example sentences.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, params};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{Parley, not_started, open_store};
use crate::api::{Attribution, PackProgress, PackState, PackStatus, language_name, megabytes};
use crate::content::{Lang, PackEntry, Syllabus, nfc, nfc_lower, parse_packs_lock};
use crate::env::{Fs, HttpRequest};
use crate::error::{AppError, ErrorCode};
use crate::exercise::{Body, Lesson};
use crate::store::Store;

/// The most sentences a lesson takes from the pack for one lemma.
const SENTENCES_PER_LEMMA: usize = 20;

/// A sentence of an installed pack with its English translations and author.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackSentence {
    pub id: i64,
    pub text: String,
    pub en: Vec<String>,
    /// The Tatoeba author; empty for a sentence without one.
    pub author: String,
}

/// The pack file of a language and version, relative to the data directory.
pub(crate) fn pack_file(lang: Lang, version: &str) -> String {
    format!("packs/{}-{version}.sqlite3", lang.as_str())
}

/// The lock's entry for a language; a lock that does not parse publishes nothing.
fn lock_entry(packs_lock: &str, lang: Lang) -> Option<PackEntry> {
    parse_packs_lock(packs_lock)
        .ok()?
        .langs
        .get(lang.as_str())
        .cloned()
}

/// A pack file opened read-only, when it is one: it opens and has its sentence tables.
fn open_pack(path: &Path) -> Option<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let usable = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM sentences) + (SELECT COUNT(*) FROM sentence_lemmas)
               + (SELECT COUNT(*) FROM translations)",
            [],
            |row| row.get::<_, i64>(0),
        )
        .is_ok();
    usable.then_some(conn)
}

/// The installed pack of a started course: its version and the open file.
fn installed(data_dir: &Path, store: &Store, lang: Lang) -> Option<(String, Connection)> {
    let version = store.course(lang).ok()??.pack_version?;
    let path: PathBuf = data_dir.join(pack_file(lang, &version));
    open_pack(&path).map(|conn| (version, conn))
}

/// Up to 20 sentences per lemma from an installed pack (`sentence_lemmas`, by `max_rank` then
/// id), each once, in the order found. No pack: none.
pub(crate) fn pack_sentences(
    data_dir: &Path,
    store: &Store,
    lang: Lang,
    lemmas: &[String],
) -> Vec<PackSentence> {
    let Some((_, conn)) = installed(data_dir, store, lang) else {
        return Vec::new();
    };
    let mut found: Vec<PackSentence> = Vec::new();
    let mut seen: HashSet<i64> = HashSet::new();
    let mut keys_seen: HashSet<String> = HashSet::new();
    for lemma in lemmas {
        let exact = nfc(lemma);
        let lower = nfc_lower(lemma);
        if !keys_seen.insert(exact.clone()) {
            continue;
        }
        let rows: Vec<(i64, String, String)> = conn
            .prepare_cached(
                "SELECT s.id, s.text, s.author FROM sentence_lemmas l
                 JOIN sentences s ON s.id = l.sentence_id
                 WHERE l.lemma = ?1 OR l.lemma = ?2
                 GROUP BY s.id ORDER BY s.max_rank, s.id LIMIT ?3",
            )
            .and_then(|mut statement| {
                statement
                    .query_map(params![exact, lower, SENTENCES_PER_LEMMA as i64], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                    })?
                    .collect()
            })
            .unwrap_or_default();
        for (id, text, author) in rows {
            if !seen.insert(id) {
                continue;
            }
            let en: Vec<String> = conn
                .prepare_cached(
                    "SELECT en_text FROM translations WHERE sentence_id = ?1 ORDER BY en_id",
                )
                .and_then(|mut statement| {
                    statement
                        .query_map([id], |row| row.get(0))?
                        .collect::<Result<Vec<String>, _>>()
                })
                .unwrap_or_default();
            found.push(PackSentence {
                id,
                text,
                en,
                author,
            });
        }
    }
    found
}

/// The author of every `fill_gap` whose `full_sentence` is a pack sentence's text and no
/// syllabus example's, in lesson order.
pub fn attributions(
    lesson: &Lesson,
    pack: &[PackSentence],
    syllabus: &Syllabus,
) -> Vec<Attribution> {
    let examples: HashSet<String> = syllabus
        .skills()
        .flat_map(|(_, skill)| skill.examples.iter().map(|example| nfc(&example.target)))
        .collect();
    lesson
        .exercises
        .iter()
        .filter_map(|ex| {
            let Body::FillGap { full_sentence, .. } = &ex.body else {
                return None;
            };
            let text = nfc(full_sentence);
            if examples.contains(&text) {
                return None;
            }
            pack.iter()
                .find(|sentence| nfc(&sentence.text) == text)
                .map(|sentence| Attribution {
                    exercise_id: ex.id.clone(),
                    author: sentence.author.clone(),
                })
        })
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The status of a language whose pack is not published: no version, size or bytes.
fn not_published(lang: Lang) -> PackStatus {
    PackStatus {
        lang: lang.as_str().to_string(),
        state: PackState::NotPublished,
        version: None,
        bytes: None,
        size: None,
    }
}

impl Parley {
    /// Whether the language's pack is installed (the course's `pack_version` names a file that
    /// opens as a pack), available in the lock, or not published.
    pub fn pack_status(&mut self, lang: &str) -> Result<PackStatus, AppError> {
        let lang = self.lang(lang)?;
        let store = open_store(&mut self.store)?;
        let entry = lock_entry(&self.config.packs_lock, lang);
        if let Some((version, _)) = installed(&self.config.data_dir, store, lang) {
            let bytes = entry
                .filter(|entry| entry.version == version)
                .map(|entry| entry.bytes);
            return Ok(PackStatus {
                lang: lang.as_str().to_string(),
                state: PackState::Installed,
                version: Some(version),
                bytes,
                size: bytes.map(megabytes),
            });
        }
        Ok(match entry {
            Some(entry) => PackStatus {
                lang: lang.as_str().to_string(),
                state: PackState::Available,
                version: Some(entry.version),
                bytes: Some(entry.bytes),
                size: Some(megabytes(entry.bytes)),
            },
            None => not_published(lang),
        })
    }

    /// Downloads the language's pack from the lock's URL, reporting progress before the request
    /// and once the body has arrived; checks the SHA-256 before writing anything; writes the
    /// `.part` file, renames it into place, removes the course's previous pack and records the
    /// new version. A URL that answers 404 or 410 means the pack is not published yet: nothing is
    /// written and the not-published status is returned, so the learner goes on without it.
    pub fn pack_download(
        &mut self,
        lang: &str,
        progress: &mut dyn FnMut(PackProgress),
    ) -> Result<PackStatus, AppError> {
        let lang = self.lang(lang)?;
        let store = open_store(&mut self.store)?;
        let Some(course) = store.course(lang)? else {
            return Err(not_started());
        };
        let Some(entry) = lock_entry(&self.config.packs_lock, lang) else {
            return Err(AppError::new(
                ErrorCode::PackMissing,
                format!(
                    "No course pack is published for {} yet.",
                    language_name(lang)
                ),
            ));
        };
        let report = |done: u64| PackProgress {
            lang: lang.as_str().to_string(),
            bytes_done: done,
            bytes_total: entry.bytes,
        };
        progress(report(0));
        let response = self
            .env
            .net
            .send(HttpRequest {
                method: "GET".into(),
                url: entry.url.clone(),
                headers: Vec::new(),
                body: Vec::new(),
            })
            .map_err(|_| {
                AppError::new(
                    ErrorCode::Offline,
                    "You're offline. Parley works without the pack, with fewer example sentences.",
                )
            })?;
        match response.status {
            200 => {}
            // The lock names a release asset that does not exist (yet, or any more): trying
            // again cannot help until it is published, so this is the not-published outcome, not
            // a failure to retry.
            404 | 410 => return Ok(not_published(lang)),
            _ => {
                return Err(AppError::new(
                    ErrorCode::PackMissing,
                    "The course pack could not be downloaded. Try again later.",
                ));
            }
        }
        progress(report(entry.bytes));
        if !sha256_hex(&response.body).eq_ignore_ascii_case(entry.sha256.trim()) {
            return Err(AppError::new(
                ErrorCode::ChecksumMismatch,
                "The download was damaged. Try again.",
            ));
        }
        write_pack(
            &*self.env.fs,
            store,
            lang,
            course.pack_version.as_deref(),
            &entry.version,
            &response.body,
        )?;
        self.pack_status(lang.as_str())
    }
}

/// Puts a checked pack in place, the one way every pack is installed: the bytes go to
/// `packs/<lang>-<version>.sqlite3.part` through the data directory's [`Fs`] and are renamed
/// into place, so a failed write never leaves a partial pack; the new version is recorded; only
/// then is the course's previous pack, when its version differs, removed. If the version cannot
/// be recorded, the new file is removed (unless it has the recorded version's own name) and the
/// old pack and the store stay as they were, so the store never names a pack that is gone.
/// Returns the pack's path relative to the data directory.
fn write_pack(
    fs: &dyn Fs,
    store: &mut Store,
    lang: Lang,
    previous: Option<&str>,
    version: &str,
    bytes: &[u8],
) -> Result<String, AppError> {
    let target = pack_file(lang, version);
    let part = format!("{target}.part");
    let written = fs
        .write(Path::new(&part), bytes)
        .and_then(|()| fs.rename(Path::new(&part), Path::new(&target)));
    if written.is_err() {
        let _ = fs.remove(Path::new(&part));
        return Err(super::cannot_write());
    }
    let replaced = previous.filter(|v| *v != version);
    if let Err(e) = store.set_pack_version(lang, Some(version)) {
        if previous != Some(version) {
            let _ = fs.remove(Path::new(&target));
        }
        return Err(e.into());
    }
    if let Some(previous) = replaced {
        let _ = fs.remove(Path::new(&pack_file(lang, previous)));
    }
    Ok(target)
}

/// Why `parley pack install` installed nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum PackInstallError {
    /// The lock has no entry for the pack's language, or the entry's SHA-256 or size differ from
    /// the file's.
    NotLocked,
    /// The course of the pack's language has not been started in the data directory.
    NotStarted(Lang),
    /// The data file could not be used or the pack could not be written.
    App(AppError),
}

impl fmt::Display for PackInstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackInstallError::NotLocked => f.write_str("This pack does not match packs.lock.toml."),
            PackInstallError::NotStarted(lang) => write!(
                f,
                "Start the {} course in Parley first.",
                language_name(*lang)
            ),
            PackInstallError::App(e) => f.write_str(&e.message),
        }
    }
}

impl std::error::Error for PackInstallError {}

impl From<AppError> for PackInstallError {
    fn from(e: AppError) -> Self {
        PackInstallError::App(e)
    }
}

/// What `parley pack install` prints: the language, the locked version and the pack's path
/// relative to the data directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstalledPack {
    pub lang: Lang,
    pub version: String,
    pub path: String,
}

/// A pack file that `packs.lock.toml` pins: the lock's entry for the file's language has its
/// SHA-256 and size. Only [`LockedPack::verify`] makes one, so nothing unchecked is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedPack {
    lang: Lang,
    entry: PackEntry,
    bytes: Vec<u8>,
}

impl LockedPack {
    /// Checks the bytes of a pack whose `meta` names `lang_code` against the lock's text.
    pub fn verify(
        packs_lock: &str,
        lang_code: &str,
        bytes: Vec<u8>,
    ) -> Result<LockedPack, PackInstallError> {
        let lang: Lang = lang_code.parse().map_err(|_| PackInstallError::NotLocked)?;
        let entry = lock_entry(packs_lock, lang).ok_or(PackInstallError::NotLocked)?;
        let matches = entry.bytes == bytes.len() as u64
            && sha256_hex(&bytes).eq_ignore_ascii_case(entry.sha256.trim());
        if !matches {
            return Err(PackInstallError::NotLocked);
        }
        Ok(LockedPack { lang, entry, bytes })
    }

    pub fn lang(&self) -> Lang {
        self.lang
    }

    /// The locked version, which names the file.
    pub fn version(&self) -> &str {
        &self.entry.version
    }

    /// Installs the pack into the started course of its language, through the same steps as a
    /// download (`write_pack`).
    pub fn install(
        &self,
        store: &mut Store,
        fs: &dyn Fs,
    ) -> Result<InstalledPack, PackInstallError> {
        let course = store
            .course(self.lang)
            .map_err(AppError::from)?
            .ok_or(PackInstallError::NotStarted(self.lang))?;
        let path = write_pack(
            fs,
            store,
            self.lang,
            course.pack_version.as_deref(),
            &self.entry.version,
            &self.bytes,
        )?;
        Ok(InstalledPack {
            lang: self.lang,
            version: self.entry.version.clone(),
            path,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::super::test_support::{NO_PACKS, ScriptedNet, parley, parley_with};
    use super::*;
    use crate::env::{HttpResponse, NetError};
    use crate::exercise::{Exercise, Origin};
    use crate::srs::Facet;

    /// A tiny pack: the tables of `parley.pack/1` with one sentence by `fixture_a`.
    fn pack_bytes(dir: &Path) -> Vec<u8> {
        let path = dir.join("built.sqlite3");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE sentences (id INTEGER PRIMARY KEY, text TEXT NOT NULL, norm TEXT NOT NULL,
               tokens INTEGER NOT NULL, max_rank INTEGER NOT NULL, author TEXT NOT NULL);
             CREATE TABLE translations (sentence_id INTEGER NOT NULL, en_id INTEGER NOT NULL,
               en_text TEXT NOT NULL, en_author TEXT NOT NULL, PRIMARY KEY (sentence_id, en_id));
             CREATE TABLE sentence_lemmas (lemma TEXT NOT NULL, sentence_id INTEGER NOT NULL,
               PRIMARY KEY (lemma, sentence_id)) WITHOUT ROWID;
             INSERT INTO meta VALUES ('schema', 'parley.pack/1');
             INSERT INTO sentences VALUES (7, 'Buenas noches, Ana.', 'buenas noches ana', 3, 2, 'fixture_a');
             INSERT INTO translations VALUES (7, 70, 'Good night, Ana.', 'fixture_b');
             INSERT INTO sentence_lemmas VALUES ('noche', 7);
             INSERT INTO sentence_lemmas VALUES ('bueno', 7);",
        )
        .unwrap();
        drop(conn);
        std::fs::read(&path).unwrap()
    }

    fn lock(bytes: &[u8]) -> String {
        format!(
            "schema = \"parley.packs/1\"\n[es]\nversion = \"2026-10-01\"\nurl = \"https://example.com/es.sqlite3\"\nsha256 = \"{}\"\nbytes = 8200000\ncontent_sha256 = \"x\"\n",
            sha256_hex(bytes)
        )
    }

    fn net(status: u16, body: Vec<u8>) -> Box<ScriptedNet> {
        Box::new(ScriptedNet {
            response: Ok(HttpResponse {
                status,
                headers: Vec::new(),
                body,
            }),
            requests: Arc::new(Mutex::new(Vec::new())),
        })
    }

    #[test]
    fn a_verified_download_installs_the_pack() {
        let dir = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let bytes = pack_bytes(scratch.path());
        let mut app = parley_with(dir.path(), &lock(&bytes), net(200, bytes.clone()));
        let status = app.pack_status("es").unwrap();
        assert_eq!(status.state, PackState::Available);
        assert_eq!(status.size.as_deref(), Some("8.2 MB"));
        assert_eq!(
            app.pack_download("es", &mut |_| {}).unwrap_err().message,
            "This course has not been started."
        );
        app.course_start("es").unwrap();
        let mut seen = Vec::new();
        let status = app
            .pack_download("es", &mut |p| seen.push((p.bytes_done, p.bytes_total)))
            .unwrap();
        assert_eq!(status.state, PackState::Installed);
        assert_eq!(status.version.as_deref(), Some("2026-10-01"));
        assert_eq!(seen, [(0, 8_200_000), (8_200_000, 8_200_000)]);
        assert!(dir.path().join("packs/es-2026-10-01.sqlite3").exists());
        assert!(!dir.path().join("packs/es-2026-10-01.sqlite3.part").exists());
        let store = app.store.as_ref().unwrap();
        let sentences = pack_sentences(
            dir.path(),
            store,
            Lang::Es,
            &["noche".into(), "bueno".into()],
        );
        assert_eq!(
            sentences,
            [PackSentence {
                id: 7,
                text: "Buenas noches, Ana.".into(),
                en: vec!["Good night, Ana.".into()],
                author: "fixture_a".into()
            }]
        );
    }

    #[test]
    fn a_failed_download_writes_nothing_and_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let bytes = pack_bytes(scratch.path());
        let mut damaged = bytes.clone();
        damaged[100] ^= 1;
        let mut app = parley_with(dir.path(), &lock(&bytes), net(200, damaged));
        app.course_start("es").unwrap();
        let err = app.pack_download("es", &mut |_| {}).unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (
                ErrorCode::ChecksumMismatch,
                "The download was damaged. Try again."
            )
        );
        assert!(!dir.path().join("packs").exists(), "nothing written");

        // A server error may pass, so it is worth trying again.
        let dir = tempfile::tempdir().unwrap();
        let mut app = parley_with(dir.path(), &lock(&bytes), net(500, bytes.clone()));
        app.course_start("es").unwrap();
        let err = app.pack_download("es", &mut |_| {}).unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (
                ErrorCode::PackMissing,
                "The course pack could not be downloaded. Try again later."
            )
        );
        assert!(!dir.path().join("packs").exists(), "nothing written");

        let dir = tempfile::tempdir().unwrap();
        let offline = Box::new(ScriptedNet {
            response: Err(NetError::Offline),
            requests: Arc::new(Mutex::new(Vec::new())),
        });
        let mut app = parley_with(dir.path(), &lock(&bytes), offline);
        app.course_start("es").unwrap();
        let err = app.pack_download("es", &mut |_| {}).unwrap_err();
        assert_eq!(err.code, ErrorCode::Offline);
        assert_eq!(
            err.message,
            "You're offline. Parley works without the pack, with fewer example sentences."
        );

        let dir = tempfile::tempdir().unwrap();
        let mut app = parley(dir.path());
        app.course_start("es").unwrap();
        assert_eq!(
            app.pack_status("es").unwrap().state,
            PackState::NotPublished
        );
        let err = app.pack_download("es", &mut |_| {}).unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (
                ErrorCode::PackMissing,
                "No course pack is published for Spanish yet."
            )
        );
        assert!(NO_PACKS.contains("parley.packs/1"));
    }

    #[test]
    fn a_pack_url_that_answers_404_or_410_means_the_pack_is_not_published() {
        let scratch = tempfile::tempdir().unwrap();
        let bytes = pack_bytes(scratch.path());
        for status in [404, 410] {
            let dir = tempfile::tempdir().unwrap();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let answering = Box::new(ScriptedNet {
                response: Ok(HttpResponse {
                    status,
                    headers: Vec::new(),
                    body: b"Not Found".to_vec(),
                }),
                requests: Arc::clone(&requests),
            });
            let mut app = parley_with(dir.path(), &lock(&bytes), answering);
            app.course_start("es").unwrap();
            let mut seen = Vec::new();
            let outcome = app
                .pack_download("es", &mut |p| seen.push((p.bytes_done, p.bytes_total)))
                .unwrap();
            assert_eq!(
                outcome,
                PackStatus {
                    lang: "es".into(),
                    state: PackState::NotPublished,
                    version: None,
                    bytes: None,
                    size: None,
                },
                "{status}"
            );
            assert_eq!(
                *requests.lock().unwrap(),
                ["https://example.com/es.sqlite3"],
                "{status}: one request"
            );
            assert_eq!(seen, [(0, 8_200_000)], "{status}: no body arrived");
            assert!(
                !dir.path().join("packs").exists(),
                "{status}: nothing written"
            );
            assert_ne!(
                app.pack_status("es").unwrap().state,
                PackState::Installed,
                "{status}"
            );
            let store = app.store.as_ref().unwrap();
            assert_eq!(
                store.course(Lang::Es).unwrap().unwrap().pack_version,
                None,
                "{status}: no version recorded"
            );
        }
    }

    /// A lock that pins `bytes` exactly, at version 2026-11-05.
    fn exact_lock(bytes: &[u8]) -> String {
        format!(
            "schema = \"parley.packs/1\"\n[es]\nversion = \"2026-11-05\"\nurl = \"https://example.com/es.sqlite3\"\nsha256 = \"{}\"\nbytes = {}\ncontent_sha256 = \"x\"\n",
            sha256_hex(bytes),
            bytes.len()
        )
    }

    #[test]
    fn only_a_pack_the_lock_pins_is_accepted() {
        let scratch = tempfile::tempdir().unwrap();
        let bytes = pack_bytes(scratch.path());
        let lock = exact_lock(&bytes);
        let locked = LockedPack::verify(&lock, "es", bytes.clone()).unwrap();
        assert_eq!((locked.lang(), locked.version()), (Lang::Es, "2026-11-05"));

        let mut damaged = bytes.clone();
        damaged[100] ^= 1;
        let err = LockedPack::verify(&lock, "es", damaged).unwrap_err();
        assert_eq!(err, PackInstallError::NotLocked);
        assert_eq!(err.to_string(), "This pack does not match packs.lock.toml.");
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(
            LockedPack::verify(&lock, "es", longer).unwrap_err(),
            PackInstallError::NotLocked
        );
        // The size must agree as well as the hash.
        let wrong_size = lock.replace(&format!("bytes = {}", bytes.len()), "bytes = 1");
        assert_eq!(
            LockedPack::verify(&wrong_size, "es", bytes.clone()).unwrap_err(),
            PackInstallError::NotLocked
        );
        for (lock, lang) in [
            (lock.as_str(), "fr"),
            (lock.as_str(), "it"),
            (NO_PACKS, "es"),
        ] {
            assert_eq!(
                LockedPack::verify(lock, lang, bytes.clone()).unwrap_err(),
                PackInstallError::NotLocked,
                "{lang}"
            );
        }
    }

    #[test]
    fn installing_needs_a_started_course_and_takes_the_download_path() {
        let dir = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let bytes = pack_bytes(scratch.path());
        let lock = exact_lock(&bytes);
        let mut app = parley_with(dir.path(), &lock, Box::new(crate::env::FailingNet));
        let locked = LockedPack::verify(&lock, "es", bytes.clone()).unwrap();

        let store = app.store.as_mut().unwrap();
        let err = locked.install(store, &*app.env.fs).unwrap_err();
        assert_eq!(err, PackInstallError::NotStarted(Lang::Es));
        assert_eq!(err.to_string(), "Start the Spanish course in Parley first.");
        assert!(!dir.path().join("packs").exists(), "nothing written");

        app.course_start("es").unwrap();
        // An older pack of the course is replaced.
        std::fs::create_dir_all(dir.path().join("packs")).unwrap();
        std::fs::write(dir.path().join("packs/es-2026-10-01.sqlite3"), b"old").unwrap();
        let store = app.store.as_mut().unwrap();
        store
            .set_pack_version(Lang::Es, Some("2026-10-01"))
            .unwrap();
        let installed = locked.install(store, &*app.env.fs).unwrap();
        assert_eq!(
            serde_json::to_value(&installed).unwrap(),
            serde_json::json!({"lang": "es", "version": "2026-11-05",
                               "path": "packs/es-2026-11-05.sqlite3"})
        );
        assert_eq!(
            std::fs::read(dir.path().join("packs/es-2026-11-05.sqlite3")).unwrap(),
            bytes
        );
        assert!(!dir.path().join("packs/es-2026-11-05.sqlite3.part").exists());
        assert!(!dir.path().join("packs/es-2026-10-01.sqlite3").exists());
        let status = app.pack_status("es").unwrap();
        assert_eq!(status.state, PackState::Installed);
        assert_eq!(status.version.as_deref(), Some("2026-11-05"));
        assert_eq!(status.bytes, Some(bytes.len() as u64));
    }

    #[test]
    fn a_version_that_cannot_be_recorded_keeps_the_old_pack_and_drops_the_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let bytes = pack_bytes(scratch.path());
        let lock = exact_lock(&bytes);
        let mut app = parley_with(dir.path(), &lock, Box::new(crate::env::FailingNet));
        app.course_start("es").unwrap();
        std::fs::create_dir_all(dir.path().join("packs")).unwrap();
        std::fs::write(dir.path().join("packs/es-2026-10-01.sqlite3"), b"old").unwrap();
        app.store
            .as_mut()
            .unwrap()
            .set_pack_version(Lang::Es, Some("2026-10-01"))
            .unwrap();
        // A second connection makes every later update of the course row fail.
        let other = Connection::open(dir.path().join(super::super::STORE_FILE)).unwrap();
        other
            .execute_batch(
                "CREATE TRIGGER refuse BEFORE UPDATE ON courses
                 BEGIN SELECT RAISE(ABORT, 'refused'); END;",
            )
            .unwrap();
        drop(other);

        let locked = LockedPack::verify(&lock, "es", bytes).unwrap();
        let store = app.store.as_mut().unwrap();
        let err = locked.install(store, &*app.env.fs).unwrap_err();
        assert!(
            matches!(&err, PackInstallError::App(e) if e.code == ErrorCode::Storage),
            "{err:?}"
        );
        assert_eq!(
            std::fs::read(dir.path().join("packs/es-2026-10-01.sqlite3")).unwrap(),
            b"old"
        );
        assert!(!dir.path().join("packs/es-2026-11-05.sqlite3").exists());
        assert!(!dir.path().join("packs/es-2026-11-05.sqlite3.part").exists());
        let course = app
            .store
            .as_ref()
            .unwrap()
            .course(Lang::Es)
            .unwrap()
            .unwrap();
        assert_eq!(course.pack_version.as_deref(), Some("2026-10-01"));
    }

    #[test]
    fn an_install_that_cannot_write_leaves_no_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let bytes = pack_bytes(scratch.path());
        let lock = exact_lock(&bytes);
        let mut app = parley_with(dir.path(), &lock, Box::new(crate::env::FailingNet));
        app.course_start("es").unwrap();
        // A folder where the pack goes cannot be replaced by a file.
        std::fs::create_dir_all(dir.path().join("packs/es-2026-11-05.sqlite3/inside")).unwrap();
        let locked = LockedPack::verify(&lock, "es", bytes).unwrap();
        let store = app.store.as_mut().unwrap();
        let err = locked.install(store, &*app.env.fs).unwrap_err();
        assert_eq!(err.to_string(), "Parley could not write that file.");
        assert!(!dir.path().join("packs/es-2026-11-05.sqlite3.part").exists());
        assert_eq!(
            app.store
                .as_ref()
                .unwrap()
                .course(Lang::Es)
                .unwrap()
                .unwrap()
                .pack_version,
            None
        );
    }

    #[test]
    fn only_pack_sentences_are_attributed() {
        let syllabus =
            crate::content::parse_syllabus("x", crate::content::test_support::EXAMPLE_SYLLABUS)
                .unwrap();
        let gap = |id: &str, full: &str| Exercise {
            id: id.into(),
            item_id: "es.a1.u01.s01#l:noche".into(),
            facet: Facet::Productive,
            origin: Origin::Template,
            body: Body::FillGap {
                sentence: "x ___".into(),
                answer: "noches".into(),
                full_sentence: full.into(),
                hint_en: "night".into(),
            },
        };
        let lesson = Lesson {
            schema: Lesson::SCHEMA.into(),
            lang: Lang::Es,
            skill_id: "es.a1.u01.s01".into(),
            seq: 1,
            origin: Origin::Template,
            exercises: vec![
                gap("e1", "Buenas noches, Ana."),
                gap("e2", "Hasta la noche."),
                gap("e3", "Buenas noches."),
            ],
            ai: None,
        };
        let pack = [
            PackSentence {
                id: 1,
                text: "Hasta la noche.".into(),
                en: vec![],
                author: "fixture_a".into(),
            },
            PackSentence {
                id: 2,
                text: "Buenas noches, Ana.".into(),
                en: vec![],
                author: "fixture_b".into(),
            },
        ];
        // "Buenas noches, Ana." is a syllabus example, so nobody is named for it.
        assert_eq!(
            attributions(&lesson, &pack, &syllabus),
            [Attribution {
                exercise_id: "e2".into(),
                author: "fixture_a".into()
            }]
        );
        assert!(attributions(&lesson, &[], &syllabus).is_empty());
    }
}
