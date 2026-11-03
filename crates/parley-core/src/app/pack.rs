//! Course packs: `pack_status`, `pack_download`, the sentences a lesson takes from an installed
//! pack, and their attribution (docs/modules/04-app-shell-and-offline-course.md, Design notes).
//!
//! A download is checked against the SHA-256 in `content/packs.lock.toml` before anything is
//! written; it is written as `packs/<lang>-<version>.sqlite3.part` and renamed into place, so a
//! damaged or partial download never becomes the pack. The app works without a pack, with fewer
//! example sentences.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, params};
use sha2::{Digest, Sha256};

use super::{Parley, not_started, open_store};
use crate::api::{Attribution, PackProgress, PackState, PackStatus, language_name, megabytes};
use crate::content::{Lang, PackEntry, Syllabus, nfc, nfc_lower, parse_packs_lock};
use crate::env::HttpRequest;
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
            None => PackStatus {
                lang: lang.as_str().to_string(),
                state: PackState::NotPublished,
                version: None,
                bytes: None,
                size: None,
            },
        })
    }

    /// Downloads the language's pack from the lock's URL, reporting progress before the request
    /// and once the body has arrived; checks the SHA-256 before writing anything; writes the
    /// `.part` file, renames it into place, removes the course's previous pack and records the
    /// new version.
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
        if response.status != 200 {
            return Err(AppError::new(
                ErrorCode::PackMissing,
                "The course pack could not be downloaded. Try again later.",
            ));
        }
        progress(report(entry.bytes));
        if !sha256_hex(&response.body).eq_ignore_ascii_case(entry.sha256.trim()) {
            return Err(AppError::new(
                ErrorCode::ChecksumMismatch,
                "The download was damaged. Try again.",
            ));
        }
        let target = pack_file(lang, &entry.version);
        let part = format!("{target}.part");
        let fs = &*self.env.fs;
        let written = fs
            .write(Path::new(&part), &response.body)
            .and_then(|()| fs.rename(Path::new(&part), Path::new(&target)));
        if written.is_err() {
            let _ = fs.remove(Path::new(&part));
            return Err(super::cannot_write());
        }
        if let Some(previous) = course.pack_version.filter(|v| *v != entry.version) {
            let _ = fs.remove(Path::new(&pack_file(lang, &previous)));
        }
        store.set_pack_version(lang, Some(&entry.version))?;
        self.pack_status(lang.as_str())
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

        let dir = tempfile::tempdir().unwrap();
        let mut app = parley_with(dir.path(), &lock(&bytes), net(404, Vec::new()));
        app.course_start("es").unwrap();
        let err = app.pack_download("es", &mut |_| {}).unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (
                ErrorCode::PackMissing,
                "The course pack could not be downloaded. Try again later."
            )
        );

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
