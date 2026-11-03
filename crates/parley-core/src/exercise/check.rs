//! Structural checks of an exercise, L5 to L9 (docs/modules/03-exercise-engine-and-grading.md,
//! Design notes), shared with AI lesson validation in module 07: vocabulary coverage, answers,
//! duplicates, length and focus. They take the exercise as data, so a template exercise and an
//! AI one are held to the same rules.

use std::fmt;

use super::types::{Body, Exercise, ExerciseType};
use crate::content::validate::Known;
use crate::content::{Cefr, Forms, Lang, Lemma, Syllabus, TaughtForms, Token, tokenise};
use crate::grading::normalise;
use crate::grading::normalise::words;

/// The words a lesson of one skill may use: the syllabus's function words, names and number
/// words, and every lemma of the skills up to and including that skill, in syllabus order.
#[derive(Clone, Debug)]
pub struct Allowed {
    known: Known,
    taught: Vec<TaughtForms>,
    /// The allowed lemmas as the syllabus writes them, in syllabus order.
    lemmas: Vec<String>,
    function_words: Vec<String>,
}

impl Allowed {
    /// The allowed words for a lesson of `skill_id`. A skill the syllabus does not have allows
    /// no lemma, only the function words, names and numbers.
    pub fn for_skill(syllabus: &Syllabus, numbers: &[String], skill_id: &str) -> Allowed {
        let mut taught = Vec::new();
        let mut lemmas = Vec::new();
        if let Some(last) = syllabus
            .skills()
            .position(|(_, skill)| skill.id == skill_id)
        {
            for (_, skill) in syllabus.skills().take(last + 1) {
                for lemma in &skill.lemmas {
                    taught.push(TaughtForms::of(lemma));
                    lemmas.push(lemma.lemma.clone());
                }
            }
        }
        Allowed {
            known: Known::new(syllabus, numbers),
            taught,
            lemmas,
            function_words: syllabus.function_words.clone(),
        }
    }

    /// Module 01's V12 rule: a number, a name, a function word, or a form of an allowed lemma
    /// (`TaughtForms::matches`, so each word of a multiword lemma counts).
    pub fn allows(&self, token: &Token) -> bool {
        self.known.is_number(token)
            || self.known.is_name(token)
            || self.known.is_function(token)
            || self.taught.iter().any(|forms| forms.matches(token))
    }

    /// Whether coverage leaves the token out: a number token, a word of `numbers.yaml`, or a
    /// name of the syllabus.
    pub fn is_excluded(&self, token: &Token) -> bool {
        self.known.is_number(token) || self.known.is_name(token)
    }

    /// The allowed lemmas as written, in syllabus order.
    pub(crate) fn lemmas(&self) -> &[String] {
        &self.lemmas
    }

    /// The syllabus's function words as written.
    pub(crate) fn function_words(&self) -> &[String] {
        &self.function_words
    }
}

/// How many counted tokens of `text` the allowed words cover: (covered, counted). Numbers and
/// names are not counted.
pub fn coverage(lang: Lang, text: &str, allowed: &Allowed, forms: &Forms) -> (usize, usize) {
    let mut covered = 0;
    let mut counted = 0;
    for token in tokenise(lang, text, forms) {
        if allowed.is_excluded(&token) {
            continue;
        }
        counted += 1;
        if allowed.allows(&token) {
            covered += 1;
        }
    }
    (covered, counted)
}

/// The most words a main text may have at a CEFR level: A1 8, A2 12.
pub fn max_words(cefr: Cefr) -> usize {
    match cefr {
        Cefr::A1 => 8,
        Cefr::A2 => 12,
    }
}

/// What the checks know besides the exercise.
#[derive(Clone, Copy, Debug)]
pub struct CheckCtx<'a> {
    pub lang: Lang,
    pub forms: &'a Forms,
    pub allowed: &'a Allowed,
    /// The most words of a main text (L8).
    pub max_words: usize,
    /// The lemmas the exercise must practise (L9); none means any.
    pub focus: &'a [Lemma],
    /// The type and first main text of each earlier exercise of the lesson (L7).
    pub seen: &'a [(ExerciseType, String)],
}

/// The first check an exercise fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckFailure {
    /// L5: fewer than 90% of the counted tokens are allowed words.
    Coverage { covered: usize, counted: usize },
    /// L6: the answers do not fit the exercise; the text says how.
    Answers(String),
    /// L7: an earlier exercise has the same type and main text.
    Duplicate,
    /// L8: a main text is too long for the level.
    TooLong { words: usize, max: usize },
    /// L9: no main text uses a focus lemma.
    Focus,
}

