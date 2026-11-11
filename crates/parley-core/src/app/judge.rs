//! Judging and explanations in the app service (docs/modules/08-judging-and-explanations.md):
//! the AI judge of an unchecked answer (automatic after `exercise_submit`, or **Check with AI** and
//! **Try again**, the command `exercise_check`) and Explain my answer (`explain_answer`).
//!
//! Like a fresh lesson, a call is made in three steps so it holds no lock: `judge_begin` (or
//! `explain_begin`) decides whether to call and returns a job; the job's `run` makes the call; the
//! completion applies the answer to the run in progress. A job whose answer was decided meanwhile
//! (**I was right**) changes nothing.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::lesson::{AiAccess, open_cache, run_error, run_settings};
use super::{Parley, not_available, open_store};
use crate::ai::{
    AiClient, AiError, EXPLAIN_NOT_AVAILABLE, EXPLAIN_OFFLINE, ExplainRequest, Explanation,
    JUDGE_OFFLINE, JudgeRequest, Judgement, apply_judgement, explain, request_judgement,
    verdict_grade,
};
use crate::content::{Cefr, Lang};
use crate::error::{AppError, ErrorCode};
use crate::exercise::{Body, Exercise, ExerciseType};
use crate::grading::grade::prompt_is_items_own;
use crate::grading::{Answer, Grade, Verdict};

/// What a judge step gives: a final grade, or a call to make with no lock held.
pub enum JudgeBegin {
    Done(Grade),
    Pending(JudgeJob),
}

/// A P3 call to make with no lock held; owns the client, the cache path, the request and the time.
pub struct JudgeJob {
    session_id: String,
    exercise_id: String,
    client: Arc<dyn AiClient>,
    cache_path: PathBuf,
    request: JudgeRequest,
    now_ms: i64,
    kind: ExerciseType,
    /// The exercise's first accepted answer, shown under a `wrong` verdict.
    first_accepted: Option<String>,
    /// Right after `exercise_submit`, not asked for by the learner.
    automatic: bool,
}

/// What a judge job's call gave.
pub struct JudgeOutcome(pub(crate) Result<Judgement, AiError>);

impl JudgeJob {
    /// What the job asks the judge.
    pub fn request(&self) -> &JudgeRequest {
        &self.request
    }

    /// Opens cache/ai.sqlite3 and makes the call (request_judgement).
    pub fn run(&self) -> JudgeOutcome {
        let cache = open_cache(&self.cache_path, self.now_ms);
        JudgeOutcome(request_judgement(&*self.client, &cache, &self.request))
    }
}

/// A P4 call to make with no lock held.
pub struct ExplainJob {
    session_id: String,
    client: Arc<dyn AiClient>,
    cache_path: PathBuf,
    request: ExplainRequest,
    now_ms: i64,
}

/// What an explain job's call gave.
pub struct ExplainOutcome(pub(crate) Result<Explanation, AiError>);

impl ExplainJob {
    /// What the job asks for.
    pub fn request(&self) -> &ExplainRequest {
        &self.request
    }

    /// Opens cache/ai.sqlite3 and makes the call (explain).
    pub fn run(&self) -> ExplainOutcome {
        let cache = open_cache(&self.cache_path, self.now_ms);
        ExplainOutcome(explain(&*self.client, &cache, &self.request))
    }
}

/// `explain_answer`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Explained {
    pub text: String,
    pub tip: Option<String>,
    pub cached: bool,
}

/// "AI features are not set up yet." (`needs_consent`): the interface opens the AI setup dialog.
fn needs_consent() -> AppError {
    AiError::NeedsConsent.into()
}

fn answer_first() -> AppError {
    AppError::new(ErrorCode::InvalidInput, "Type an answer first.")
}

fn already_checked() -> AppError {
    AppError::new(
        ErrorCode::InvalidInput,
        "This exercise was already checked.",
    )
}

fn not_explainable() -> AppError {
    AppError::new(ErrorCode::InvalidInput, EXPLAIN_NOT_AVAILABLE)
}

/// Why a judge call gave no verdict, as the result sheet shows it: offline is "You're offline, so
/// Parley can't check free answers."; every other error is its own.
fn judge_error(error: AiError) -> AppError {
    match error {
        AiError::Offline => AppError::new(ErrorCode::Offline, JUDGE_OFFLINE),
        other => other.into(),
    }
}

