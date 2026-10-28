//! Sentence packs: `parley pack build` turns Tatoeba exports into one SQLite file with
//! attribution, `pack info` describes it, `verify_pack` checks a download, and
//! `packs.lock.toml` pins the published packs (docs/modules/01-content-model-and-syllabus.md).
//!
//! The pack schema, the six filters, the frequency ranks and the content hash are specified
//! there. The content hash is SHA-256 over every row of every table in primary-key order, as TSV
//! lines, tables in the order `meta`, `sentences`, `translations`, `sentence_lemmas`, `forms`,
//! `freq`, leaving out the `meta` rows `built_at` and `content_sha256`, so two builds from the
//! same inputs on different days hash the same. Building and `pack_info` both compute it by
//! reading the tables back, so the two always agree.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, params};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::forms::Forms;
use super::numbers::{load_exclude, load_numbers};
use super::syllabus::Syllabus;
use super::tokenise::{Token, TokenKind, tokenise};
use super::wiktextract::read_wiktextract;
use super::{ContentError, Lang, display_path, load_syllabus, nfc, nfc_lower, read_text};

/// The pack's schema tag (`meta.schema`).
pub const PACK_SCHEMA: &str = "parley.pack/1";
/// The schema tag of `packs.lock.toml`.
pub const PACKS_LOCK_SCHEMA: &str = "parley.packs/1";
/// The licence of every pack's sentences.
pub const LICENCE: &str = "CC BY 2.0 FR";
/// The attribution sentence stored in every pack.
pub const ATTRIBUTION: &str = "Sentences and translations come from Tatoeba (tatoeba.org) and its contributors, under CC BY 2.0 FR.";

/// What `build_pack` reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackInput {
    pub lang: Lang,
    /// Holds `<iso3>_sentences_detailed.tsv`, `eng_sentences_detailed.tsv` and
    /// `<iso3>-eng_links.tsv`, decompressed.
    pub tatoeba_dir: PathBuf,
    pub wiktextract: PathBuf,
    pub syllabus: PathBuf,
    /// The exclude list; when none, `exclude.txt` beside the syllabus, else empty.
    pub exclude: Option<PathBuf>,
    /// `YYYY-MM-DD`, stored as `meta.built_at`; the core never reads the clock.
    pub built_at: String,
}

/// Why sentences were dropped, one count per filter.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Dropped {
    pub no_translation: usize,
    pub too_short: usize,
    pub too_long: usize,
    pub excluded_lemma: usize,
    pub unknown_word: usize,
    pub duplicate: usize,
}

/// The build report `pack build` prints.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PackReport {
    pub lang: Lang,
    pub kept: usize,
    pub dropped: Dropped,
}

/// What `pack info` prints.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PackInfo {
    pub schema: String,
    pub lang: String,
    pub built_at: String,
    pub tool_version: String,
    pub licence: String,
    pub sentences: u64,
    pub translations: u64,
    pub forms: u64,
    pub ranked_lemmas: u64,
    pub bytes: u64,
    /// SHA-256 of the file.
    pub sha256: String,
    /// Recomputed from the tables; equals the stored `meta` value.
    pub content_sha256: String,
    /// One entry per input file, keyed by file name.
    pub source_sha256: BTreeMap<String, String>,
}

/// Why a pack could not be built, read or verified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackError {
    /// An input or the pack is missing; the text is the path as typed.
    NotFound(String),
    /// The file's SHA-256 is not the expected one.
    ChecksumMismatch,
    /// The syllabus or another content file has errors.
    Content(Vec<ContentError>),
    /// A read or write failed; the text never carries an absolute path.
    Io(String),
    /// SQLite refused something; the text never carries an absolute path.
    Sqlite(String),
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackError::NotFound(file) => write!(f, "{file} not found"),
            PackError::ChecksumMismatch => f.write_str("The download was damaged. Try again."),
            PackError::Content(errors) => {
                let lines: Vec<&str> = errors.iter().map(|e| e.0.as_str()).collect();
                f.write_str(&lines.join("\n"))
            }
            PackError::Io(message) | PackError::Sqlite(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for PackError {}

impl From<rusqlite::Error> for PackError {
    fn from(e: rusqlite::Error) -> Self {
        PackError::Sqlite(e.to_string())
    }
}

/// `content/packs.lock.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PacksLock {
    pub schema: String,
    pub langs: BTreeMap<String, PackEntry>,
}

/// One language's published pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackEntry {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
    pub content_sha256: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn read_bytes(path: &Path, shown: &str) -> Result<Vec<u8>, PackError> {
    fs::read(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            PackError::NotFound(shown.to_string())
        } else {
            PackError::Io(format!("{shown}: {e}"))
        }
    })
}

