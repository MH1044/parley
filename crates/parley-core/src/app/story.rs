//! Stories and course packs in the app service (docs/modules/07-lesson-generation.md): the Story
//! node's `lesson_start(mode: story)`, its questions through `exercise_submit`, its XP through
//! `lesson_finish`; `course_pack_export` and `course_pack_import`.
//!
//! A story is read, not reviewed: its questions are graded here and never touch a card or the
//! review log; finishing it credits the day and the streak like any session.

use std::path::Path;
use std::sync::Arc;

use super::lesson::{AiAccess, JobWork, LessonBegin, LessonJob};
use super::{
    Course, Parley, cannot_read, cannot_write, not_available, not_started, open_store, session_id,
};
use crate::ai::{
    AiClient, AiError, Cache, Called, Story, StoryCtx, StoryPrompt, StoryQuestion, generate_story,
    has_story, story_xp,
};
use crate::api::LessonStarted;
use crate::content::Lang;
use crate::coursepack::{
    CoursePackReport, CoursePackSummary, PackCheck, PackError, export_course_pack,
    import_course_pack, parse_course_pack,
};
use crate::env::{LocalDate, local_date};
use crate::error::{AppError, ErrorCode};
use crate::exercise::{LessonSummary, Origin};
use crate::game::{SessionKind, WEEKLY_RECORD};
use crate::grading::{Answer, Grade, Verdict};
use crate::store::{SessionEnd, SessionStart};

/// "This skill has no story."
fn no_story() -> AppError {
    AppError::new(ErrorCode::NotFound, "This skill has no story.")
}

/// A P2 call's own part of a job: the prompt and the course it is checked in.
pub(crate) struct StoryJob {
    prompt: StoryPrompt,
    course: Arc<Course>,
}

impl StoryJob {
    pub(crate) fn run(
        &self,
        client: &dyn AiClient,
        cache: &Cache,
    ) -> Result<Called<Story>, AiError> {
        let ctx = StoryCtx {
            prompt: self.prompt.clone(),
            syllabus: &self.course.syllabus,
            forms: &self.course.forms,
            numbers: &self.course.numbers,
        };
        generate_story(client, cache, &ctx)
    }
}

/// A story being read.
pub(crate) struct StoryRun {
    local_date: LocalDate,
    questions: Vec<StoryQuestion>,
    /// Whether each question was answered right, once answered.
    answered: Vec<Option<bool>>,
}

impl Parley {
    /// `lesson_begin` in story mode: a stored story at the story number plays as stored, with no
    /// client needed; otherwise P2, asked whenever the learner asks (even with needs-credit).
    pub(crate) fn story_begin(
        &mut self,
        skill_id: &str,
        ai: Option<AiAccess>,
    ) -> Result<LessonBegin, AppError> {
        let (lang, course) = self.skill_course(skill_id)?;
        if !has_story(&course.syllabus, skill_id) {
            return Err(no_story());
        }
        let store = open_store(&mut self.store)?;
        let seq = store.story_sessions(skill_id)? + 1;
        if let Some((story, origin)) = store.stored_story(skill_id, seq)? {
            let origin = if origin == "ai" {
                Origin::Ai
            } else {
                Origin::CoursePack
            };
            return self
                .start_story_run(lang, skill_id, story, origin, None)
                .map(LessonBegin::Started);
        }
        let ai = if self.settings.ai.provider == "unset" {
            None
        } else {
            ai
        };
        let client = match ai {
            None => return Err(AiError::NeedsConsent.into()),
            Some(Err(error)) => return Err(error.into()),
            Some(Ok(client)) => client,
        };
        let store = open_store(&mut self.store)?;
        let introduced = store.introduced_skills(lang)?;
        let prompt =
            StoryPrompt::new(&course.syllabus, &introduced, skill_id, seq).ok_or_else(no_story)?;
        let key = format!("story:{skill_id}");
        let token = self.register_job(&key);
        Ok(LessonBegin::Pending(LessonJob {
            token,
            key,
            skill_id: skill_id.to_string(),
            seq,
            lang,
            client,
            cache_path: self.config.data_dir.join(super::AI_CACHE_FILE),
            now_ms: self.env.clock.now_ms(),
            work: JobWork::Story(StoryJob { prompt, course }),
        }))
    }

    /// The story answer stored and its session started; a failed check is "The AI answer could
    /// not be used." and stores nothing.
    pub(crate) fn story_complete(
        &mut self,
        lang: Lang,
        skill_id: &str,
        seq: u32,
        _job: StoryJob,
        result: Result<Called<Story>, AiError>,
    ) -> Result<LessonStarted, AppError> {
        let called = result.map_err(AppError::from)?;
        let now = self.env.clock.now_ms();
        let store = open_store(&mut self.store)?;
        store.put_story(lang, skill_id, seq, &called.value, now)?;
        self.start_story_run(
            lang,
            skill_id,
            called.value,
            Origin::Ai,
            called.charge_micros,
        )
    }

    fn start_story_run(
        &mut self,
        lang: Lang,
        skill_id: &str,
        story: Story,
        origin: Origin,
        cost_micros: Option<i64>,
    ) -> Result<LessonStarted, AppError> {
        let store = open_store(&mut self.store)?;
        let clock = &*self.env.clock;
        let id = session_id("s", &mut *self.env.rng);
        let date = local_date(clock);
        store.start_session(
            &SessionStart {
                id: &id,
                lang,
                kind: SessionKind::Story,
                skill_id: Some(skill_id),
                started_at: clock.now_ms(),
                local_date: date,
            },
            None,
        )?;
        if let Some(micros) = cost_micros {
            store.add_session_cost(&id, micros)?;
        }
        self.story_runs.insert(
            id.clone(),
            StoryRun {
                local_date: date,
                questions: story.questions.clone(),
                answered: vec![None; story.questions.len()],
            },
        );
        Ok(LessonStarted {
            session_id: id,
            kind: "story".into(),
            skill_id: Some(skill_id.to_string()),
            origin,
            exercises: Vec::new(),
            attributions: Vec::new(),
            notice: None,
            ai_error: None,
            story: Some(story),
        })
    }