/// Why an explain call gave nothing: offline is "Explanations need a connection or a local
/// model."; every other error is its own (a 402 with its message and `top_up_url`).
fn explain_error(error: AiError) -> AppError {
    match error {
        AiError::Offline => AppError::new(ErrorCode::Offline, EXPLAIN_OFFLINE),
        other => other.into(),
    }
}

/// What an exercise asks and expects, as P4 names them (PUBLIK-INTEGRATION §6.6): none for
/// `match_pairs`.
fn prompt_and_expected(ex: &Exercise) -> Option<(String, String)> {
    let first = |accepted: &[String], fallback: &str| {
        accepted
            .first()
            .cloned()
            .unwrap_or_else(|| fallback.to_string())
    };
    match &ex.body {
        Body::TranslateToTarget {
            prompt_en,
            target,
            accepted,
            ..
        } => Some((prompt_en.clone(), first(accepted, target))),
        Body::TranslateToEn {
            prompt_target,
            accepted,
        } => Some((prompt_target.clone(), first(accepted, ""))),
        Body::PickMeaning {
            prompt_target,
            options,
            answer_index,
        } => Some((
            prompt_target.clone(),
            options
                .get(*answer_index as usize)
                .cloned()
                .unwrap_or_default(),
        )),
        Body::ListenType { text, .. } => Some((text.clone(), text.clone())),
        Body::Speak { text, prompt_en } => Some((prompt_en.clone(), text.clone())),
        Body::FillGap {
            sentence, answer, ..
        } => Some((sentence.clone(), answer.clone())),
        Body::MatchPairs { .. } => None,
    }
}

/// The learner's text, or the text of the option chosen; none for matched pairs.
fn answer_text(ex: &Exercise, answer: &Answer) -> Option<String> {
    match (answer, &ex.body) {
        (Answer::Text(text), _) => Some(text.clone()),
        (Answer::Choice(index), Body::PickMeaning { options, .. }) => {
            options.get(*index as usize).cloned()
        }
        _ => None,
    }
}

impl Parley {
    /// The level of an item and the grammar note of its skill: from the bundled course (its item,
    /// else the skill the store files it under); A1 and no note for an item the course lacks.
    fn level_and_note(&self, lang: Lang, item_id: &str) -> (Cefr, Option<String>) {
        let Some(course) = self.content.get(&lang) else {
            return (Cefr::A1, None);
        };
        let (cefr, skill_id) = match course.items.iter().find(|item| item.id == item_id) {
            Some(item) => (Some(item.cefr), Some(item.skill.clone())),
            None => (
                None,
                self.store
                    .as_ref()
                    .ok()
                    .and_then(|store| store.item(item_id).ok())
                    .map(|item| item.skill_id),
            ),
        };
        let skill = skill_id.and_then(|id| {
            course
                .syllabus
                .skills()
                .map(|(_, skill)| skill)
                .find(|skill| skill.id == id)
        });
        match skill {
            Some(skill) => (cefr.unwrap_or(skill.cefr), skill.grammar_note.clone()),
            None => (cefr.unwrap_or(Cefr::A1), None),
        }
    }

    /// Keeps a judge failure on the run's grade and returns that grade.
    fn keep_judge_error(
        &mut self,
        session_id: &str,
        exercise_id: &str,
        error: AppError,
    ) -> Result<Grade, AppError> {
        let active = self.runs.get_mut(session_id).ok_or_else(not_available)?;
        active
            .run
            .judge_failed(exercise_id, error)
            .map_err(run_error)
    }

    /// The P3 request for a text answer to a translation of a run of `lang`.
    fn judge_request(
        &mut self,
        lang: Lang,
        ex: &Exercise,
        answer: &Answer,
    ) -> Result<JudgeRequest, AppError> {
        let (Answer::Text(text), Some(direction), Some(source)) =
            (answer, ex.direction(), ex.prompt())
        else {
            return Err(already_checked());
        };
        let (cefr, _) = self.level_and_note(lang, &ex.item_id);
        let store = open_store(&mut self.store)?;
        let item = store.item(&ex.item_id)?;
        Ok(JudgeRequest {
            lang,
            direction,
            cefr,
            source: source.to_string(),
            accepted: ex.own_answers().into_iter().map(str::to_string).collect(),
            answer: text.clone(),
            item_id: item.id.clone(),
            own_prompt: prompt_is_items_own(&item, ex),
        })
    }

