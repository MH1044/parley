//! Lessons and reviews: `lesson_start`, `lesson_use_course_pack`, `review_start`,
//! `exercise_submit`, `exercise_i_was_right` and `lesson_finish`; course packs and the local
//! model's validity rate (docs/modules/07-lesson-generation.md, Commands and events); the AI judge
//! of an unchecked answer, `exercise_check` and `explain_answer`
//! (docs/modules/08-judging-and-explanations.md, Commands and events).
//!
//! A fresh lesson or a story starts in three steps so the AI call holds no lock: the lesson is
//! begun with the service locked, the call runs on a blocking thread with nothing locked, and the
//! lesson is completed with the service locked again. Meanwhile **Use the course pack instead**
//! can start the template lesson, and the late answer is only cached. A client is asked for only
//! when a provider is chosen (`ai.provider` is not `unset`), so nothing reaches any provider
//! before the learner has chosen one (invariants I9 and I12).
//!
//! An answer is graded offline with the service locked; only an `unchecked` translation asks for a
//! client, and the judge's call runs on a blocking thread with nothing locked, between the begin
//! and the completion, as a fresh lesson's does. Explain works the same way.

use std::path::PathBuf;

use parley_core::ai::LessonChecks;
use parley_core::api::{AI_STATUS_EVENT, LessonStarted};
use parley_core::app::{AiAccess, Explained, JudgeBegin, LessonBegin};
use parley_core::coursepack::{CoursePackReport, CoursePackSummary};
use parley_core::error::AppError;
use parley_core::exercise::LessonSummary;
use parley_core::grading::{Answer, Grade, Verdict};
use tauri::{AppHandle, Emitter, Manager, State};

use super::ai::unfinished;
use crate::state::AppState;

/// The chosen provider's client for a lesson of `mode`, or None when none may be asked for: no
/// provider chosen, review mode, or a learn lesson with AI lessons off. The AI service is locked
/// only while the client is built; the settings are read before, with the service unlocked.
fn ai_access(state: &AppState, mode: &str) -> Option<AiAccess> {
    let ai = state.with(|p| p.settings_get().ai);
    let wanted = ai.provider != "unset"
        && match mode {
            "learn" => ai.fresh_lessons,
            "story" => true,
            _ => false,
        };
    wanted.then(|| state.with_ai(|service| service.client(&ai)))
}

/// The chosen provider's client for judging or explaining an answer, or None when no provider is
/// chosen. The settings are read first; the AI service is locked only while the client is built.
fn answer_access(state: &AppState) -> Option<AiAccess> {
    let ai = state.with(|p| p.settings_get().ai);
    (ai.provider != "unset").then(|| state.with_ai(|service| service.client(&ai)))
}

/// Judges an unchecked answer: begun with the service locked, the call made with nothing locked,
/// completed with the service locked again; the status is emitted after a call.
fn judge(
    app: &AppHandle,
    state: &AppState,
    session_id: &str,
    exercise_id: &str,
    automatic: bool,
) -> Result<Grade, AppError> {
    let ai = answer_access(state);
    match state.with(|p| p.judge_begin(session_id, exercise_id, ai, automatic))? {
        JudgeBegin::Done(grade) => Ok(grade),
        JudgeBegin::Pending(job) => {
            let outcome = job.run();
            let grade = state.with(|p| p.judge_complete(job, outcome));
            emit_status(app, state);
            grade
        }
    }
}

/// Emits the AI status after a call, so a 402 or a reconnect shows on every view at once.
pub(crate) fn emit_status(app: &AppHandle, state: &AppState) {
    let ai = state.with(|p| p.settings_get().ai);
    let status = state.with_ai(|service| service.status(&ai, false));
    // A window that cannot hear the event still gets the command's result.
    let _ = app.emit(AI_STATUS_EVENT, &status);
}

#[tauri::command(rename_all = "snake_case")]
pub async fn lesson_start(
    app: AppHandle,
    skill_id: String,
    mode: String,
) -> Result<LessonStarted, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let ai = ai_access(&state, &mode);
        match state.with(|p| p.lesson_begin(&skill_id, &mode, ai))? {
            LessonBegin::Started(started) => Ok(started),
            LessonBegin::Pending(job) => {
                let outcome = job.run();
                let started = state.with(|p| p.lesson_complete(job, outcome));
                emit_status(&app, &state);
                started
            }
        }
    })
    .await
    .map_err(|_| unfinished())?
}

