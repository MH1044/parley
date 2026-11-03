//! `parley srs replay`: the cards a review log gives (docs/modules/02-srs-and-learner-model.md).

use std::io::Write;
use std::path::Path;

use parley_core::srs::{Card, ReviewLog, SrsParams, replay};
use serde::Serialize;

use super::{failure, missing, not_found, parse, print_json, shown, unknown, usage};

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match args.first().map(String::as_str) {
        Some("replay") => replay_command(&args[1..], out, err),
        Some(other) => unknown(err, other),
        None => missing(err, "srs replay"),
    }
}

/// The output of `srs replay`.
#[derive(Serialize)]
struct Replayed {
    cards: Vec<Card>,
}

/// `--retention` as a number from 0.80 to 0.95; anything else is the usage error.
fn parse_retention(text: &str) -> Option<f64> {
    let r: f64 = text.parse().ok()?;
    (SrsParams::MIN_RETENTION..=SrsParams::MAX_RETENTION)
        .contains(&r)
        .then_some(r)
}

fn replay_command(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse(args, &["--retention"], &["--json"]) {
        Ok(parsed) => parsed,
        Err(message) => return usage(err, &message),
    };
    if let Some(extra) = parsed.positional.get(1) {
        return unknown(err, extra);
    }
    let Some(given) = parsed.positional.first() else {
        return missing(err, "<log.json>");
    };
    let retention = match parsed.value("--retention") {
        Some(text) => match parse_retention(text) {
            Some(r) => r,
            None => {
                return usage(
                    err,
                    "--retention must be a number from 0.80 to 0.95. Run parley --help.",
                );
            }
        },
        None => SrsParams::DEFAULT_RETENTION,
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
    let log = match ReviewLog::parse(&text) {
        Ok(log) => log,
        Err(message) => return failure(err, &format!("{file}: {message}")),
    };
    let cards = replay(&log.entries, &SrsParams::with_retention(retention));
    print_json(out, &Replayed { cards });
    0
}

#[cfg(test)]
mod tests {
    use super::*;

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

    const LOG: &str = r#"{"schema":"parley.review-log/1","entries":[
        {"item_id":"es.a1.u01.s01#l:hola","facet":"receptive","reviewed_at":0,"rating":3,
         "exercise_type":"pick_meaning","verdict":"correct","session_id":"s1"}]}"#;

    #[test]
    fn retention_must_be_in_range() {
        assert_eq!(parse_retention("0.80"), Some(0.8));
        assert_eq!(parse_retention("0.95"), Some(0.95));
        assert_eq!(parse_retention("0.79"), None);
        assert_eq!(parse_retention("0.951"), None);
        assert_eq!(parse_retention("NaN"), None);
        assert_eq!(parse_retention("high"), None);
    }

    #[test]
    fn a_log_replays_to_its_cards() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.json");
        std::fs::write(&path, LOG).unwrap();
        let path = path.to_string_lossy().into_owned();
        let (code, out, err) = run_capture(&["replay", &path, "--json"]);
        assert_eq!((code, err.as_str()), (0, ""));
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        let card = &value["cards"][0];
        assert_eq!(card["state"], "review");
        assert_eq!(card["due"], 2 * 86_400_000_i64);
        let (_, low, _) = run_capture(&["replay", &path, "--retention", "0.8"]);
        let low: serde_json::Value = serde_json::from_str(&low).unwrap();
        assert_eq!(low["cards"][0]["stability"], card["stability"]);
        assert!(low["cards"][0]["due"].as_i64() > card["due"].as_i64());
    }

    #[test]
    fn usage_errors_print_one_line() {
        let (code, out, err) = run_capture(&["replay"]);
        assert_eq!((code, out.as_str()), (2, ""));
        assert_eq!(err, "error: <log.json> is required. Run parley --help.\n");

        let (code, out, err) = run_capture(&["replay", "no/such/log.json"]);
        assert_eq!((code, out.as_str()), (2, ""));
        assert_eq!(err, "error: no/such/log.json not found\n");

        let (code, out, err) = run_capture(&["replay", "log.json", "--retention", "0.5"]);
        assert_eq!((code, out.as_str()), (2, ""));
        assert_eq!(
            err,
            "error: --retention must be a number from 0.80 to 0.95. Run parley --help.\n"
        );

        let (code, _, err) = run_capture(&[]);
        assert_eq!(code, 2);
        assert_eq!(err, "error: srs replay is required. Run parley --help.\n");
    }

    #[test]
    fn a_file_that_is_not_a_log_fails_with_the_parser_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cards.json");
        std::fs::write(&path, r#"{"schema":"parley.settings/1","entries":[]}"#).unwrap();
        let path = path.to_string_lossy().into_owned();
        let (code, out, err) = run_capture(&["replay", &path]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert_eq!(
            err,
            format!(
                "error: {}: schema must be parley.review-log/1\n",
                shown(&path)
            )
        );
        std::fs::write(dir.path().join("cards.json"), "[1, 2").unwrap();
        let (code, out, err) = run_capture(&["replay", &path]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(
            err.starts_with(&format!("error: {}: ", shown(&path))),
            "{err}"
        );
        assert_eq!(err.lines().count(), 1);
    }
}
