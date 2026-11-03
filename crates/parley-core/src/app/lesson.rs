//! Lessons and reviews: `lesson_start`, `review_start`, `exercise_submit`,
//! `exercise_i_was_right` and `lesson_finish` (docs/modules/04-app-shell-and-offline-course.md,
//! Design notes). Runs stay in memory until finished; nothing limits how many a learner does
//! (invariant I8).

use std::collections::HashSet;

use super::pack::{attributions, pack_sentences};
use super::{
    ActiveRun, Course, Parley, no_such_skill, not_available, not_started, open_store, session_id,
    unknown_value,
};
use crate::api::{LessonStarted, NOTHING_TO_REVIEW_YET, next_review_notice};
use crate::content::{Example, Lang};
use crate::error::{AppError, ErrorCode};
use crate::exercise::compose::kind_for;
use crate::exercise::{
    Allowed, Capabilities, ComposeRequest, Lesson, LessonRun, LessonSummary, Origin, Plan,
    PlanEntry, RunError, RunSettings, Submission, TemplateCtx, compose, template_lesson,
};
use crate::grading::{Answer, Grade};
use crate::settings::Settings;
use crate::srs::due_queue;
use crate::store::{Store, StoreError};

/// The settings a run grades and schedules with.
fn run_settings(settings: &Settings) -> RunSettings {
    RunSettings {
        accent: settings.accent(),
        daily_goal_xp: settings.daily_goal_xp,
        srs: settings.srs(),
    }
}

/// The runner's refusals with their messages; a store failure is the data-file error.
fn run_error(e: RunError) -> AppError {
    match e {
        RunError::EmptyAnswer => AppError::new(ErrorCode::InvalidInput, "Type an answer first."),
        RunError::AlreadyChecked => AppError::new(
            ErrorCode::InvalidInput,
            "This exercise was already checked.",
        ),
        RunError::NotAvailable => not_available(),
        RunError::Store(e) => e.into(),
    }
}

/// The template lesson of a plan in the context of `skill_id`, with the installed pack's
/// sentences for the plan's lemmas as extra `fill_gap` candidates; and their attributions.
fn build_lesson(
    parley_dir: &std::path::Path,
    store: &Store,
    course: &Course,
    skill_id: &str,
    plan: &Plan,
    seq: u32,
    rng: &mut dyn crate::env::Rng,
) -> (Lesson, Vec<crate::api::Attribution>) {
    let plan_items: Vec<String> = plan.entries.iter().map(|e| e.item_id.clone()).collect();
    let mut lemmas: Vec<String> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for id in &plan_items {
        if let Some(item) = course.items.iter().find(|item| item.id == *id) {
            for lemma in &item.lemmas {
                if seen.insert(lemma.as_str()) {
                    lemmas.push(lemma.clone());
                }
            }
        }
    }
    let pack = pack_sentences(parley_dir, store, course.syllabus.lang, &lemmas);
    let sentences: Vec<Example> = pack
        .iter()
        .map(|sentence| Example {
            id: sentence.id,
            target: sentence.text.clone(),
            en: sentence.en.clone(),
        })
        .collect();
    let allowed = Allowed::for_skill(&course.syllabus, &course.numbers, skill_id);
    let ctx = TemplateCtx {
        syllabus: &course.syllabus,
        forms: &course.forms,
        items: &course.items,
        allowed: &allowed,
        sentences: &sentences,
        skill_id,
        plan_items: &plan_items,
    };
    let lesson = template_lesson(plan, &ctx, seq, rng);
    let credits = attributions(&lesson, &pack, &course.syllabus);
    (lesson, credits)
}

impl Parley {
    /// Starts a lesson of a skill: `learn` introduces new items, `review` none. Composed with the
    /// settings' lesson length and new-item count, built from the template, stored, and run.
    pub fn lesson_start(&mut self, skill_id: &str, mode: &str) -> Result<LessonStarted, AppError> {
        let learn = match mode {
            "learn" => true,
            "review" => false,
            other => return Err(unknown_value(other, "mode")),
        };
        let store = open_store(&mut self.store)?;
        let info = match store.skill_info(skill_id) {
            Ok(info) if info.ord >= 0 => info,
            Ok(_) | Err(StoreError::UnknownSkill(_)) => return Err(no_such_skill()),
            Err(e) => return Err(e.into()),
        };
        let lang: Lang = info.lang.parse().map_err(|_| no_such_skill())?;
        let course = self.content.get(&lang).ok_or_else(no_such_skill)?;
        let clock = &*self.env.clock;
        let now = clock.now_ms();
        let request = ComposeRequest {
            lang,
            skill_id: skill_id.to_string(),
            lesson_length: self.settings.lesson_length as usize,
            new_items_per_lesson: if learn {
                self.settings.new_items_per_lesson as usize
            } else {
                0
            },
            capabilities: Capabilities::default(),
        };
        let plan = compose(&request, store, now)?;
        let seq = store.next_lesson_seq(skill_id)?;
        let rng = &mut *self.env.rng;
        let (lesson, credits) = build_lesson(
            &self.config.data_dir,
            store,
            course,
            skill_id,
            &plan,
            seq,
            rng,
        );
        let id = session_id("s", rng);
        let run = LessonRun::start_lesson(store, &lesson, &id, clock)?;
        self.runs.insert(id.clone(), ActiveRun { lang, run });
        Ok(LessonStarted {
            session_id: id,
            kind: "lesson".into(),
            skill_id: Some(skill_id.to_string()),
            origin: lesson.origin,
            exercises: lesson.exercises,
            attributions: credits,
            notice: None,
        })
    }

