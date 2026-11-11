//! The lesson runner: start a lesson or a review, submit answers, "I was right", finish
//! (docs/modules/03-exercise-engine-and-grading.md, Design notes).
//!
//! Every graded answer reviews its card through module 02's store, so the review log stays
//! append-only and the cards equal its replay (invariant I6). Finishing credits XP and the day
//! and recomputes the streak; freezes are written only there, as the streak earns or uses them
//! (invariant I7). Nothing counts or limits lessons (invariant I8).

use std::fmt;

use serde::{Deserialize, Serialize};

use super::types::{Exercise, Lesson};
use crate::content::{Forms, Lang};
use crate::env::{Clock, LocalDate, local_date};
use crate::error::AppError;
use crate::game::{SessionKind, SessionResult, WEEKLY_RECORD, xp_for};
use crate::grading::{
    AccentLeniency, Answer, Grade, Verdict, accept_learner_answer, grade, grade_ctx, normalise,
};
use crate::srs::{self, Outcome, SrsParams, rating_for};
use crate::store::{ReviewContext, SessionEnd, SessionStart, Store, StoreError};

/// The learner's settings a run uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunSettings {
    pub accent: AccentLeniency,
    pub daily_goal_xp: u32,
    pub srs: SrsParams,
}

impl Default for RunSettings {
    /// Lenient accents, a daily goal of 20 XP, desired retention 0.90.
    fn default() -> Self {
        RunSettings {
            accent: AccentLeniency::Lenient,
            daily_goal_xp: 20,
            srs: SrsParams::default(),
        }
    }
}

/// One answer to one exercise of the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submission {
    pub exercise_id: String,
    pub answer: Answer,
    pub response_ms: Option<i64>,
    pub used_hint: bool,
}

/// Why the runner refused a submission. The first three display the sheet's messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// "Type an answer first."
    EmptyAnswer,
    /// "This exercise was already checked."
    AlreadyChecked,
    /// "This exercise is no longer available."
    NotAvailable,
    Store(StoreError),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunError::EmptyAnswer => f.write_str("Type an answer first."),
            RunError::AlreadyChecked => f.write_str("This exercise was already checked."),
            RunError::NotAvailable => f.write_str("This exercise is no longer available."),
            RunError::Store(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for RunError {}

impl From<StoreError> for RunError {
    fn from(e: StoreError) -> Self {
        RunError::Store(e)
    }
}

/// What finishing a lesson or review shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LessonSummary {
    pub xp: u32,
    /// Answers graded correct.
    pub correct: u32,
    /// Exercises in the run.
    pub total: u32,
    pub streak: u32,
    pub freezes: u32,
    pub record_week: bool,
    /// "New weekly record!" when this session set one.
    pub message: Option<String>,
    /// The day's XP after this session.
    pub daily_xp: u32,
    pub goal_xp: u32,
}

/// One answered exercise.
#[derive(Debug, Clone)]
struct Answered {
    answer: Answer,
    response_ms: Option<i64>,
    used_hint: bool,
    grade: Grade,
    /// The AI judge's error tags, once it gave a verdict (Explain sends them).
    judge_tags: Vec<String>,
}

/// One exercise of a run, and what was answered once it was.
pub(crate) struct ExerciseState<'a> {
    pub exercise: &'a Exercise,
    /// The answer, its grade and the AI judge's error tags.
    pub answered: Option<(&'a Answer, &'a Grade, &'a [String])>,
}

/// The submitted answer of `slot`, when it is still unchecked.
fn unchecked(slot: &mut Option<Answered>) -> Result<&mut Answered, RunError> {
    let Some(answered) = slot.as_mut() else {
        return Err(RunError::EmptyAnswer);
    };
    if answered.grade.verdict != Verdict::Unchecked {
        return Err(RunError::AlreadyChecked);
    }
    Ok(answered)
}

/// A lesson or review in progress.
#[derive(Debug, Clone)]
pub struct LessonRun {
    session_id: String,
    lang: Lang,
    kind: SessionKind,
    /// The local date of the start, the day the session counts on.
    local_date: LocalDate,
    exercises: Vec<Exercise>,
    answered: Vec<Option<Answered>>,
}

impl LessonRun {
    fn new(
        session_id: &str,
        lang: Lang,
        kind: SessionKind,
        local_date: LocalDate,
        exercises: Vec<Exercise>,
    ) -> LessonRun {
        let answered = vec![None; exercises.len()];
        LessonRun {
            session_id: session_id.to_string(),
            lang,
            kind,
            local_date,
            exercises,
            answered,
        }
    }

