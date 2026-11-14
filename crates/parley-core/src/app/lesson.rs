//! Lessons and reviews: `lesson_start`, `review_start`, `exercise_submit`,
//! `exercise_i_was_right` and `lesson_finish` (docs/modules/04-app-shell-and-offline-course.md,
//! Design notes), and from module 07 the fresh lesson with a provider, Use the course pack instead
//! and Prepare the next lesson (docs/modules/07-lesson-generation.md). Runs stay in memory until
//! finished; nothing limits how many a learner does (invariant I8).
//!
//! A lesson that needs an AI call starts in three steps, so the call is made with no lock held:
//! `lesson_begin` composes the plan and returns a [`LessonJob`]; [`LessonJob::run`] makes the call;
//! `lesson_complete` builds, stores and starts the lesson. A job superseded in between (the learner
//! chose the course pack) stores and starts nothing; its answer is only cached.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::pack::{attributions, pack_sentences};
use super::story::StoryJob;
use super::{
    ActiveRun, Course, Parley, no_such_skill, not_available, not_started, open_store, session_id,
    unknown_value,
};
use crate::ai::{
    self, AiClient, AiError, Cache, Called, LessonChecks, LessonCtx, LessonPrompt, LessonResponse,
    OFFLINE_LESSON, ProviderKind, lesson_checks,
};
use crate::api::{LessonStarted, NOTHING_TO_REVIEW_YET, next_review_notice};
use crate::content::{Example, Lang};
use crate::env::{Clock, Rng};
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

/// The chosen provider's client, or why there is none.
pub type AiAccess = Result<Arc<dyn AiClient>, AiError>;

/// The first step of a lesson start: a started lesson, or a call to make first.
pub enum LessonBegin {
    Started(LessonStarted),
    Pending(LessonJob),
}

/// A clock stopped at one moment, for the cache a job opens on another thread.
struct At(i64);

impl Clock for At {
    fn now_ms(&self) -> i64 {
        self.0
    }
    fn utc_offset_minutes(&self) -> i32 {
        0
    }
}

/// The AI cache at `path`; when it cannot be opened, one in memory, so the call still works and
/// only the saving is lost.
pub(crate) fn open_cache(path: &Path, now_ms: i64) -> Cache {
    Cache::open(path, Box::new(At(now_ms)))
        .unwrap_or_else(|_| Cache::in_memory(Box::new(At(now_ms))))
}

/// The AI cache at `path` when its file exists; none otherwise, so a read creates no file.
pub(crate) fn open_existing_cache(path: &Path, now_ms: i64) -> Option<Cache> {
    Cache::open_existing(path, Box::new(At(now_ms)))
}

pub(crate) enum JobWork {
    Lesson { prompt: LessonPrompt, plan: Plan },
    Story(StoryJob),
}

/// A P1 or P2 call to make with no lock held; owns everything it needs.
pub struct LessonJob {
    pub(crate) token: u64,
    pub(crate) key: String,
    pub(crate) skill_id: String,
    pub(crate) seq: u32,
    pub(crate) lang: Lang,
    pub(crate) client: Arc<dyn AiClient>,
    pub(crate) cache_path: PathBuf,
    pub(crate) now_ms: i64,
    pub(crate) work: JobWork,
}

pub(crate) enum Outcome {
    /// The P1 result and what the request was charged, usable answer or not.
    Lesson(Result<Called<LessonResponse>, AiError>, Option<i64>),
    Story(Result<Called<ai::Story>, AiError>),
}

/// What a job's call gave.
pub struct JobOutcome(pub(crate) Outcome);

impl LessonJob {
    pub fn skill_id(&self) -> &str {
        &self.skill_id
    }

    pub fn seq(&self) -> u32 {
        self.seq
    }

    /// Opens cache/ai.sqlite3 and makes the call (request_lesson, with its charge, or
    /// generate_story).
    pub fn run(&self) -> JobOutcome {
        let cache = open_cache(&self.cache_path, self.now_ms);
        JobOutcome(match &self.work {
            JobWork::Lesson { prompt, .. } => {
                let (result, charge) = ai::request_lesson_charged(&*self.client, &cache, prompt);
                Outcome::Lesson(result, charge)
            }
            JobWork::Story(story) => Outcome::Story(story.run(&*self.client, &cache)),
        })
    }
}