/// A sentence row of a Tatoeba detailed export: id, lang, text, username, added, modified.
struct Sentence {
    id: i64,
    text: String,
    author: String,
}

fn parse_sentences(bytes: &[u8], shown: &str) -> Result<Vec<Sentence>, PackError> {
    let text = String::from_utf8_lossy(bytes);
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let n = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        let columns: Vec<&str> = line.split('\t').collect();
        if columns.len() < 3 {
            return Err(PackError::Io(format!(
                "{shown}: line {n}: expected at least 3 tab-separated columns"
            )));
        }
        let id: i64 = columns[0]
            .trim()
            .parse()
            .map_err(|_| PackError::Io(format!("{shown}: line {n}: id must be an integer")))?;
        // `\N` is Tatoeba's marker for a sentence without an owner.
        let author = match columns.get(3).map(|a| a.trim()) {
            Some("\\N") | None => "",
            Some(author) => author,
        };
        rows.push(Sentence {
            id,
            text: columns[2].to_string(),
            author: author.to_string(),
        });
    }
    Ok(rows)
}

fn parse_links(bytes: &[u8], shown: &str) -> Result<Vec<(i64, i64)>, PackError> {
    let text = String::from_utf8_lossy(bytes);
    let mut links = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let n = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        let mut columns = line.split('\t');
        let parse = |column: Option<&str>| -> Result<i64, PackError> {
            column.and_then(|c| c.trim().parse().ok()).ok_or_else(|| {
                PackError::Io(format!("{shown}: line {n}: expected two integer columns"))
            })
        };
        let from = parse(columns.next())?;
        let to = parse(columns.next())?;
        links.push((from, to));
    }
    Ok(links)
}

const SCHEMA_SQL: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE sentences (id INTEGER PRIMARY KEY, text TEXT NOT NULL, norm TEXT NOT NULL,
  tokens INTEGER NOT NULL, max_rank INTEGER NOT NULL, author TEXT NOT NULL);
CREATE TABLE translations (sentence_id INTEGER NOT NULL, en_id INTEGER NOT NULL,
  en_text TEXT NOT NULL, en_author TEXT NOT NULL, PRIMARY KEY (sentence_id, en_id));
CREATE TABLE sentence_lemmas (lemma TEXT NOT NULL, sentence_id INTEGER NOT NULL,
  PRIMARY KEY (lemma, sentence_id)) WITHOUT ROWID;
CREATE TABLE forms (form TEXT NOT NULL, lemma TEXT NOT NULL, pos TEXT NOT NULL,
  gender TEXT NOT NULL, tags TEXT NOT NULL, PRIMARY KEY (form, lemma, pos)) WITHOUT ROWID;
CREATE TABLE freq (lemma TEXT PRIMARY KEY, rank INTEGER NOT NULL, count REAL NOT NULL);
";

/// The tables in hash order with their primary keys.
const TABLES: [(&str, &str); 6] = [
    ("meta", "key"),
    ("sentences", "id"),
    ("translations", "sentence_id, en_id"),
    ("sentence_lemmas", "lemma, sentence_id"),
    ("forms", "form, lemma, pos"),
    ("freq", "lemma"),
];

/// A column as the content hash writes it: integers in decimal, reals as whole numbers, empty
/// text for an empty value.
fn hashed_column(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Null => String::new(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(r) => (r.round() as i64).to_string(),
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
        ValueRef::Blob(b) => String::from_utf8_lossy(b).into_owned(),
    }
}

