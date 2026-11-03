//! The placement test: an adaptive up-down staircase over the course's skills (Levitt 1971,
//! "Transformed up-down methods in psychoacoustics"), its estimate, CEFR label and tested-out
//! skills (docs/modules/02-srs-and-learner-model.md, Design notes).

use serde::{Deserialize, Serialize};

use crate::content::Cefr;
use crate::env::Rng;

/// A skill as placement sees it: its id and CEFR level, in syllabus order.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SkillRef {
    pub id: String,
    pub cefr: Cefr,
}

/// A finished placement test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementResult {
    /// The skill index of every question, in the order asked.
    pub questions: Vec<usize>,
    /// Whether each answer was correct.
    pub answers: Vec<bool>,
    /// The skill index of every reversal, in order.
    pub reversals: Vec<usize>,
    /// The skill index the learner starts at.
    pub estimate: usize,
    /// "Pre-A1", or "around " and the CEFR level of the estimate's skill.
    pub label: String,
    /// The ids of the skills below the estimate, in syllabus order.
    pub tested_out: Vec<String>,
}

/// The most questions a test asks.
const MAX_QUESTIONS: usize = 20;
/// The fewest questions after which enough reversals end the test.
const MIN_QUESTIONS: usize = 12;
/// The reversals that end the test once [`MIN_QUESTIONS`] are asked.
const STOP_REVERSALS: usize = 8;
/// Wrong answers in a row at the first skill that end the test early.
const EARLY_WRONG: usize = 4;
/// Reversals left out of the estimate, made while the staircase is still converging.
const SKIPPED_REVERSALS: usize = 2;
/// An estimate below this skill index is labelled "Pre-A1".
const PRE_A1_BELOW: usize = 3;

/// A placement test in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    skills: Vec<SkillRef>,
    index: usize,
    step: usize,
    questions: Vec<usize>,
    answers: Vec<bool>,
    reversals: Vec<usize>,
    finished: bool,
}

/// The mean of `values` rounded half up in integer arithmetic: (2·sum + count) / (2·count).
fn rounded_mean(values: &[usize]) -> usize {
    if values.is_empty() {
        return 0;
    }
    let sum: usize = values.iter().sum();
    let count = values.len();
    (2 * sum + count) / (2 * count)
}

impl Placement {
    /// A test over `skills` in syllabus order. It starts at index N / 4 with step max(1, N / 4);
    /// with no skills it is finished at once with estimate 0.
    pub fn new(skills: Vec<SkillRef>) -> Self {
        let n = skills.len();
        Placement {
            index: n / 4,
            step: (n / 4).max(1),
            finished: n == 0,
            skills,
            questions: Vec::new(),
            answers: Vec::new(),
            reversals: Vec::new(),
        }
    }

    /// The skill the next question is about, or `None` once the test is finished.
    pub fn next_question(&self) -> Option<SkillRef> {
        if self.finished {
            return None;
        }
        self.skills.get(self.index).cloned()
    }

    /// Records the answer to the current question and moves the staircase: a change of
    /// correctness is a reversal at the current index and halves the step (at least 1); then up
    /// by the step when correct, down when wrong. Ignored once the test is finished.
    pub fn answer(&mut self, correct: bool) {
        if self.finished {
            return;
        }
        let last = self.skills.len() - 1;
        if self
            .answers
            .last()
            .is_some_and(|&previous| previous != correct)
        {
            self.reversals.push(self.index);
            self.step = (self.step / 2).max(1);
        }
        self.questions.push(self.index);
        self.answers.push(correct);
        self.index = if correct {
            (self.index + self.step).min(last)
        } else {
            self.index.saturating_sub(self.step)
        };
        self.finished = self.should_stop();
    }

    /// Twenty questions; or twelve or more with eight reversals; or four wrong answers in a row
    /// at index 0, which ends a beginner's test early on purpose.
    fn should_stop(&self) -> bool {
        let asked = self.questions.len();
        if asked >= MAX_QUESTIONS {
            return true;
        }
        if asked >= MIN_QUESTIONS && self.reversals.len() >= STOP_REVERSALS {
            return true;
        }
        asked >= EARLY_WRONG
            && self.questions[asked - EARLY_WRONG..]
                .iter()
                .zip(&self.answers[asked - EARLY_WRONG..])
                .all(|(&index, &correct)| index == 0 && !correct)
    }

    /// The estimate: with 3 or more reversals the mean of those after the first two, otherwise
    /// the mean of the last 4 question indices.
    fn estimate(&self) -> usize {
        if self.reversals.len() > SKIPPED_REVERSALS {
            rounded_mean(&self.reversals[SKIPPED_REVERSALS..])
        } else {
            let from = self.questions.len().saturating_sub(4);
            rounded_mean(&self.questions[from..])
        }
    }

