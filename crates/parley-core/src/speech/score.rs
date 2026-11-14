//! Scoring a speaking check (docs/modules/10-speaking-and-pronunciation.md, Design notes): the
//! words of the expected sentence and of what was heard, their comparison keys, the alignment,
//! the score and pass, and the verdict, rating and grade a check gives.
//!
//! A transcript spells numbers as digits and drops or adds hyphens, accents and case at will, so
//! both texts become words the same way: module 03's normaliser, hyphens between letters split,
//! numbers 0 to 100 spelled from the course's `numbers.yaml`, and marks folded for comparison.

use serde::{Deserialize, Serialize};

use super::align::{Edit, align};
use crate::content::{Lang, Numbers};
use crate::grading::normalise::normalise_cased;
use crate::grading::{Grade, Verdict, fold_marks};
use crate::srs::Rating;

/// What became of one expected word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WordStatus {
    Ok,
    Missed,
    Substituted,
}

/// One expected word (as the sentence writes it), the heard word it was matched with (lower case;
/// none when missed), and its status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordResult {
    pub expected: String,
    pub heard: Option<String>,
    pub status: WordStatus,
}

/// The words, the extra heard words in order, the score (ok words over expected words) and the
/// pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeakScore {
    pub words: Vec<WordResult>,
    pub extra: Vec<String>,
    pub score: f64,
    pub pass: bool,
}

/// The pass allowance, so that 4 of 5 passes at a pass mark of 0.8.
const PASS_EPSILON: f64 = 1e-9;

/// The value of a word of ASCII digits, when it has one that fits.
fn digits_value(word: &str) -> Option<u64> {
    if word.is_empty() || !word.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    word.parse().ok()
}

/// The parts of one normalised word: a hyphen between letters splits it (`est-ce` → `est`, `ce`);
/// the normaliser keeps a hyphen only between letters, so no part is empty.
fn hyphen_parts(word: &str) -> impl Iterator<Item = &str> {
    word.split('-').filter(|part| !part.is_empty())
}

/// The words of `text` for scoring: module 03's normaliser steps (lower case unless `keep_case`),
/// a hyphen between letters splitting the word, an apostrophe kept inside it (`l'eau` is one
/// word), and a word of ASCII digits whose value is 0 to 100 replaced by the words of that
/// `numbers.yaml` entry, split the same way and in lower case (`70` → `soixante`, `dix`). Larger
/// numbers, decimals and words mixing digits and letters stay as written, and so does every
/// number when the course has no numbers file.
pub fn speech_words(lang: Lang, text: &str, numbers: &Numbers, keep_case: bool) -> Vec<String> {
    let _ = lang;
    let normal = normalise_cased(text, !keep_case);
    let mut words = Vec::new();
    for word in normal.split(' ').filter(|w| !w.is_empty()) {
        for part in hyphen_parts(word) {
            let spelled = digits_value(part)
                .filter(|n| *n <= 100)
                .and_then(|n| numbers.spell(n));
            match spelled {
                Some(spelled) => {
                    let spelled = normalise_cased(spelled, true);
                    for spelled_word in spelled.split(' ').filter(|w| !w.is_empty()) {
                        words.extend(hyphen_parts(spelled_word).map(str::to_string));
                    }
                }
                None => words.push(part.to_string()),
            }
        }
    }
    words
}

/// The comparison key of one word: lower case with its marks folded (F2 of module 03, `días` →
/// `dias`), and in German also ß → ss.
pub fn word_key(lang: Lang, word: &str) -> String {
    let folded = fold_marks(&word.to_lowercase());
    if lang == Lang::De {
        folded.replace('ß', "ss")
    } else {
        folded
    }
}

/// `text` with every `[...]` span (whisper.cpp's `[BLANK_AUDIO]`) replaced by a space, so the
/// words on either side stay apart. A `[` with no `]` after it stays.
pub(crate) fn without_brackets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']') else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push(' ');
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

