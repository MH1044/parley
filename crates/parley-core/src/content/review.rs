//! The review status of a course's units (`content/<lang>/review.yaml`, docs/modules/
//! 05-course-content-a1-a2.md): every unit is `unreviewed` or `reviewed` by a native speaker,
//! with the day the status was last set and no reviewer names. Units still unreviewed ship as a
//! labelled beta.
//!
//! Rules, in this order: **RV01** YAML or schema (`{file}: {pointer}: {message}`, worded as V01),
//! which stops the others; **RV02** `{file}: unit {unit} is listed twice`; **RV03**
//! `{file}: unit {unit} is not in the syllabus`; **RV04** `{file}: unit {unit} is missing`, in
//! syllabus order. RV02 and RV03 name each unit once, where it is first listed or repeated.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::syllabus::Syllabus;
use super::validate::Report;
use super::{ContentError, Lang, load_yaml, parse_yaml, review_validator};

/// Whether a native speaker has reviewed a unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewStatus {
    Unreviewed,
    Reviewed,
}

/// One unit's status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitReview {
    pub unit: String,
    pub status: ReviewStatus,
    /// `YYYY-MM-DD`: the day the status was last set.
    pub date: String,
}

/// A language's `review.yaml`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFile {
    pub schema: String,
    pub lang: Lang,
    pub units: Vec<UnitReview>,
}

impl ReviewFile {
    /// The units of `s` this file marks `unreviewed`, in syllabus order.
    pub fn unreviewed_units(&self, s: &Syllabus) -> Vec<String> {
        let unreviewed: HashSet<&str> = self
            .units
            .iter()
            .filter(|unit| unit.status == ReviewStatus::Unreviewed)
            .map(|unit| unit.unit.as_str())
            .collect();
        s.units
            .iter()
            .filter(|unit| unreviewed.contains(unit.id.as_str()))
            .map(|unit| unit.id.clone())
            .collect()
    }
}

/// Parses review text named `file` (the path as typed): RV01 messages only.
pub fn parse_review(file: &str, text: &str) -> Result<ReviewFile, Vec<ContentError>> {
    parse_yaml(file, text, review_validator())
}

/// Reads and parses a review file: RV01 messages only.
pub fn load_review(path: &Path) -> Result<ReviewFile, Vec<ContentError>> {
    load_yaml(path, review_validator())
}