    /// Starts a lesson: stores it (`lessons` id `{skill_id}/{seq}`; a lesson already stored for
    /// that skill and seq is kept) and a `lesson` session dated where the learner is.
    pub fn start_lesson(
        store: &mut Store,
        lesson: &Lesson,
        session_id: &str,
        clock: &dyn Clock,
    ) -> Result<LessonRun, StoreError> {
        let date = local_date(clock);
        store.start_session(
            &SessionStart {
                id: session_id,
                lang: lesson.lang,
                kind: SessionKind::Lesson,
                skill_id: Some(&lesson.skill_id),
                started_at: clock.now_ms(),
                local_date: date,
            },
            Some(lesson),
        )?;
        Ok(LessonRun::new(
            session_id,
            lesson.lang,
            SessionKind::Lesson,
            date,
            lesson.exercises.clone(),
        ))
    }

    /// Starts a review session over `exercises`; only the session is stored.
    pub fn start_review(
        store: &mut Store,
        lang: Lang,
        exercises: Vec<Exercise>,
        session_id: &str,
        clock: &dyn Clock,
    ) -> Result<LessonRun, StoreError> {
        let date = local_date(clock);
        store.start_session(
            &SessionStart {
                id: session_id,
                lang,
                kind: SessionKind::Review,
                skill_id: None,
                started_at: clock.now_ms(),
                local_date: date,
            },
            None,
        )?;
        Ok(LessonRun::new(
            session_id,
            lang,
            SessionKind::Review,
            date,
            exercises,
        ))
    }

    pub fn exercises(&self) -> &[Exercise] {
        &self.exercises
    }

    fn index_of(&self, exercise_id: &str) -> Result<usize, RunError> {
        self.exercises
            .iter()
            .position(|ex| ex.id == exercise_id)
            .ok_or(RunError::NotAvailable)
    }

    fn review_context(
        &self,
        ex: &Exercise,
        verdict: srs::Verdict,
        response_ms: Option<i64>,
    ) -> ReviewContext {
        ReviewContext {
            exercise_type: ex.kind().as_str().to_string(),
            verdict,
            response_ms,
            session_id: self.session_id.clone(),
        }
    }

    /// Grades one answer and reviews its card. An unknown exercise is no longer available; a
    /// second answer is refused as already checked; an empty answer (no text after
    /// normalising, or no pairs) is refused and nothing is recorded. Otherwise the item is
    /// introduced if it has no cards and, unless the answer is unchecked, its card (item, facet)
    /// is reviewed with the rating of the verdict, the hint and the response time, which the
    /// returned grade carries.
    pub fn submit(
        &mut self,
        store: &mut Store,
        forms: &Forms,
        settings: &RunSettings,
        sub: &Submission,
        clock: &dyn Clock,
    ) -> Result<Grade, RunError> {
        let index = self.index_of(&sub.exercise_id)?;
        if self.answered[index].is_some() {
            return Err(RunError::AlreadyChecked);
        }
        let empty = match &sub.answer {
            Answer::Text(text) => normalise(self.lang, text).is_empty(),
            Answer::Pairs(pairs) => pairs.is_empty(),
            Answer::Choice(_) => false,
        };
        if empty {
            return Err(RunError::EmptyAnswer);
        }
        let ex = &self.exercises[index];
        let now = clock.now_ms();
        let ctx = grade_ctx(store, forms, settings.accent, ex, &sub.answer)?;
        let mut graded = grade(&ctx, ex, &sub.answer);
        store.introduce(&ex.item_id, now)?;
        if let Some(verdict) = graded.verdict.srs() {
            let rating = rating_for(&Outcome {
                verdict,
                used_hint: sub.used_hint,
                exercise_type: ex.kind().as_str().to_string(),
                response_ms: sub.response_ms,
                learner_override: false,
            });
            let context = self.review_context(ex, verdict, sub.response_ms);
            store.review(&ex.item_id, ex.facet, rating, now, &context, &settings.srs)?;
            graded.rating = Some(rating);
        }
        self.answered[index] = Some(Answered {
            answer: sub.answer.clone(),
            response_ms: sub.response_ms,
            used_hint: sub.used_hint,
            grade: graded.clone(),
            judge_tags: Vec::new(),
        });
        Ok(graded)
    }