/// The settings a run grades and schedules with.
pub(crate) fn run_settings(settings: &Settings) -> RunSettings {
    RunSettings {
        accent: settings.accent(),
        daily_goal_xp: settings.daily_goal_xp,
        srs: settings.srs(),
    }
}

/// The runner's refusals with their messages; a store failure is the data-file error.
pub(crate) fn run_error(e: RunError) -> AppError {
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

/// "This lesson was replaced by one from the course pack."
pub(crate) fn replaced() -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        "This lesson was replaced by one from the course pack.",
    )
}

/// The pack sentences for a plan's lemmas, as template exercises use them.
fn plan_sentences(
    parley_dir: &Path,
    store: &Store,
    course: &Course,
    plan: &Plan,
) -> (Vec<Example>, Vec<super::PackSentence>) {
    let mut lemmas: Vec<String> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for entry in &plan.entries {
        if let Some(item) = course.items.iter().find(|item| item.id == entry.item_id) {
            for lemma in &item.lemmas {
                if seen.insert(lemma.as_str()) {
                    lemmas.push(lemma.clone());
                }
            }
        }
    }
    let pack = pack_sentences(parley_dir, store, course.syllabus.lang, &lemmas);
    let sentences = pack
        .iter()
        .map(|sentence| Example {
            id: sentence.id,
            target: sentence.text.clone(),
            en: sentence.en.clone(),
        })
        .collect();
    (sentences, pack)
}

