//! Command argument and result types (docs/ARCHITECTURE.md §5;
//! docs/modules/04-app-shell-and-offline-course.md, Interfaces).
//!
//! `app/src/api/commands.ts` mirrors these by hand. [`COMMANDS`] lists every command name in the
//! order the UI wrapper lists them; a test compares the two lists. Field names serialise as
//! written, and an empty option as `null`.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ai::{Story, VoiceSource};
use crate::content::{Cefr, Lang};
use crate::env::{LocalDate, local_date_at};
use crate::error::AppError;
use crate::exercise::{Exercise, Origin};
use crate::grading::Grade;
use crate::speech::SpeakScore;
use crate::srs::{DAY_MS, SkillState};

/// Every Tauri command the core backs, in the order `app/src/api/commands.ts` lists them.
pub const COMMANDS: &[&str] = &[
    "app_start",
    "app_info",
    "file_pick",
    "data_delete_all",
    "cache_clear",
    "settings_get",
    "settings_set",
    "course_list",
    "course_start",
    "pack_status",
    "pack_download",
    "path_get",
    "skill_get",
    "placement_start",
    "placement_answer",
    "lesson_start",
    "review_start",
    "exercise_submit",
    "exercise_i_was_right",
    "lesson_finish",
    "stats_get",
    "backup_export",
    "backup_import",
    // Module 06: AI providers.
    "ai_status",
    "ai_consent_accept",
    "ai_consent_decline",
    "ai_set_own_key",
    "ai_clear_own_key",
    "ai_local_probe",
    "ai_set_local",
    "ai_check_connection",
    "ai_disconnect_publik",
    "ai_reconnect_publik",
    "ai_open_link",
    // Module 07: lesson generation and course packs.
    "lesson_use_course_pack",
    "course_pack_export",
    "course_pack_import",
    "ai_lesson_checks",
    // Module 08: judging and explanations.
    "exercise_check",
    "explain_answer",
    // Module 09: listening and voices.
    "voices_list",
    "tts_play",
    "tts_stop",
    "tts_prefetch",
    "audio_status",
    "session_skip",
    // Module 10: speaking and pronunciation.
    "mic_start",
    "mic_stop",
    "recording_play",
    "speak_check",
    "whisper_models",
    "whisper_download",
    "speaking_status",
];

/// The event every AI command that changes the status emits it on, with the [`crate::ai::AiStatus`].
pub const AI_STATUS_EVENT: &str = "ai://status";

/// The event `tts_play` emits when the clip or the OS voice it started ends (module 09).
pub const AUDIO_ENDED_EVENT: &str = "audio://ended";

/// The event `mic_start` emits with the level of every 50 ms of input, a [`MicLevel`] (module 10).
pub const MIC_LEVEL_EVENT: &str = "mic://level";

/// The event `mic_start` emits once, with `{}`, when the take reaches its limit (module 10).
pub const MIC_LIMIT_EVENT: &str = "mic://limit";

/// The event `whisper_download` reports its progress on, a [`ModelProgress`] (module 10).
pub const MODEL_PROGRESS_EVENT: &str = "model://progress";

/// The version shared by every crate and the app (the workspace version in Cargo.toml).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The notice of a review with nothing due and no reviewed card at all.
pub const NOTHING_TO_REVIEW_YET: &str =
    "Nothing to review yet. Finish a lesson to start your reviews.";

/// Result of `app_info`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInfo {
    pub version: String,
    /// The app data directory, shown on the About page so a learner can find their data.
    pub data_dir: String,
}

/// The running version and the data directory the app was given.
pub fn app_info(data_dir: &Path) -> AppInfo {
    AppInfo {
        version: VERSION.to_string(),
        data_dir: data_dir.display().to_string(),
    }
}

/// A size in decimal megabytes to one decimal place, rounded half up in integer arithmetic:
/// `(bytes + 50,000) / 100,000` tenths (8,200,000 bytes → "8.2 MB").
pub fn megabytes(bytes: u64) -> String {
    let tenths = bytes.saturating_add(50_000) / 100_000;
    format!("{}.{} MB", tenths / 10, tenths % 10)
}