    /// After a submission graded `unchecked`: whether to ask the judge. `automatic` is true right
    /// after `exercise_submit`, false for Check with AI and Try again (module 08's table).
    pub fn judge_begin(
        &mut self,
        session_id: &str,
        exercise_id: &str,
        ai: Option<AiAccess>,
        automatic: bool,
    ) -> Result<JudgeBegin, AppError> {
        let active = self.runs.get(session_id).ok_or_else(not_available)?;
        let state = active.run.state(exercise_id).map_err(run_error)?;
        let Some((answer, grade, _)) = state.answered else {
            return Err(answer_first());
        };
        if grade.verdict != Verdict::Unchecked {
            return Err(already_checked());
        }
        let (grade, exercise, answer) = (grade.clone(), state.exercise.clone(), answer.clone());
        let (lang, stopped) = (active.lang, active.judging_stopped);

        let client = if automatic {
            match ai {
                None => return Ok(JudgeBegin::Done(grade)),
                Some(_) if stopped => return Ok(JudgeBegin::Done(grade)),
                Some(Err(AiError::NeedsConsent)) => return Ok(JudgeBegin::Done(grade)),
                Some(Err(error)) => {
                    return self
                        .keep_judge_error(session_id, exercise_id, judge_error(error))
                        .map(JudgeBegin::Done);
                }
                Some(Ok(client)) => {
                    let status = client.status();
                    if status.needs_consent || status.needs_credit {
                        return Ok(JudgeBegin::Done(grade));
                    }
                    client
                }
            }
        } else {
            match ai {
                None | Some(Err(AiError::NeedsConsent)) => return Err(needs_consent()),
                Some(Err(error)) => {
                    return self
                        .keep_judge_error(session_id, exercise_id, judge_error(error))
                        .map(JudgeBegin::Done);
                }
                Some(Ok(client)) => {
                    if client.status().needs_consent {
                        return Err(needs_consent());
                    }
                    client
                }
            }
        };

        let request = self.judge_request(lang, &exercise, &answer)?;
        Ok(JudgeBegin::Pending(JudgeJob {
            session_id: session_id.to_string(),
            exercise_id: exercise_id.to_string(),
            client,
            cache_path: self.config.data_dir.join(super::AI_CACHE_FILE),
            request,
            now_ms: self.env.clock.now_ms(),
            kind: exercise.kind(),
            first_accepted: exercise.own_answers().first().map(|a| a.to_string()),
            automatic,
        }))
    }

    /// Applies a job's outcome to the run and returns the grade the sheet shows. A valid verdict
    /// reviews the card (the verdict, the submission's hint and response time), becomes the grade,
    /// is stored as a judgement (and a correct answer to the item's own prompt as an accepted
    /// answer), and its charge is added to the session's cost. A failure keeps the grade
    /// unchecked with `judge_error` set: a 402 also stops automatic judging for the rest of the
    /// run, offline reads "You're offline, so Parley can't check free answers.", and no consent is
    /// the command error `needs_consent` (after an automatic call, the grade as it is).
    pub fn judge_complete(
        &mut self,
        job: JudgeJob,
        outcome: JudgeOutcome,
    ) -> Result<Grade, AppError> {
        let JudgeJob {
            session_id,
            exercise_id,
            client,
            request,
            kind,
            first_accepted,
            automatic,
            ..
        } = job;
        let settings = run_settings(&self.settings);
        let store = open_store(&mut self.store)?;
        let active = self.runs.get_mut(&session_id).ok_or_else(not_available)?;
        let state = active.run.state(&exercise_id).map_err(run_error)?;
        let current = match state.answered {
            Some((_, grade, _)) => grade.clone(),
            None => return Err(answer_first()),
        };
        // Decided meanwhile (I was right): nothing changes.
        if current.verdict != Verdict::Unchecked {
            return Ok(current);
        }
        match outcome.0 {
            Ok(judgement) => {
                let clock = &*self.env.clock;
                let grade = verdict_grade(kind, &judgement, first_accepted.as_deref());
                let graded = active
                    .run
                    .judged(
                        store,
                        &settings,
                        &exercise_id,
                        grade,
                        judgement.tags.clone(),
                        clock,
                    )
                    .map_err(run_error)?;
                // A store that cannot be written loses only the saving; the verdict stands.
                let _ = apply_judgement(store, &request, &judgement, client.kind(), clock.now_ms());
                if let Some(charge) = judgement.charge_micros {
                    let _ = store.add_session_cost(&session_id, charge);
                }
                Ok(graded)
            }
            Err(AiError::NeedsConsent) if automatic => Ok(current),
            Err(AiError::NeedsConsent) => Err(needs_consent()),
            Err(error) => {
                if matches!(error, AiError::NeedsCredit { .. }) {
                    active.judging_stopped = true;
                }
                active
                    .run
                    .judge_failed(&exercise_id, judge_error(error))
                    .map_err(run_error)
            }
        }
    }