    /// Starts a review of up to `limit` due cards, one exercise per card by the composer's kind
    /// rules, in the context of the latest skill among them. With nothing due it starts no
    /// session and says when the next review is.
    pub fn review_start(&mut self, lang: &str, limit: usize) -> Result<LessonStarted, AppError> {
        let lang = self.lang(lang)?;
        let store = open_store(&mut self.store)?;
        if store.course(lang)?.is_none() {
            return Err(not_started());
        }
        let course = &self.content[&lang];
        let clock = &*self.env.clock;
        let now = clock.now_ms();
        let queue = due_queue(store, lang, now, limit)?;
        if queue.is_empty() {
            let notice = match store.next_due_after(lang, now)? {
                Some(due) => next_review_notice(due, now, clock.utc_offset_minutes()),
                None => NOTHING_TO_REVIEW_YET.to_string(),
            };
            return Ok(LessonStarted {
                session_id: String::new(),
                kind: "review".into(),
                skill_id: None,
                origin: Origin::Template,
                exercises: Vec::new(),
                attributions: Vec::new(),
                notice: Some(notice),
            });
        }
        let mut entries = Vec::new();
        let mut latest: Option<(i64, String)> = None;
        for (index, card) in queue.iter().enumerate() {
            let item = store.item(&card.item_id)?;
            if let Ok(info) = store.skill_info(&item.skill_id)
                && info.ord >= 0
                && latest.as_ref().is_none_or(|(ord, _)| info.ord > *ord)
            {
                latest = Some((info.ord, info.id));
            }
            entries.push(PlanEntry {
                n: index as u32 + 1,
                item_id: item.id.clone(),
                facet: card.facet,
                kind: kind_for(&item, card.facet),
                focus: item.target.clone(),
                new: false,
            });
        }
        let skill_id = latest.map(|(_, id)| id).unwrap_or_default();
        let plan = Plan { entries };
        let rng = &mut *self.env.rng;
        let (lesson, credits) = build_lesson(
            &self.config.data_dir,
            store,
            course,
            &skill_id,
            &plan,
            0,
            rng,
        );
        let id = session_id("s", rng);
        let run = LessonRun::start_review(store, lang, lesson.exercises.clone(), &id, clock)?;
        self.runs.insert(id.clone(), ActiveRun { lang, run });
        Ok(LessonStarted {
            session_id: id,
            kind: "review".into(),
            skill_id: None,
            origin: Origin::Template,
            exercises: lesson.exercises,
            attributions: credits,
            notice: None,
        })
    }

    /// Grades one answer of a run in progress (module 03's grade) and reviews its card.
    pub fn exercise_submit(
        &mut self,
        session_id: &str,
        exercise_id: &str,
        answer: &Answer,
        response_ms: Option<i64>,
        used_hint: bool,
    ) -> Result<Grade, AppError> {
        let store = open_store(&mut self.store)?;
        let active = self.runs.get_mut(session_id).ok_or_else(not_available)?;
        let course = self.content.get(&active.lang).ok_or_else(not_available)?;
        let submission = Submission {
            exercise_id: exercise_id.to_string(),
            answer: answer.clone(),
            response_ms,
            used_hint,
        };
        active
            .run
            .submit(
                store,
                &course.forms,
                &run_settings(&self.settings),
                &submission,
                &*self.env.clock,
            )
            .map_err(run_error)
    }

    /// "I was right" on an unchecked answer.
    pub fn exercise_i_was_right(
        &mut self,
        session_id: &str,
        exercise_id: &str,
    ) -> Result<Grade, AppError> {
        let store = open_store(&mut self.store)?;
        let active = self.runs.get_mut(session_id).ok_or_else(not_available)?;
        active
            .run
            .i_was_right(
                store,
                &run_settings(&self.settings),
                exercise_id,
                &*self.env.clock,
            )
            .map_err(run_error)
    }

    /// Finishes a run: XP, the day's credit, the streak and the weekly record (module 03).
    pub fn lesson_finish(&mut self, session_id: &str) -> Result<LessonSummary, AppError> {
        let store = open_store(&mut self.store)?;
        let active = self.runs.remove(session_id).ok_or_else(not_available)?;
        active
            .run
            .finish(store, &run_settings(&self.settings), &*self.env.clock)
            .map_err(run_error)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI64, Ordering};

