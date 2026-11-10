//! XP for a session (docs/modules/03-exercise-engine-and-grading.md, Design notes).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::grading::Verdict;

/// The kinds of session this module writes to `sessions.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    Lesson,
    Review,
    /// A graded story (module 07).
    Story,
}

impl SessionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionKind::Lesson => "lesson",
            SessionKind::Review => "review",
            SessionKind::Story => "story",
        }
    }
}

impl fmt::Display for SessionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a session earned XP for: its kind, the final verdict of each answered exercise, and
/// whether the learner reached its end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionResult {
    pub kind: SessionKind,
    pub verdicts: Vec<Verdict>,
    pub finished: bool,
}

/// XP per answer that is correct, a typo or almost.
const XP_PER_ANSWER: u32 = 1;
/// XP for finishing a lesson.
const XP_LESSON: u32 = 10;
/// XP for finishing a review session.
const XP_REVIEW: u32 = 5;
/// Extra XP for a finished lesson with no wrong answer.
const XP_NO_WRONG: u32 = 5;
/// XP for finishing a story, and per correct answer to its questions
/// (docs/PUBLIK-INTEGRATION.md §6.4).
const XP_STORY: u32 = 10;
const XP_PER_STORY_ANSWER: u32 = 2;

/// XP of a finished story: 10 plus 2 per correct answer.
pub fn story_xp(correct: u32) -> u32 {
    XP_STORY + XP_PER_STORY_ANSWER * correct
}

/// The session's XP: 1 per correct, typo or almost answer (unchecked earns nothing and is not
/// wrong); 10 for finishing a lesson, 5 for a finished review session; 5 more for a finished
/// lesson with no wrong answer. Nothing is ever taken away.
pub fn xp_for(session: &SessionResult) -> u32 {
    if session.kind == SessionKind::Story {
        let correct = session
            .verdicts
            .iter()
            .filter(|v| **v == Verdict::Correct)
            .count() as u32;
        return if session.finished {
            story_xp(correct)
        } else {
            XP_PER_STORY_ANSWER * correct
        };
    }
    let answers = session
        .verdicts
        .iter()
        .filter(|v| matches!(v, Verdict::Correct | Verdict::Typo | Verdict::Almost))
        .count() as u32
        * XP_PER_ANSWER;
    if !session.finished {
        return answers;
    }
    match session.kind {
        SessionKind::Lesson => {
            let flawless = !session.verdicts.contains(&Verdict::Wrong);
            answers + XP_LESSON + if flawless { XP_NO_WRONG } else { 0 }
        }
        SessionKind::Review => answers + XP_REVIEW,
        SessionKind::Story => unreachable!("stories return above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(kind: SessionKind, verdicts: &[Verdict], finished: bool) -> SessionResult {
        SessionResult {
            kind,
            verdicts: verdicts.to_vec(),
            finished,
        }
    }

    #[test]
    fn a_lesson_earns_per_answer_plus_finishing_and_the_bonus() {
        use Verdict::*;
        let mut ten_right = vec![Correct; 10];
        ten_right.extend([Wrong, Wrong]);
        assert_eq!(xp_for(&session(SessionKind::Lesson, &ten_right, true)), 20);
        assert_eq!(
            xp_for(&session(SessionKind::Lesson, &[Correct; 12], true)),
            27
        );
        assert_eq!(
            xp_for(&session(
                SessionKind::Lesson,
                &[Correct, Typo, Almost, Unchecked],
                true
            )),
            3 + 10 + 5,
            "unchecked earns nothing and is not wrong"
        );
        assert_eq!(
            xp_for(&session(SessionKind::Lesson, &[Correct; 3], false)),
            3
        );
    }

    #[test]
    fn a_review_session_earns_five_for_finishing() {
        use Verdict::*;
        assert_eq!(
            xp_for(&session(SessionKind::Review, &[Correct, Wrong], true)),
            1 + 5
        );
        assert_eq!(xp_for(&session(SessionKind::Review, &[], true)), 5);
        assert_eq!(
            serde_json::to_string(&SessionKind::Review).unwrap(),
            "\"review\""
        );
    }
}