    /// judge_begin, then the call and judge_complete when there is one to make.
    fn judge_now(
        &mut self,
        session_id: &str,
        exercise_id: &str,
        ai: Option<AiAccess>,
        automatic: bool,
    ) -> Result<Grade, AppError> {
        match self.judge_begin(session_id, exercise_id, ai, automatic)? {
            JudgeBegin::Done(grade) => Ok(grade),
            JudgeBegin::Pending(job) => {
                let outcome = job.run();
                self.judge_complete(job, outcome)
            }
        }
    }

    /// exercise_submit, then (for an unchecked grade) judge_begin(.., true), run and complete, in
    /// one go (the CLI and tests). `exercise_submit` itself never calls the judge.
    pub fn exercise_submit_with(
        &mut self,
        session_id: &str,
        exercise_id: &str,
        answer: &Answer,
        response_ms: Option<i64>,
        used_hint: bool,
        ai: Option<AiAccess>,
    ) -> Result<Grade, AppError> {
        let grade =
            self.exercise_submit(session_id, exercise_id, answer, response_ms, used_hint)?;
        if grade.verdict != Verdict::Unchecked || !self.runs.contains_key(session_id) {
            return Ok(grade);
        }
        self.judge_now(session_id, exercise_id, ai, true)
    }

    /// Check with AI and Try again in one go: judge_begin(.., false), run, complete.
    pub fn exercise_check_with(
        &mut self,
        session_id: &str,
        exercise_id: &str,
        ai: Option<AiAccess>,
    ) -> Result<Grade, AppError> {
        self.judge_now(session_id, exercise_id, ai, false)
    }

    /// Explain my answer: refuses an unknown session or exercise (`not_found`), an answer that is
    /// not `almost` or `wrong`, not yet given, or to `match_pairs` (`invalid_input`); no provider
    /// chosen or no consent is `needs_consent`; a client that could not be made is its error
    /// (offline: "Explanations need a connection or a local model.").
    pub fn explain_begin(
        &mut self,
        session_id: &str,
        exercise_id: &str,
        ai: Option<AiAccess>,
    ) -> Result<ExplainJob, AppError> {
        let active = self.runs.get(session_id).ok_or_else(not_available)?;
        let state = active.run.state(exercise_id).map_err(run_error)?;
        let ex = state.exercise;
        let Some((answer, grade, judge_tags)) = state.answered else {
            return Err(not_explainable());
        };
        if !matches!(grade.verdict, Verdict::Almost | Verdict::Wrong) {
            return Err(not_explainable());
        }
        let (Some((prompt, expected)), Some(answer), Some(verdict)) = (
            prompt_and_expected(ex),
            answer_text(ex, answer),
            grade.verdict.srs(),
        ) else {
            return Err(not_explainable());
        };
        let tags: Vec<String> = if judge_tags.is_empty() {
            grade
                .tags
                .iter()
                .map(|tag| tag.as_str().to_string())
                .collect()
        } else {
            judge_tags.to_vec()
        };
        let (lang, kind, item_id) = (active.lang, ex.kind(), ex.item_id.clone());

        let client = match ai {
            None | Some(Err(AiError::NeedsConsent)) => return Err(needs_consent()),
            Some(Err(error)) => return Err(explain_error(error)),
            Some(Ok(client)) => {
                if client.status().needs_consent {
                    return Err(needs_consent());
                }
                client
            }
        };
        let (cefr, grammar_note) = self.level_and_note(lang, &item_id);
        Ok(ExplainJob {
            session_id: session_id.to_string(),
            client,
            cache_path: self.config.data_dir.join(super::AI_CACHE_FILE),
            request: ExplainRequest {
                lang,
                cefr,
                exercise_type: kind,
                prompt,
                expected,
                answer,
                verdict,
                tags,
                grammar_note,
            },
            now_ms: self.env.clock.now_ms(),
        })
    }

