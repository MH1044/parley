//! `parley streak simulate`: the streak, freezes and weekly records a list of sessions gives
//! (docs/modules/03-exercise-engine-and-grading.md, Interfaces).

use std::io::Write;
use std::path::Path;

use parley_core::env::{LocalDate, local_date_at};
use parley_core::game::{DayRow, FreezeEvent, WEEKLY_RECORD, streak, weekly_record};
use serde::{Deserialize, Serialize};

use super::{failure, missing, not_found, parse, print_json, shown, unknown, usage};

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match args.first().map(String::as_str) {
        Some("simulate") => simulate_command(&args[1..], out, err),
        Some(other) => unknown(err, other),
        None => missing(err, "streak simulate"),
    }
}

fn default_goal() -> u32 {
    20
}

/// A days file: the day to compute the streak for, the learner's UTC offset, the daily goal and
/// the sessions in the order they happened.
#[derive(Deserialize)]
struct DaysFile {
    today: LocalDate,
    #[serde(default)]
    utc_offset_minutes: i32,
    #[serde(default = "default_goal")]
    goal_xp: u32,
    sessions: Vec<SessionIn>,
}

#[derive(Deserialize)]
struct SessionIn {
    /// Milliseconds since the epoch.
    started_at: i64,
    xp: u32,
}

#[derive(Serialize)]
struct DayOut {
    local_date: LocalDate,
    xp: u32,
    sessions: u32,
}

#[derive(Serialize)]
struct RecordOut {
    local_date: LocalDate,
    message: &'static str,
}

/// The output, in the order the spec lists its fields.
#[derive(Serialize)]
struct Simulated {
    length: u32,
    freezes: u32,
    active_today: bool,
    frozen: Vec<LocalDate>,
    events: Vec<FreezeEvent>,
    days: Vec<DayOut>,
    records: Vec<RecordOut>,
}

/// Each session counts on the local date of its start; its weekly record is decided against the
/// days before it, then it is added to its day. The streak is computed over the days at the end.
fn simulate(file: &DaysFile) -> Simulated {
    let mut rows: Vec<DayRow> = Vec::new();
    let mut records = Vec::new();
    for session in &file.sessions {
        let date = local_date_at(session.started_at, file.utc_offset_minutes);
        if weekly_record(&rows, date, session.xp) {
            records.push(RecordOut {
                local_date: date,
                message: WEEKLY_RECORD,
            });
        }
        match rows.binary_search_by(|row| row.local_date.cmp(&date)) {
            Ok(at) => {
                rows[at].xp += session.xp;
                rows[at].sessions += 1;
            }
            Err(at) => rows.insert(
                at,
                DayRow {
                    local_date: date,
                    xp: session.xp,
                    sessions: 1,
                    goal_xp: file.goal_xp,
                    frozen: false,
                },
            ),
        }
    }
    let state = streak(&rows, file.today);
    Simulated {
        length: state.length,
        freezes: state.freezes,
        active_today: state.active_today,
        frozen: state.frozen,
        events: state.events,
        days: rows
            .iter()
            .map(|row| DayOut {
                local_date: row.local_date,
                xp: row.xp,
                sessions: row.sessions,
            })
            .collect(),
        records,
    }
}

