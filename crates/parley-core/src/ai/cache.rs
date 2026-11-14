//! The AI response cache, `cache/ai.sqlite3` (docs/ARCHITECTURE.md §4;
//! docs/PUBLIK-INTEGRATION.md §6.1): the `ai_cache`, `audio_cache`, `speech_charges`,
//! `transcribe_charges` and `ai_attempts` tables, the canonical JSON of a cache key, and the
//! idempotency key derived from it.
//!
//! Identical input never spends twice (invariant I5): a valid answer is stored under the SHA-256 of
//! the canonical JSON of {feature, v, provider, model, input}, and caches are per provider because
//! `provider` and `model` are in the key. `ai_attempts` counts, per key, the answers that failed
//! validation, so a Try again after one sends a new idempotency key, even after a restart.
//!
//! An on-disk cache opens the file for each operation and closes it straight after, so nothing
//! keeps it open between commands: clearing the cache or deleting everything can remove the file
//! at any time (Windows refuses to delete an open file), and the next operation starts a fresh one.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use super::client::ProviderKind;
use crate::env::Clock;
use crate::store::StoreError;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS ai_cache (key TEXT PRIMARY KEY, provider TEXT, model TEXT,
  prompt_id TEXT, prompt_version INTEGER, response TEXT, charge_micros INTEGER, created_at INTEGER);
CREATE TABLE IF NOT EXISTS audio_cache (key TEXT PRIMARY KEY, provider TEXT, voice TEXT, speed REAL,
  file TEXT, bytes INTEGER, created_at INTEGER);
CREATE TABLE IF NOT EXISTS ai_attempts (key TEXT PRIMARY KEY, attempts INTEGER);
CREATE TABLE IF NOT EXISTS speech_charges (key TEXT PRIMARY KEY, charge_micros INTEGER NOT NULL,
  created_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS transcribe_charges (id INTEGER PRIMARY KEY,
  charge_micros INTEGER NOT NULL);
";

enum Backing {
    /// The file, opened for each operation.
    File(PathBuf),
    /// A database that lives as long as the cache (P6's session-only cache).
    Memory(Mutex<Connection>),
}

/// The response cache.
pub struct Cache {
    backing: Backing,
    clock: Box<dyn Clock + Send + Sync>,
}

impl fmt::Debug for Cache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.backing {
            Backing::File(path) => f.debug_struct("Cache").field("file", path).finish(),
            Backing::Memory(_) => f.debug_struct("Cache").field("file", &"memory").finish(),
        }
    }
}

/// One stored answer, as `call` writes it.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheEntry<'a> {
    pub key: &'a str,
    pub provider: ProviderKind,
    pub model: &'a str,
    pub prompt_id: &'a str,
    pub prompt_version: u32,
    /// The answer's JSON.
    pub response: &'a str,
    pub charge_micros: Option<i64>,
}

/// One cached sentence of natural voice (module 09): its file under `cache/audio/` is the cache
/// itself; this row is its record for clearing.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioEntry<'a> {
    pub key: &'a str,
    pub provider: ProviderKind,
    pub voice: &'a str,
    pub speed: f64,
    /// `cache/audio/<key>.mp3`, relative to the data directory.
    pub file: &'a str,
    /// The file's size.
    pub bytes: u64,
}

/// Opens the file, creating its folder (SQLite creates the file, not its folder) and the tables.
fn open_file(path: &Path) -> Result<Connection, StoreError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).map_err(|e| StoreError::Sqlite(e.to_string()))?;
    }
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(2))?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

impl Cache {
    /// The cache in the file at `path` (normally `cache/ai.sqlite3` under the data directory).
    /// The file and its tables are created when missing.
    pub fn open(path: &Path, clock: Box<dyn Clock + Send + Sync>) -> Result<Cache, StoreError> {
        drop(open_file(path)?);
        Ok(Cache {
            backing: Backing::File(path.to_path_buf()),
            clock,
        })
    }

