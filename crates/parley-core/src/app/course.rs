//! Courses and the path: `course_list`, `course_start`, `path_get` and `skill_get`.

use super::{Course, Parley, no_such_skill, not_started, open_store};
use crate::api::{
    CourseSummary, PathSkill, PathStory, PathUnit, PathView, SkillDetail, Word, language_name,
};
use crate::content::{ItemKind, Lang};
use crate::env::local_date;
use crate::error::AppError;
use crate::game::streak;
use crate::srs::mastery::first_unmet_prerequisite;
use crate::srs::{SkillState, skill_mastery, skill_state};
use crate::store::{CourseRow, Store, StoreError};

/// One course as the language list shows it.
fn summary(
    lang: Lang,
    course: &Course,
    row: Option<CourseRow>,
    active: Option<&str>,
) -> CourseSummary {
    CourseSummary {
        lang: lang.as_str().to_string(),
        name: language_name(lang).to_string(),
        started: row.is_some(),
        active: active == Some(lang.as_str()),
        placement_done: row.as_ref().is_some_and(|r| r.placement_index.is_some()),
        cefr_estimate: row.and_then(|r| r.cefr_estimate),
        skills: course.syllabus.skills().count(),
    }
}

/// The skill's state and mastery now; a skill the store does not know is locked.
fn progress(store: &Store, skill_id: &str, now: i64) -> Result<(SkillState, f64), StoreError> {
    match skill_state(store, skill_id, now) {
        Ok(state) => Ok((state, skill_mastery(store, skill_id, now)?)),
        Err(StoreError::UnknownSkill(_)) => Ok((SkillState::Locked, 0.0)),
        Err(e) => Err(e),
    }
}

impl Parley {
    /// Every bundled course, in the order es, fr, de.
    pub fn course_list(&mut self) -> Result<Vec<CourseSummary>, AppError> {
        let store = open_store(&mut self.store)?;
        let active = self.settings.active_course.as_deref();
        let mut list = Vec::new();
        for (lang, course) in &self.content {
            list.push(summary(*lang, course, store.course(*lang)?, active));
        }
        Ok(list)
    }

    /// Starts (or brings up to date) a course from the bundled syllabus and makes it the
    /// active course.
    pub fn course_start(&mut self, lang: &str) -> Result<CourseSummary, AppError> {
        let lang = self.lang(lang)?;
        let now = self.env.clock.now_ms();
        let store = open_store(&mut self.store)?;
        let course = &self.content[&lang];
        store.start_course(&course.syllabus, &course.forms, now)?;
        let row = store.course(lang)?;
        let mut settings = self.settings.clone();
        settings.active_course = Some(lang.as_str().to_string());
        self.save_settings(settings)?;
        Ok(summary(
            lang,
            &self.content[&lang],
            row,
            Some(lang.as_str()),
        ))
    }

    /// The path of a started course: its units in syllabus order with each skill's state and
    /// mastery, the skill Continue opens (the first available or learning one), the due count,
    /// the streak and today's XP.
    pub fn path_get(&mut self, lang: &str) -> Result<PathView, AppError> {
        let lang = self.lang(lang)?;
        let store = open_store(&mut self.store)?;
        if store.course(lang)?.is_none() {
            return Err(not_started());
        }
        let clock = &*self.env.clock;
        let now = clock.now_ms();
        let course = &self.content[&lang];
        let mut units = Vec::new();
        let mut next_skill_id = None;
        for (index, unit) in course.syllabus.units.iter().enumerate() {
            let mut skills = Vec::new();
            for skill in &unit.skills {
                let (state, mastery) = progress(store, &skill.id, now)?;
                let tested_out = match store.skill_info(&skill.id) {
                    Ok(info) => info.tested_out,
                    Err(StoreError::UnknownSkill(_)) => false,
                    Err(e) => return Err(e.into()),
                };
                if next_skill_id.is_none()
                    && matches!(state, SkillState::Available | SkillState::Learning)
                {
                    next_skill_id = Some(skill.id.clone());
                }
                skills.push(PathSkill {
                    id: skill.id.clone(),
                    title: skill.title.clone(),
                    can_do: skill.can_do.clone(),
                    cefr: skill.cefr,
                    state,
                    mastery,
                    tested_out,
                });
            }
            // The Story node ends the unit while stories are on; it opens once the unit's last
            // skill is practised (module 07).
            let story = skills
                .last()
                .filter(|_| self.settings.ai.stories)
                .map(|last| PathStory {
                    skill_id: last.id.clone(),
                    available: matches!(last.state, SkillState::Practised | SkillState::Mastered),
                });
            units.push(PathUnit {
                id: unit.id.clone(),
                n: index + 1,
                title: unit.title.clone(),
                cefr: unit.cefr,
                skills,
                story,
            });
        }
        let today = local_date(clock);
        let daily = store.daily()?;
        let state = streak(&daily, today);
        let today_xp = daily
            .iter()
            .find(|row| row.local_date == today)
            .map_or(0, |row| row.xp);
        Ok(PathView {
            lang: lang.as_str().to_string(),
            units,
            next_skill_id,
            due: store.due_count(lang, now)?,
            streak: state.length,
            freezes: state.freezes,
            today_xp,
            goal_xp: self.settings.daily_goal_xp,
        })
    }