impl CheckFailure {
    /// The rule's name: `L5` to `L9`.
    pub fn rule(&self) -> &'static str {
        match self {
            CheckFailure::Coverage { .. } => "L5",
            CheckFailure::Answers(_) => "L6",
            CheckFailure::Duplicate => "L7",
            CheckFailure::TooLong { .. } => "L8",
            CheckFailure::Focus => "L9",
        }
    }
}

impl fmt::Display for CheckFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckFailure::Coverage { covered, counted } => write!(
                f,
                "L5: {covered} of {counted} words are taught by this skill or an earlier one"
            ),
            CheckFailure::Answers(why) => write!(f, "L6: {why}"),
            CheckFailure::Duplicate => f.write_str("L7: an earlier exercise is the same"),
            CheckFailure::TooLong { words, max } => {
                write!(f, "L8: {words} words; the limit is {max}")
            }
            CheckFailure::Focus => f.write_str("L9: no text uses the lemma being practised"),
        }
    }
}

/// The share of covered words L5 asks for, as a fraction: 9 in 10.
const COVERAGE_NUMERATOR: usize = 9;
const COVERAGE_DENOMINATOR: usize = 10;

fn answers(why: &str) -> CheckFailure {
    CheckFailure::Answers(why.to_string())
}

/// Whether the normal forms of `texts` are all non-empty and distinct.
fn distinct(lang: Lang, texts: impl Iterator<Item = String>) -> bool {
    let mut seen: Vec<String> = Vec::new();
    for text in texts {
        let norm = normalise(lang, &text);
        if norm.is_empty() || seen.contains(&norm) {
            return false;
        }
        seen.push(norm);
    }
    true
}

/// L6 for a typed target: at least one accepted answer, and the target's normal form among
/// theirs.
fn target_accepted(lang: Lang, target: &str, accepted: &[String]) -> Result<(), CheckFailure> {
    if accepted.is_empty() {
        return Err(answers("no accepted answer"));
    }
    let target = normalise(lang, target);
    if accepted.iter().any(|a| normalise(lang, a) == target) {
        Ok(())
    } else {
        Err(answers("the target is not among the accepted answers"))
    }
}

/// L6, the answers of each type.
fn check_answers(ex: &Exercise, lang: Lang) -> Result<(), CheckFailure> {
    match &ex.body {
        Body::TranslateToTarget {
            target, accepted, ..
        } => target_accepted(lang, target, accepted),
        Body::ListenType { text, accepted } => target_accepted(lang, text, accepted),
        Body::TranslateToEn { accepted, .. } => {
            if accepted.iter().any(|a| !normalise(lang, a).is_empty()) {
                Ok(())
            } else {
                Err(answers("no accepted answer"))
            }
        }
        Body::FillGap {
            sentence,
            answer,
            full_sentence,
            ..
        } => {
            if sentence.matches("___").count() != 1 {
                return Err(answers("the sentence needs exactly one ___"));
            }
            if words(&normalise(lang, answer)).len() != 1 {
                return Err(answers("the answer must be one word"));
            }
            let filled = sentence.replacen("___", answer, 1);
            if normalise(lang, &filled) == normalise(lang, full_sentence) {
                Ok(())
            } else {
                Err(answers(
                    "the answer in the gap does not give the full sentence",
                ))
            }
        }
        Body::PickMeaning {
            options,
            answer_index,
            ..
        } => {
            if options.len() != 4 {
                return Err(answers("pick_meaning needs 4 options"));
            }
            if !distinct(lang, options.iter().cloned()) {
                return Err(answers("the options must differ"));
            }
            if *answer_index > 3 {
                return Err(answers("answer_index must be 0 to 3"));
            }
            Ok(())
        }
        Body::MatchPairs { pairs } => {
            if !(4..=5).contains(&pairs.len()) {
                return Err(answers("match_pairs needs 4 or 5 pairs"));
            }
            let left = distinct(lang, pairs.iter().map(|pair| pair[0].clone()));
            let right = distinct(lang, pairs.iter().map(|pair| pair[1].clone()));
            if left && right {
                Ok(())
            } else {
                Err(answers("the pairs must differ on each side"))
            }
        }
        Body::Speak { .. } => Ok(()),
    }
}

