//! The placement test: `placement_start` and `placement_answer` (module 02's staircase over the
//! course's skills; docs/modules/04-app-shell-and-offline-course.md, Design notes).
//!
//! Each question is a `translate_to_en` exercise of the skill's first vocabulary item (else its
//! first item), graded offline at the settings' accent leniency: correct, almost and typo count as
//! right, an empty answer as wrong. Nothing is stored until the test ends; then the tested-out
//! skills are recorded with the placement's session id.

use super::{ActivePlacement, Course, Parley, not_available, not_started, open_store, session_id};
use crate::api::{PlacementOutcome, PlacementQuestion, PlacementStep};
use crate::content::{ItemKind, Lang};
use crate::error::AppError;
use crate::exercise::{Body, Exercise, Origin};
use crate::grading::{Answer, Verdict, grade, grade_ctx, normalise};
use crate::srs::{Facet, Placement, SkillRef};

/// The question about `skill`: a `translate_to_en` of its first vocabulary item, else its first
/// item; none for a skill without items.
fn question_for(course: &Course, skill: &SkillRef, n: usize) -> Option<Exercise> {
    let items = || course.items.iter().filter(|item| item.skill == skill.id);
    let item = items()
        .find(|item| item.kind == ItemKind::Vocab)
        .or_else(|| items().next())?;
    Some(Exercise {
        id: format!("p{n}"),
        item_id: item.id.clone(),
        facet: Facet::Receptive,
        origin: Origin::Template,
        body: Body::TranslateToEn {
            prompt_target: item.target.clone(),
            accepted: item.accepted.to_en.clone(),
        },
    })
}

/// The next question of a test in progress, numbered `n`: a skill without items is answered
/// wrong and skipped. None once the test is finished.
fn next_question(
    course: &Course,
    placement: &mut Placement,
    id: &str,
    n: usize,
) -> Option<PlacementQuestion> {
    while let Some(skill) = placement.next_question() {
        match question_for(course, &skill, n) {
            Some(exercise) => {
                return Some(PlacementQuestion {
                    session_id: id.to_string(),
                    n,
                    skill_id: skill.id,
                    exercise,
                });
            }
            None => placement.answer(false),
        }
    }
    None
}

impl Parley {
    /// Starts a placement test of a started course and asks its first question.
    pub fn placement_start(&mut self, lang: &str) -> Result<PlacementQuestion, AppError> {
        let lang: Lang = self.lang(lang)?;
        let store = open_store(&mut self.store)?;
        if store.course(lang)?.is_none() {
            return Err(not_started());
        }
        let course = &self.content[&lang];
        let mut placement = Placement::new(store.skills(lang)?);
        let id = session_id("p", &mut *self.env.rng);
        let question = next_question(course, &mut placement, &id, 1).ok_or_else(not_started)?;
        self.placements.insert(
            id,
            ActivePlacement {
                lang,
                placement,
                question: question.clone(),
            },
        );
        Ok(question)
    }

    /// Answers the current question: the next one, or, when the test ends, the outcome, with
    /// the tested-out skills recorded.
    pub fn placement_answer(
        &mut self,
        session_id: &str,
        answer: &Answer,
    ) -> Result<PlacementStep, AppError> {
        let store = open_store(&mut self.store)?;
        let active = self
            .placements
            .get_mut(session_id)
            .ok_or_else(not_available)?;
        let lang = active.lang;
        let course = self.content.get(&lang).ok_or_else(not_available)?;
        let exercise = &active.question.exercise;
        let right = match answer {
            Answer::Text(text) if !normalise(lang, text).is_empty() => {
                let ctx = grade_ctx(
                    store,
                    &course.forms,
                    self.settings.accent(),
                    exercise,
                    answer,
                )?;
                matches!(
                    grade(&ctx, exercise, answer).verdict,
                    Verdict::Correct | Verdict::Almost | Verdict::Typo
                )
            }
            _ => false,
        };
        active.placement.answer(right);
        let n = active.question.n + 1;
        if let Some(next) = next_question(course, &mut active.placement, session_id, n) {
            active.question = next.clone();
            return Ok(PlacementStep {
                next: Some(next),
                result: None,
            });
        }
        let result = active
            .placement
            .result()
            .expect("a placement test without a next question is finished");
        store.apply_placement(
            lang,
            &result,
            self.env.clock.now_ms(),
            session_id,
            &self.settings.srs(),
        )?;
        self.placements.remove(session_id);
        let skills = store.skills(lang)?;
        let start = skills
            .get(result.estimate.min(skills.len().saturating_sub(1)))
            .map(|skill| skill.id.clone())
            .unwrap_or_default();
        let (start_unit, start_unit_title) = course
            .syllabus
            .units
            .iter()
            .enumerate()
            .find(|(_, unit)| unit.skills.iter().any(|skill| skill.id == start))
            .map_or((1, String::new()), |(index, unit)| {
                (index + 1, unit.title.clone())
            });
        Ok(PlacementStep {
            next: None,
            result: Some(PlacementOutcome {
                estimate: result.estimate,
                label: result.label,
                tested_out: result.tested_out.len(),
                start_skill_id: start,
                start_unit,
                start_unit_title,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::parley;
    use crate::exercise::Body;
    use crate::grading::Answer;

    #[test]
    fn a_beginner_s_test_ends_at_the_first_skill() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = parley(dir.path());
        assert_eq!(
            app.placement_start("es").unwrap_err().message,
            "This course has not been started."
        );
        app.course_start("es").unwrap();
        let first = app.placement_start("es").unwrap();
        assert!(first.session_id.starts_with("p-"));
        assert_eq!((first.n, first.exercise.id.as_str()), (1, "p1"));
        let Body::TranslateToEn { prompt_target, .. } = &first.exercise.body else {
            panic!("{:?}", first.exercise);
        };
        assert_eq!(prompt_target, "hola");
        // One skill: four empty answers at the first skill end the test.
        let mut step = app
            .placement_answer(&first.session_id, &Answer::Text(String::new()))
            .unwrap();
        let mut asked = 1;
        while let Some(next) = step.next {
            asked += 1;
            step = app
                .placement_answer(&next.session_id, &Answer::Text(String::new()))
                .unwrap();
        }
        assert_eq!(asked, 4);
        let outcome = step.result.unwrap();
        assert_eq!(outcome.label, "Pre-A1");
        assert_eq!((outcome.start_unit, outcome.tested_out), (1, 0));
        assert_eq!(outcome.start_skill_id, "es.a1.u01.s01");
        assert!(app.course_list().unwrap()[0].placement_done);
        assert_eq!(
            app.placement_answer(&first.session_id, &Answer::Text("x".into()))
                .unwrap_err()
                .message,
            "This exercise is no longer available."
        );
    }
}