/// The content hash of an open pack, read back from its tables in primary-key order.
fn content_hash(conn: &Connection) -> Result<String, PackError> {
    let mut hasher = Sha256::new();
    for (table, key) in TABLES {
        let filter = if table == "meta" {
            " WHERE key NOT IN ('built_at', 'content_sha256')"
        } else {
            ""
        };
        let sql = format!("SELECT * FROM {table}{filter} ORDER BY {key}");
        let mut statement = conn.prepare(&sql)?;
        let column_count = statement.column_count();
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let mut line = String::new();
            for column in 0..column_count {
                if column > 0 {
                    line.push('\t');
                }
                line.push_str(&hashed_column(row.get_ref(column)?));
            }
            line.push('\n');
            hasher.update(line.as_bytes());
        }
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// A sentence that passed every filter.
struct Kept {
    id: i64,
    text: String,
    norm: String,
    tokens: usize,
    author: String,
    lemmas: BTreeSet<String>,
    translations: Vec<i64>,
}

/// The words of the syllabus the filters accept besides forms: function words, names and numbers.
struct Known {
    function: HashSet<String>,
    names: HashSet<String>,
    names_lower: HashSet<String>,
    numbers: HashSet<String>,
    exclude: HashSet<String>,
}

impl Known {
    fn is_excluded(&self, token: &Token) -> bool {
        self.exclude.contains(&token.norm)
            || token
                .lemma_keys
                .iter()
                .any(|key| self.exclude.contains(key))
    }

    fn is_unknown(&self, token: &Token) -> bool {
        !(token.kind == TokenKind::Number
            || token.kind == TokenKind::Name
            || self.numbers.contains(&token.norm)
            || self.function.contains(&token.norm)
            || self.names.contains(&nfc(&token.surface))
            || self.names_lower.contains(&token.norm)
            || !token.lemma_keys.is_empty())
    }
}

fn sibling(path: &Path, name: &str) -> PathBuf {
    path.parent()
        .map(|dir| dir.join(name))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Builds the pack at `out` (replaced when it exists) and returns the report.
pub fn build_pack(input: &PackInput, out: &Path) -> Result<PackReport, PackError> {
    let lang = input.lang;
    let iso3 = lang.iso3();
    let dir = display_path(&input.tatoeba_dir);
    let tatoeba_names = [
        format!("{iso3}_sentences_detailed.tsv"),
        "eng_sentences_detailed.tsv".to_string(),
        format!("{iso3}-eng_links.tsv"),
    ];
    // Inputs are checked in order, so the first missing one is the error.
    let mut tatoeba_bytes = Vec::new();
    for name in &tatoeba_names {
        let shown = format!("{dir}/{name}");
        tatoeba_bytes.push((
            shown.clone(),
            read_bytes(&input.tatoeba_dir.join(name), &shown)?,
        ));
    }
    let wiktextract_shown = display_path(&input.wiktextract);
    let wiktextract_bytes = read_bytes(&input.wiktextract, &wiktextract_shown)?;
    let syllabus_shown = display_path(&input.syllabus);
    let syllabus_bytes = read_bytes(&input.syllabus, &syllabus_shown)?;
    let syllabus: Syllabus = load_syllabus(&input.syllabus).map_err(PackError::Content)?;

    let mut source_sha256: BTreeMap<String, String> = BTreeMap::new();
    for (name, (_, bytes)) in tatoeba_names.iter().zip(&tatoeba_bytes) {
        source_sha256.insert(name.clone(), sha256_hex(bytes));
    }
    source_sha256.insert(
        file_name(&input.wiktextract),
        sha256_hex(&wiktextract_bytes),
    );
    source_sha256.insert(file_name(&input.syllabus), sha256_hex(&syllabus_bytes));

    let exclude_path = match &input.exclude {
        Some(path) => Some(path.clone()),
        None => {
            let beside = sibling(&input.syllabus, "exclude.txt");
            beside.is_file().then_some(beside)
        }
    };
    let exclude: Vec<String> = match &exclude_path {
        Some(path) => {
            let shown = display_path(path);
            let bytes = read_bytes(path, &shown)?;
            source_sha256.insert(file_name(path), sha256_hex(&bytes));
            load_exclude(&String::from_utf8_lossy(&bytes))
        }
        None => Vec::new(),
    };
    let numbers_path = sibling(&input.syllabus, "numbers.yaml");
    let numbers: Vec<String> = if numbers_path.is_file() {
        let text = read_text(&numbers_path, &display_path(&numbers_path))
            .map_err(|e| PackError::Content(vec![e]))?;
        load_numbers(&text).map_err(|e| PackError::Content(vec![e]))?
    } else {
        Vec::new()
    };

    let extract = read_wiktextract(BufReader::new(&wiktextract_bytes[..]), lang, None)
        .map_err(|e| PackError::Content(vec![e]))?;
    let forms = Forms::from_rows(extract.rows);

    let mut sentences = parse_sentences(&tatoeba_bytes[0].1, &tatoeba_bytes[0].0)?;
    let english: HashMap<i64, Sentence> =
        parse_sentences(&tatoeba_bytes[1].1, &tatoeba_bytes[1].0)?
            .into_iter()
            .map(|s| (s.id, s))
            .collect();
    let links = parse_links(&tatoeba_bytes[2].1, &tatoeba_bytes[2].0)?;
    let mut english_of: HashMap<i64, BTreeSet<i64>> = HashMap::new();
    for (from, to) in links {
        if english.contains_key(&to) {
            english_of.entry(from).or_default().insert(to);
        }
        if english.contains_key(&from) {
            english_of.entry(to).or_default().insert(from);
        }
    }
    sentences.sort_by_key(|s| s.id);
    sentences.dedup_by_key(|s| s.id);

    let known = Known {
        function: syllabus
            .function_words
            .iter()
            .map(|w| nfc_lower(w))
            .collect(),
        names: syllabus.names.iter().map(|n| nfc(n)).collect(),
        names_lower: syllabus.names.iter().map(|n| nfc_lower(n)).collect(),
        numbers: numbers.iter().map(|w| nfc_lower(w)).collect(),
        exclude: exclude.iter().map(|w| nfc(w)).collect(),
    };

    let mut dropped = Dropped::default();
    let mut kept: Vec<Kept> = Vec::new();
    let mut seen_norms: HashSet<String> = HashSet::new();
    for sentence in sentences {
        let translations: Vec<i64> = english_of
            .get(&sentence.id)
            .map(|ids| ids.iter().copied().collect())
            .unwrap_or_default();
        if translations.is_empty() {
            dropped.no_translation += 1;
            continue;
        }
        let tokens = tokenise(lang, &sentence.text, &forms);
        if tokens.len() < 3 {
            dropped.too_short += 1;
            continue;
        }
        if tokens.len() > 12 || sentence.text.chars().count() > 120 {
            dropped.too_long += 1;
            continue;
        }
        if tokens.iter().any(|t| known.is_excluded(t)) {
            dropped.excluded_lemma += 1;
            continue;
        }
        if tokens.iter().any(|t| known.is_unknown(t)) {
            dropped.unknown_word += 1;
            continue;
        }
        let norm: Vec<&str> = tokens.iter().map(|t| t.norm.as_str()).collect();
        let norm = norm.join(" ");
        if !seen_norms.insert(norm.clone()) {
            dropped.duplicate += 1;
            continue;
        }
        kept.push(Kept {
            id: sentence.id,
            text: sentence.text,
            norm,
            tokens: tokens.len(),
            author: sentence.author,
            lemmas: tokens
                .iter()
                .flat_map(|t| t.lemma_keys.iter().cloned())
                .collect(),
            translations,
        });
    }

    // Frequency: per lemma, the kept sentences that contain it; ranked by count descending then
    // lemma ascending, from 1.
    let mut counts: HashMap<&str, u64> = HashMap::new();
    for sentence in &kept {
        for lemma in &sentence.lemmas {
            *counts.entry(lemma.as_str()).or_default() += 1;
        }
    }
    let mut ranked: Vec<(&str, u64)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let rank_of: HashMap<&str, u64> = ranked
        .iter()
        .enumerate()
        .map(|(i, (lemma, _))| (*lemma, i as u64 + 1))
        .collect();

    // Write the file.
    let out_shown = display_path(out);
    for stale in [out.to_path_buf(), {
        let mut journal = out.as_os_str().to_owned();
        journal.push("-journal");
        PathBuf::from(journal)
    }] {
        if let Err(e) = fs::remove_file(&stale)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            return Err(PackError::Io(format!("{out_shown}: {e}")));
        }
    }
    let mut conn = Connection::open(out)
        .map_err(|e| PackError::Sqlite(format!("{out_shown}: could not open: {e}")))?;
    conn.execute_batch(SCHEMA_SQL)?;
    let tx = conn.transaction()?;
    {
        let mut meta = tx.prepare("INSERT INTO meta (key, value) VALUES (?1, ?2)")?;
        meta.execute(params!["schema", PACK_SCHEMA])?;
        meta.execute(params!["lang", lang.as_str()])?;
        meta.execute(params!["built_at", input.built_at])?;
        meta.execute(params!["tool_version", crate::api::VERSION])?;
        meta.execute(params!["licence", LICENCE])?;
        meta.execute(params!["attribution", ATTRIBUTION])?;
        for (name, hash) in &source_sha256 {
            meta.execute(params![format!("source_sha256.{name}"), hash])?;
        }
        let mut insert_sentence = tx.prepare(
            "INSERT INTO sentences (id, text, norm, tokens, max_rank, author) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        let mut insert_translation = tx.prepare(
            "INSERT INTO translations (sentence_id, en_id, en_text, en_author) VALUES (?1, ?2, ?3, ?4)",
        )?;
        let mut insert_lemma =
            tx.prepare("INSERT INTO sentence_lemmas (lemma, sentence_id) VALUES (?1, ?2)")?;
        for sentence in &kept {
            let max_rank = sentence
                .lemmas
                .iter()
                .filter_map(|l| rank_of.get(l.as_str()))
                .max()
                .copied()
                .unwrap_or(0);
            insert_sentence.execute(params![
                sentence.id,
                sentence.text,
                sentence.norm,
                sentence.tokens as i64,
                max_rank as i64,
                sentence.author
            ])?;
            for en_id in &sentence.translations {
                let en = &english[en_id];
                insert_translation.execute(params![sentence.id, en_id, en.text, en.author])?;
            }
            for lemma in &sentence.lemmas {
                insert_lemma.execute(params![lemma, sentence.id])?;
            }
        }
        let mut insert_form = tx.prepare(
            "INSERT OR IGNORE INTO forms (form, lemma, pos, gender, tags) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for row in forms.rows() {
            insert_form.execute(params![
                row.form,
                row.lemma,
                row.pos,
                row.gender.map_or("", |g| g.as_str()),
                row.tags
            ])?;
        }
        let mut insert_freq =
            tx.prepare("INSERT INTO freq (lemma, rank, count) VALUES (?1, ?2, ?3)")?;
        for (i, (lemma, count)) in ranked.iter().enumerate() {
            insert_freq.execute(params![lemma, i as i64 + 1, *count as f64])?;
        }
    }
    tx.commit()?;
    let hash = content_hash(&conn)?;
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('content_sha256', ?1)",
        params![hash],
    )?;
    drop(conn);

    Ok(PackReport {
        lang,
        kept: kept.len(),
        dropped,
    })
}