/// The English name of a course language.
pub fn language_name(lang: Lang) -> &'static str {
    match lang {
        Lang::Es => "Spanish",
        Lang::Fr => "French",
        Lang::De => "German",
    }
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The notice of a review with nothing due: "Nothing to review. Next review: {when}." where
/// {when} is the next card's due time at the clock's UTC offset: "today at HH:MM" on today's
/// local date, "tomorrow at HH:MM" on the next, otherwise "on {day} {Month}" (the day without a
/// leading zero, the English month name).
pub fn next_review_notice(due_ms: i64, now_ms: i64, utc_offset_minutes: i32) -> String {
    let due: LocalDate = local_date_at(due_ms, utc_offset_minutes);
    let today = local_date_at(now_ms, utc_offset_minutes);
    let local_ms = due_ms.saturating_add(i64::from(utc_offset_minutes) * 60_000);
    let minutes = local_ms.rem_euclid(DAY_MS) / 60_000;
    let clock = format!("{:02}:{:02}", minutes / 60, minutes % 60);
    let when = if due == today {
        format!("today at {clock}")
    } else if due.days() == today.days() + 1 {
        format!("tomorrow at {clock}")
    } else {
        format!(
            "on {} {}",
            due.day,
            MONTHS[(due.month as usize).clamp(1, 12) - 1]
        )
    };
    format!("Nothing to review. Next review: {when}.")
}

/// Result of `app_start`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    /// A course is active and started: the app opens on its path.
    pub onboarded: bool,
    pub active_course: Option<String>,
    /// A notice to show as a banner, such as the settings notice.
    pub notice: Option<String>,
}

/// Result of `cache_clear`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheCleared {
    pub files_removed: u32,
}

/// One course as `course_list` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CourseSummary {
    pub lang: String,
    /// "Spanish", "French" or "German".
    pub name: String,
    pub started: bool,
    /// The settings' active course.
    pub active: bool,
    pub placement_done: bool,
    pub cefr_estimate: Option<String>,
    /// The number of skills in the bundled syllabus.
    pub skills: usize,
}

/// Whether a course pack is published, available to download or installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackState {
    NotPublished,
    Available,
    Installed,
}

/// Result of `pack_status` and `pack_download`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackStatus {
    pub lang: String,
    pub state: PackState,
    pub version: Option<String>,
    pub bytes: Option<u64>,
    /// `megabytes(bytes)`.
    pub size: Option<String>,
}

/// The payload of the `pack://progress` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackProgress {
    pub lang: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

/// Result of `path_get`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathView {
    pub lang: String,
    pub units: Vec<PathUnit>,
    /// The skill Continue opens: the first available or learning skill in syllabus order.
    pub next_skill_id: Option<String>,
    /// Cards due now.
    pub due: usize,
    pub streak: u32,
    pub freezes: u32,
    pub today_xp: u32,
    pub goal_xp: u32,
}

/// A unit of the path, numbered from 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathUnit {
    pub id: String,
    pub n: usize,
    pub title: String,
    pub cefr: Cefr,
    pub skills: Vec<PathSkill>,
    /// The unit's Story node while `ai.stories` is on (module 07).
    pub story: Option<PathStory>,
}

/// A unit's Story node: the skill whose story it is (the unit's last), and whether that skill is
/// practised, so the story can be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathStory {
    pub skill_id: String,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathSkill {
    pub id: String,
    pub title: String,
    pub can_do: String,
    pub cefr: Cefr,
    pub state: SkillState,
    pub mastery: f64,
    pub tested_out: bool,
}

/// Result of `skill_get`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillDetail {
    pub id: String,
    pub title: String,
    pub can_do: String,
    pub cefr: Cefr,
    pub grammar_note: Option<String>,
    pub unit_title: String,
    pub state: SkillState,
    pub mastery: f64,
    /// The skill's vocabulary items, in syllabus order.
    pub words: Vec<Word>,
    /// For a locked skill, the title of the skill that opens it.
    pub opened_by: Option<String>,
}

/// A word a skill teaches: its target (with article for a noun) and its first English gloss.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Word {
    pub target: String,
    pub en: String,
}

/// A placement question: a `translate_to_en` or `translate_to_target` exercise of an item of the
/// skill being tested.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementQuestion {
    pub session_id: String,
    /// The question's number, from 1.
    pub n: usize,
    pub skill_id: String,
    pub exercise: Exercise,
}

/// Result of `placement_answer`: the next question, or the outcome once the test ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementStep {
    pub next: Option<PlacementQuestion>,
    pub result: Option<PlacementOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementOutcome {
    /// The skill index the learner starts at.
    pub estimate: usize,
    /// "Pre-A1" or "around A1", "around A2".
    pub label: String,
    /// How many skills were tested out.
    pub tested_out: usize,
    pub start_skill_id: String,
    /// The unit of the start skill, numbered from 1.
    pub start_unit: usize,
    pub start_unit_title: String,
}