    /// A cache in memory only, gone when it is dropped (P6's session-only cache).
    pub fn in_memory(clock: Box<dyn Clock + Send + Sync>) -> Cache {
        let conn = Connection::open_in_memory().expect("SQLite opens a database in memory");
        conn.execute_batch(SCHEMA)
            .expect("the cache tables are created in memory");
        Cache {
            backing: Backing::Memory(Mutex::new(conn)),
            clock,
        }
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T, StoreError> {
        match &self.backing {
            Backing::File(path) => {
                let conn = open_file(path)?;
                Ok(f(&conn)?)
            }
            Backing::Memory(conn) => {
                let conn = conn.lock().unwrap_or_else(PoisonError::into_inner);
                Ok(f(&conn)?)
            }
        }
    }

    /// The cache in the file at `path` when that file exists; none otherwise, so reading the
    /// cache never creates it.
    pub(crate) fn open_existing(path: &Path, clock: Box<dyn Clock + Send + Sync>) -> Option<Cache> {
        if !path.is_file() {
            return None;
        }
        Cache::open(path, clock).ok()
    }

    /// The stored answer JSON for a key.
    pub fn get(&self, key: &str) -> Option<String> {
        self.with(|conn| {
            conn.query_row(
                "SELECT response FROM ai_cache WHERE key = ?1",
                params![key],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
        })
        .ok()
        .flatten()
        .flatten()
    }

    /// Rows in `ai_cache`.
    pub fn len(&self) -> usize {
        self.with(|conn| conn.query_row("SELECT COUNT(*) FROM ai_cache", [], |row| row.get(0)))
            .map_or(0, |n: i64| usize::try_from(n).unwrap_or(0))
    }

    /// No answer is stored.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many answers for `key` failed validation; 0 when none did.
    pub fn attempts(&self, key: &str) -> u32 {
        self.with(|conn| {
            conn.query_row(
                "SELECT attempts FROM ai_attempts WHERE key = ?1",
                params![key],
                |row| row.get::<_, i64>(0),
            )
            .optional()
        })
        .ok()
        .flatten()
        .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX))
    }

    /// Counts one more failed answer for `key`; returns the new count.
    pub fn add_attempt(&self, key: &str) -> Result<u32, StoreError> {
        self.with(|conn| {
            conn.execute(
                "INSERT INTO ai_attempts (key, attempts) VALUES (?1, 1)
                 ON CONFLICT(key) DO UPDATE SET attempts = attempts + 1",
                params![key],
            )?;
            conn.query_row(
                "SELECT attempts FROM ai_attempts WHERE key = ?1",
                params![key],
                |row| row.get::<_, i64>(0),
            )
        })
        .map(|n| u32::try_from(n).unwrap_or(u32::MAX))
    }

    /// Stores a valid answer (replacing any earlier one under the key).
    pub fn put(&self, entry: &CacheEntry<'_>) -> Result<(), StoreError> {
        let now = self.clock.now_ms();
        self.with(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO ai_cache (key, provider, model, prompt_id, prompt_version,
                   response, charge_micros, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    entry.key,
                    entry.provider.as_str(),
                    entry.model,
                    entry.prompt_id,
                    i64::from(entry.prompt_version),
                    entry.response,
                    entry.charge_micros,
                    now
                ],
            )
            .map(|_| ())
        })
    }

    /// Records a cached sentence (replacing any earlier row under the key).
    pub fn put_audio(&self, entry: &AudioEntry<'_>) -> Result<(), StoreError> {
        let now = self.clock.now_ms();
        let bytes = i64::try_from(entry.bytes).unwrap_or(i64::MAX);
        self.with(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO audio_cache (key, provider, voice, speed, file, bytes,
                   created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    entry.key,
                    entry.provider.as_str(),
                    entry.voice,
                    entry.speed,
                    entry.file,
                    bytes,
                    now
                ],
            )
            .map(|_| ())
        })
    }

    /// Rows in `audio_cache`.
    pub fn audio_len(&self) -> usize {
        self.with(|conn| conn.query_row("SELECT COUNT(*) FROM audio_cache", [], |row| row.get(0)))
            .map_or(0, |n: i64| usize::try_from(n).unwrap_or(0))
    }

    /// Removes every `audio_cache` row; returns how many there were. The files themselves are the
    /// caller's to remove.
    pub fn clear_audio(&self) -> Result<usize, StoreError> {
        self.with(|conn| conn.execute("DELETE FROM audio_cache", []))
    }

    /// Records what one sentence was charged (`x-publik-charge-micros`), whether its audio could
    /// be used or not; a sentence charged again keeps its latest charge.
    pub fn add_speech_charge(&self, key: &str, micros: i64) -> Result<(), StoreError> {
        let now = self.clock.now_ms();
        self.with(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO speech_charges (key, charge_micros, created_at)
                 VALUES (?1, ?2, ?3)",
                params![key, micros, now],
            )
            .map(|_| ())
        })
    }

    /// Every recorded sentence charge, oldest first: the measured cost line's figures.
    pub fn speech_charges(&self) -> Vec<i64> {
        self.with(|conn| {
            let mut stmt =
                conn.prepare("SELECT charge_micros FROM speech_charges ORDER BY created_at, key")?;
            let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
            rows.collect::<rusqlite::Result<Vec<i64>>>()
        })
        .unwrap_or_default()
    }

    /// Records what one speaking take was charged (module 10), usable reply or not: an amount
    /// alone, with no key, hash, text or time, because nothing about a take is kept on disk.
    pub fn add_transcribe_charge(&self, micros: i64) -> Result<(), StoreError> {
        self.with(|conn| {
            conn.execute(
                "INSERT INTO transcribe_charges (charge_micros) VALUES (?1)",
                params![micros],
            )
            .map(|_| ())
        })
    }

    /// Every recorded take charge, in the order recorded: the speaking cost line's figures.
    pub fn transcribe_charges(&self) -> Vec<i64> {
        self.with(|conn| {
            let mut stmt =
                conn.prepare("SELECT charge_micros FROM transcribe_charges ORDER BY id")?;
            let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
            rows.collect::<rusqlite::Result<Vec<i64>>>()
        })
        .unwrap_or_default()
    }
}