    /// The explanation, its charge added to the session's cost; or the call's error.
    pub fn explain_complete(
        &mut self,
        job: ExplainJob,
        outcome: ExplainOutcome,
    ) -> Result<Explained, AppError> {
        let explanation = outcome.0.map_err(explain_error)?;
        if let Some(charge) = explanation.charge_micros
            && let Ok(store) = open_store(&mut self.store)
        {
            let _ = store.add_session_cost(&job.session_id, charge);
        }
        Ok(Explained {
            text: explanation.text,
            tip: explanation.tip,
            cached: explanation.cached,
        })
    }

    /// explain_begin, run and complete in one go.
    pub fn explain_answer_with(
        &mut self,
        session_id: &str,
        exercise_id: &str,
        ai: Option<AiAccess>,
    ) -> Result<Explained, AppError> {
        let job = self.explain_begin(session_id, exercise_id, ai)?;
        let outcome = job.run();
        self.explain_complete(job, outcome)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::ActiveRun;
    use super::super::test_support::parley;
    use super::*;
    use crate::ai::AiStatus;
    use crate::ai::test_client::Scripted;
    use crate::exercise::{LessonRun, Origin};
    use crate::grading::Direction;
    use crate::srs::{Facet, Rating};
    use crate::store::AnswerSource;

    const SESSION: &str = "s-0000000000000001";
    const SKILL: &str = "es.a1.u01.s01";

    fn exercise(id: &str, item: &str, body: Body) -> Exercise {
        Exercise {
            id: id.into(),
            item_id: format!("{SKILL}#{item}"),
            facet: body.kind().facet(),
            origin: Origin::Template,
            body,
        }
    }

    /// A run of four exercises: a translation of the item's own prompt (e1), one of another
    /// prompt (e2), a gap (e3) and pairs (e4).
    fn app_with_run(dir: &std::path::Path) -> Parley {
        let mut app = parley(dir);
        app.course_start("es").unwrap();
        let translate = |prompt: &str| Body::TranslateToTarget {
            prompt_en: prompt.into(),
            target: "Buenos días.".into(),
            accepted: vec!["Buenos días.".into()],
            tiles: None,
        };
        let lesson = crate::exercise::Lesson {
            schema: crate::exercise::Lesson::SCHEMA.into(),
            lang: Lang::Es,
            skill_id: SKILL.into(),
            seq: 1,
            origin: Origin::Template,
            exercises: vec![
                exercise("e1", "e:2", translate("Good morning.")),
                exercise("e2", "e:2", translate("Morning!")),
                exercise(
                    "e3",
                    "l:día",
                    Body::FillGap {
                        sentence: "Hola, buenos ___.".into(),
                        answer: "días".into(),
                        full_sentence: "Hola, buenos días.".into(),
                        hint_en: "day".into(),
                    },
                ),
                exercise(
                    "e4",
                    "l:hola",
                    Body::MatchPairs {
                        pairs: (0..4).map(|i| [format!("t{i}"), format!("e{i}")]).collect(),
                    },
                ),
            ],
            ai: None,
        };
        let store = open_store(&mut app.store).unwrap();
        let run = LessonRun::start_lesson(store, &lesson, SESSION, &*app.env.clock).unwrap();
        app.runs.insert(
            SESSION.into(),
            ActiveRun {
                lang: Lang::Es,
                run,
                skill_id: None,
                judging_stopped: false,
            },
        );
        app
    }

    fn text(s: &str) -> Answer {
        Answer::Text(s.into())
    }

    fn client(replies: Vec<Result<crate::ai::ChatReply, AiError>>) -> Arc<Scripted> {
        Arc::new(Scripted::new(replies))
    }

    fn access(client: &Arc<Scripted>) -> Option<AiAccess> {
        let client: Arc<dyn AiClient> = client.clone();
        Some(Ok(client))
    }

    fn with_status(client: &Arc<Scripted>, status: AiStatus) -> Option<AiAccess> {
        *client.status.lock().unwrap() = status;
        access(client)
    }

    const CORRECT: &str = r#"{"verdict": "correct", "corrected": "Buen día", "explanation": "Also a greeting.", "error_tags": []}"#;
    const WRONG: &str = r#"{"verdict": "wrong", "corrected": "Buenas noches", "explanation": "This greets the night.", "error_tags": ["meaning"]}"#;

    fn submit(app: &mut Parley, id: &str, answer: &str) -> Grade {
        app.exercise_submit(SESSION, id, &text(answer), Some(4_000), false)
            .unwrap()
    }

    #[test]
    fn judge_begin_refuses_what_it_cannot_judge() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_run(dir.path());
        let message = |r: Result<JudgeBegin, AppError>| match r {
            Err(e) => (e.code, e.message),
            Ok(_) => panic!("refused"),
        };
        let gone = (
            ErrorCode::NotFound,
            "This exercise is no longer available.".to_string(),
        );
        assert_eq!(message(app.judge_begin("s-9", "e1", None, false)), gone);
        assert_eq!(message(app.judge_begin(SESSION, "e9", None, false)), gone);
        assert_eq!(
            message(app.judge_begin(SESSION, "e1", None, false)),
            (ErrorCode::InvalidInput, "Type an answer first.".to_string())
        );
        submit(&mut app, "e3", "días");
        assert_eq!(
            message(app.judge_begin(SESSION, "e3", None, false)),
            (
                ErrorCode::InvalidInput,
                "This exercise was already checked.".to_string()
            )
        );
    }

