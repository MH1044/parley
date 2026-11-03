//! `parley placement simulate`: the placement test against a simulated learner
//! (docs/modules/02-srs-and-learner-model.md).

use std::io::Write;
use std::path::Path;

use parley_core::content::{Report, load_syllabus};
use parley_core::env::SplitMix64;
use parley_core::srs::{SkillRef, simulate};
use serde::Serialize;

use super::{missing, not_found, parse, print_json, shown, unknown, usage};

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match args.first().map(String::as_str) {
        Some("simulate") => simulate_command(&args[1..], out, err),
        Some(other) => unknown(err, other),
        None => missing(err, "placement simulate"),
    }
}

/// The output of `placement simulate`, in the order the spec lists its fields.
#[derive(Serialize)]
struct Simulated {
    skills: usize,
    questions: Vec<usize>,
    answers: Vec<bool>,
    reversals: Vec<usize>,
    estimate: usize,
    label: String,
    tested_out: Vec<String>,
}

/// `--slip` as a probability from 0 to 1.
fn parse_slip(text: &str) -> Option<f64> {
    let p: f64 = text.parse().ok()?;
    (0.0..=1.0).contains(&p).then_some(p)
}

fn simulate_command(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse(
        args,
        &["--syllabus", "--boundary", "--slip", "--seed"],
        &["--json"],
    ) {
        Ok(parsed) => parsed,
        Err(message) => return usage(err, &message),
    };
    if let Some(extra) = parsed.positional.first() {
        return unknown(err, extra);
    }
    let Some(syllabus) = parsed.value("--syllabus") else {
        return missing(err, "--syllabus");
    };
    let Some(boundary) = parsed.value("--boundary") else {
        return missing(err, "--boundary");
    };
    let Ok(boundary) = boundary.parse::<usize>() else {
        return usage(err, "--boundary must be a whole number. Run parley --help.");
    };
    let slip = match parsed.value("--slip") {
        Some(text) => match parse_slip(text) {
            Some(p) => p,
            None => {
                return usage(
                    err,
                    "--slip must be a number from 0 to 1. Run parley --help.",
                );
            }
        },
        None => 0.0,
    };
    let seed = match parsed.value("--seed") {
        Some(text) => match text.parse::<u64>() {
            Ok(seed) => seed,
            Err(_) => return usage(err, "--seed must be a whole number. Run parley --help."),
        },
        None => 0,
    };
    let path = Path::new(syllabus);
    if !path.is_file() {
        return not_found(err, &shown(syllabus));
    }
    let syllabus = match load_syllabus(path) {
        Ok(syllabus) => syllabus,
        Err(errors) => {
            let report = Report {
                errors: errors.into_iter().map(|e| e.0).collect(),
                warnings: Vec::new(),
            };
            print_json(out, &report);
            return 1;
        }
    };
    let skills: Vec<SkillRef> = syllabus
        .skills()
        .map(|(_, skill)| SkillRef {
            id: skill.id.clone(),
            cefr: skill.cefr,
        })
        .collect();
    let count = skills.len();
    let result = simulate(skills, boundary, slip, &mut SplitMix64::new(seed));
    print_json(
        out,
        &Simulated {
            skills: count,
            questions: result.questions,
            answers: result.answers,
            reversals: result.reversals,
            estimate: result.estimate,
            label: result.label,
            tested_out: result.tested_out,
        },
    );
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

    fn usage_line(args: &[&str]) -> String {
        let (code, out, err) = run_capture(args);
        assert_eq!((code, out.as_str()), (2, ""), "{args:?}");
        err
    }

    #[test]
    fn every_usage_error_has_its_line() {
        assert_eq!(
            usage_line(&["simulate", "--boundary", "3"]),
            "error: --syllabus is required. Run parley --help.\n"
        );
        assert_eq!(
            usage_line(&["simulate", "--syllabus", "s.yaml"]),
            "error: --boundary is required. Run parley --help.\n"
        );
        assert_eq!(
            usage_line(&["simulate", "--syllabus", "s.yaml", "--boundary", "2.5"]),
            "error: --boundary must be a whole number. Run parley --help.\n"
        );
        assert_eq!(
            usage_line(&["simulate", "--syllabus", "s.yaml", "--boundary", "-1"]),
            "error: --boundary must be a whole number. Run parley --help.\n"
        );
        assert_eq!(
            usage_line(&[
                "simulate",
                "--syllabus",
                "s.yaml",
                "--boundary",
                "3",
                "--slip",
                "1.5"
            ]),
            "error: --slip must be a number from 0 to 1. Run parley --help.\n"
        );
        assert_eq!(
            usage_line(&[
                "simulate",
                "--syllabus",
                "s.yaml",
                "--boundary",
                "3",
                "--seed",
                "x"
            ]),
            "error: --seed must be a whole number. Run parley --help.\n"
        );
        assert_eq!(
            usage_line(&["simulate", "--syllabus", "no/such.yaml", "--boundary", "3"]),
            "error: no/such.yaml not found\n"
        );
        assert_eq!(
            usage_line(&[]),
            "error: placement simulate is required. Run parley --help.\n"
        );
    }

    #[test]
    fn slip_takes_zero_to_one() {
        assert_eq!(parse_slip("0"), Some(0.0));
        assert_eq!(parse_slip("1"), Some(1.0));
        assert_eq!(parse_slip("0.1"), Some(0.1));
        assert_eq!(parse_slip("-0.1"), None);
        assert_eq!(parse_slip("NaN"), None);
    }

    #[test]
    fn a_syllabus_that_does_not_load_prints_the_report() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("syllabus.yaml");
        std::fs::write(&path, "units: [\n").unwrap();
        let path = path.to_string_lossy().into_owned();
        let (code, out, err) = run_capture(&["simulate", "--syllabus", &path, "--boundary", "1"]);
        assert_eq!((code, err.as_str()), (1, ""));
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["errors"].as_array().unwrap().len(), 1);
        assert_eq!(value["warnings"], serde_json::json!([]));
    }
}