/// Writes `value` canonically into `out`.
fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => out.push_str(&value.to_string()),
        Value::String(s) => out.push_str(&nfc_string_json(s)),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut entries: Vec<(String, &Value)> = map
                .iter()
                .map(|(key, inner)| (key.nfc().collect::<String>(), inner))
                .collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            out.push('{');
            for (i, (key, inner)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).expect("a string serialises"));
                out.push(':');
                write_canonical(inner, out);
            }
            out.push('}');
        }
    }
}

fn nfc_string_json(s: &str) -> String {
    serde_json::to_string(&s.nfc().collect::<String>()).expect("a string serialises")
}

/// Sorted keys, no insignificant whitespace, every string NFC.
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

/// canonical_json of {"feature", "v", "provider", "model", "input"}.
pub fn cache_key_json(
    feature: &str,
    version: u32,
    provider: ProviderKind,
    model: &str,
    input: &Value,
) -> String {
    canonical_json(&serde_json::json!({
        "feature": feature,
        "v": version,
        "provider": provider.as_str(),
        "model": model,
        "input": input,
    }))
}

/// SHA-256 of `bytes`, 64 lowercase hex.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// SHA-256, 64 lowercase hex.
pub fn cache_key(key_json: &str) -> String {
    sha256_hex(key_json.as_bytes())
}