/// Result of `lesson_start` and `review_start`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LessonStarted {
    /// Empty when a review starts no session (nothing due).
    pub session_id: String,
    /// "lesson", "review" or "story".
    pub kind: String,
    pub skill_id: Option<String>,
    pub origin: Origin,
    pub exercises: Vec<Exercise>,
    pub attributions: Vec<Attribution>,
    pub notice: Option<String>,
    /// Why a fresh lesson could not be written (a 402 with its link, a disconnected publik API,
    /// a busy provider); the lesson itself came from the course pack (module 07).
    pub ai_error: Option<AppError>,
    /// The story of a story session (module 07); its questions are answered as `q1`, `q2`, ….
    pub story: Option<Story>,
}

/// The author of a pack sentence an exercise shows; empty for a sentence without one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Attribution {
    pub exercise_id: String,
    pub author: String,
}

/// Result of `stats_get`. Streak, freezes, days and weekly XP cover every course; the rest is
/// the language's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub lang: String,
    /// The language has no finished session.
    pub empty: bool,
    pub streak: u32,
    pub freezes: u32,
    /// The 30 local dates ending today, oldest first.
    pub days: Vec<StatsDay>,
    pub week_xp: u32,
    pub record_week_xp: u32,
    pub words_known: u32,
    pub vocab_items: u32,
    pub skills: Vec<SkillProgress>,
    pub cefr_estimate: Option<String>,
    pub recall_rate: Option<f64>,
    pub reviews_30: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatsDay {
    pub local_date: LocalDate,
    pub xp: u32,
    pub active: bool,
    pub frozen: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillProgress {
    pub id: String,
    pub title: String,
    pub state: SkillState,
    pub mastery: f64,
}

/// Result of `backup_export`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportSummary {
    pub bytes: u64,
    pub courses: usize,
    pub cards: usize,
    pub reviews: usize,
}

/// Result of `backup_import`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    pub courses: usize,
    pub items: usize,
    pub cards: usize,
    pub reviews: usize,
    pub skipped_cards: usize,
}

/// One voice of `voices_list`: an installed OS voice (`source` "os") or the natural voice the
/// settings name (`source` "natural").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Voice {
    pub id: String,
    pub name: String,
    pub source: String,
    pub installed: bool,
}

/// Result of `tts_play`: what spoke, whether it came from the audio cache, the notice of a
/// fallback to the OS voice, and the AI error to show as a banner (a 402).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TtsPlayed {
    pub source: VoiceSource,
    pub cached: bool,
    pub notice: Option<String>,
    pub ai_error: Option<AppError>,
}

/// Result of `tts_prefetch`: whether a sentence was fetched and cached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prefetched {
    pub prefetched: bool,
}

/// Result of `audio_status`: the measured cost line, and until when listening exercises are off
/// (ms since the epoch), while they are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioStatus {
    pub cost_line: String,
    pub listen_off_until: Option<i64>,
}

/// Result of `mic_stop`: the take's id (a version-4 UUID) and its length after trimming.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recording {
    pub id: String,
    pub duration_ms: u64,
}

/// Which transcriber answered a speaking check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transcriber {
    Provider,
    Local,
}

/// Result of `speak_check`: the words, extra words, score and pass of the take, what was heard,
/// the grade the result sheet shows (the first scored take's when this one rated nothing), and
/// how the take was checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeakResult {
    #[serde(flatten)]
    pub scored: SpeakScore,
    /// The transcript as `heard_text` gives it.
    pub heard: String,
    pub grade: Grade,
    /// This take reviewed the card (the first scored take of the exercise).
    pub rated: bool,
    pub transcriber: Transcriber,
    /// The transcript was held from an earlier check of the same take: no request was made.
    pub cached: bool,
    pub notice: Option<String>,
}

/// One local Whisper model of `whisper_models`: its size (`megabytes(bytes)`), whether it is on
/// this computer, and whether `speaking.whisper_model` names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub bytes: u64,
    pub size: String,
    pub downloaded: bool,
    pub chosen: bool,
}

/// The payload of the `model://progress` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProgress {
    pub name: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

/// Result of `speaking_status`: the measured cost line, until when speaking exercises are off
/// (ms since the epoch) while they are, whether local Whisper is in this build, and whether a
/// microphone exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeakingStatus {
    pub cost_line: String,
    pub speak_off_until: Option<i64>,
    pub local_available: bool,
    pub microphone: bool,
}