#[tauri::command(rename_all = "snake_case")]
pub fn lesson_use_course_pack(
    state: State<'_, AppState>,
    skill_id: String,
) -> Result<LessonStarted, AppError> {
    state.with(|p| p.lesson_use_course_pack(&skill_id))
}

#[tauri::command(rename_all = "snake_case")]
pub fn review_start(
    state: State<'_, AppState>,
    lang: String,
    limit: usize,
) -> Result<LessonStarted, AppError> {
    state.with(|p| p.review_start(&lang, limit))
}

/// Grades an answer offline; an `unchecked` translation then goes to the judge when a provider is
/// chosen and ready (module 08).
#[tauri::command(rename_all = "snake_case")]
pub async fn exercise_submit(
    app: AppHandle,
    session_id: String,
    exercise_id: String,
    answer: Answer,
    response_ms: Option<i64>,
    used_hint: bool,
) -> Result<Grade, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let grade = state.with(|p| {
            p.exercise_submit(&session_id, &exercise_id, &answer, response_ms, used_hint)
        })?;
        if grade.verdict != Verdict::Unchecked {
            return Ok(grade);
        }
        judge(&app, &state, &session_id, &exercise_id, true)
    })
    .await
    .map_err(|_| unfinished())?
}

/// **Check with AI** and **Try again** on an unchecked answer.
#[tauri::command(rename_all = "snake_case")]
pub async fn exercise_check(
    app: AppHandle,
    session_id: String,
    exercise_id: String,
) -> Result<Grade, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        judge(&app, &state, &session_id, &exercise_id, false)
    })
    .await
    .map_err(|_| unfinished())?
}

/// **Explain** after an answer that was almost right or wrong.
#[tauri::command(rename_all = "snake_case")]
pub async fn explain_answer(
    app: AppHandle,
    session_id: String,
    exercise_id: String,
) -> Result<Explained, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let ai = answer_access(&state);
        let job = state.with(|p| p.explain_begin(&session_id, &exercise_id, ai))?;
        let outcome = job.run();
        let explained = state.with(|p| p.explain_complete(job, outcome));
        emit_status(&app, &state);
        explained
    })
    .await
    .map_err(|_| unfinished())?
}

#[tauri::command(rename_all = "snake_case")]
pub fn exercise_i_was_right(
    state: State<'_, AppState>,
    session_id: String,
    exercise_id: String,
) -> Result<Grade, AppError> {
    state.with(|p| p.exercise_i_was_right(&session_id, &exercise_id))
}

/// Finishes a run; after a learn lesson, the next lesson of the skill is prepared in the
/// background (one P1 call whose answer is only cached), when the settings and the provider allow.
#[tauri::command(rename_all = "snake_case")]
pub fn lesson_finish(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<LessonSummary, AppError> {
    let skill = state.with(|p| p.run_skill(&session_id));
    let summary = state.with(|p| p.lesson_finish(&session_id))?;
    if let Some(skill) = skill {
        tauri::async_runtime::spawn_blocking(move || {
            let state = app.state::<AppState>();
            let ai = ai_access(&state, "learn");
            if let Ok(Some(job)) = state.with(|p| p.next_lesson_job(&skill, ai)) {
                let _ = job.run();
            }
        });
    }
    Ok(summary)
}

#[tauri::command(rename_all = "snake_case")]
pub fn course_pack_export(
    state: State<'_, AppState>,
    lang: String,
    dest_path: String,
) -> Result<CoursePackSummary, AppError> {
    state.with(|p| p.course_pack_export(&lang, &PathBuf::from(dest_path)))
}

#[tauri::command(rename_all = "snake_case")]
pub fn course_pack_import(
    state: State<'_, AppState>,
    src_path: String,
) -> Result<CoursePackReport, AppError> {
    state.with(|p| p.course_pack_import(&PathBuf::from(src_path)))
}

#[tauri::command(rename_all = "snake_case")]
pub fn ai_lesson_checks(state: State<'_, AppState>) -> Result<LessonChecks, AppError> {
    state.with(|p| p.ai_lesson_checks())
}