/// Runs RV02 to RV04 over a review file that parsed, against its language's syllabus. Without a
/// syllabus (it does not load) only RV02 runs: RV03 and RV04 need to know the units.
pub fn check_review(file: &str, review: &ReviewFile, s: Option<&Syllabus>) -> Report {
    let mut errors = Vec::new();

    // RV02: once per unit, where it is listed the second time.
    let mut seen: HashSet<&str> = HashSet::new();
    let mut repeated: HashSet<&str> = HashSet::new();
    for unit in &review.units {
        if !seen.insert(&unit.unit) && repeated.insert(&unit.unit) {
            errors.push(format!("{file}: unit {} is listed twice", unit.unit));
        }
    }

    let Some(s) = s else {
        return Report {
            errors,
            warnings: Vec::new(),
        };
    };
    let in_syllabus: HashSet<&str> = s.units.iter().map(|unit| unit.id.as_str()).collect();
    let listed: HashSet<&str> = review.units.iter().map(|unit| unit.unit.as_str()).collect();

    // RV03: once per unit, where it is first listed.
    let mut named: HashSet<&str> = HashSet::new();
    for unit in &review.units {
        if !in_syllabus.contains(unit.unit.as_str()) && named.insert(&unit.unit) {
            errors.push(format!("{file}: unit {} is not in the syllabus", unit.unit));
        }
    }

    // RV04
    for unit in &s.units {
        if !listed.contains(unit.id.as_str()) {
            errors.push(format!("{file}: unit {} is missing", unit.id));
        }
    }

    Report {
        errors,
        warnings: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::parse_syllabus;
    use crate::content::test_support::EXAMPLE_SYLLABUS;

    /// The example syllabus with a second unit, so a file can miss one.
    fn syllabus() -> Syllabus {
        let mut s = parse_syllabus("s.yaml", EXAMPLE_SYLLABUS).unwrap();
        let mut second = s.units[0].clone();
        second.id = "es.a1.u02".into();
        s.units.push(second);
        s
    }

    const REVIEW: &str = "\
schema: parley.review/1
lang: es
units:
  - {unit: es.a1.u01, status: reviewed, date: \"2026-11-05\"}
  - {unit: es.a1.u02, status: unreviewed, date: \"2026-11-04\"}
";

    fn check(text: &str) -> Report {
        let review = parse_review("r.yaml", text).unwrap();
        check_review("r.yaml", &review, Some(&syllabus()))
    }

    #[test]
    fn a_complete_file_passes_and_names_its_unreviewed_units() {
        assert_eq!(check(REVIEW), Report::default());
        let review = parse_review("r.yaml", REVIEW).unwrap();
        assert_eq!(review.unreviewed_units(&syllabus()), ["es.a1.u02"]);
        assert_eq!(review.units[0].date, "2026-11-05");
    }

    #[test]
    fn an_unquoted_date_is_still_text() {
        let text = REVIEW.replace("\"2026-11-05\"", "2026-11-05");
        let review = parse_review("r.yaml", &text).unwrap();
        assert_eq!(review.units[0].date, "2026-11-05");
    }

    #[test]
    fn rv01_uses_the_v01_wording_for_the_schema() {
        let text = REVIEW.replace("status: reviewed", "status: done");
        assert_eq!(
            parse_review("r.yaml", &text).unwrap_err(),
            [ContentError(
                "r.yaml: /units/0/status: must be one of unreviewed, reviewed".into()
            )]
        );
        let text = REVIEW.replace(", date: \"2026-11-04\"", ", reviewer: someone");
        assert_eq!(
            parse_review("r.yaml", &text)
                .unwrap_err()
                .into_iter()
                .map(|e| e.0)
                .collect::<Vec<_>>(),
            [
                "r.yaml: /units/1: date is required",
                "r.yaml: /units/1: unknown field reviewer"
            ]
        );
        let text = REVIEW.replace("\"2026-11-05\"", "\"5 November\"");
        let errors = parse_review("r.yaml", &text).unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].0.starts_with("r.yaml: /units/0/date: "),
            "{errors:?}"
        );
    }

    #[test]
    fn rv02_names_a_unit_listed_twice_once() {
        let text = format!(
            "{REVIEW}  - {{unit: es.a1.u01, status: unreviewed, date: \"2026-11-06\"}}\n  - {{unit: es.a1.u01, status: reviewed, date: \"2026-11-07\"}}\n"
        );
        assert_eq!(
            check(&text).errors,
            ["r.yaml: unit es.a1.u01 is listed twice"]
        );
    }

    #[test]
    fn rv03_names_a_unit_the_syllabus_lacks() {
        let text =
            format!("{REVIEW}  - {{unit: es.a2.u01, status: unreviewed, date: \"2026-11-06\"}}\n");
        assert_eq!(
            check(&text).errors,
            ["r.yaml: unit es.a2.u01 is not in the syllabus"]
        );
    }

    #[test]
    fn without_a_syllabus_only_repeats_are_reported() {
        let text = format!(
            "{REVIEW}  - {{unit: es.a1.u01, status: unreviewed, date: \"2026-11-06\"}}\n  - {{unit: es.a2.u09, status: unreviewed, date: \"2026-11-06\"}}\n"
        );
        let review = parse_review("r.yaml", &text).unwrap();
        assert_eq!(
            check_review("r.yaml", &review, None).errors,
            ["r.yaml: unit es.a1.u01 is listed twice"]
        );
    }

    #[test]
    fn rv04_names_missing_units_in_syllabus_order() {
        let text = "schema: parley.review/1\nlang: es\nunits: []\n";
        assert_eq!(
            check(text).errors,
            [
                "r.yaml: unit es.a1.u01 is missing",
                "r.yaml: unit es.a1.u02 is missing"
            ]
        );
    }
}