fn count(conn: &Connection, table: &str) -> Result<u64, PackError> {
    let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })?;
    Ok(n.max(0) as u64)
}

/// Describes a pack: its `meta`, row counts, file size and hashes. The content hash is recomputed
/// from the tables and must equal the stored one.
pub fn pack_info(path: &Path) -> Result<PackInfo, PackError> {
    let shown = display_path(path);
    let bytes = read_bytes(path, &shown)?;
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| PackError::Sqlite(format!("{shown}: could not open: {e}")))?;
    let mut meta: BTreeMap<String, String> = BTreeMap::new();
    {
        let mut statement = conn.prepare("SELECT key, value FROM meta")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        for row in rows {
            let (key, value): (String, String) = row?;
            meta.insert(key, value);
        }
    }
    let content_sha256 = content_hash(&conn)?;
    if let Some(stored) = meta.get("content_sha256")
        && *stored != content_sha256
    {
        return Err(PackError::Sqlite(format!(
            "{shown}: content_sha256 in meta does not match the tables"
        )));
    }
    let get = |key: &str| meta.get(key).cloned().unwrap_or_default();
    let source_sha256 = meta
        .iter()
        .filter_map(|(key, value)| {
            key.strip_prefix("source_sha256.")
                .map(|name| (name.to_string(), value.clone()))
        })
        .collect();
    Ok(PackInfo {
        schema: get("schema"),
        lang: get("lang"),
        built_at: get("built_at"),
        tool_version: get("tool_version"),
        licence: get("licence"),
        sentences: count(&conn, "sentences")?,
        translations: count(&conn, "translations")?,
        forms: count(&conn, "forms")?,
        ranked_lemmas: count(&conn, "freq")?,
        bytes: bytes.len() as u64,
        sha256: sha256_hex(&bytes),
        content_sha256,
        source_sha256,
    })
}

