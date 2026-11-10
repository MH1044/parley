//! The backup file, `parley.backup/1` (docs/modules/04-app-shell-and-offline-course.md,
//! Interfaces and Design notes).
//!
//! A backup is the settings and every table of `parley.sqlite3` except what the bundled course
//! rebuilds (syllabus items and `content` answers), pretty-printed with two spaces, keys in the
//! documented order. It never holds a key, the Publik credential, a cache, a recording or a pack
//! file. Reading one checks, in order: JSON, the schema tag (a newer `parley.backup/N` is named as
//! such), `schemas/backup.schema.json`, then the settings it carries.

use std::sync::OnceLock;

use jsonschema::Validator;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::VERSION;
use crate::env::LocalDate;
use crate::settings::{SETTINGS_SCHEMA, Settings, settings_from_value};
use crate::srs::DAY_MS;
use crate::store::{
    AnswerDump, CardDump, CourseDump, DailyDump, FreezeDump, ItemDump, JudgementDump, LessonDump,
    ReviewDump, SessionDump, StoryDump, Tables,
};

/// The schema tag of a backup.
pub const BACKUP_SCHEMA: &str = "parley.backup/1";

/// The message of a file that is not a backup this version can read.
pub const NOT_A_BACKUP: &str = "This is not a Parley backup.";
/// The message of a backup from a newer version.
pub const NEWER_BACKUP: &str =
    "This backup was made by a newer Parley. Update Parley to import it.";

const SCHEMA_TEXT: &str = include_str!("../../../schemas/backup.schema.json");

/// The compiled `schemas/backup.schema.json`.
pub fn backup_validator() -> &'static Validator {
    static VALIDATOR: OnceLock<Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| {
        let schema: Value =
            serde_json::from_str(SCHEMA_TEXT).expect("the backup schema is valid JSON");
        jsonschema::validator_for(&schema).expect("the backup schema is a valid JSON Schema")
    })
}

/// A backup file, keys in the order they are written. The schema requires every key; within a
/// row, a column that is nullable or has a default may be left out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Backup {
    pub schema: String,
    #[serde(default)]
    pub app_version: String,
    #[serde(default)]
    pub exported_at: String,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub courses: Vec<CourseDump>,
    #[serde(default)]
    pub items: Vec<ItemDump>,
    #[serde(default)]
    pub accepted_answers: Vec<AnswerDump>,
    #[serde(default)]
    pub cards: Vec<CardDump>,
    #[serde(default)]
    pub review_log: Vec<ReviewDump>,
    #[serde(default)]
    pub sessions: Vec<SessionDump>,
    #[serde(default)]
    pub lessons: Vec<LessonDump>,
    #[serde(default)]
    pub judgements: Vec<JudgementDump>,
    #[serde(default)]
    pub daily: Vec<DailyDump>,
    #[serde(default)]
    pub freeze_events: Vec<FreezeDump>,
    /// Stories (module 07), optional so a backup made before them still imports.
    #[serde(default)]
    pub stories: Vec<StoryDump>,
}

impl Backup {
    /// A backup of `tables` and `settings` made at `now_ms`.
    pub fn new(settings: &Settings, tables: Tables, now_ms: i64) -> Backup {
        Backup {
            schema: BACKUP_SCHEMA.into(),
            app_version: VERSION.into(),
            exported_at: utc_timestamp(now_ms),
            settings: settings.clone(),
            courses: tables.courses,
            items: tables.items,
            accepted_answers: tables.accepted_answers,
            cards: tables.cards,
            review_log: tables.review_log,
            sessions: tables.sessions,
            lessons: tables.lessons,
            judgements: tables.judgements,
            daily: tables.daily,
            freeze_events: tables.freeze_events,
            stories: tables.stories,
        }
    }

    /// The tables the backup carries.
    pub fn tables(&self) -> Tables {
        Tables {
            courses: self.courses.clone(),
            items: self.items.clone(),
            accepted_answers: self.accepted_answers.clone(),
            cards: self.cards.clone(),
            review_log: self.review_log.clone(),
            sessions: self.sessions.clone(),
            lessons: self.lessons.clone(),
            judgements: self.judgements.clone(),
            daily: self.daily.clone(),
            freeze_events: self.freeze_events.clone(),
            stories: self.stories.clone(),
        }
    }

    /// The file's text: pretty-printed with two spaces and a final newline.
    pub fn to_file_text(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).expect("a backup serialises");
        text.push('\n');
        text
    }
}

/// Why a file could not be read as a backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupError {
    /// Not JSON, another schema, or not matching the backup or settings schema.
    NotABackup,
    /// `parley.backup/N` with N > 1.
    Newer,
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BackupError::NotABackup => NOT_A_BACKUP,
            BackupError::Newer => NEWER_BACKUP,
        })
    }
}

impl std::error::Error for BackupError {}