/// The first 32 lowercase hex of the SHA-256 of `key_json` for attempt 1; for attempt n ≥ 2, of
/// the canonical JSON with one more field "attempt": n.
pub fn idempotency_key(key_json: &str, attempt: u32) -> String {
    let text = if attempt <= 1 {
        key_json.to_string()
    } else {
        match serde_json::from_str::<Value>(key_json) {
            Ok(Value::Object(mut map)) => {
                map.insert("attempt".into(), Value::from(attempt));
                canonical_json(&Value::Object(map))
            }
            // Not a key's JSON: the attempt still makes a new key.
            _ => format!("{key_json}\u{0}attempt:{attempt}"),
        }
    };
    let mut hex = sha256_hex(text.as_bytes());
    hex.truncate(32);
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Fixed;

    impl Clock for Fixed {
        fn now_ms(&self) -> i64 {
            1_000
        }
        fn utc_offset_minutes(&self) -> i32 {
            0
        }
    }

    #[test]
    fn canonical_json_sorts_keys_drops_whitespace_and_composes_strings() {
        let value: Value = serde_json::from_str(
            r#"{ "b": [1, 2.5, true, null], "a": {"z": "café", "y": "x"}, "é": 1 }"#,
        )
        .unwrap();
        assert_eq!(
            canonical_json(&value),
            "{\"a\":{\"y\":\"x\",\"z\":\"café\"},\"b\":[1,2.5,true,null],\"é\":1}"
        );
        assert_eq!(canonical_json(&json!("a\"b")), r#""a\"b""#);
    }

    #[test]
    fn the_key_json_has_the_five_fields_and_the_key_is_its_sha256() {
        let key_json = cache_key_json(
            "judge",
            1,
            ProviderKind::Publik,
            "publik-fast",
            &json!({"answer_norm": "hola"}),
        );
        assert_eq!(
            key_json,
            r#"{"feature":"judge","input":{"answer_norm":"hola"},"model":"publik-fast","provider":"publik","v":1}"#
        );
        // SHA-256 of "abc" (FIPS 180-2, appendix B.1).
        assert_eq!(
            cache_key("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            idempotency_key("abc", 1),
            "ba7816bf8f01cfea414140de5dae2223"
        );
        let first = idempotency_key(&key_json, 1);
        assert_eq!(first, cache_key(&key_json)[..32]);
        let second = idempotency_key(&key_json, 2);
        let with_attempt = r#"{"attempt":2,"feature":"judge","input":{"answer_norm":"hola"},"model":"publik-fast","provider":"publik","v":1}"#;
        assert_eq!(second, cache_key(with_attempt)[..32]);
        assert_ne!(second, idempotency_key(&key_json, 3));
        assert!(
            second
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );
    }

    #[test]
    fn answers_and_attempts_persist_in_the_file_and_survive_its_removal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache").join("ai.sqlite3");
        let cache = Cache::open(&path, Box::new(Fixed)).unwrap();
        assert_eq!((cache.len(), cache.attempts("k")), (0, 0));
        assert!(cache.is_empty());
        cache
            .put(&CacheEntry {
                key: "k",
                provider: ProviderKind::Local,
                model: "llama3.1:8b",
                prompt_id: "ping",
                prompt_version: 1,
                response: r#"{"ok":true}"#,
                charge_micros: None,
            })
            .unwrap();
        assert_eq!(cache.add_attempt("j").unwrap(), 1);
        assert_eq!(cache.add_attempt("j").unwrap(), 2);
        let again = Cache::open(&path, Box::new(Fixed)).unwrap();
        assert_eq!(again.get("k").as_deref(), Some(r#"{"ok":true}"#));
        assert_eq!((again.len(), again.attempts("j")), (1, 2));
        // Nothing holds the file open, so it can be removed between operations.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(again.get("k"), None);
        assert_eq!(again.len(), 0);
        assert!(path.exists(), "the next operation starts a fresh file");
    }

    #[test]
    fn audio_rows_and_speech_charges_are_kept_and_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache").join("ai.sqlite3");
        assert!(Cache::open_existing(&path, Box::new(Fixed)).is_none());
        assert!(!path.exists(), "looking does not create the file");
        let cache = Cache::open(&path, Box::new(Fixed)).unwrap();
        assert_eq!((cache.audio_len(), cache.speech_charges()), (0, vec![]));
        for (key, voice) in [("a", "alloy"), ("b", "nova"), ("a", "alloy")] {
            cache
                .put_audio(&AudioEntry {
                    key,
                    provider: ProviderKind::Publik,
                    voice,
                    speed: 0.75,
                    file: &format!("cache/audio/{key}.mp3"),
                    bytes: 32_000,
                })
                .unwrap();
        }
        assert_eq!(cache.audio_len(), 2, "a key is one row");
        cache.add_speech_charge("a", 600).unwrap();
        cache.add_speech_charge("b", 640).unwrap();
        cache.add_speech_charge("b", 650).unwrap();
        let again = Cache::open_existing(&path, Box::new(Fixed)).unwrap();
        assert_eq!(again.speech_charges(), vec![600, 650]);
        assert_eq!(again.clear_audio().unwrap(), 2);
        assert_eq!(again.audio_len(), 0);
        assert_eq!(again.clear_audio().unwrap(), 0);
        assert_eq!(
            again.speech_charges(),
            vec![600, 650],
            "charges outlive the rows"
        );
        assert_eq!(again.len(), 0, "the answer cache is untouched");
    }

    #[test]
    fn take_charges_are_amounts_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache").join("ai.sqlite3");
        let cache = Cache::open(&path, Box::new(Fixed)).unwrap();
        assert!(cache.transcribe_charges().is_empty());
        for micros in [538, 500, 538] {
            cache.add_transcribe_charge(micros).unwrap();
        }
        let again = Cache::open_existing(&path, Box::new(Fixed)).unwrap();
        assert_eq!(again.transcribe_charges(), vec![538, 500, 538]);
        assert!(again.speech_charges().is_empty());
    }

    #[test]
    fn a_memory_cache_works_the_same_and_debug_names_no_path_for_it() {
        let cache = Cache::in_memory(Box::new(Fixed));
        assert_eq!(cache.get("k"), None);
        assert_eq!(cache.add_attempt("k").unwrap(), 1);
        assert_eq!(cache.attempts("k"), 1);
        assert!(format!("{cache:?}").contains("memory"));
    }
}
