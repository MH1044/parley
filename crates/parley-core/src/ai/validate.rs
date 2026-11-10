//! The checks an AI exercise goes through before anyone sees it (docs/PUBLIK-INTEGRATION.md §6.3,
//! L4 to L9; docs/modules/07-lesson-generation.md), shared by fresh lessons and imported course
//! packs, and the coverage count with its Snowball stem fallback (L5, S3).
//!
//! L6 to L9 are module 03's structural checks (`exercise::check`), so a template exercise and an AI
//! one are held to the same answer, length and focus rules. What is added here is what only text
//! from outside needs: the language check (L4) and coverage that also accepts a word form the forms
//! table does not list when its stem is an allowed lemma's stem.

use std::collections::HashSet;

use unicode_segmentation::UnicodeSegmentation;

use super::call::RuleFailure;
use super::langid::{TextLang, is_language_in, stem};
use crate::content::{Forms, Lang, Lemma, Syllabus, nfc_lower, tokenise};
use crate::exercise::check::{CheckCtx, check_structure, coverage_rule, coverage_texts};
use crate::exercise::{Allowed, Body, Exercise, ExerciseType};

/// The Allowed of an AI call: the syllabus's function words, names and numbers, and exactly the
/// lemmas of `allowed_lemmas` (which the prompt sent).
pub fn allowed_from_list(
    syllabus: &Syllabus,
    numbers: &[String],
    allowed_lemmas: &[String],
) -> Allowed {
    Allowed::from_list(syllabus, numbers, allowed_lemmas)
}

/// The Snowball stems of every word of the allowed lemmas and the function words.
pub(crate) fn stems_of(lang: Lang, allowed: &Allowed) -> HashSet<String> {
    allowed
        .lemmas()
        .iter()
        .chain(allowed.function_words())
        .flat_map(|lemma| {
            lemma
                .unicode_words()
                .map(|word| stem(lang, &nfc_lower(word)))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// (covered, counted) with the stems of the allowed lemmas computed once.
pub(crate) fn coverage_counts(
    lang: Lang,
    text: &str,
    allowed: &Allowed,
    stems: &HashSet<String>,
    forms: &Forms,
) -> (usize, usize) {
    let mut covered = 0;
    let mut counted = 0;
    for token in tokenise(lang, text, forms) {
        if allowed.is_excluded(&token) {
            continue;
        }
        counted += 1;
        if allowed.allows(&token) || stems.contains(&stem(lang, &token.norm)) {
            covered += 1;
        }
    }
    (covered, counted)
}

/// (covered, counted) as exercise::coverage, a token also counting as covered when its stem equals
/// the stem of an allowed lemma (each word of a multiword lemma, function words included).
pub fn coverage_with_stems(
    lang: Lang,
    text: &str,
    allowed: &Allowed,
    forms: &Forms,
) -> (usize, usize) {
    coverage_counts(lang, text, allowed, &stems_of(lang, allowed), forms)
}

/// What the checks of one AI exercise need besides the exercise.
pub(crate) struct AiCheck<'a> {
    pub lang: Lang,
    pub forms: &'a Forms,
    pub allowed: &'a Allowed,
    pub stems: &'a HashSet<String>,
    pub max_words: usize,
}

fn failure(rule: &str, detail: impl Into<String>) -> RuleFailure {
    RuleFailure {
        rule: rule.to_string(),
        detail: detail.into(),
    }
}

/// The texts of an exercise in the course language and in English, as L4 checks them.
fn texts_by_language(ex: &Exercise) -> (Vec<&str>, Vec<&str>) {
    let mut target: Vec<&str> = Vec::new();
    let mut english: Vec<&str> = Vec::new();
    match &ex.body {
        Body::PickMeaning {
            prompt_target,
            options,
            ..
        } => {
            target.push(prompt_target);
            english.extend(options.iter().map(String::as_str));
        }
        Body::TranslateToTarget {
            prompt_en,
            target: text,
            accepted,
            ..
        } => {
            english.push(prompt_en);
            target.push(text);
            target.extend(accepted.iter().map(String::as_str));
        }
        Body::TranslateToEn {
            prompt_target,
            accepted,
        } => {
            target.push(prompt_target);
            english.extend(accepted.iter().map(String::as_str));
        }
        Body::ListenType { text, accepted } => {
            target.push(text);
            target.extend(accepted.iter().map(String::as_str));
        }
        Body::Speak { text, prompt_en } => {
            target.push(text);
            english.push(prompt_en);
        }
        Body::FillGap {
            full_sentence,
            hint_en,
            ..
        } => {
            target.push(full_sentence);
            english.push(hint_en);
        }
        Body::MatchPairs { pairs } => {
            for [left, right] in pairs {
                target.push(left);
                english.push(right);
            }
        }
    }
    (target, english)
}

/// L4 to L9 of one exercise, in that order; the first failure. `focus` is the plan entry's item's
/// lemmas (L9); `seen` the type and normal first main text of every exercise kept before it (L7,
/// compared whatever the type, `match_pairs` exempt).
pub(crate) fn check_ai_exercise(
    ex: &Exercise,
    focus: &[Lemma],
    seen: &[(ExerciseType, String)],
    check: &AiCheck,
) -> Result<(), RuleFailure> {
    // L4
    let course = TextLang::from(check.lang);
    let (target, english) = texts_by_language(ex);
    if let Some(text) = target
        .iter()
        .find(|text| !is_language_in(text, course, check.lang))
    {
        return Err(failure("L4", format!("not in the course language: {text}")));
    }
    if let Some(text) = english
        .iter()
        .find(|text| !is_language_in(text, TextLang::En, check.lang))
    {
        return Err(failure("L4", format!("not English: {text}")));
    }
    // L5
    let (mut covered, mut counted) = (0, 0);
    for text in coverage_texts(ex) {
        let (c, n) = coverage_counts(check.lang, text, check.allowed, check.stems, check.forms);
        covered += c;
        counted += n;
    }
    let structure = CheckCtx {
        lang: check.lang,
        forms: check.forms,
        allowed: check.allowed,
        max_words: check.max_words,
        focus,
        seen,
    };
    coverage_rule(covered, counted)
        .and_then(|()| check_structure(ex, &structure, true))
        .map_err(|e| failure(e.rule(), e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::parse_syllabus;
    use crate::content::test_support::{EXAMPLE_SYLLABUS, example_forms};

    #[test]
    fn stems_cover_a_form_the_table_does_not_list() {
        let syllabus = parse_syllabus("valid-es.yaml", EXAMPLE_SYLLABUS).unwrap();
        let forms = example_forms();
        let lemmas: Vec<String> = vec!["noche".into()];
        let allowed = allowed_from_list(&syllabus, &[], &lemmas);
        // "nochecitas" is in no table; its stem is not noche's either.
        assert_eq!(
            coverage_with_stems(Lang::Es, "nochecitas", &allowed, &forms),
            (0, 1)
        );
        // "noches" is in the example table; "noche" is the lemma itself.
        assert_eq!(
            coverage_with_stems(Lang::Es, "noches noche", &allowed, &forms),
            (2, 2)
        );
    }
}