    use super::super::test_support::{MONDAY_NOON, NO_PACKS, parley, parley_at};
    use crate::api::NOTHING_TO_REVIEW_YET;
    use crate::env::FailingNet;
    use crate::error::ErrorCode;
    use crate::exercise::{Body, ExerciseType};
    use crate::grading::{Answer, Verdict};
    use crate::srs::DAY_MS;

    #[test]
    fn a_lesson_runs_offline_and_refuses_with_the_run_messages() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = parley(dir.path());
        app.course_start("es").unwrap();
        assert_eq!(
            app.lesson_start("es.a1.u01.s01", "story")
                .unwrap_err()
                .message,
            "Unknown value story for mode."
        );
        assert_eq!(
            app.lesson_start("es.a1.u09.s09", "learn")
                .unwrap_err()
                .message,
            "This skill is not in the course."
        );
        let started = app.lesson_start("es.a1.u01.s01", "learn").unwrap();
        assert!(started.session_id.starts_with("s-"));
        assert_eq!(started.session_id.len(), 18);
        assert_eq!(started.kind, "lesson");
        assert!(started.attributions.is_empty(), "no pack, no attributions");
        let pick = started
            .exercises
            .iter()
            .find(|ex| ex.kind() == ExerciseType::PickMeaning)
            .unwrap();
        let Body::PickMeaning { answer_index, .. } = pick.body else {
            unreachable!()
        };
        let grade = app
            .exercise_submit(
                &started.session_id,
                &pick.id,
                &Answer::Choice(answer_index),
                Some(3_000),
                false,
            )
            .unwrap();
        assert_eq!(grade.verdict, Verdict::Correct);
        let again = app
            .exercise_submit(
                &started.session_id,
                &pick.id,
                &Answer::Choice(answer_index),
                None,
                false,
            )
            .unwrap_err();
        assert_eq!(
            (again.code, again.message.as_str()),
            (
                ErrorCode::InvalidInput,
                "This exercise was already checked."
            )
        );
        let text = started
            .exercises
            .iter()
            .find(|ex| ex.kind() == ExerciseType::FillGap)
            .unwrap();
        assert_eq!(
            app.exercise_submit(
                &started.session_id,
                &text.id,
                &Answer::Text("  ".into()),
                None,
                false
            )
            .unwrap_err()
            .message,
            "Type an answer first."
        );
        assert_eq!(
            app.exercise_submit("s-0", "e1", &Answer::Choice(0), None, false)
                .unwrap_err()
                .message,
            "This exercise is no longer available."
        );
        let summary = app.lesson_finish(&started.session_id).unwrap();
        assert_eq!(summary.xp, 1 + 10 + 5);
        assert_eq!(summary.streak, 1);
        assert_eq!(
            app.lesson_finish(&started.session_id).unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn a_review_says_when_the_next_one_is() {
        let dir = tempfile::tempdir().unwrap();
        let now = Arc::new(AtomicI64::new(MONDAY_NOON));
        let mut app = parley_at(dir.path(), NO_PACKS, Box::new(FailingNet), now.clone());
        app.course_start("es").unwrap();
        let empty = app.review_start("es", 20).unwrap();
        assert_eq!(
            (empty.session_id.as_str(), empty.notice.as_deref()),
            ("", Some(NOTHING_TO_REVIEW_YET))
        );
        let started = app.lesson_start("es.a1.u01.s01", "learn").unwrap();
        for ex in &started.exercises {
            let answer = match &ex.body {
                Body::PickMeaning { answer_index, .. } => Answer::Choice(*answer_index),
                Body::MatchPairs { pairs } => {
                    Answer::Pairs((0..pairs.len() as u32).map(|i| [i, i]).collect())
                }
                Body::FillGap { answer, .. } => Answer::Text(answer.clone()),
                Body::TranslateToTarget { target, .. } => Answer::Text(target.clone()),
                Body::TranslateToEn { accepted, .. } => Answer::Text(accepted[0].clone()),
                _ => Answer::Text("x".into()),
            };
            app.exercise_submit(&started.session_id, &ex.id, &answer, Some(5_000), false)
                .unwrap();
        }
        app.lesson_finish(&started.session_id).unwrap();
        let none_due = app.review_start("es", 20).unwrap();
        assert!(none_due.exercises.is_empty());
        let notice = none_due.notice.unwrap();
        assert!(
            notice.starts_with("Nothing to review. Next review: "),
            "{notice}"
        );
        now.fetch_add(400 * DAY_MS, Ordering::SeqCst);
        let review = app.review_start("es", 20).unwrap();
        assert_eq!(review.kind, "review");
        assert!(!review.exercises.is_empty());
        assert!(review.exercises.len() <= 20);
        let summary = app.lesson_finish(&review.session_id).unwrap();
        assert_eq!(summary.xp, 5);
        assert_eq!(
            app.review_start("fr", 20).unwrap_err().message,
            "Unknown value fr for lang."
        );
    }
}
