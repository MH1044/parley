//! The placement test: `placement_start` and `placement_answer` (module 02's staircase over the
//! course's skills; docs/modules/04-app-shell-and-offline-course.md, Design notes).
//!
//! The staircase may visit a skill many times. Each visit asks one of the skill's vocabulary items
//! (else its other items), drawn with the injected generator from the item-and-form pairs this
//! test has not asked yet: `translate_to_en` while any item is left in that form, then
//! `translate_to_target`, so no item is asked twice in the same form. A skill with nothing left to
//! ask gets its most recent answer again, without a question. Answers are graded offline at the
//! settings' accent leniency: correct, almost and typo count as right, an empty answer as wrong.
//! Nothing is stored until the test ends; then the tested-out skills are recorded with the
//! placement's session id.

use std::collections::HashMap;

use super::{ActivePlacement, Course, Parley, not_available, not_started, open_store, session_id};
use crate::api::{PlacementOutcome, PlacementQuestion, PlacementStep};
use crate::content::{Item, ItemKind, Lang};
use crate::env::{Rng, below};
use crate::error::AppError;
use crate::exercise::{Body, Exercise, ExerciseType, Origin};
use crate::grading::{Answer, Verdict, grade, grade_ctx, normalise};
use crate::srs::{Placement, SkillRef};

/// The forms a placement question takes, in the order a skill's visits use them.
const FORMS: [ExerciseType; 2] = [ExerciseType::TranslateToEn, ExerciseType::TranslateToTarget];

/// What a placement test in progress has asked about one skill.
#[derive(Debug, Default)]
pub(crate) struct Asked {
    /// The item-and-form pairs asked so far: an item id and the exercise type it was asked as.
    pairs: Vec<(String, ExerciseType)>,
    /// The learner's most recent answer to a question about the skill.
    last: Option<bool>,
}

/// `item` asked as `form`: a `translate_to_en` with the item's English answers, or a typed
/// `translate_to_target` of its first English meaning (none for an item without one).
fn body(item: &Item, form: ExerciseType) -> Option<Body> {
    match form {
        ExerciseType::TranslateToEn => Some(Body::TranslateToEn {
            prompt_target: item.target.clone(),
            accepted: item.accepted.to_en.clone(),
        }),
        ExerciseType::TranslateToTarget => Some(Body::TranslateToTarget {
            prompt_en: item.en.first()?.clone(),
            target: item.target.clone(),
            accepted: item.accepted.to_target.clone(),
            tiles: None,
        }),
        _ => None,
    }
}

/// The question of a visit to `skill`, numbered `n`, over the skill's vocabulary items (all its
/// items when it has none): the first form of [`FORMS`] that some item has not been asked in,
/// with the item drawn at random among those. So every item is asked in English before any comes
/// back, and none comes back in a form it was asked in. None when nothing is left to ask, or the
/// skill has no items.
fn question_for(
    course: &Course,
    skill: &SkillRef,
    asked: &Asked,
    n: usize,
    rng: &mut dyn Rng,
) -> Option<Exercise> {
    let in_skill = || course.items.iter().filter(|item| item.skill == skill.id);
    let mut items: Vec<&Item> = in_skill()
        .filter(|item| item.kind == ItemKind::Vocab)
        .collect();
    if items.is_empty() {
        items = in_skill().collect();
    }
    FORMS.into_iter().find_map(|form| {
        let mut open: Vec<(&Item, Body)> = items
            .iter()
            .filter(|item| {
                !asked
                    .pairs
                    .iter()
                    .any(|(id, kind)| *id == item.id && *kind == form)
            })
            .filter_map(|item| Some((*item, body(item, form)?)))
            .collect();
        if open.is_empty() {
            return None;
        }
        let (item, chosen) = open.swap_remove(below(rng, open.len()));
        Some(Exercise {
            id: format!("p{n}"),
            item_id: item.id.clone(),
            facet: form.facet(),
            origin: Origin::Template,
            body: chosen,
        })
    })
}