    /// A skill's detail: its can-do statement, grammar note, words, state and, when locked, the
    /// skill that opens it.
    pub fn skill_get(&mut self, skill_id: &str) -> Result<SkillDetail, AppError> {
        let store = open_store(&mut self.store)?;
        let info = match store.skill_info(skill_id) {
            Ok(info) => info,
            Err(StoreError::UnknownSkill(_)) => return Err(no_such_skill()),
            Err(e) => return Err(e.into()),
        };
        let course = info
            .lang
            .parse::<Lang>()
            .ok()
            .and_then(|lang| self.content.get(&lang))
            .ok_or_else(no_such_skill)?;
        let (unit, skill) = course
            .syllabus
            .skills()
            .find(|(_, skill)| skill.id == skill_id)
            .ok_or_else(no_such_skill)?;
        let now = self.env.clock.now_ms();
        let (state, mastery) = progress(store, skill_id, now)?;
        let opened_by = if state == SkillState::Locked {
            first_unmet_prerequisite(store, skill_id)?.map(|id| {
                course
                    .syllabus
                    .skills()
                    .find(|(_, s)| s.id == id)
                    .map_or(id, |(_, s)| s.title.clone())
            })
        } else {
            None
        };
        let words = course
            .items
            .iter()
            .filter(|item| item.skill == skill_id && item.kind == ItemKind::Vocab)
            .map(|item| Word {
                target: item.target.clone(),
                en: item.en.first().cloned().unwrap_or_default(),
            })
            .collect();
        Ok(SkillDetail {
            id: skill.id.clone(),
            title: skill.title.clone(),
            can_do: skill.can_do.clone(),
            cefr: skill.cefr,
            grammar_note: skill.grammar_note.clone(),
            unit_title: unit.title.clone(),
            state,
            mastery,
            words,
            opened_by,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::parley;
    use crate::error::ErrorCode;
    use crate::srs::SkillState;

    #[test]
    fn a_started_course_has_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = parley(dir.path());
        let list = app.course_list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].name.as_str(), list[0].started), ("Spanish", false));
        assert_eq!(
            app.path_get("es").unwrap_err().message,
            "This course has not been started."
        );
        let started = app.course_start("es").unwrap();
        assert!(started.started && started.active && !started.placement_done);
        let path = app.path_get("es").unwrap();
        assert_eq!(path.units.len(), 1);
        assert_eq!(path.units[0].n, 1);
        assert_eq!(path.units[0].skills[0].state, SkillState::Available);
        assert_eq!(path.next_skill_id.as_deref(), Some("es.a1.u01.s01"));
        assert_eq!(
            (path.due, path.streak, path.today_xp, path.goal_xp),
            (0, 0, 0, 20)
        );
        let detail = app.skill_get("es.a1.u01.s01").unwrap();
        assert_eq!(detail.words[0].target, "hola");
        assert_eq!(detail.opened_by, None);
        let err = app.skill_get("es.a1.u09.s09").unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (ErrorCode::NotFound, "This skill is not in the course.")
        );
        assert_eq!(
            app.path_get("de").unwrap_err().message,
            "Unknown value de for lang.",
            "a language the app does not ship"
        );
    }
}