/// Reads a backup file's bytes.
pub fn parse_backup(bytes: &[u8]) -> Result<Backup, BackupError> {
    let text = std::str::from_utf8(bytes).map_err(|_| BackupError::NotABackup)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut value: Value = serde_json::from_str(text).map_err(|_| BackupError::NotABackup)?;
    let schema = value
        .get("schema")
        .and_then(Value::as_str)
        .ok_or(BackupError::NotABackup)?;
    if schema != BACKUP_SCHEMA {
        let newer = schema
            .strip_prefix("parley.backup/")
            .and_then(|n| n.parse::<u64>().ok())
            .is_some_and(|n| n > 1);
        return Err(if newer {
            BackupError::Newer
        } else {
            BackupError::NotABackup
        });
    }
    if !backup_validator().is_valid(&value) {
        return Err(BackupError::NotABackup);
    }
    // The settings are checked on their own; a settings object without its tag is taken as
    // version 1.
    let settings = match value.get_mut("settings") {
        Some(Value::Object(map)) => {
            map.entry("schema")
                .or_insert_with(|| Value::String(SETTINGS_SCHEMA.into()));
            settings_from_value(Value::Object(map.clone())).map_err(|_| BackupError::NotABackup)?
        }
        _ => Settings::default(),
    };
    if let Some(object) = value.as_object_mut() {
        object.remove("settings");
    }
    let mut backup: Backup = serde_json::from_value(value).map_err(|_| BackupError::NotABackup)?;
    for date in backup
        .sessions
        .iter()
        .map(|s| &s.local_date)
        .chain(backup.daily.iter().map(|d| &d.local_date))
        .chain(backup.freeze_events.iter().map(|e| &e.local_date))
    {
        if LocalDate::parse(date).is_none() {
            return Err(BackupError::NotABackup);
        }
    }
    backup.settings = settings;
    Ok(backup)
}

/// `YYYY-MM-DDTHH:MM:SSZ` for a time in milliseconds since the epoch.
pub fn utc_timestamp(ms: i64) -> String {
    let date = LocalDate::from_days(ms.div_euclid(DAY_MS));
    let seconds = ms.rem_euclid(DAY_MS) / 1_000;
    format!(
        "{date}T{:02}:{:02}:{:02}Z",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal() -> Value {
        serde_json::json!({
            "schema": "parley.backup/1",
            "app_version": "0.4.0",
            "exported_at": "2026-10-12T08:30:00Z",
            "settings": {"schema": "parley.settings/1", "active_course": "es", "daily_goal_xp": 30},
            "courses": [{"lang": "es", "syllabus_version": 1, "pack_version": null,
                         "started_at": 1760000000000_i64, "placement_index": null,
                         "cefr_estimate": null, "tested_out": []}],
            "items": [], "accepted_answers": [], "cards": [], "review_log": [],
            "sessions": [], "lessons": [], "judgements": [], "daily": [], "freeze_events": []
        })
    }

    fn parse(value: &Value) -> Result<Backup, BackupError> {
        parse_backup(serde_json::to_string(value).unwrap().as_bytes())
    }

    #[test]
    fn timestamps_are_utc_to_the_second() {
        assert_eq!(utc_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(
            utc_timestamp(20_738 * DAY_MS + (8 * 3_600 + 30 * 60) * 1_000 + 999),
            "2026-10-12T08:30:00Z"
        );
    }

    #[test]
    fn a_backup_round_trips_with_its_keys_in_order() {
        let backup = parse(&minimal()).unwrap();
        assert_eq!(backup.settings.daily_goal_xp, 30);
        assert_eq!(backup.courses[0].lang, "es");
        let text = backup.to_file_text();
        let keys: Vec<&str> = text
            .lines()
            .filter(|line| line.starts_with("  \"") && !line.starts_with("    "))
            .map(|line| line.trim().split('"').nth(1).unwrap())
            .collect();
        assert_eq!(
            keys,
            [
                "schema",
                "app_version",
                "exported_at",
                "settings",
                "courses",
                "items",
                "accepted_answers",
                "cards",
                "review_log",
                "sessions",
                "lessons",
                "judgements",
                "daily",
                "freeze_events",
                "stories"
            ]
        );
        assert_eq!(parse_backup(text.as_bytes()).unwrap(), backup);
    }

    #[test]
    fn other_files_are_refused_with_their_message() {
        let mut other = minimal();
        other["schema"] = "other".into();
        assert_eq!(parse(&other).unwrap_err(), BackupError::NotABackup);
        let mut newer = minimal();
        newer["schema"] = "parley.backup/2".into();
        assert_eq!(parse(&newer).unwrap_err().to_string(), NEWER_BACKUP);
        let mut extra = minimal();
        extra["colour"] = "blue".into();
        assert_eq!(parse(&extra).unwrap_err(), BackupError::NotABackup);
        let mut short = minimal();
        short.as_object_mut().unwrap().remove("freeze_events");
        assert_eq!(parse(&short).unwrap_err(), BackupError::NotABackup);
        assert_eq!(
            parse(&serde_json::json!({"schema": "parley.backup/1"})).unwrap_err(),
            BackupError::NotABackup
        );
        let mut bad_settings = minimal();
        bad_settings["settings"]["lesson_length"] = 99.into();
        assert_eq!(parse(&bad_settings).unwrap_err(), BackupError::NotABackup);
        let mut bad_date = minimal();
        bad_date["daily"] = serde_json::json!([{"local_date": "2026-02-30", "xp": 1, "sessions": 1, "goal_xp": 20}]);
        assert_eq!(parse(&bad_date).unwrap_err(), BackupError::NotABackup);
        assert_eq!(
            parse_backup(b"not json").unwrap_err().to_string(),
            NOT_A_BACKUP
        );
        assert_eq!(parse_backup(b"[]").unwrap_err(), BackupError::NotABackup);
    }

    #[test]
    fn columns_with_defaults_may_be_left_out() {
        let mut lean = minimal();
        lean["courses"] =
            serde_json::json!([{"lang": "es", "syllabus_version": 1, "started_at": 5}]);
        lean["daily"] = serde_json::json!([{"local_date": "2026-10-12", "xp": 12, "sessions": 1, "goal_xp": 20}]);
        let backup = parse(&lean).unwrap();
        assert_eq!(backup.courses[0].tested_out, Vec::<String>::new());
        assert_eq!(backup.daily[0].frozen, 0);
        let mut no_tag = minimal();
        no_tag["settings"] = serde_json::json!({"daily_goal_xp": 50});
        assert_eq!(parse(&no_tag).unwrap().settings.daily_goal_xp, 50);
    }
}