/// The next question of a test in progress, numbered `n`, recorded in `asked`. A skill with
/// nothing left to ask gets its most recent answer again without a question (a skill without
/// items, never answered, is answered wrong), and the staircase moves on. None once the test is
/// finished.
fn next_question(
    course: &Course,
    placement: &mut Placement,
    asked: &mut HashMap<String, Asked>,
    rng: &mut dyn Rng,
    id: &str,
    n: usize,
) -> Option<PlacementQuestion> {
    while let Some(skill) = placement.next_question() {
        let seen = asked.entry(skill.id.clone()).or_default();
        match question_for(course, &skill, seen, n, rng) {
            Some(exercise) => {
                seen.pairs.push((exercise.item_id.clone(), exercise.kind()));
                return Some(PlacementQuestion {
                    session_id: id.to_string(),
                    n,
                    skill_id: skill.id,
                    exercise,
                });
            }
            None => placement.answer(seen.last.unwrap_or(false)),
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
        let mut asked = HashMap::new();
        let rng = &mut *self.env.rng;
        let id = session_id("p", rng);
        let question = next_question(course, &mut placement, &mut asked, rng, &id, 1)
            .ok_or_else(not_started)?;
        self.placements.insert(
            id,
            ActivePlacement {
                lang,
                placement,
                question: question.clone(),
                asked,
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
        active
            .asked
            .entry(active.question.skill_id.clone())
            .or_default()
            .last = Some(right);
        active.placement.answer(right);
        let n = active.question.n + 1;
        if let Some(next) = next_question(
            course,
            &mut active.placement,
            &mut active.asked,
            &mut *self.env.rng,
            session_id,
            n,
        ) {
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
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::AtomicI64;

    use super::super::test_support::{MONDAY_NOON, NO_PACKS, TestClock, TestFs, config, parley};
    use super::super::{Env, Parley};
    use crate::api::{PlacementOutcome, PlacementQuestion};
    use crate::content::{Cefr, Lang};
    use crate::env::{FailingNet, SplitMix64};
    use crate::exercise::{Body, ExerciseType};
    use crate::grading::Answer;
    use crate::srs::{Facet, Placement, SkillRef};

    const TO_EN: ExerciseType = ExerciseType::TranslateToEn;
    const TO_TARGET: ExerciseType = ExerciseType::TranslateToTarget;

    /// The vocabulary of the test courses below: Spanish number words and their English.
    const WORDS: [(&str, &str); 15] = [
        ("uno", "one"),
        ("dos", "two"),
        ("tres", "three"),
        ("cuatro", "four"),
        ("cinco", "five"),
        ("seis", "six"),
        ("siete", "seven"),
        ("ocho", "eight"),
        ("nueve", "nine"),
        ("diez", "ten"),
        ("once", "eleven"),
        ("doce", "twelve"),
        ("trece", "thirteen"),
        ("catorce", "fourteen"),
        ("quince", "fifteen"),
    ];

    /// The id of skill `index` (from 0) of a test course.
    fn skill(index: usize) -> String {
        format!("es.a1.u01.s{:02}", index + 1)
    }

    /// A Spanish syllabus of one unit whose skill `i` has `sizes[i]` vocabulary items (number
    /// words, none shared, taken in order) and no examples.
    fn syllabus(sizes: &[usize]) -> String {
        let mut text = String::from(
            "schema: parley.syllabus/1\nlang: es\nname: Counting\nvariety: Spain Spanish\n\
             version: 1\nfunction_words: []\nnames: []\nunits:\n  - id: es.a1.u01\n    \
             title: Numbers\n    cefr: A1\n    skills:\n",
        );
        let mut words = WORDS.iter();
        for (index, size) in sizes.iter().enumerate() {
            text.push_str(&format!(
                "      - id: {}\n        kind: vocab\n        title: Numbers {}\n        \
                 cefr: A1\n        can_do: I can count.\n        requires: []\n        lemmas:\n",
                skill(index),
                index + 1
            ));
            for (lemma, en) in words.by_ref().take(*size) {
                text.push_str(&format!(
                    "          - {{lemma: {lemma}, pos: num, en: [{en}], rank: null}}\n"
                ));
            }
            text.push_str("        examples: []\n");
        }
        text
    }

    /// A `Parley` whose Spanish course is a test course of `sizes`, started, with the generator
    /// seeded `seed`.
    fn started(dir: &Path, sizes: &[usize], seed: u64) -> Parley {
        let mut config = config(dir, NO_PACKS);
        if let Some(files) = config.courses.get_mut(&Lang::Es) {
            files.syllabus_yaml = syllabus(sizes);
        }
        let mut app = Parley::new(
            config,
            Env {
                clock: Box::new(TestClock(Arc::new(AtomicI64::new(MONDAY_NOON)), 0)),
                rng: Box::new(SplitMix64::new(seed)),
                fs: Box::new(TestFs(dir.to_path_buf())),
                net: Box::new(FailingNet),
            },
        );
        app.course_start("es").unwrap();
        app
    }

    /// The answer the question accepts first.
    fn right_answer(question: &PlacementQuestion) -> String {
        match &question.exercise.body {
            Body::TranslateToEn { accepted, .. } | Body::TranslateToTarget { accepted, .. } => {
                accepted[0].clone()
            }
            other => panic!("a placement question is a translation: {other:?}"),
        }
    }

    /// A whole placement test of the Spanish course: the learner answers a question for which
    /// `knows` is true with its first accepted answer, and any other with "I don't know" (an
    /// empty text). Every question asked, in order, and the outcome.
    fn take(
        app: &mut Parley,
        mut knows: impl FnMut(&PlacementQuestion) -> bool,
    ) -> (Vec<PlacementQuestion>, PlacementOutcome) {
        let mut question = app.placement_start("es").unwrap();
        let mut asked = Vec::new();
        loop {
            let text = if knows(&question) {
                right_answer(&question)
            } else {
                String::new()
            };
            let step = app
                .placement_answer(&question.session_id, &Answer::Text(text))
                .unwrap();
            asked.push(question);
            match (step.next, step.result) {
                (Some(next), None) => question = next,
                (None, Some(outcome)) => return (asked, outcome),
                other => panic!("a step carries exactly one of next and result: {other:?}"),
            }
        }
    }

    /// The skill index of a question about a test course.
    fn index(question: &PlacementQuestion) -> usize {
        (0..WORDS.len())
            .find(|&i| skill(i) == question.skill_id)
            .unwrap_or_else(|| panic!("{} is a test skill", question.skill_id))
    }

    /// The number word a question is about.
    fn lemma(question: &PlacementQuestion) -> &str {
        let prefix = format!("{}#l:", question.skill_id);
        question
            .exercise
            .item_id
            .strip_prefix(&prefix)
            .unwrap_or_else(|| panic!("a vocabulary item of the skill: {question:?}"))
    }

    /// Each question of a test course is numbered in order and is a translation of one of its
    /// skill's number words, into English (receptive) or into Spanish, typed (productive); no
    /// item is asked twice in the same form.
    fn check_questions(asked: &[PlacementQuestion]) {
        let mut pairs = BTreeSet::new();
        for (i, question) in asked.iter().enumerate() {
            let ex = &question.exercise;
            assert_eq!((question.n, ex.id.clone()), (i + 1, format!("p{}", i + 1)));
            let word = lemma(question);
            let (_, en) = WORDS
                .iter()
                .find(|(w, _)| *w == word)
                .unwrap_or_else(|| panic!("{word} is a test word"));
            match &ex.body {
                Body::TranslateToEn {
                    prompt_target,
                    accepted,
                } => {
                    assert_eq!(ex.facet, Facet::Receptive);
                    assert_eq!(
                        (prompt_target.as_str(), accepted.clone()),
                        (word, vec![en.to_string()])
                    );
                }
                Body::TranslateToTarget {
                    prompt_en,
                    target,
                    accepted,
                    tiles,
                } => {
                    assert_eq!(ex.facet, Facet::Productive);
                    assert_eq!((prompt_en.as_str(), target.as_str()), (*en, word));
                    assert_eq!(accepted, &vec![word.to_string()]);
                    assert_eq!(tiles, &None, "a placement answer is typed");
                }
                other => panic!("a placement question is a translation: {other:?}"),
            }
            assert!(
                pairs.insert((ex.item_id.clone(), ex.kind())),
                "question {} asks {} as {} again",
                question.n,
                ex.item_id,
                ex.kind()
            );
        }
    }

    /// The questions about skill `at` asked every one of its `size` items in English, in some
    /// order, before any came back; the ones that came back were asked in Spanish
    /// (`check_questions` shows each only once).
    fn check_visits(asked: &[PlacementQuestion], at: usize, size: usize) {
        let visits: Vec<&PlacementQuestion> = asked.iter().filter(|q| index(q) == at).collect();
        assert!(
            visits.len() > size,
            "skill {at} is asked about more than {size} times"
        );
        let (first, again) = visits.split_at(size);
        assert!(
            first.iter().all(|q| q.exercise.kind() == TO_EN),
            "{first:?}"
        );
        let words: BTreeSet<&str> = first.iter().map(|q| lemma(q)).collect();
        assert_eq!(
            words.len(),
            size,
            "every item of skill {at} before a repeat"
        );
        assert!(
            again.iter().all(|q| q.exercise.kind() == TO_TARGET),
            "{again:?}"
        );
        assert!(again.iter().all(|q| words.contains(lemma(q))));
    }

    /// The skills of a test course as module 02's staircase takes them.
    fn skill_refs(count: usize) -> Vec<SkillRef> {
        (0..count)
            .map(|i| SkillRef {
                id: skill(i),
                cefr: Cefr::A1,
            })
            .collect()
    }

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
        // One of the vocabulary items of Greetings, the example syllabus's only skill.
        assert!(first.exercise.item_id.starts_with("es.a1.u01.s01#l:"));
        assert!(
            [
                "hola", "adiós", "bueno", "el día", "la noche", "tal", "gracias"
            ]
            .contains(&prompt_target.as_str()),
            "{prompt_target}"
        );
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

    #[test]
    fn a_learner_who_answers_everything_wrong_never_sees_an_item_twice_in_the_same_form() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = started(dir.path(), &[2, 3, 3, 3], 7);
        let (asked, outcome) = take(&mut app, |_| false);
        check_questions(&asked);
        // Module 02 over four skills starts at index 1 with step 1: one wrong answer there, then
        // four in a row at index 0 end the test.
        let skills: Vec<usize> = asked.iter().map(index).collect();
        assert_eq!(skills, [1, 0, 0, 0, 0]);
        // The first skill has two items: both in English, then both in Spanish.
        let forms: Vec<ExerciseType> = asked[1..].iter().map(|q| q.exercise.kind()).collect();
        assert_eq!(forms, [TO_EN, TO_EN, TO_TARGET, TO_TARGET]);
        let in_english: BTreeSet<&str> = asked[1..3].iter().map(lemma).collect();
        let in_spanish: BTreeSet<&str> = asked[3..].iter().map(lemma).collect();
        assert_eq!(in_english, BTreeSet::from(["dos", "uno"]));
        assert_eq!(in_spanish, in_english);
        assert_eq!(
            (outcome.estimate, outcome.label.as_str(), outcome.tested_out),
            (0, "Pre-A1", 0)
        );
    }

    #[test]
    fn a_learner_who_reverses_around_a_skill_gets_its_other_items_before_a_repeat() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = started(dir.path(), &[2, 4, 3, 2], 7);
        // A learner who knows the first two skills: the staircase alternates between indices 1
        // and 2, every answer after the first a reversal, and stops at 12 questions with 11
        // reversals. Estimate: the mean of the reversals after the first two, 2, 1, 2, 1, 2, 1,
        // 2, 1, 2, is (2·14 + 9) / 18 = 2.
        let (asked, outcome) = take(&mut app, |q| index(q) < 2);
        check_questions(&asked);
        let skills: Vec<usize> = asked.iter().map(index).collect();
        assert_eq!(skills, [1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2]);
        // Six visits each: skill 1's four items, then two of them in Spanish; skill 2's three
        // items, then all three in Spanish.
        check_visits(&asked, 1, 4);
        check_visits(&asked, 2, 3);
        assert_eq!(
            (outcome.estimate, outcome.label.as_str(), outcome.tested_out),
            (2, "Pre-A1", 2)
        );
        assert_eq!(outcome.start_skill_id, skill(2));
    }

    #[test]
    fn once_a_skill_s_items_are_all_asked_in_english_the_next_visit_asks_in_spanish() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = started(dir.path(), &[3, 3, 3, 3], 7);
        // Wrong in English and right in Spanish: index 1, then index 0 three times (its three
        // items in English); the fourth visit there asks in Spanish.
        let (asked, _) = take(&mut app, |q| q.exercise.kind() == TO_TARGET);
        check_questions(&asked);
        let skills: Vec<usize> = asked.iter().map(index).collect();
        assert_eq!(skills[..5], [1, 0, 0, 0, 0]);
        let in_english: BTreeSet<&str> = asked[1..4].iter().map(lemma).collect();
        assert_eq!(in_english, BTreeSet::from(["dos", "tres", "uno"]));
        assert!(asked[1..4].iter().all(|q| q.exercise.kind() == TO_EN));
        let fifth = &asked[4];
        assert_eq!(fifth.exercise.kind(), TO_TARGET);
        assert!(in_english.contains(lemma(fifth)));
        // The Spanish answer is graded offline and counts as right: the staircase moves up.
        assert_eq!(index(&asked[5]), 1);
    }

    #[test]
    fn a_skill_with_nothing_left_to_ask_is_answered_from_its_last_answer() {
        // The first skill has one item: one question in English, one in Spanish, then nothing.
        let dir = tempfile::tempdir().unwrap();
        let mut app = started(dir.path(), &[1, 3, 3, 3], 7);
        // A learner who knows only the first skill. The staircase alternates between indices 1
        // and 0; from the third visit on, index 0 has nothing left and gets its last answer,
        // right, again without a question, so the staircase climbs straight back to index 1.
        // Twelve answers, eight of them asked.
        let (asked, outcome) = take(&mut app, |q| index(q) < 1);
        check_questions(&asked);
        let skills: Vec<usize> = asked.iter().map(index).collect();
        assert_eq!(skills, [1, 0, 1, 0, 1, 1, 1, 1]);
        assert_eq!((lemma(&asked[1]), asked[1].exercise.kind()), ("uno", TO_EN));
        assert_eq!(
            (lemma(&asked[3]), asked[3].exercise.kind()),
            ("uno", TO_TARGET)
        );
        check_visits(&asked, 1, 3);
        // Module 02's staircase given the same twelve answers, the four unasked ones included.
        let mut oracle = Placement::new(skill_refs(4));
        for knows in [false, true].repeat(6) {
            assert!(oracle.next_question().is_some());
            oracle.answer(knows);
        }
        let reference = oracle
            .result()
            .expect("twelve answers with eleven reversals end it");
        assert_eq!(reference.questions, [1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0]);
        assert_eq!(
            (outcome.estimate, outcome.label, outcome.tested_out),
            (
                reference.estimate,
                reference.label,
                reference.tested_out.len()
            )
        );

        // When that last answer was wrong, the staircase gets a wrong answer: one question at
        // index 1, the item in English and in Spanish at index 0, then two answers there without
        // a question make four wrong in a row at index 0, which ends the test.
        let dir = tempfile::tempdir().unwrap();
        let mut app = started(dir.path(), &[1, 3, 3, 3], 7);
        let (asked, outcome) = take(&mut app, |_| false);
        check_questions(&asked);
        let skills: Vec<usize> = asked.iter().map(index).collect();
        assert_eq!(skills, [1, 0, 0]);
        assert_eq!(
            (outcome.estimate, outcome.label.as_str(), outcome.tested_out),
            (0, "Pre-A1", 0)
        );
    }

    #[test]
    fn the_same_seed_asks_the_same_questions() {
        let questions = |seed: u64| {
            let dir = tempfile::tempdir().unwrap();
            let mut app = started(dir.path(), &[2, 4, 3, 2], seed);
            let (asked, _) = take(&mut app, |q| index(q) < 2);
            asked
                .into_iter()
                .map(|q| {
                    let kind = q.exercise.kind();
                    (q.skill_id, q.exercise.item_id, kind)
                })
                .collect::<Vec<_>>()
        };
        let first = questions(11);
        assert_eq!(first.len(), 12);
        assert_eq!(questions(11), first);
        // The items come from the generator: other seeds ask them in other orders.
        assert!((12..=16).any(|seed| questions(seed) != first));
    }
}