/// Checks a downloaded pack against its pinned SHA-256.
pub fn verify_pack(path: &Path, expected_sha256: &str) -> Result<(), PackError> {
    let bytes = read_bytes(path, &display_path(path))?;
    if sha256_hex(&bytes).eq_ignore_ascii_case(expected_sha256.trim()) {
        Ok(())
    } else {
        Err(PackError::ChecksumMismatch)
    }
}

/// Parses `packs.lock.toml`: `schema = "parley.packs/1"` and per language a table `[es]` with
/// `version`, `url`, `sha256`, `bytes` and `content_sha256`.
pub fn parse_packs_lock(text: &str) -> Result<PacksLock, ContentError> {
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| ContentError(format!("packs.lock.toml: {}", e.message())))?;
    let schema = table
        .get("schema")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| ContentError("packs.lock.toml: schema is required".to_string()))?;
    if schema != PACKS_LOCK_SCHEMA {
        return Err(ContentError(format!(
            "packs.lock.toml: schema must be {PACKS_LOCK_SCHEMA}"
        )));
    }
    let mut langs = BTreeMap::new();
    for (lang, value) in &table {
        if lang == "schema" {
            continue;
        }
        let entry = value
            .as_table()
            .ok_or_else(|| ContentError(format!("packs.lock.toml: [{lang}] must be a table")))?;
        let text = |key: &str| -> Result<String, ContentError> {
            entry
                .get(key)
                .ok_or_else(|| ContentError(format!("packs.lock.toml: [{lang}] needs {key}")))?
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| {
                    ContentError(format!("packs.lock.toml: [{lang}] {key} must be a string"))
                })
        };
        let version = text("version")?;
        let url = text("url")?;
        let sha256 = text("sha256")?;
        let bytes = entry
            .get("bytes")
            .ok_or_else(|| ContentError(format!("packs.lock.toml: [{lang}] needs bytes")))?
            .as_integer()
            .and_then(|n| u64::try_from(n).ok())
            .ok_or_else(|| {
                ContentError(format!(
                    "packs.lock.toml: [{lang}] bytes must be an integer"
                ))
            })?;
        let content_sha256 = text("content_sha256")?;
        langs.insert(
            lang.clone(),
            PackEntry {
                version,
                url,
                sha256,
                bytes,
                content_sha256,
            },
        );
    }
    Ok(PacksLock {
        schema: schema.to_string(),
        langs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::test_support::EXAMPLE_SYLLABUS;

    const EXTRACT: &str = concat!(
        r#"{"word":"bueno","pos":"adj","lang_code":"es","forms":[{"form":"buenos","tags":["masculine","plural"]},{"form":"buenas","tags":["feminine","plural"]}]}"#,
        "\n",
        r#"{"word":"día","pos":"noun","lang_code":"es","tags":["masculine"],"forms":[{"form":"días","tags":["plural"]}]}"#,
        "\n",
        r#"{"word":"noche","pos":"noun","lang_code":"es","tags":["feminine"],"forms":[{"form":"noches","tags":["plural"]}]}"#,
        "\n",
        r#"{"word":"gracias","pos":"intj","lang_code":"es"}"#,
        "\n",
        r#"{"word":"hola","pos":"intj","lang_code":"es"}"#,
        "\n",
        r#"{"word":"ser","pos":"verb","lang_code":"es","forms":[{"form":"es","tags":["present"]},{"form":"soy","tags":["present"]}]}"#,
        "\n",
        r#"{"word":"tonto","pos":"adj","lang_code":"es"}"#,
        "\n",
    );

    /// A tiny Tatoeba export whose fates are known by construction.
    fn write_inputs(dir: &Path) -> (PathBuf, PathBuf) {
        let tatoeba = dir.join("tatoeba");
        fs::create_dir_all(&tatoeba).unwrap();
        let spa = "\
1001\tspa\tBuenos días, Ana.\tfixture_a\t2020-01-01 00:00:00\t2020-01-01 00:00:00
1002\tspa\tBuenas noches.\t\\N\t2020-01-01 00:00:00\t2020-01-01 00:00:00
1003\tspa\tHola, buenos días.\tfixture_b\t2020-01-01 00:00:00\t2020-01-01 00:00:00
1004\tspa\tHola.\tfixture_a\t2020-01-01 00:00:00\t2020-01-01 00:00:00
1005\tspa\tBuenos días a todos.\tfixture_a\t2020-01-01 00:00:00\t2020-01-01 00:00:00
1006\tspa\tEs tonto el día.\tfixture_a\t2020-01-01 00:00:00\t2020-01-01 00:00:00
1007\tspa\tbuenos DÍAS, Ana!\tfixture_a\t2020-01-01 00:00:00\t2020-01-01 00:00:00
1008\tspa\tHola hola hola hola hola hola hola hola hola hola hola hola hola.\tfixture_a\t2020-01-01 00:00:00\t2020-01-01 00:00:00
1009\tspa\tBuenas noches, Luis.\tfixture_a\t2020-01-01 00:00:00\t2020-01-01 00:00:00
";
        let eng = "\
2001\teng\tGood morning, Ana.\tfixture_e\t2020-01-01 00:00:00\t2020-01-01 00:00:00
2002\teng\tGood night.\tfixture_e\t2020-01-01 00:00:00\t2020-01-01 00:00:00
2003\teng\tHi, good morning.\tfixture_e\t2020-01-01 00:00:00\t2020-01-01 00:00:00
2004\teng\tHi.\tfixture_e\t2020-01-01 00:00:00\t2020-01-01 00:00:00
2006\teng\tThe day is silly.\tfixture_e\t2020-01-01 00:00:00\t2020-01-01 00:00:00
2007\teng\tGood morning, Ana!\tfixture_e\t2020-01-01 00:00:00\t2020-01-01 00:00:00
2008\teng\tHi hi.\tfixture_e\t2020-01-01 00:00:00\t2020-01-01 00:00:00
2009\teng\tGood night, Luis.\tfixture_e\t2020-01-01 00:00:00\t2020-01-01 00:00:00
";
        let links = "1001\t2001\n1002\t2002\n1003\t2003\n1004\t2004\n1006\t2006\n1007\t2007\n1008\t2008\n2009\t1009\n1005\t9999\n";
        fs::write(tatoeba.join("spa_sentences_detailed.tsv"), spa).unwrap();
        fs::write(tatoeba.join("eng_sentences_detailed.tsv"), eng).unwrap();
        fs::write(tatoeba.join("spa-eng_links.tsv"), links).unwrap();
        let content = dir.join("content");
        fs::create_dir_all(&content).unwrap();
        fs::write(content.join("syllabus.yaml"), EXAMPLE_SYLLABUS).unwrap();
        fs::write(content.join("exclude.txt"), "# never shown\ntonto\n").unwrap();
        fs::write(dir.join("extract.jsonl"), EXTRACT).unwrap();
        (tatoeba, content.join("syllabus.yaml"))
    }

    fn input(dir: &Path) -> PackInput {
        let (tatoeba_dir, syllabus) = write_inputs(dir);
        PackInput {
            lang: Lang::Es,
            tatoeba_dir,
            wiktextract: dir.join("extract.jsonl"),
            syllabus,
            exclude: None,
            built_at: "2026-10-01".to_string(),
        }
    }

    #[test]
    fn filters_drop_each_sentence_for_its_reason() {
        let dir = tempfile::tempdir().unwrap();
        let input = input(dir.path());
        let out = dir.path().join("es.sqlite3");
        let report = build_pack(&input, &out).unwrap();
        // 1001 kept; 1002 kept (3 tokens? "Buenas noches." is 2 tokens: too short);
        // 1003 kept; 1004 too short; 1005 no translation; 1006 excluded (tonto);
        // 1007 duplicate of 1001; 1008 too long (13 words); 1009 kept (reversed link).
        assert_eq!(
            report,
            PackReport {
                lang: Lang::Es,
                kept: 3,
                dropped: Dropped {
                    no_translation: 1,
                    too_short: 2,
                    too_long: 1,
                    excluded_lemma: 1,
                    unknown_word: 0,
                    duplicate: 1,
                },
            }
        );
        assert_eq!(
            serde_json::to_string(&report).unwrap(),
            r#"{"lang":"es","kept":3,"dropped":{"no_translation":1,"too_short":2,"too_long":1,"excluded_lemma":1,"unknown_word":0,"duplicate":1}}"#
        );

        let info = pack_info(&out).unwrap();
        assert_eq!(info.schema, PACK_SCHEMA);
        assert_eq!(info.lang, "es");
        assert_eq!(info.built_at, "2026-10-01");
        assert_eq!(info.tool_version, crate::api::VERSION);
        assert_eq!(info.licence, LICENCE);
        assert_eq!(info.sentences, 3);
        assert_eq!(info.translations, 3);
        assert_eq!(
            info.forms, 13,
            "every row of the extract, headwords included"
        );
        assert!(info.ranked_lemmas >= 4);
        assert_eq!(info.bytes, fs::metadata(&out).unwrap().len());
        assert_eq!(info.sha256.len(), 64);
        let keys: Vec<&String> = info.source_sha256.keys().collect();
        assert_eq!(
            keys,
            [
                "eng_sentences_detailed.tsv",
                "exclude.txt",
                "extract.jsonl",
                "spa-eng_links.tsv",
                "spa_sentences_detailed.tsv",
                "syllabus.yaml"
            ]
        );

        let conn = Connection::open(&out).unwrap();
        let (text, author, max_rank): (String, String, i64) = conn
            .query_row(
                "SELECT text, author, max_rank FROM sentences WHERE id = 1001",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(text, "Buenos días, Ana.");
        assert_eq!(author, "fixture_a");
        assert!(max_rank >= 1);
        let en: (i64, String) = conn
            .query_row(
                "SELECT en_id, en_text FROM translations WHERE sentence_id = 1001",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(en, (2001, "Good morning, Ana.".to_string()));
        let stored: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'content_sha256'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, info.content_sha256);
        let rank_bueno: i64 = conn
            .query_row("SELECT rank FROM freq WHERE lemma = 'bueno'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rank_bueno, 1, "bueno is in all three kept sentences");
    }

    #[test]
    fn two_builds_on_different_days_hash_the_same_and_verify_catches_one_byte() {
        let dir = tempfile::tempdir().unwrap();
        let mut input = input(dir.path());
        let a = dir.path().join("a.sqlite3");
        let b = dir.path().join("b.sqlite3");
        build_pack(&input, &a).unwrap();
        input.built_at = "2026-11-01".to_string();
        build_pack(&input, &b).unwrap();
        let info_a = pack_info(&a).unwrap();
        let info_b = pack_info(&b).unwrap();
        assert_eq!(info_a.content_sha256, info_b.content_sha256);
        assert_ne!(info_a.built_at, info_b.built_at);

        assert_eq!(verify_pack(&a, &info_a.sha256), Ok(()));
        let mut bytes = fs::read(&a).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        let damaged = dir.path().join("damaged.sqlite3");
        fs::write(&damaged, bytes).unwrap();
        let err = verify_pack(&damaged, &info_a.sha256).unwrap_err();
        assert_eq!(err, PackError::ChecksumMismatch);
        assert_eq!(err.to_string(), "The download was damaged. Try again.");
        assert_eq!(
            verify_pack(Path::new("missing/pack.sqlite3"), "x").unwrap_err(),
            PackError::NotFound("missing/pack.sqlite3".to_string())
        );
    }

    #[test]
    fn a_missing_tatoeba_file_is_reported_first_and_as_typed() {
        let dir = tempfile::tempdir().unwrap();
        let mut input = input(dir.path());
        fs::remove_file(input.tatoeba_dir.join("spa-eng_links.tsv")).unwrap();
        input.tatoeba_dir = PathBuf::from("tests\\fixtures\\tatoeba-mini");
        let err = build_pack(&input, &dir.path().join("x.sqlite3")).unwrap_err();
        assert_eq!(
            err,
            PackError::NotFound("tests/fixtures/tatoeba-mini/spa_sentences_detailed.tsv".into())
        );
        assert_eq!(
            err.to_string(),
            "tests/fixtures/tatoeba-mini/spa_sentences_detailed.tsv not found"
        );
    }

    #[test]
    fn packs_lock_needs_every_key() {
        let lock = parse_packs_lock("schema = \"parley.packs/1\"\n").unwrap();
        assert!(lock.langs.is_empty());
        let err = parse_packs_lock(
            "schema = \"parley.packs/1\"\n[es]\nversion = \"2026-10-01\"\nurl = \"https://example.invalid/es.sqlite3\"\nbytes = 1\ncontent_sha256 = \"a\"\n",
        )
        .unwrap_err();
        assert_eq!(err.0, "packs.lock.toml: [es] needs sha256");
        let lock = parse_packs_lock(
            "schema = \"parley.packs/1\"\n[es]\nversion = \"2026-10-01\"\nurl = \"https://example.invalid/es.sqlite3\"\nsha256 = \"b\"\nbytes = 1\ncontent_sha256 = \"a\"\n",
        )
        .unwrap();
        assert_eq!(lock.langs["es"].bytes, 1);
        assert_eq!(lock.langs["es"].sha256, "b");
        let err = parse_packs_lock("schema = \"other\"\n").unwrap_err();
        assert_eq!(err.0, "packs.lock.toml: schema must be parley.packs/1");
        let err = parse_packs_lock("schema = [\n").unwrap_err();
        assert!(err.0.starts_with("packs.lock.toml: "), "{}", err.0);
        assert!(!err.0.contains('\n'), "{}", err.0);
    }

    #[test]
    fn pack_info_refuses_a_pack_whose_stored_hash_differs_from_its_tables() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("es.sqlite3");
        build_pack(&input(dir.path()), &out).unwrap();
        {
            let conn = Connection::open(&out).unwrap();
            conn.execute(
                "UPDATE meta SET value = ?1 WHERE key = 'content_sha256'",
                ["0".repeat(64)],
            )
            .unwrap();
        }
        let err = pack_info(&out).unwrap_err();
        assert!(matches!(err, PackError::Sqlite(_)), "{err:?}");
        assert!(
            err.to_string()
                .ends_with("content_sha256 in meta does not match the tables"),
            "{err}"
        );
    }

    #[test]
    fn packs_lock_names_a_missing_schema_and_a_wrong_shape() {
        let cases = [
            (
                "[es]\nversion = \"2026-10-01\"\n",
                "packs.lock.toml: schema is required",
            ),
            (
                "schema = \"parley.packs/1\"\nes = 1\n",
                "packs.lock.toml: [es] must be a table",
            ),
            (
                "schema = \"parley.packs/1\"\n[es]\nversion = 1\nurl = \"u\"\nsha256 = \"s\"\nbytes = 1\ncontent_sha256 = \"c\"\n",
                "packs.lock.toml: [es] version must be a string",
            ),
            (
                "schema = \"parley.packs/1\"\n[es]\nversion = \"v\"\nurl = \"u\"\nsha256 = \"s\"\nbytes = \"1\"\ncontent_sha256 = \"c\"\n",
                "packs.lock.toml: [es] bytes must be an integer",
            ),
        ];
        for (text, message) in cases {
            let err = parse_packs_lock(text).expect_err(message);
            assert_eq!(err.0, message);
        }
    }
}