/// Every `[...]` span of `heard` removed, both texts as [`speech_words`], aligned on
/// [`word_key`]. One result per expected word (display form; `heard` lower case, none when
/// missed); `extra` the inserted heard words in order; score = ok ÷ expected words (0.0 with
/// none); pass = score ≥ pass − 1e-9.
pub fn score(lang: Lang, expected: &str, heard: &str, numbers: &Numbers, pass: f64) -> SpeakScore {
    let expected_words = speech_words(lang, expected, numbers, true);
    let heard_words = speech_words(lang, &without_brackets(heard), numbers, false);
    let expected_keys: Vec<String> = expected_words.iter().map(|w| word_key(lang, w)).collect();
    let heard_keys: Vec<String> = heard_words.iter().map(|w| word_key(lang, w)).collect();
    let mut words = Vec::with_capacity(expected_words.len());
    let mut extra = Vec::new();
    for edit in align(&expected_keys, &heard_keys) {
        match edit {
            Edit::Match(i, j) => words.push(WordResult {
                expected: expected_words[i].clone(),
                heard: Some(heard_words[j].clone()),
                status: WordStatus::Ok,
            }),
            Edit::Substitute(i, j) => words.push(WordResult {
                expected: expected_words[i].clone(),
                heard: Some(heard_words[j].clone()),
                status: WordStatus::Substituted,
            }),
            Edit::Delete(i) => words.push(WordResult {
                expected: expected_words[i].clone(),
                heard: None,
                status: WordStatus::Missed,
            }),
            Edit::Insert(j) => extra.push(heard_words[j].clone()),
        }
    }
    let ok = words.iter().filter(|w| w.status == WordStatus::Ok).count();
    let score = if words.is_empty() {
        0.0
    } else {
        ok as f64 / words.len() as f64
    };
    SpeakScore {
        words,
        extra,
        score,
        pass: score >= pass - PASS_EPSILON,
    }
}

/// pass → Correct; score ≥ 0.5 → Almost; otherwise Wrong.
pub fn speak_verdict(score: f64, pass: bool) -> Verdict {
    if pass {
        Verdict::Correct
    } else if score >= 0.5 {
        Verdict::Almost
    } else {
        Verdict::Wrong
    }
}

/// pass → Good; score ≥ 0.5 → Hard; otherwise Again.
pub fn speak_rating(score: f64, pass: bool) -> Rating {
    if pass {
        Rating::Good
    } else if score >= 0.5 {
        Rating::Hard
    } else {
        Rating::Again
    }
}