    /// One question of a story, `q1`, `q2`, …: right when the choice is its answer, else wrong
    /// with the right option; never a rating.
    pub(crate) fn story_answer(
        &mut self,
        session_id: &str,
        exercise_id: &str,
        answer: &Answer,
    ) -> Result<Grade, AppError> {
        let run = self
            .story_runs
            .get_mut(session_id)
            .ok_or_else(not_available)?;
        let index = exercise_id
            .strip_prefix('q')
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|n| n.checked_sub(1))
            .filter(|&i| i < run.questions.len())
            .ok_or_else(not_available)?;
        let Answer::Choice(choice) = answer else {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "Choose an answer first.",
            ));
        };
        if run.answered[index].is_some() {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "This exercise was already checked.",
            ));
        }
        let question = &run.questions[index];
        let right = *choice == question.answer_index;
        run.answered[index] = Some(right);
        Ok(Grade {
            verdict: if right {
                Verdict::Correct
            } else {
                Verdict::Wrong
            },
            tags: Vec::new(),
            message: None,
            corrected: (!right)
                .then(|| {
                    question
                        .options
                        .get(question.answer_index as usize)
                        .cloned()
                })
                .flatten(),
            words: Vec::new(),
            rating: None,
            explanation: None,
            expected: None,
            judge_error: None,
        })
    }

    /// Finishes a story: 10 XP plus 2 per correct answer; the day and the streak credited.
    pub(crate) fn story_finish(&mut self, session_id: &str) -> Result<LessonSummary, AppError> {
        let run = self
            .story_runs
            .remove(session_id)
            .ok_or_else(not_available)?;
        let correct = run.answered.iter().filter(|a| **a == Some(true)).count() as u32;
        let answered = run.answered.iter().flatten().count() as u32;
        let xp = story_xp(correct);
        let goal_xp = self.settings.daily_goal_xp;
        let clock = &*self.env.clock;
        let store = open_store(&mut self.store)?;
        let done = store.finish_session(&SessionEnd {
            id: session_id,
            ended_at: clock.now_ms(),
            xp,
            exercises: answered,
            correct,
            day: run.local_date,
            today: local_date(clock),
            goal_xp,
        })?;
        Ok(LessonSummary {
            xp,
            correct,
            total: run.questions.len() as u32,
            streak: done.streak.length,
            freezes: done.streak.freezes,
            record_week: done.record_week,
            message: done.record_week.then(|| WEEKLY_RECORD.to_string()),
            daily_xp: done.daily_xp,
            goal_xp,
        })
    }

    /// Writes the course pack of `lang` to `dest_path` (a path the learner picked).
    pub fn course_pack_export(
        &mut self,
        lang: &str,
        dest_path: &Path,
    ) -> Result<CoursePackSummary, AppError> {
        let lang = self.lang(lang)?;
        let version = self.content[&lang].syllabus.version;
        let today = local_date(&*self.env.clock);
        let store = open_store(&mut self.store)?;
        if store.course(lang)?.is_none() {
            return Err(not_started());
        }
        let pack = export_course_pack(store, lang, version, today)?;
        let mut text = serde_json::to_string_pretty(&pack).expect("a course pack serialises");
        text.push('\n');
        self.env
            .fs
            .write(dest_path, text.as_bytes())
            .map_err(|_| cannot_write())?;
        Ok(CoursePackSummary {
            bytes: text.len() as u64,
            lessons: pack.lessons.len(),
            stories: pack.stories.len(),
            exercises: pack.lessons.iter().map(|l| l.exercises.len()).sum(),
        })
    }

    /// Reads a course pack from `src_path` (a path the learner picked) into the active course.
    pub fn course_pack_import(&mut self, src_path: &Path) -> Result<CoursePackReport, AppError> {
        let bytes = self.env.fs.read(src_path).map_err(|_| cannot_read())?;
        let active = self
            .settings
            .active_course
            .as_deref()
            .and_then(|lang| lang.parse::<Lang>().ok())
            .filter(|lang| self.content.contains_key(lang))
            .ok_or_else(not_started)?;
        let pack = parse_course_pack(&bytes).map_err(pack_error)?;
        let course = self.content[&active].clone();
        let now = self.env.clock.now_ms();
        let store = open_store(&mut self.store)?;
        if store.course(active)?.is_none() {
            return Err(not_started());
        }
        let check = PackCheck {
            active,
            syllabus: &course.syllabus,
            forms: &course.forms,
            items: &course.items,
            numbers: &course.numbers,
        };
        import_course_pack(store, &pack, &check, now).map_err(pack_error)
    }
}

/// A course pack refusal as the command shows it.
fn pack_error(e: PackError) -> AppError {
    match e {
        PackError::TooLarge => AppError::new(ErrorCode::FileTooLarge, e.message()),
        PackError::NotACoursePack | PackError::WrongLanguage { .. } => {
            AppError::new(ErrorCode::InvalidInput, e.message())
        }
        PackError::Store(e) => e.into(),
    }
}