/// The payload of the `mic://level` event: the level of 50 ms of input, −60 to 0 dBFS.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MicLevel {
    pub dbfs: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_decimal_megabytes_to_one_place() {
        assert_eq!(megabytes(8_200_000), "8.2 MB");
        assert_eq!(megabytes(8_249_999), "8.2 MB");
        assert_eq!(megabytes(8_250_000), "8.3 MB");
        assert_eq!(megabytes(0), "0.0 MB");
        assert_eq!(megabytes(49_999), "0.0 MB");
        assert_eq!(megabytes(50_000), "0.1 MB");
        assert_eq!(megabytes(12_345_678_901), "12345.7 MB");
    }

    #[test]
    fn languages_have_english_names() {
        let names: Vec<&str> = Lang::ALL.iter().map(|l| language_name(*l)).collect();
        assert_eq!(names, ["Spanish", "French", "German"]);
    }

    #[test]
    fn the_next_review_reads_today_tomorrow_or_a_date() {
        // 2026-10-12 is day 20,738; 08:30 there in UTC.
        let day = 20_738 * DAY_MS;
        let now = day + (8 * 60 + 30) * 60_000;
        let at = |days: i64, h: i64, m: i64| day + days * DAY_MS + (h * 60 + m) * 60_000;
        assert_eq!(
            next_review_notice(at(0, 9, 0), now, 0),
            "Nothing to review. Next review: today at 09:00."
        );
        assert_eq!(
            next_review_notice(at(1, 9, 0), now, 0),
            "Nothing to review. Next review: tomorrow at 09:00."
        );
        assert_eq!(
            next_review_notice(at(2, 9, 0), now, 0),
            "Nothing to review. Next review: on 14 October."
        );
        assert_eq!(
            next_review_notice(at(20, 23, 59), now, 0),
            "Nothing to review. Next review: on 1 November."
        );
        // 23:30 UTC on the 12th is 01:30 on the 13th two hours east: tomorrow there.
        assert_eq!(
            next_review_notice(at(0, 23, 30), now, 120),
            "Nothing to review. Next review: tomorrow at 01:30."
        );
        // Ten hours west, now is 22:30 on the 11th and 07:00 UTC on the 13th is 21:00 on the 12th.
        assert_eq!(
            next_review_notice(at(1, 7, 0), now, -600),
            "Nothing to review. Next review: tomorrow at 21:00."
        );
    }

    #[test]
    fn result_types_serialise_with_their_field_names() {
        let status = PackStatus {
            lang: "es".into(),
            state: PackState::NotPublished,
            version: None,
            bytes: None,
            size: None,
        };
        assert_eq!(
            serde_json::to_string(&status).unwrap(),
            r#"{"lang":"es","state":"not_published","version":null,"bytes":null,"size":null}"#
        );
        assert_eq!(COMMANDS.len(), 53);
        assert_eq!(COMMANDS[0], "app_start");
        assert_eq!(COMMANDS[22], "backup_import");
        assert_eq!(COMMANDS[23], "ai_status");
        assert_eq!(COMMANDS[33], "ai_open_link");
        assert_eq!(COMMANDS[34], "lesson_use_course_pack");
        assert_eq!(COMMANDS[37], "ai_lesson_checks");
        assert_eq!(&COMMANDS[38..40], ["exercise_check", "explain_answer"]);
        assert_eq!(
            &COMMANDS[40..46],
            [
                "voices_list",
                "tts_play",
                "tts_stop",
                "tts_prefetch",
                "audio_status",
                "session_skip"
            ]
        );
        assert_eq!(
            &COMMANDS[46..],
            [
                "mic_start",
                "mic_stop",
                "recording_play",
                "speak_check",
                "whisper_models",
                "whisper_download",
                "speaking_status"
            ]
        );
        assert_eq!(
            (MIC_LEVEL_EVENT, MIC_LIMIT_EVENT, MODEL_PROGRESS_EVENT),
            ("mic://level", "mic://limit", "model://progress")
        );
        let played = TtsPlayed {
            source: VoiceSource::Os,
            cached: false,
            notice: None,
            ai_error: None,
        };
        assert_eq!(
            serde_json::to_string(&played).unwrap(),
            r#"{"source":"os","cached":false,"notice":null,"ai_error":null}"#
        );
        assert_eq!(AUDIO_ENDED_EVENT, "audio://ended");
    }
}