/// Runs L5 to L9 in order and returns the first failure.
///
/// L5: at least 90% of the counted tokens of the main texts (and the accepted answers of
/// `translate_to_target` and `listen_type`) are allowed; nothing counted passes. L6: the answers
/// fit the type. L7: no earlier exercise in `seen` has the same type and the same normal form of
/// its first main text (`match_pairs` is exempt). L8: every main text has at most `max_words`
/// words. L9: with a focus, a token of a main text is a form of a focus lemma.
pub fn check_exercise(ex: &Exercise, ctx: &CheckCtx) -> Result<(), CheckFailure> {
    let lang = ctx.lang;
    let targets = ex.targets();

    // L5
    let mut texts: Vec<&str> = targets.clone();
    match &ex.body {
        Body::TranslateToTarget { accepted, .. } | Body::ListenType { accepted, .. } => {
            texts.extend(accepted.iter().map(String::as_str));
        }
        _ => {}
    }
    let (mut covered, mut counted) = (0, 0);
    for text in texts {
        let (c, n) = coverage(lang, text, ctx.allowed, ctx.forms);
        covered += c;
        counted += n;
    }
    if counted > 0 && covered * COVERAGE_DENOMINATOR < counted * COVERAGE_NUMERATOR {
        return Err(CheckFailure::Coverage { covered, counted });
    }

    // L6
    check_answers(ex, lang)?;

    // L7
    let kind = ex.kind();
    if kind != ExerciseType::MatchPairs
        && let Some(first) = targets.first()
    {
        let first = normalise(lang, first);
        if ctx
            .seen
            .iter()
            .any(|(seen_kind, text)| *seen_kind == kind && normalise(lang, text) == first)
        {
            return Err(CheckFailure::Duplicate);
        }
    }

    // L8
    for text in &targets {
        let count = words(&normalise(lang, text)).len();
        if count > ctx.max_words {
            return Err(CheckFailure::TooLong {
                words: count,
                max: ctx.max_words,
            });
        }
    }

    // L9
    if !ctx.focus.is_empty() {
        let focus: Vec<TaughtForms> = ctx.focus.iter().map(TaughtForms::of).collect();
        let used = targets.iter().any(|text| {
            tokenise(lang, text, ctx.forms)
                .iter()
                .any(|token| focus.iter().any(|forms| forms.matches(token)))
        });
        if !used {
            return Err(CheckFailure::Focus);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::test_support::{EXAMPLE_SYLLABUS, example_forms};
    use crate::content::{Pos, parse_syllabus};
    use crate::exercise::types::Origin;
    use crate::srs::Facet;

    fn syllabus() -> Syllabus {
        parse_syllabus("valid-es.yaml", EXAMPLE_SYLLABUS).unwrap()
    }

    fn exercise(body: Body) -> Exercise {
        Exercise {
            id: "e1".into(),
            item_id: "es.a1.u01.s01#e:1".into(),
            facet: Facet::Receptive,
            origin: Origin::Template,
            body,
        }
    }

    fn lemma(text: &str) -> Lemma {
        Lemma {
            lemma: text.into(),
            pos: Pos::Noun,
            gender: None,
            en: vec!["x".into()],
            rank: None,
            separable: None,
            article: None,
        }
    }

    fn check(
        ex: &Exercise,
        focus: &[Lemma],
        seen: &[(ExerciseType, String)],
    ) -> Result<(), CheckFailure> {
        let s = syllabus();
        let forms = example_forms();
        let allowed = Allowed::for_skill(&s, &[], "es.a1.u01.s01");
        let ctx = CheckCtx {
            lang: Lang::Es,
            forms: &forms,
            allowed: &allowed,
            max_words: 8,
            focus,
            seen,
        };
        check_exercise(ex, &ctx)
    }

    fn to_en(text: &str) -> Exercise {
        exercise(Body::TranslateToEn {
            prompt_target: text.into(),
            accepted: vec!["Good morning.".into()],
        })
    }

    #[test]
    fn every_example_of_a_valid_syllabus_is_covered() {
        let s = syllabus();
        let forms = example_forms();
        let allowed = Allowed::for_skill(&s, &[], "es.a1.u01.s01");
        for (_, skill) in s.skills() {
            for example in &skill.examples {
                let (covered, counted) = coverage(Lang::Es, &example.target, &allowed, &forms);
                assert_eq!(covered, counted, "{}", example.target);
            }
        }
        let (covered, counted) = coverage(Lang::Es, "Hola, Ana, tengo 3 perros.", &allowed, &forms);
        assert_eq!((covered, counted), (1, 3), "Ana and 3 are not counted");
        let none = Allowed::for_skill(&s, &[], "es.a1.u09.s01");
        assert_eq!(coverage(Lang::Es, "hola", &none, &forms), (0, 1));
        assert_eq!(coverage(Lang::Es, "que", &none, &forms), (1, 1));
    }

    #[test]
    fn coverage_below_ninety_percent_fails_l5() {
        let failure = check(&to_en("Buenos días, tengo perros."), &[], &[]).unwrap_err();
        assert_eq!(
            failure,
            CheckFailure::Coverage {
                covered: 2,
                counted: 4
            }
        );
        assert_eq!(failure.rule(), "L5");
        assert_eq!(check(&to_en("Buenos días."), &[], &[]), Ok(()));
        assert_eq!(
            check(&to_en("Ana, 3."), &[], &[]),
            Ok(()),
            "nothing counted passes"
        );
    }

    #[test]
    fn answers_must_fit_the_type() {
        let missing = exercise(Body::TranslateToTarget {
            prompt_en: "Good morning.".into(),
            target: "Buenos días.".into(),
            accepted: vec!["Buenas noches.".into()],
            tiles: None,
        });
        assert_eq!(check(&missing, &[], &[]).unwrap_err().rule(), "L6");
        let fine = exercise(Body::TranslateToTarget {
            prompt_en: "Good morning.".into(),
            target: "Buenos días.".into(),
            accepted: vec!["buenos días".into()],
            tiles: None,
        });
        assert_eq!(check(&fine, &[], &[]), Ok(()));
        let gap = |sentence: &str, answer: &str| {
            exercise(Body::FillGap {
                sentence: sentence.into(),
                answer: answer.into(),
                full_sentence: "Buenos días.".into(),
                hint_en: "day".into(),
            })
        };
        assert_eq!(check(&gap("Buenos ___.", "días"), &[], &[]), Ok(()));
        assert_eq!(
            check(&gap("Buenos días.", "días"), &[], &[])
                .unwrap_err()
                .rule(),
            "L6"
        );
        assert_eq!(
            check(&gap("___ ___.", "días"), &[], &[])
                .unwrap_err()
                .rule(),
            "L6"
        );
        assert_eq!(
            check(&gap("___.", "Buenos días"), &[], &[])
                .unwrap_err()
                .rule(),
            "L6"
        );
        assert_eq!(
            check(&gap("Buenos ___.", "noches"), &[], &[])
                .unwrap_err()
                .rule(),
            "L6"
        );
        let pick = |options: &[&str], index: u32| {
            exercise(Body::PickMeaning {
                prompt_target: "el día".into(),
                options: options.iter().map(|s| s.to_string()).collect(),
                answer_index: index,
            })
        };
        assert_eq!(
            check(&pick(&["day", "night", "hello", "good"], 0), &[], &[]),
            Ok(())
        );
        for bad in [
            pick(&["day", "night", "hello"], 0),
            pick(&["day", "Day", "hello", "good"], 0),
            pick(&["day", "night", "hello", "good"], 4),
        ] {
            assert_eq!(check(&bad, &[], &[]).unwrap_err().rule(), "L6");
        }
        let pairs = |n: usize| {
            exercise(Body::MatchPairs {
                pairs: ["hola", "adiós", "bueno", "tal", "gracias", "noche"][..n]
                    .iter()
                    .zip(["hello", "goodbye", "good", "such", "thanks", "night"])
                    .map(|(t, e)| [t.to_string(), e.to_string()])
                    .collect(),
            })
        };
        assert_eq!(check(&pairs(4), &[], &[]), Ok(()));
        assert_eq!(check(&pairs(5), &[], &[]), Ok(()));
        assert_eq!(check(&pairs(3), &[], &[]).unwrap_err().rule(), "L6");
        assert_eq!(check(&pairs(6), &[], &[]).unwrap_err().rule(), "L6");
    }

    #[test]
    fn a_repeat_of_an_earlier_exercise_fails_l7() {
        let seen = [(ExerciseType::TranslateToEn, "buenos días".to_string())];
        assert_eq!(
            check(&to_en("Buenos días."), &[], &seen),
            Err(CheckFailure::Duplicate)
        );
        let other_type = [(ExerciseType::ListenType, "Buenos días.".to_string())];
        assert_eq!(check(&to_en("Buenos días."), &[], &other_type), Ok(()));
    }

    #[test]
    fn a_long_text_fails_l8_and_a_missing_focus_l9() {
        let long = to_en("Hola, hola, hola, hola, hola, hola, hola, hola, hola.");
        assert_eq!(
            check(&long, &[], &[]),
            Err(CheckFailure::TooLong { words: 9, max: 8 })
        );
        assert_eq!(check(&to_en("Buenos días."), &[lemma("día")], &[]), Ok(()));
        assert_eq!(
            check(&to_en("Buenos días."), &[lemma("noche")], &[]),
            Err(CheckFailure::Focus)
        );
        assert_eq!(CheckFailure::Focus.rule(), "L9");
        assert_eq!(max_words(Cefr::A1), 8);
        assert_eq!(max_words(Cefr::A2), 12);
    }
}
