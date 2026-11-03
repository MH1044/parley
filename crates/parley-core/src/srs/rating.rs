//! From a graded outcome to an FSRS rating (docs/modules/02-srs-and-learner-model.md, Design
//! notes: the rating table).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::fsrs::Rating;

/// What grading decided about an answer. `almost` covers an accent, a typo, an article or an AI
/// judge's "almost".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Correct,
    Almost,
    Wrong,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Correct => "correct",
            Verdict::Almost => "almost",
            Verdict::Wrong => "wrong",
        }
    }
}

impl FromStr for Verdict {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "correct" => Ok(Verdict::Correct),
            "almost" => Ok(Verdict::Almost),
            "wrong" => Ok(Verdict::Wrong),
            other => Err(format!("unknown verdict {other}")),
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One graded answer, as the exercise engine reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub verdict: Verdict,
    pub used_hint: bool,
    /// The exercise type, such as `pick_meaning`, `match_pairs` or `translate`.
    pub exercise_type: String,
    /// How long the learner took to answer, when it was measured.
    pub response_ms: Option<i64>,
    /// The learner pressed "I was right".
    pub learner_override: bool,
}

/// The fastest answer, in milliseconds, that a recognition exercise rates Easy.
const EASY_WITHIN_MS: i64 = 2_500;

/// The exercise types fast enough to recognise that a quick correct answer is rated Easy.
const EASY_TYPES: [&str; 2] = ["pick_meaning", "match_pairs"];

/// The rating of an outcome; the first matching row wins: an override is Good; `wrong` is Again;
/// `almost` is Hard; correct with a hint is Hard; correct, no hint, `pick_meaning` or
/// `match_pairs` within 2,500 ms is Easy; any other correct answer is Good. A missing response
/// time never gives Easy.
pub fn rating_for(outcome: &Outcome) -> Rating {
    if outcome.learner_override {
        return Rating::Good;
    }
    match outcome.verdict {
        Verdict::Wrong => Rating::Again,
        Verdict::Almost => Rating::Hard,
        Verdict::Correct if outcome.used_hint => Rating::Hard,
        Verdict::Correct => {
            let quick = outcome.response_ms.is_some_and(|ms| ms <= EASY_WITHIN_MS);
            if quick && EASY_TYPES.contains(&outcome.exercise_type.as_str()) {
                Rating::Easy
            } else {
                Rating::Good
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(verdict: Verdict, hint: bool, kind: &str, ms: Option<i64>, over: bool) -> Outcome {
        Outcome {
            verdict,
            used_hint: hint,
            exercise_type: kind.into(),
            response_ms: ms,
            learner_override: over,
        }
    }

    #[test]
    fn each_row_of_the_table_gives_its_rating() {
        use Verdict::*;
        let cases = [
            (outcome(Wrong, true, "translate", None, true), Rating::Good),
            (
                outcome(Wrong, false, "pick_meaning", Some(900), false),
                Rating::Again,
            ),
            (
                outcome(Almost, false, "pick_meaning", Some(900), false),
                Rating::Hard,
            ),
            (
                outcome(Correct, true, "pick_meaning", Some(900), false),
                Rating::Hard,
            ),
            (
                outcome(Correct, false, "pick_meaning", Some(2_500), false),
                Rating::Easy,
            ),
            (
                outcome(Correct, false, "match_pairs", Some(0), false),
                Rating::Easy,
            ),
            (
                outcome(Correct, false, "pick_meaning", Some(2_501), false),
                Rating::Good,
            ),
            (
                outcome(Correct, false, "pick_meaning", None, false),
                Rating::Good,
            ),
            (
                outcome(Correct, false, "translate", Some(500), false),
                Rating::Good,
            ),
        ];
        for (case, expected) in cases {
            assert_eq!(rating_for(&case), expected, "{case:?}");
        }
    }

    #[test]
    fn verdicts_round_trip_through_text() {
        for verdict in [Verdict::Correct, Verdict::Almost, Verdict::Wrong] {
            assert_eq!(verdict.as_str().parse::<Verdict>(), Ok(verdict));
            assert_eq!(
                serde_json::to_string(&verdict).unwrap(),
                format!("\"{verdict}\"")
            );
        }
        assert!("maybe".parse::<Verdict>().is_err());
    }
}