fn simulate_command(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse(args, &[], &["--json"]) {
        Ok(parsed) => parsed,
        Err(message) => return usage(err, &message),
    };
    if let Some(extra) = parsed.positional.get(1) {
        return unknown(err, extra);
    }
    let Some(given) = parsed.positional.first() else {
        return missing(err, "<days.json>");
    };
    let file = shown(given);
    let path = Path::new(given);
    if !path.is_file() {
        return not_found(err, &file);
    }
    let text = match std::fs::read(path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => return failure(err, &format!("{file}: not valid UTF-8")),
        },
        Err(e) => return failure(err, &format!("{file}: {e}")),
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let days: DaysFile = match serde_json::from_str(text) {
        Ok(days) => days,
        Err(e) => return failure(err, &format!("{file}: {e}")),
    };
    print_json(out, &simulate(&days));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY_MS: i64 = 86_400_000;

    fn run_capture(args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(&args, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    /// Runs the command over `json` written to a temporary file.
    fn simulate_json(json: &serde_json::Value) -> serde_json::Value {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("days.json");
        std::fs::write(&path, json.to_string()).unwrap();
        let path = path.to_string_lossy().into_owned();
        let (code, out, err) = run_capture(&["simulate", &path, "--json"]);
        assert_eq!((code, err.as_str()), (0, ""));
        serde_json::from_str(&out).unwrap()
    }

    /// Midday UTC on 2026-01-05 (a Monday) plus `n − 1` days.
    fn day_ms(n: i64) -> i64 {
        LocalDate::parse("2026-01-05").unwrap().days() * DAY_MS + (n - 1) * DAY_MS + DAY_MS / 2
    }

    #[test]
    fn a_week_of_lessons_earns_a_freeze() {
        let sessions: Vec<serde_json::Value> = (1..=7)
            .map(|n| serde_json::json!({"started_at": day_ms(n), "xp": 15}))
            .collect();
        let value =
            simulate_json(&serde_json::json!({"today": "2026-01-11", "sessions": sessions}));
        assert_eq!(value["length"], 7);
        assert_eq!(value["freezes"], 1);
        assert_eq!(value["active_today"], true);
        assert_eq!(
            value["events"],
            serde_json::json!([{"local_date": "2026-01-11", "kind": "earned", "streak_len": 7}])
        );
        assert_eq!(
            value["days"][0],
            serde_json::json!({"local_date": "2026-01-05", "xp": 15, "sessions": 1})
        );
        assert_eq!(value["records"], serde_json::json!([]));
    }

    #[test]
    fn late_evening_and_after_midnight_are_two_days() {
        // 22:30 and 23:10 UTC on 2026-01-05 are 23:30 and 00:10 at UTC+1.
        let evening = day_ms(1) + 10 * 3_600_000 + 30 * 60_000;
        let night = evening + 40 * 60_000;
        let value = simulate_json(&serde_json::json!({
            "today": "2026-01-06", "utc_offset_minutes": 60,
            "sessions": [{"started_at": evening, "xp": 10}, {"started_at": night, "xp": 10}]
        }));
        let days: Vec<&str> = value["days"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["local_date"].as_str().unwrap())
            .collect();
        assert_eq!(days, ["2026-01-05", "2026-01-06"]);
        assert_eq!(value["length"], 2);
    }

    #[test]
    fn a_record_week_is_reported_once() {
        let value = simulate_json(&serde_json::json!({
            "today": "2026-01-21",
            "sessions": [
                {"started_at": day_ms(1), "xp": 100},
                {"started_at": day_ms(8), "xp": 80},
                {"started_at": day_ms(15), "xp": 60},
                {"started_at": day_ms(16), "xp": 41},
                {"started_at": day_ms(17), "xp": 30}
            ]
        }));
        assert_eq!(
            value["records"],
            serde_json::json!([{"local_date": "2026-01-20", "message": "New weekly record!"}])
        );
    }

    #[test]
    fn usage_and_file_errors() {
        let (code, out, err) = run_capture(&["simulate"]);
        assert_eq!((code, out.as_str()), (2, ""));
        assert_eq!(err, "error: <days.json> is required. Run parley --help.\n");
        let (code, out, err) = run_capture(&["simulate", "no/such/days.json"]);
        assert_eq!((code, out.as_str()), (2, ""));
        assert_eq!(err, "error: no/such/days.json not found\n");
        let (code, _, err) = run_capture(&[]);
        assert_eq!(code, 2);
        assert_eq!(
            err,
            "error: streak simulate is required. Run parley --help.\n"
        );

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.json");
        std::fs::write(&path, r#"{"schema":"parley.review-log/1","entries":[]}"#).unwrap();
        let path = path.to_string_lossy().into_owned();
        let (code, out, err) = run_capture(&["simulate", &path]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(
            err.starts_with(&format!("error: {}: ", shown(&path))),
            "{err}"
        );
        assert_eq!(err.lines().count(), 1);
        std::fs::write(
            dir.path().join("log.json"),
            r#"{"today":"2026-02-30","sessions":[]}"#,
        )
        .unwrap();
        let (code, _, err) = run_capture(&["simulate", &path]);
        assert_eq!(code, 1);
        assert!(err.contains("2026-02-30"), "{err}");
    }
}