/// The grade a check shows: [`speak_verdict`], no tags, no message, `corrected` the sentence
/// unless passed, `words` the expected words (display form) that were not ok, `rating`
/// [`speak_rating`].
pub fn speak_grade(sentence: &str, scored: &SpeakScore) -> Grade {
    Grade {
        verdict: speak_verdict(scored.score, scored.pass),
        tags: Vec::new(),
        message: None,
        corrected: (!scored.pass).then(|| sentence.to_string()),
        words: scored
            .words
            .iter()
            .filter(|w| w.status != WordStatus::Ok)
            .map(|w| w.expected.clone())
            .collect(),
        rating: Some(speak_rating(scored.score, scored.pass)),
        explanation: None,
        expected: None,
        judge_error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Entries 0 to 100 with the spec's examples at their numbers.
    fn numbers(lang: Lang) -> Numbers {
        let mut words: Vec<String> = (0..=100).map(|n| format!("n{n}")).collect();
        match lang {
            Lang::Es => {
                words[3] = "tres".into();
                words[31] = "treinta y uno".into();
            }
            Lang::Fr => words[70] = "soixante-dix".into(),
            Lang::De => words[10] = "zehn".into(),
        }
        Numbers { words }
    }

    fn statuses(scored: &SpeakScore) -> Vec<(&str, Option<&str>, WordStatus)> {
        scored
            .words
            .iter()
            .map(|w| (w.expected.as_str(), w.heard.as_deref(), w.status))
            .collect()
    }

    #[test]
    fn words_split_hyphens_keep_apostrophes_and_spell_small_numbers() {
        let es = numbers(Lang::Es);
        let fr = numbers(Lang::Fr);
        assert_eq!(
            speech_words(Lang::Fr, "Est-ce que l'eau est froide ?", &fr, true),
            ["Est", "ce", "que", "l'eau", "est", "froide"]
        );
        assert_eq!(
            speech_words(Lang::Fr, "J'ai 70 ans.", &fr, false),
            ["j'ai", "soixante", "dix", "ans"]
        );
        assert_eq!(
            speech_words(Lang::Es, "Tengo 31, 3,5 y 110 o 3er. 03", &es, true),
            [
                "Tengo", "treinta", "y", "uno", "3,5", "y", "110", "o", "3er", "tres"
            ]
        );
        assert_eq!(
            speech_words(Lang::Es, "Tengo 3 gatos", &Numbers::default(), false),
            ["tengo", "3", "gatos"]
        );
        assert!(speech_words(Lang::Es, " ¡! ", &es, false).is_empty());
    }

    #[test]
    fn keys_fold_marks_and_german_sharp_s() {
        assert_eq!(word_key(Lang::Es, "Días"), "dias");
        assert_eq!(word_key(Lang::De, "Straße"), "strasse");
        assert_eq!(word_key(Lang::Es, "straße"), "straße");
        assert_eq!(word_key(Lang::De, "Mädchen"), "madchen");
    }

    #[test]
    fn brackets_are_removed_from_heard_text_only() {
        assert_eq!(without_brackets("[BLANK_AUDIO] hola"), "  hola");
        assert_eq!(without_brackets("a[x]b [y"), "a b [y");
        let es = numbers(Lang::Es);
        let scored = score(Lang::Es, "hola", "[BLANK_AUDIO] hola", &es, 0.8);
        assert_eq!(statuses(&scored), [("hola", Some("hola"), WordStatus::Ok)]);
    }

    #[test]
    fn the_spec_cases_score_as_computed_by_hand() {
        let es = numbers(Lang::Es);
        let soy = score(Lang::Es, "yo soy estudiante", "yo es estudiante", &es, 0.8);
        assert_eq!(
            statuses(&soy),
            [
                ("yo", Some("yo"), WordStatus::Ok),
                ("soy", Some("es"), WordStatus::Substituted),
                ("estudiante", Some("estudiante"), WordStatus::Ok)
            ]
        );
        assert!((soy.score - 2.0 / 3.0).abs() < 1e-9);
        assert!(!soy.pass);
        assert!(score(Lang::Es, "yo soy estudiante", "yo es estudiante", &es, 0.6).pass);
        let un = score(
            Lang::Es,
            "quiero un café con leche",
            "quiero café con leche",
            &es,
            0.8,
        );
        assert_eq!(un.words[1].status, WordStatus::Missed);
        assert_eq!(un.words[1].heard, None);
        assert!((un.score - 0.8).abs() < 1e-12);
        assert!(un.pass, "4 of 5 passes at 0.8");
        let extra = score(Lang::Es, "buenos días", "buenos días señor", &es, 0.8);
        assert_eq!(
            (extra.score, extra.extra.clone()),
            (1.0, vec!["señor".into()])
        );
        assert_eq!(
            score(Lang::Es, "tengo tres gatos", "tengo 3 gatos", &es, 0.8).score,
            1.0
        );
        let accent = score(Lang::Es, "días", "dias", &es, 0.8);
        assert_eq!(statuses(&accent), [("días", Some("dias"), WordStatus::Ok)]);
        let empty = score(Lang::Es, "Buenos días.", "", &es, 0.8);
        assert_eq!(
            statuses(&empty),
            [
                ("Buenos", None, WordStatus::Missed),
                ("días", None, WordStatus::Missed)
            ]
        );
        assert_eq!((empty.score, empty.pass), (0.0, false));
        let nothing = score(Lang::Es, "", "hola", &es, 0.8);
        assert_eq!((nothing.score, nothing.extra.len()), (0.0, 1));
        let de = score(
            Lang::De,
            "Die Straße ist lang.",
            "die Strasse ist lang",
            &numbers(Lang::De),
            0.8,
        );
        assert_eq!(de.score, 1.0);
        assert_eq!(de.words[1].heard.as_deref(), Some("strasse"));
    }

    #[test]
    fn verdicts_ratings_and_grades_follow_the_score() {
        assert_eq!(speak_rating(1.0, true), Rating::Good);
        assert_eq!(speak_rating(0.6, false), Rating::Hard);
        assert_eq!(speak_rating(0.5, false), Rating::Hard);
        assert_eq!(speak_rating(0.4, false), Rating::Again);
        assert_eq!(speak_verdict(0.6, false), Verdict::Almost);
        assert_eq!(speak_verdict(0.4, false), Verdict::Wrong);
        assert_eq!(speak_verdict(0.8, true), Verdict::Correct);
        let es = numbers(Lang::Es);
        let scored = score(Lang::Es, "Yo soy estudiante.", "yo es estudiante", &es, 0.8);
        let grade = speak_grade("Yo soy estudiante.", &scored);
        assert_eq!(grade.verdict, Verdict::Almost);
        assert_eq!(grade.corrected.as_deref(), Some("Yo soy estudiante."));
        assert_eq!(grade.words, ["soy"]);
        assert_eq!(grade.rating, Some(Rating::Hard));
        assert!(grade.tags.is_empty() && grade.message.is_none());
        let passed = speak_grade("Hola.", &score(Lang::Es, "Hola.", "hola", &es, 0.8));
        assert_eq!((passed.verdict, passed.corrected), (Verdict::Correct, None));
        assert!(passed.words.is_empty());
    }
}