    /// The exercise `exercise_id` and, once answered, its answer, grade and the judge's tags. An
    /// unknown exercise is no longer available.
    pub(crate) fn state(&self, exercise_id: &str) -> Result<ExerciseState<'_>, RunError> {
        let index = self.index_of(exercise_id)?;
        Ok(ExerciseState {
            exercise: &self.exercises[index],
            answered: self.answered[index]
                .as_ref()
                .map(|a| (&a.answer, &a.grade, a.judge_tags.as_slice())),
        })
    }

    /// The AI judge's verdict on a submitted, unchecked answer: the card is reviewed with the
    /// verdict, the submission's hint and response time, and `grade` (module 08's verdict grade)
    /// becomes the answer's grade with the rating as reviewed; `tags` are the judge's error tags.
    /// Refused like "I was right": an unknown exercise, one not yet submitted, or one decided.
    pub(crate) fn judged(
        &mut self,
        store: &mut Store,
        settings: &RunSettings,
        exercise_id: &str,
        mut grade: Grade,
        tags: Vec<String>,
        clock: &dyn Clock,
    ) -> Result<Grade, RunError> {
        let index = self.index_of(exercise_id)?;
        let ex = &self.exercises[index];
        let answered = unchecked(&mut self.answered[index])?;
        let Some(verdict) = grade.verdict.srs() else {
            return Ok(answered.grade.clone());
        };
        let rating = rating_for(&Outcome {
            verdict,
            used_hint: answered.used_hint,
            exercise_type: ex.kind().as_str().to_string(),
            response_ms: answered.response_ms,
            learner_override: false,
        });
        let context = ReviewContext {
            exercise_type: ex.kind().as_str().to_string(),
            verdict,
            response_ms: answered.response_ms,
            session_id: self.session_id.clone(),
        };
        store.review(
            &ex.item_id,
            ex.facet,
            rating,
            clock.now_ms(),
            &context,
            &settings.srs,
        )?;
        grade.rating = Some(rating);
        answered.grade = grade.clone();
        answered.judge_tags = tags;
        Ok(grade)
    }

    /// Why the AI judge gave no verdict on a submitted, unchecked answer: kept on its grade, which
    /// stays unchecked (so "I was right" still works), and returned. A decided answer keeps its
    /// grade, which is returned as it is.
    pub(crate) fn judge_failed(
        &mut self,
        exercise_id: &str,
        error: AppError,
    ) -> Result<Grade, RunError> {
        let index = self.index_of(exercise_id)?;
        let slot = &mut self.answered[index];
        match unchecked(slot) {
            Ok(answered) => {
                answered.grade.judge_error = Some(error);
                Ok(answered.grade.clone())
            }
            Err(RunError::AlreadyChecked) => slot
                .as_ref()
                .map(|a| a.grade.clone())
                .ok_or(RunError::EmptyAnswer),
            Err(e) => Err(e),
        }
    }

    /// "I was right" on a submitted, unchecked answer: the answer is accepted from now on
    /// (`accept_learner_answer`) and the card is reviewed Good as a learner override. An unknown
    /// exercise is no longer available; one not yet submitted needs an answer first; any other
    /// verdict, or a second override, was already checked.
    pub fn i_was_right(
        &mut self,
        store: &mut Store,
        settings: &RunSettings,
        exercise_id: &str,
        clock: &dyn Clock,
    ) -> Result<Grade, RunError> {
        let index = self.index_of(exercise_id)?;
        let Some(answered) = &self.answered[index] else {
            return Err(RunError::EmptyAnswer);
        };
        if answered.grade.verdict != Verdict::Unchecked {
            return Err(RunError::AlreadyChecked);
        }
        let ex = &self.exercises[index];
        let now = clock.now_ms();
        let accepted = accept_learner_answer(store, ex, &answered.answer, now)?;
        let rating = rating_for(&Outcome {
            verdict: srs::Verdict::Correct,
            used_hint: false,
            exercise_type: ex.kind().as_str().to_string(),
            response_ms: answered.response_ms,
            learner_override: true,
        });
        let context = self.review_context(ex, srs::Verdict::Correct, answered.response_ms);
        store.review(&ex.item_id, ex.facet, rating, now, &context, &settings.srs)?;
        let mut graded = accepted;
        graded.rating = Some(rating);
        if let Some(answered) = self.answered[index].as_mut() {
            answered.grade = graded.clone();
        }
        Ok(graded)
    }

    /// Finishes the run: XP from `xp_for` over the final verdicts; the session row completed; the
    /// session's day credited with the XP, one more session and the daily goal; the weekly
    /// record decided before the credit; the streak recomputed with its frozen days and new
    /// freeze events written.
    pub fn finish(
        self,
        store: &mut Store,
        settings: &RunSettings,
        clock: &dyn Clock,
    ) -> Result<LessonSummary, RunError> {
        let verdicts: Vec<Verdict> = self
            .answered
            .iter()
            .flatten()
            .map(|answered| answered.grade.verdict)
            .collect();
        let correct = verdicts.iter().filter(|v| **v == Verdict::Correct).count() as u32;
        let exercises = verdicts.len() as u32;
        let xp = xp_for(&SessionResult {
            kind: self.kind,
            verdicts,
            finished: true,
        });
        let done = store.finish_session(&SessionEnd {
            id: &self.session_id,
            ended_at: clock.now_ms(),
            xp,
            exercises,
            correct,
            day: self.local_date,
            today: local_date(clock),
            goal_xp: settings.daily_goal_xp,
        })?;
        Ok(LessonSummary {
            xp,
            correct,
            total: self.exercises.len() as u32,
            streak: done.streak.length,
            freezes: done.streak.freezes,
            record_week: done.record_week,
            message: done.record_week.then(|| WEEKLY_RECORD.to_string()),
            daily_xp: done.daily_xp,
            goal_xp: settings.daily_goal_xp,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::test_support::{EXAMPLE_SYLLABUS, example_forms};
    use crate::content::{Syllabus, parse_syllabus};
    use crate::exercise::types::{Body, Origin};
    use crate::grading::Direction;
    use crate::srs::{DAY_MS, Facet, Rating, replay};
    use crate::store::AnswerSource;

    const SKILL: &str = "es.a1.u01.s01";

    struct At(i64);

    impl Clock for At {
        fn now_ms(&self) -> i64 {
            self.0
        }
        fn utc_offset_minutes(&self) -> i32 {
            0
        }
    }

    /// Midday UTC on day `n` after 2026-01-04 (day 1 is a Monday).
    fn day(n: i64) -> At {
        let start = LocalDate::parse("2026-01-04").unwrap().days();
        At((start + n) * DAY_MS + DAY_MS / 2)
    }

    fn syllabus() -> Syllabus {
        parse_syllabus("valid-es.yaml", EXAMPLE_SYLLABUS).unwrap()
    }

    fn started() -> Store {
        let mut store = Store::open_in_memory().unwrap();
        store
            .start_course(&syllabus(), &example_forms(), 0)
            .unwrap();
        store
    }

    fn exercise(id: &str, item: &str, body: Body) -> Exercise {
        Exercise {
            id: id.into(),
            item_id: format!("{SKILL}#{item}"),
            facet: body.kind().facet(),
            origin: Origin::Template,
            body,
        }
    }

    /// Three exercises: a pick, a translation of the item's own prompt, and one of another
    /// prompt.
    fn lesson(seq: u32) -> Lesson {
        Lesson {
            schema: Lesson::SCHEMA.into(),
            lang: Lang::Es,
            skill_id: SKILL.into(),
            seq,
            origin: Origin::Template,
            exercises: vec![
                exercise(
                    "e1",
                    "l:día",
                    Body::PickMeaning {
                        prompt_target: "el día".into(),
                        options: vec!["night".into(), "day".into(), "hello".into(), "good".into()],
                        answer_index: 1,
                    },
                ),
                exercise(
                    "e2",
                    "e:2",
                    Body::TranslateToTarget {
                        prompt_en: "Good morning.".into(),
                        target: "Buenos días.".into(),
                        accepted: vec!["Buenos días.".into()],
                        tiles: None,
                    },
                ),
                exercise(
                    "e3",
                    "e:2",
                    Body::TranslateToTarget {
                        prompt_en: "Morning!".into(),
                        target: "Buenos días.".into(),
                        accepted: vec!["Buenos días.".into()],
                        tiles: None,
                    },
                ),
            ],
            ai: None,
        }
    }

    fn submit(id: &str, answer: Answer) -> Submission {
        Submission {
            exercise_id: id.into(),
            answer,
            response_ms: Some(4_000),
            used_hint: false,
        }
    }

    fn text(s: &str) -> Answer {
        Answer::Text(s.into())
    }

    #[test]
    fn the_runner_refuses_with_the_three_messages() {
        let mut store = started();
        let forms = example_forms();
        let settings = RunSettings::default();
        let clock = day(1);
        let mut run = LessonRun::start_lesson(&mut store, &lesson(1), "s1", &clock).unwrap();
        assert_eq!(run.exercises().len(), 3);
        let err = run
            .submit(
                &mut store,
                &forms,
                &settings,
                &submit("e9", Answer::Choice(1)),
                &clock,
            )
            .unwrap_err();
        assert_eq!(err.to_string(), "This exercise is no longer available.");
        let err = run
            .submit(
                &mut store,
                &forms,
                &settings,
                &submit("e2", text(" ¡! ")),
                &clock,
            )
            .unwrap_err();
        assert_eq!(err.to_string(), "Type an answer first.");
        let err = run
            .submit(
                &mut store,
                &forms,
                &settings,
                &submit("e1", Answer::Pairs(vec![])),
                &clock,
            )
            .unwrap_err();
        assert_eq!(err, RunError::EmptyAnswer);
        assert!(store.review_log().unwrap().is_empty(), "nothing recorded");
        let grade = run
            .submit(
                &mut store,
                &forms,
                &settings,
                &submit("e1", Answer::Choice(1)),
                &clock,
            )
            .unwrap();
        assert_eq!(grade.verdict, Verdict::Correct);
        let err = run
            .submit(
                &mut store,
                &forms,
                &settings,
                &submit("e1", Answer::Choice(1)),
                &clock,
            )
            .unwrap_err();
        assert_eq!(err.to_string(), "This exercise was already checked.");
        let err = run
            .i_was_right(&mut store, &settings, "e1", &clock)
            .unwrap_err();
        assert_eq!(err, RunError::AlreadyChecked);
        let err = run
            .i_was_right(&mut store, &settings, "e2", &clock)
            .unwrap_err();
        assert_eq!(err, RunError::EmptyAnswer);
        let err = run
            .i_was_right(&mut store, &settings, "e7", &clock)
            .unwrap_err();
        assert_eq!(err, RunError::NotAvailable);
        assert_eq!(
            RunError::Store(StoreError::UnknownItem("x".into())).to_string(),
            "unknown item x"
        );
    }

    #[test]
    fn i_was_right_makes_the_same_answer_correct_offline() {
        let mut store = started();
        let forms = example_forms();
        let settings = RunSettings::default();
        let clock = day(1);
        let mut run = LessonRun::start_lesson(&mut store, &lesson(1), "s1", &clock).unwrap();
        let grade = run
            .submit(
                &mut store,
                &forms,
                &settings,
                &submit("e2", text("Buen día")),
                &clock,
            )
            .unwrap();
        assert_eq!((grade.verdict, grade.rating), (Verdict::Unchecked, None));
        assert!(
            store.review_log().unwrap().is_empty(),
            "unchecked reviews nothing"
        );
        assert!(
            store
                .card(&format!("{SKILL}#e:2"), Facet::Productive)
                .unwrap()
                .is_some()
        );
        let grade = run
            .i_was_right(&mut store, &settings, "e2", &clock)
            .unwrap();
        assert_eq!(
            (grade.verdict, grade.rating),
            (Verdict::Correct, Some(Rating::Good))
        );
        assert_eq!(
            run.i_was_right(&mut store, &settings, "e2", &clock)
                .unwrap_err(),
            RunError::AlreadyChecked
        );
        let learner = store
            .accepted_answers(&format!("{SKILL}#e:2"), Direction::ToTarget)
            .unwrap();
        assert_eq!(learner.last().unwrap().source, AnswerSource::Learner);
        assert_eq!(learner.last().unwrap().display, "Buen día");
        let grade = run
            .submit(
                &mut store,
                &forms,
                &settings,
                &submit("e3", text("Buen día")),
                &clock,
            )
            .unwrap();
        assert_eq!(
            grade.verdict,
            Verdict::Unchecked,
            "another prompt, no stored answers"
        );
        run.i_was_right(&mut store, &settings, "e3", &clock)
            .unwrap();
        let log = store.review_log().unwrap();
        assert_eq!(log.len(), 2);
        assert!(
            log.iter()
                .all(|e| e.rating == Rating::Good && e.response_ms == Some(4_000))
        );

        let mut next = LessonRun::start_lesson(&mut store, &lesson(2), "s2", &clock).unwrap();
        for id in ["e2", "e3"] {
            let grade = next
                .submit(
                    &mut store,
                    &forms,
                    &settings,
                    &submit(id, text("buen día")),
                    &clock,
                )
                .unwrap();
            assert_eq!(grade.verdict, Verdict::Correct, "{id}");
        }
        let cards: Vec<_> = store
            .cards()
            .unwrap()
            .into_iter()
            .filter(|c| c.reps > 0)
            .collect();
        assert_eq!(replay(&store.review_log().unwrap(), &settings.srs), cards);
    }

    #[test]
    fn finishing_awards_xp_and_the_bonus() {
        let mut store = started();
        let forms = example_forms();
        let settings = RunSettings::default();
        let clock = day(1);
        let mut run = LessonRun::start_lesson(&mut store, &lesson(1), "s1", &clock).unwrap();
        let fast = Submission {
            response_ms: Some(1_000),
            ..submit("e1", Answer::Choice(1))
        };
        let grade = run
            .submit(&mut store, &forms, &settings, &fast, &clock)
            .unwrap();
        assert_eq!(
            grade.rating,
            Some(Rating::Easy),
            "the runner rates with the time"
        );
        run.submit(
            &mut store,
            &forms,
            &settings,
            &submit("e2", text("Buenos dias")),
            &clock,
        )
        .unwrap();
        let summary = run.finish(&mut store, &settings, &clock).unwrap();
        assert_eq!(
            summary,
            LessonSummary {
                xp: 2 + 10 + 5,
                correct: 1,
                total: 3,
                streak: 1,
                freezes: 0,
                record_week: false,
                message: None,
                daily_xp: 17,
                goal_xp: 20,
            }
        );
        let mut run = LessonRun::start_lesson(&mut store, &lesson(1), "s2", &clock).unwrap();
        run.submit(
            &mut store,
            &forms,
            &settings,
            &submit("e1", Answer::Choice(0)),
            &clock,
        )
        .unwrap();
        let summary = run.finish(&mut store, &settings, &clock).unwrap();
        assert_eq!((summary.xp, summary.daily_xp), (10, 27));
    }

    #[test]
    fn a_hundred_lessons_in_a_day_and_freezes_only_earned() {
        let mut store = started();
        let settings = RunSettings::default();
        let clock = day(1);
        for n in 0..100 {
            let run =
                LessonRun::start_lesson(&mut store, &lesson(1), &format!("a{n}"), &clock).unwrap();
            run.finish(&mut store, &settings, &clock).unwrap();
        }
        let daily = store.daily().unwrap();
        assert_eq!(daily.len(), 1);
        assert_eq!(daily[0].sessions, 100);
        assert!(store.freeze_events().unwrap().is_empty());
        for n in 2..=7 {
            let clock = day(n);
            let run =
                LessonRun::start_review(&mut store, Lang::Es, Vec::new(), &format!("r{n}"), &clock)
                    .unwrap();
            let summary = run.finish(&mut store, &settings, &clock).unwrap();
            assert_eq!((summary.streak, summary.xp), (n as u32, 5));
        }
        let events = store.freeze_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].streak_len, 7);
    }

    #[test]
    fn a_weekly_record_is_announced() {
        let mut store = started();
        let settings = RunSettings::default();
        let forms = example_forms();
        // Week 1: one lesson of 15 XP. Week 2: a lesson of 15 (no record), then one of 16.
        for (n, id) in [(1, "w1"), (8, "w2a")] {
            let clock = day(n);
            let run = LessonRun::start_lesson(&mut store, &lesson(1), id, &clock).unwrap();
            assert_eq!(
                run.finish(&mut store, &settings, &clock).unwrap().message,
                None
            );
        }
        let clock = day(9);
        let mut run = LessonRun::start_lesson(&mut store, &lesson(1), "w2b", &clock).unwrap();
        run.submit(
            &mut store,
            &forms,
            &settings,
            &submit("e1", Answer::Choice(1)),
            &clock,
        )
        .unwrap();
        let summary = run.finish(&mut store, &settings, &clock).unwrap();
        assert!(summary.record_week);
        assert_eq!(summary.message.as_deref(), Some("New weekly record!"));
    }
}