    /// The result, once the test is finished.
    pub fn result(&self) -> Option<PlacementResult> {
        if !self.finished {
            return None;
        }
        let estimate = self.estimate();
        let label = match self.skills.get(estimate) {
            Some(skill) if estimate >= PRE_A1_BELOW => format!("around {}", skill.cefr.as_str()),
            _ => "Pre-A1".to_string(),
        };
        Some(PlacementResult {
            questions: self.questions.clone(),
            answers: self.answers.clone(),
            reversals: self.reversals.clone(),
            estimate,
            label,
            tested_out: self.skills[..estimate.min(self.skills.len())]
                .iter()
                .map(|skill| skill.id.clone())
                .collect(),
        })
    }
}

/// A simulated learner who knows exactly the skills below `boundary`, each answer flipped with
/// probability `slip`: one draw per question, u = (next_u64 >> 11) / 2^53, flipped when u < slip.
pub fn simulate(
    skills: Vec<SkillRef>,
    boundary: usize,
    slip: f64,
    rng: &mut dyn Rng,
) -> PlacementResult {
    let mut placement = Placement::new(skills);
    while placement.next_question().is_some() {
        let u = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        let knows = placement.index < boundary;
        placement.answer(if u < slip { !knows } else { knows });
    }
    placement
        .result()
        .expect("a placement test without questions is finished")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::SplitMix64;

    /// N skills, the first half A1 and the rest A2.
    fn skills(n: usize) -> Vec<SkillRef> {
        (0..n)
            .map(|i| SkillRef {
                id: format!("s{i:02}"),
                cefr: if i < n / 2 { Cefr::A1 } else { Cefr::A2 },
            })
            .collect()
    }

    #[test]
    fn a_learner_correct_below_23_converges_on_23() {
        let result = simulate(skills(80), 23, 0.0, &mut SplitMix64::new(0));
        assert_eq!(
            result.questions,
            [20, 40, 30, 20, 25, 23, 21, 22, 23, 22, 23, 22]
        );
        assert_eq!(result.reversals, [40, 20, 25, 21, 23, 22, 23, 22]);
        assert_eq!(result.estimate, 23);
        assert_eq!(result.label, "around A1");
        assert_eq!(result.tested_out.len(), 23);
        assert_eq!(result.tested_out[22], "s22");
    }

    #[test]
    fn a_beginner_stops_after_four_wrong_answers_at_the_start() {
        let result = simulate(skills(80), 0, 0.0, &mut SplitMix64::new(0));
        assert_eq!(result.questions, [20, 0, 0, 0, 0]);
        assert_eq!(result.answers, [false; 5]);
        assert_eq!(result.estimate, 0);
        assert_eq!(result.label, "Pre-A1");
        assert!(result.tested_out.is_empty());
    }

    #[test]
    fn a_learner_who_knows_everything_climbs_to_the_last_skill() {
        let result = simulate(skills(80), 80, 0.0, &mut SplitMix64::new(0));
        assert_eq!(result.questions.len(), 20);
        assert_eq!(result.questions[..4], [20, 40, 60, 79]);
        assert_eq!(result.questions.last(), Some(&79));
        assert!(result.reversals.is_empty());
        assert_eq!(result.estimate, 79);
        assert_eq!(result.label, "around A2");
        assert_eq!(result.tested_out.len(), 79);
    }

    #[test]
    fn no_skills_finish_at_once() {
        let placement = Placement::new(Vec::new());
        assert_eq!(placement.next_question(), None);
        let result = placement.result().unwrap();
        assert_eq!(result.estimate, 0);
        assert_eq!(result.label, "Pre-A1");
        assert!(result.questions.is_empty() && result.tested_out.is_empty());
    }

    #[test]
    fn the_test_reports_nothing_until_it_finishes_and_ignores_extra_answers() {
        let mut placement = Placement::new(skills(8));
        assert_eq!(placement.next_question().unwrap().id, "s02");
        placement.answer(true);
        assert_eq!(placement.result(), None);
        assert_eq!(placement.next_question().unwrap().id, "s04");
        while placement.next_question().is_some() {
            placement.answer(false);
        }
        let result = placement.result().unwrap();
        placement.answer(true);
        assert_eq!(placement.result(), Some(result));
    }

    #[test]
    fn an_estimate_from_few_reversals_is_the_mean_of_the_last_four_questions() {
        assert_eq!(rounded_mean(&[1, 2]), 2, "1.5 rounds half up");
        assert_eq!(rounded_mean(&[1, 1, 2]), 1);
        assert_eq!(rounded_mean(&[]), 0);
    }

    #[test]
    fn noisy_learners_mostly_land_near_their_boundary() {
        let mut rng = SplitMix64::new(7);
        let mut close = 0;
        let runs = 200;
        for _ in 0..runs {
            let boundary = (rng.next_u64() % 81) as usize;
            let result = simulate(skills(80), boundary, 0.1, &mut rng);
            let asked = result.questions.len();
            let early = asked < 12 && result.questions.last() == Some(&0);
            assert!((12..=20).contains(&asked) || early, "{result:?}");
            if result.estimate.abs_diff(boundary) <= 4 {
                close += 1;
            }
        }
        assert!(close * 100 >= runs * 85, "{close} of {runs}");
    }
}