    #[test]
    fn automatic_judging_follows_the_table() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_run(dir.path());
        let unchecked = submit(&mut app, "e1", "Buen día");
        assert_eq!(unchecked.verdict, Verdict::Unchecked);
        let done = |r: Result<JudgeBegin, AppError>| match r.unwrap() {
            JudgeBegin::Done(grade) => grade,
            JudgeBegin::Pending(_) => panic!("no call"),
        };
        assert_eq!(done(app.judge_begin(SESSION, "e1", None, true)), unchecked);
        assert_eq!(
            done(app.judge_begin(SESSION, "e1", Some(Err(AiError::NeedsConsent)), true)),
            unchecked
        );
        let c = client(vec![]);
        let consent = AiStatus {
            needs_consent: true,
            ..AiStatus::default()
        };
        assert_eq!(
            done(app.judge_begin(SESSION, "e1", with_status(&c, consent), true)),
            unchecked
        );
        let credit = AiStatus {
            needs_credit: true,
            ..AiStatus::default()
        };
        assert_eq!(
            done(app.judge_begin(SESSION, "e1", with_status(&c, credit.clone()), true)),
            unchecked
        );
        assert_eq!(c.requests(), 0);
        let failed = done(app.judge_begin(SESSION, "e1", Some(Err(AiError::Disconnected)), true));
        assert_eq!(
            failed.judge_error.as_ref().map(|e| e.code),
            Some(ErrorCode::Disconnected)
        );
        assert_eq!(failed.verdict, Verdict::Unchecked);
        // Learner-triggered: a client with needs-credit is still asked.
        let JudgeBegin::Pending(job) = app
            .judge_begin(SESSION, "e1", with_status(&c, credit), false)
            .unwrap()
        else {
            panic!("a learner-triggered call goes out")
        };
        assert_eq!(
            (job.request().source.as_str(), job.request().own_prompt),
            ("Good morning.", true)
        );
        assert_eq!(job.request().cefr, Cefr::A1);
        assert_eq!(job.request().accepted, ["Buenos días."]);
        for refused in [None, Some(Err(AiError::NeedsConsent))] {
            let error = app
                .judge_begin(SESSION, "e1", refused, false)
                .err()
                .unwrap();
            assert_eq!(error.code, ErrorCode::NeedsConsent);
            assert_eq!(error.message, "AI features are not set up yet.");
        }
    }

    #[test]
    fn a_correct_verdict_reviews_stores_and_charges() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_run(dir.path());
        let c = client(vec![Scripted::reply(CORRECT, Some(150))]);
        let grade = app
            .exercise_submit_with(
                SESSION,
                "e1",
                &text("Buen día"),
                Some(4_000),
                true,
                access(&c),
            )
            .unwrap();
        assert_eq!(grade.verdict, Verdict::Correct);
        assert_eq!(grade.rating, Some(Rating::Hard), "rated with the hint");
        assert_eq!(grade.explanation.as_deref(), Some("Also a greeting."));
        let store = app.store.as_ref().unwrap();
        let log = store.review_log().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(
            (log[0].rating, log[0].response_ms),
            (Rating::Hard, Some(4_000))
        );
        let item = format!("{SKILL}#e:2");
        let judged: Vec<_> = store
            .accepted_answers(&item, Direction::ToTarget)
            .unwrap()
            .into_iter()
            .filter(|a| a.source == AnswerSource::Judge)
            .collect();
        assert_eq!(judged.len(), 1);
        let tables = store.dump().unwrap();
        assert_eq!(tables.judgements.len(), 1);
        let session = tables.sessions.iter().find(|s| s.id == SESSION).unwrap();
        assert_eq!(session.cost_micros, 150);
        assert!(store.card(&item, Facet::Productive).unwrap().is_some());
        // The same answer to the other prompt is judged; its verdict is stored, not accepted.
        let c = client(vec![Scripted::reply(WRONG, None)]);
        let wrong = app
            .exercise_submit_with(
                SESSION,
                "e2",
                &text("Buenas noches"),
                None,
                false,
                access(&c),
            )
            .unwrap();
        assert_eq!(
            (wrong.verdict, wrong.rating),
            (Verdict::Wrong, Some(Rating::Again))
        );
        assert_eq!(wrong.expected.as_deref(), Some("Buenos días."));
        assert_eq!(wrong.corrected.as_deref(), Some("Buenas noches"));
        // Explain gets the judge's tags.
        let job = app.explain_begin(SESSION, "e2", access(&c)).unwrap();
        assert_eq!(job.request().tags, ["meaning"]);
        assert_eq!(job.request().expected, "Buenos días.");
        assert_eq!(job.request().verdict, crate::srs::Verdict::Wrong);
    }

    #[test]
    fn a_402_stops_automatic_judging_for_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_run(dir.path());
        let credit = AiError::NeedsCredit {
            message: "Add credit to keep going.".into(),
            top_up_url: "https://example.invalid/top-up".into(),
        };
        let c = client(vec![Err(credit)]);
        let grade = app
            .exercise_submit_with(SESSION, "e1", &text("Buen día"), None, false, access(&c))
            .unwrap();
        assert_eq!(grade.verdict, Verdict::Unchecked);
        let error = grade.judge_error.unwrap();
        assert_eq!(error.code, ErrorCode::NeedsCredit);
        assert_eq!(
            error.action,
            Some(serde_json::json!({"top_up_url": "https://example.invalid/top-up"}))
        );
        let next = app
            .exercise_submit_with(SESSION, "e2", &text("Buen día"), None, false, access(&c))
            .unwrap();
        assert_eq!((next.verdict, next.judge_error), (Verdict::Unchecked, None));
        assert_eq!(c.requests(), 1, "no further automatic call");
        // I was right still works on the failed answer.
        let right = app.exercise_i_was_right(SESSION, "e1").unwrap();
        assert_eq!((right.verdict, right.judge_error), (Verdict::Correct, None));
    }

    #[test]
    fn offline_and_a_late_answer_change_nothing_stored() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_run(dir.path());
        let c = client(vec![Err(AiError::Offline)]);
        let grade = app
            .exercise_submit_with(SESSION, "e1", &text("Buen día"), None, false, access(&c))
            .unwrap();
        let error = grade.judge_error.unwrap();
        assert_eq!(
            (error.code, error.message.as_str()),
            (ErrorCode::Offline, JUDGE_OFFLINE)
        );
        // Try again, answered after I was right: nothing changes.
        let c = client(vec![Scripted::reply(WRONG, Some(150))]);
        let JudgeBegin::Pending(job) = app.judge_begin(SESSION, "e1", access(&c), false).unwrap()
        else {
            panic!("a call")
        };
        let outcome = job.run();
        app.exercise_i_was_right(SESSION, "e1").unwrap();
        let grade = app.judge_complete(job, outcome).unwrap();
        assert_eq!(grade.verdict, Verdict::Correct);
        let tables = app.store.as_ref().unwrap().dump().unwrap();
        assert!(tables.judgements.is_empty());
        assert_eq!(tables.review_log.len(), 1);
        assert_eq!(tables.sessions[0].cost_micros, 0);
    }

    #[test]
    fn a_timeout_waits_for_try_again_with_the_same_key() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_run(dir.path());
        let c = client(vec![
            Err(AiError::Timeout {
                provider: crate::ai::ProviderKind::Publik,
            }),
            Scripted::reply(CORRECT, None),
        ]);
        let grade = app
            .exercise_submit_with(SESSION, "e1", &text("Buen día"), None, false, access(&c))
            .unwrap();
        let error = grade.judge_error.unwrap();
        assert_eq!(
            (error.code, error.message.as_str()),
            (ErrorCode::Busy, "publik API did not answer in time.")
        );
        assert_eq!(c.requests(), 1, "no automatic retry");
        let checked = app.exercise_check_with(SESSION, "e1", access(&c)).unwrap();
        assert_eq!(
            (checked.verdict, checked.judge_error),
            (Verdict::Correct, None)
        );
        let keys = c.keys();
        assert_eq!(keys.len(), 2);
        assert_eq!(
            keys[0], keys[1],
            "Try again after a timeout sends the same key"
        );
        assert_eq!(c.seen.lock().unwrap()[0].timeout_s, 10);
    }

    #[test]
    fn explain_is_offered_after_almost_or_wrong_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_run(dir.path());
        let c = client(vec![Scripted::reply(
            r#"{"explanation": "Días is the plural of día, and the greeting uses the plural.", "tip": "Say buenos días."}"#,
            Some(220),
        )]);
        let refused = |r: Result<ExplainJob, AppError>| r.err().unwrap();
        assert_eq!(
            refused(app.explain_begin("s-9", "e3", access(&c))).code,
            ErrorCode::NotFound
        );
        let error = refused(app.explain_begin(SESSION, "e3", access(&c)));
        assert_eq!(
            (error.code, error.message.as_str()),
            (ErrorCode::InvalidInput, EXPLAIN_NOT_AVAILABLE)
        );
        let crossed = Answer::Pairs(vec![[0, 1], [1, 0], [2, 2], [3, 3]]);
        let pairs = app
            .exercise_submit(SESSION, "e4", &crossed, None, false)
            .unwrap();
        assert_eq!(pairs.verdict, Verdict::Wrong);
        assert_eq!(
            refused(app.explain_begin(SESSION, "e4", access(&c))).code,
            ErrorCode::InvalidInput
        );
        let gap = submit(&mut app, "e3", "noches");
        assert_eq!(gap.verdict, Verdict::Wrong);
        assert_eq!(
            refused(app.explain_begin(SESSION, "e3", None)).code,
            ErrorCode::NeedsConsent
        );
        let offline = refused(app.explain_begin(SESSION, "e3", Some(Err(AiError::Offline))));
        assert_eq!(
            (offline.code, offline.message.as_str()),
            (ErrorCode::Offline, EXPLAIN_OFFLINE)
        );
        let first = app.explain_answer_with(SESSION, "e3", access(&c)).unwrap();
        assert_eq!(first.tip.as_deref(), Some("Say buenos días."));
        assert!(!first.cached);
        let job = app.explain_begin(SESSION, "e3", access(&c)).unwrap();
        assert_eq!(
            (
                job.request().prompt.as_str(),
                job.request().expected.as_str(),
                job.request().answer.as_str()
            ),
            ("Hola, buenos ___.", "días", "noches")
        );
        let again = app.explain_answer_with(SESSION, "e3", access(&c)).unwrap();
        assert!(again.cached);
        assert_eq!(c.requests(), 1);
        let tables = app.store.as_ref().unwrap().dump().unwrap();
        assert_eq!(tables.sessions[0].cost_micros, 220);
        let correct = submit(&mut app, "e1", "Buenos días");
        assert_eq!(correct.verdict, Verdict::Correct);
        assert_eq!(
            refused(app.explain_begin(SESSION, "e1", access(&c))).message,
            EXPLAIN_NOT_AVAILABLE
        );
    }
}