/// The template lesson of a plan in the context of `skill_id`, with the installed pack's
/// sentences for the plan's lemmas as extra `fill_gap` candidates; and their attributions.
fn build_template(
    parley_dir: &Path,
    store: &Store,
    course: &Course,
    skill_id: &str,
    plan: &Plan,
    seq: u32,
    rng: &mut dyn Rng,
) -> (Lesson, Vec<crate::api::Attribution>) {
    let plan_items: Vec<String> = plan.entries.iter().map(|e| e.item_id.clone()).collect();
    let (sentences, pack) = plan_sentences(parley_dir, store, course, plan);
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

/// How a P1 error shows at the lesson start (docs/PUBLIK-INTEGRATION.md §9): offline is a notice;
/// no consent yet shows nothing (the lesson never waits for it); anything else is the AI banner.
fn error_shown(error: AiError) -> (Option<String>, Option<AppError>) {
    match error {
        AiError::Offline => (Some(OFFLINE_LESSON.to_string()), None),
        AiError::NeedsConsent => (None, None),
        other => (None, Some(other.into())),
    }
}

/// What a started lesson shows besides its exercises.
struct Shown {
    credits: Vec<crate::api::Attribution>,
    notice: Option<String>,
    ai_error: Option<AppError>,
    cost_micros: Option<i64>,
}

impl Parley {
    /// Stores `lesson`, starts its run and says what to show.
    fn start_lesson_run(
        &mut self,
        lang: Lang,
        lesson: Lesson,
        learn: bool,
        shown: Shown,
    ) -> Result<LessonStarted, AppError> {
        let store = open_store(&mut self.store)?;
        let clock = &*self.env.clock;
        let id = session_id("s", &mut *self.env.rng);
        let run = LessonRun::start_lesson(store, &lesson, &id, clock)?;
        if let Some(micros) = shown.cost_micros {
            store.add_session_cost(&id, micros)?;
        }
        self.runs.insert(
            id.clone(),
            ActiveRun {
                lang,
                run,
                skill_id: learn.then(|| lesson.skill_id.clone()),
                judging_stopped: false,
            },
        );
        Ok(LessonStarted {
            session_id: id,
            kind: "lesson".into(),
            skill_id: Some(lesson.skill_id.clone()),
            origin: lesson.origin,
            exercises: lesson.exercises,
            attributions: shown.credits,
            notice: shown.notice,
            ai_error: shown.ai_error,
            story: None,
        })
    }

    /// The skill's course, level check included.
    pub(crate) fn skill_course(&mut self, skill_id: &str) -> Result<(Lang, Arc<Course>), AppError> {
        let store = open_store(&mut self.store)?;
        let info = match store.skill_info(skill_id) {
            Ok(info) if info.ord >= 0 => info,
            Ok(_) | Err(StoreError::UnknownSkill(_)) => return Err(no_such_skill()),
            Err(e) => return Err(e.into()),
        };
        let lang: Lang = info.lang.parse().map_err(|_| no_such_skill())?;
        let course = self.content.get(&lang).ok_or_else(no_such_skill)?.clone();
        Ok((lang, course))
    }

    /// The plan of a skill's lesson now: learn mode introduces new items, review mode none. The
    /// first two `translate_to_en` entries turn into `listen_type` while listening is available
    /// for the language (module 09: a voice exists and **Can't listen now** is not on), and up to
    /// two short `translate_to_target` phrase entries into `speak` while speaking is (module 10:
    /// a microphone, a transcriber the learner can use, and **Can't speak now** not on).
    fn plan_for(&mut self, lang: Lang, skill_id: &str, learn: bool) -> Result<Plan, AppError> {
        let capabilities = Capabilities {
            audio_out: self.listen_available(lang),
            mic: self.speak_available(lang),
        };
        let request = ComposeRequest {
            lang,
            skill_id: skill_id.to_string(),
            lesson_length: self.settings.lesson_length as usize,
            new_items_per_lesson: if learn {
                self.settings.new_items_per_lesson as usize
            } else {
                0
            },
            capabilities,
        };
        let now = self.env.clock.now_ms();
        let store = open_store(&mut self.store)?;
        Ok(compose(&request, store, now)?)
    }

    /// Whether the settings ask for a fresh lesson: AI lessons on and a provider chosen.
    fn fresh_lessons_on(&self) -> bool {
        self.settings.ai.fresh_lessons && self.settings.ai.provider != "unset"
    }

    /// A new job number for `key`, which supersedes any job waiting there.
    pub(crate) fn register_job(&mut self, key: &str) -> u64 {
        self.next_job += 1;
        self.pending.insert(key.to_string(), self.next_job);
        self.next_job
    }

    /// The first step of `lesson_start`. `ai` is None when the command did not ask for a client
    /// (no provider chosen, or a learn lesson with AI lessons off).
    pub fn lesson_begin(
        &mut self,
        skill_id: &str,
        mode: &str,
        ai: Option<AiAccess>,
    ) -> Result<LessonBegin, AppError> {
        let learn = match mode {
            "learn" => true,
            "review" => false,
            "story" => return self.story_begin(skill_id, ai),
            other => return Err(unknown_value(other, "mode")),
        };
        let (lang, course) = self.skill_course(skill_id)?;
        let store = open_store(&mut self.store)?;
        let seq = store.lesson_sessions(skill_id)? + 1;
        if let Some(stored) = store.stored_lesson(skill_id, seq)? {
            // An imported lesson waits at this number: it plays as stored, with no call.
            let shown = Shown {
                credits: Vec::new(),
                notice: None,
                ai_error: None,
                cost_micros: None,
            };
            return self
                .start_lesson_run(lang, stored, learn, shown)
                .map(LessonBegin::Started);
        }
        let plan = self.plan_for(lang, skill_id, learn)?;
        let mut shown = Shown {
            credits: Vec::new(),
            notice: None,
            ai_error: None,
            cost_micros: None,
        };
        if learn && self.fresh_lessons_on() && !plan.entries.is_empty() {
            match ai {
                Some(Ok(client)) if !client.status().needs_credit => {
                    let store = open_store(&mut self.store)?;
                    let introduced = store.introduced_skills(lang)?;
                    if let Some(prompt) = LessonPrompt::new(
                        &course.syllabus,
                        &course.items,
                        &introduced,
                        skill_id,
                        seq,
                        &plan,
                    ) {
                        let token = self.register_job(skill_id);
                        return Ok(LessonBegin::Pending(LessonJob {
                            token,
                            key: skill_id.to_string(),
                            skill_id: skill_id.to_string(),
                            seq,
                            lang,
                            client,
                            cache_path: self.config.data_dir.join(super::AI_CACHE_FILE),
                            now_ms: self.env.clock.now_ms(),
                            work: JobWork::Lesson { prompt, plan },
                        }));
                    }
                }
                Some(Err(error)) => (shown.notice, shown.ai_error) = error_shown(error),
                _ => {}
            }
        }
        let store = open_store(&mut self.store)?;
        let (lesson, credits) = build_template(
            &self.config.data_dir,
            store,
            &course,
            skill_id,
            &plan,
            seq,
            &mut *self.env.rng,
        );
        shown.credits = credits;
        self.start_lesson_run(lang, lesson, learn, shown)
            .map(LessonBegin::Started)
    }

    /// The last step of `lesson_start`: builds, stores and starts the lesson of a job's answer. A
    /// superseded job stores and starts nothing.
    pub fn lesson_complete(
        &mut self,
        job: LessonJob,
        outcome: JobOutcome,
    ) -> Result<LessonStarted, AppError> {
        if self.pending.get(&job.key) != Some(&job.token) {
            return Err(replaced());
        }
        self.pending.remove(&job.key);
        let LessonJob {
            skill_id,
            lang,
            client,
            work,
            seq,
            ..
        } = job;
        match (work, outcome.0) {
            (JobWork::Lesson { prompt, plan }, Outcome::Lesson(result, charge)) => {
                let provider = client.kind();
                self.complete_lesson(lang, &skill_id, seq, prompt, plan, provider, result, charge)
            }
            (JobWork::Story(story), Outcome::Story(result)) => {
                self.story_complete(lang, &skill_id, seq, story, result)
            }
            // A job is only ever run into its own kind of outcome.
            _ => Err(replaced()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn complete_lesson(
        &mut self,
        lang: Lang,
        skill_id: &str,
        seq: u32,
        prompt: LessonPrompt,
        plan: Plan,
        provider: ProviderKind,
        result: Result<Called<LessonResponse>, AiError>,
        charge: Option<i64>,
    ) -> Result<LessonStarted, AppError> {
        let course = self.content.get(&lang).ok_or_else(no_such_skill)?.clone();
        let store = open_store(&mut self.store)?;
        let rng = &mut *self.env.rng;
        let (lesson, shown) = match result {
            Ok(_) | Err(AiError::Invalid) => {
                let (sentences, pack) =
                    plan_sentences(&self.config.data_dir, store, &course, &plan);
                let ctx = LessonCtx {
                    prompt,
                    syllabus: &course.syllabus,
                    forms: &course.forms,
                    items: &course.items,
                    numbers: &course.numbers,
                    sentences: &sentences,
                };
                // An answer that failed L1 was charged too: its charge counts like any other.
                let response = result.as_ref().ok().map(|called| &called.value);
                let draft = ai::build_lesson(response, provider, &plan, &ctx, rng);
                let credits = attributions(&draft.lesson, &pack, &course.syllabus);
                (
                    draft.lesson,
                    Shown {
                        credits,
                        notice: draft.notice,
                        ai_error: None,
                        cost_micros: charge,
                    },
                )
            }
            Err(error) => {
                let (lesson, credits) = build_template(
                    &self.config.data_dir,
                    store,
                    &course,
                    skill_id,
                    &plan,
                    seq,
                    rng,
                );
                let (notice, ai_error) = error_shown(error);
                (
                    lesson,
                    Shown {
                        credits,
                        notice,
                        ai_error,
                        cost_micros: None,
                    },
                )
            }
        };
        self.start_lesson_run(lang, lesson, true, shown)
    }

    /// begin, run and complete in one go (the CLI and tests).
    pub fn lesson_start_with(
        &mut self,
        skill_id: &str,
        mode: &str,
        ai: Option<AiAccess>,
    ) -> Result<LessonStarted, AppError> {
        match self.lesson_begin(skill_id, mode, ai)? {
            LessonBegin::Started(started) => Ok(started),
            LessonBegin::Pending(job) => {
                let outcome = job.run();
                self.lesson_complete(job, outcome)
            }
        }
    }

    /// Starts a lesson of a skill with no AI call: `learn` introduces new items, `review` none.
    pub fn lesson_start(&mut self, skill_id: &str, mode: &str) -> Result<LessonStarted, AppError> {
        self.lesson_start_with(skill_id, mode, None)
    }

    /// **Use the course pack instead**: the pending job of the skill is superseded; its template
    /// lesson starts.
    pub fn lesson_use_course_pack(&mut self, skill_id: &str) -> Result<LessonStarted, AppError> {
        self.pending.remove(skill_id);
        match self.lesson_begin(skill_id, "learn", None)? {
            LessonBegin::Started(started) => Ok(started),
            LessonBegin::Pending(_) => unreachable!("no client, no call"),
        }
    }

    /// The skill of a learn-mode lesson in progress (for the prefetch after lesson_finish).
    pub fn run_skill(&self, session_id: &str) -> Option<String> {
        self.runs
            .get(session_id)
            .and_then(|active| active.skill_id.clone())
    }

    /// After a finished learn lesson: the P1 job of the skill's next lesson, when Prepare the
    /// next lesson and AI lessons are on, a provider is chosen and ready without needs-credit, and
    /// no lesson waits at the next number. Its answer is only cached.
    pub fn next_lesson_job(
        &mut self,
        skill_id: &str,
        ai: Option<AiAccess>,
    ) -> Result<Option<LessonJob>, AppError> {
        if !(self.settings.ai.prepare_next_lesson && self.fresh_lessons_on()) {
            return Ok(None);
        }
        let Some(Ok(client)) = ai else {
            return Ok(None);
        };
        if client.status().needs_credit {
            return Ok(None);
        }
        let (lang, course) = self.skill_course(skill_id)?;
        let store = open_store(&mut self.store)?;
        let seq = store.lesson_sessions(skill_id)? + 1;
        if store.stored_lesson(skill_id, seq)?.is_some() {
            return Ok(None);
        }
        let introduced = store.introduced_skills(lang)?;
        let plan = self.plan_for(lang, skill_id, true)?;
        if plan.entries.is_empty() {
            return Ok(None);
        }
        let Some(prompt) = LessonPrompt::new(
            &course.syllabus,
            &course.items,
            &introduced,
            skill_id,
            seq,
            &plan,
        ) else {
            return Ok(None);
        };
        Ok(Some(LessonJob {
            token: 0,
            key: String::new(),
            skill_id: skill_id.to_string(),
            seq,
            lang,
            client,
            cache_path: self.config.data_dir.join(super::AI_CACHE_FILE),
            now_ms: self.env.clock.now_ms(),
            work: JobWork::Lesson { prompt, plan },
        }))
    }

    /// The validity rate of the last lessons a local model wrote (Settings > AI > Local model).
    pub fn ai_lesson_checks(&self) -> Result<LessonChecks, AppError> {
        let store = self
            .store
            .as_ref()
            .map_err(|details| super::data_file_error(details))?;
        let recent = store.recent_ai_lessons(ProviderKind::Local.as_str(), 10)?;
        Ok(lesson_checks(&recent))
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
                ai_error: None,
                story: None,
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
        let (lesson, credits) = build_template(
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
        self.runs.insert(
            id.clone(),
            ActiveRun {
                lang,
                run,
                skill_id: None,
                judging_stopped: false,
            },
        );
        Ok(LessonStarted {
            session_id: id,
            kind: "review".into(),
            skill_id: None,
            origin: Origin::Template,
            exercises: lesson.exercises,
            attributions: credits,
            notice: None,
            ai_error: None,
            story: None,
        })
    }

    /// Grades one answer of a run in progress (module 03's grade) and reviews its card, offline:
    /// an unchecked answer is left for `judge_begin` (module 08).
    pub fn exercise_submit(
        &mut self,
        session_id: &str,
        exercise_id: &str,
        answer: &Answer,
        response_ms: Option<i64>,
        used_hint: bool,
    ) -> Result<Grade, AppError> {
        if self.story_runs.contains_key(session_id) {
            return self.story_answer(session_id, exercise_id, answer);
        }
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

    /// Finishes a run: XP, the day's credit, the streak and the weekly record (module 03). Then,
    /// whatever the result, the speaking takes are removed unless **Keep my recordings** is on
    /// (module 10).
    pub fn lesson_finish(&mut self, session_id: &str) -> Result<LessonSummary, AppError> {
        let finished = self.finish_run(session_id);
        self.remove_takes_unless_kept();
        finished
    }

    fn finish_run(&mut self, session_id: &str) -> Result<LessonSummary, AppError> {
        if self.story_runs.contains_key(session_id) {
            return self.story_finish(session_id);
        }
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
            app.lesson_start("es.a1.u01.s01", "tale")
                .unwrap_err()
                .message,
            "Unknown value tale for mode."
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
