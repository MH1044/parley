//! Language identification of AI text (docs/modules/07-lesson-generation.md, Interfaces;
//! docs/PUBLIK-INTEGRATION.md §6.3 L4 and §6.4 S2 and S6) and the Snowball stem the coverage check
//! falls back on (L5, S3).
//!
//! Identification is `whatlang` (trigram profiles, Cavnar and Trenkle 1994) restricted to the four
//! languages Parley reads: English and the three course languages. A text of fewer than four words
//! is too short to identify and always passes. A longer one is in the expected language when the
//! four-language identification names it; or when it names another language with a confidence
//! below [`LANGID_THRESHOLD`] and the expected language still scores higher than its counterpart
//! (English for a course language; the course language for English). The second step exists
//! because whatlang's confidence on sentences of four to twelve words is low and its first choice
//! among four languages is often wrong on them (`Quiero un café con leche.` comes out French with
//! a confidence of 0.10), while the choice between a course language and English is right on
//! them; the module 07 Design notes record the measurements.

use std::borrow::Cow;

use rust_stemmers::{Algorithm, Stemmer};
use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;
use whatlang::Detector;

use crate::content::Lang;
use crate::grading::normalise;

/// A language an AI text may be in: English or one of the course languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextLang {
    En,
    Es,
    Fr,
    De,
}

impl From<Lang> for TextLang {
    fn from(lang: Lang) -> Self {
        match lang {
            Lang::Es => TextLang::Es,
            Lang::Fr => TextLang::Fr,
            Lang::De => TextLang::De,
        }
    }
}

impl TextLang {
    fn whatlang(self) -> whatlang::Lang {
        match self {
            TextLang::En => whatlang::Lang::Eng,
            TextLang::Es => whatlang::Lang::Spa,
            TextLang::Fr => whatlang::Lang::Fra,
            TextLang::De => whatlang::Lang::Deu,
        }
    }

    fn from_whatlang(lang: whatlang::Lang) -> Option<TextLang> {
        match lang {
            whatlang::Lang::Eng => Some(TextLang::En),
            whatlang::Lang::Spa => Some(TextLang::Es),
            whatlang::Lang::Fra => Some(TextLang::Fr),
            whatlang::Lang::Deu => Some(TextLang::De),
            _ => None,
        }
    }
}

/// The confidence an identification of another language needs before it overrules the expected
/// one. Kept at the starting value 0.5 (see the module 07 Design notes): every correct
/// four-to-twelve-word sentence measured that whatlang named in another language was named with a
/// confidence below 0.4, while a sentence in the wrong language was either named with a
/// confidence of 0.95 or more or lost the comparison with its counterpart.
pub const LANGID_THRESHOLD: f64 = 0.5;

/// The fewest words a text needs before its language is checked.
const MIN_WORDS: usize = 4;

/// Some((language, confidence)) or None when whatlang gives no answer.
pub fn identify(text: &str) -> Option<(TextLang, f64)> {
    let info = Detector::with_allowlist(vec![
        whatlang::Lang::Eng,
        whatlang::Lang::Spa,
        whatlang::Lang::Fra,
        whatlang::Lang::Deu,
    ])
    .detect(text)?;
    Some((TextLang::from_whatlang(info.lang())?, info.confidence()))
}

/// Whether `expected` scores higher than `other` on `text` when whatlang chooses between the two.
fn beats(text: &str, expected: TextLang, other: TextLang) -> bool {
    Detector::with_allowlist(vec![expected.whatlang(), other.whatlang()])
        .detect(text)
        .and_then(|info| TextLang::from_whatlang(info.lang()))
        == Some(expected)
}

/// The number of words of `text` as the language check counts them: the normal form's UAX #29
/// words.
fn word_count(text: &str) -> usize {
    normalise(Lang::Es, text).unicode_words().count()
}

/// The language check of `text` against `expected`, with `counterpart` the language it is told
/// apart from when the four-language identification is not sure.
fn check(text: &str, expected: TextLang, counterpart: impl Fn(TextLang) -> TextLang) -> bool {
    if word_count(text) < MIN_WORDS {
        return true;
    }
    match identify(text) {
        None => false,
        Some((lang, _)) if lang == expected => true,
        Some((_, confidence)) if confidence >= LANGID_THRESHOLD => false,
        Some((named, _)) => {
            let other = counterpart(named);
            other != expected && beats(text, expected, other)
        }
    }
}

/// True when `text` has fewer than four words (normalise, then UAX #29 words), or when it is
/// identified as `expected`: the four-language identification names it, or names another language
/// with a confidence below LANGID_THRESHOLD while `expected` scores higher than its counterpart
/// (English for a course language; for English, the course language whatlang named).
pub fn is_language(text: &str, expected: TextLang) -> bool {
    check(text, expected, |named| match expected {
        TextLang::En => named,
        _ => TextLang::En,
    })
}

/// [`is_language`] inside a course of `course`, as the lesson and story checks call it: the
/// counterpart of English is the course language.
pub(crate) fn is_language_in(text: &str, expected: TextLang, course: Lang) -> bool {
    check(text, expected, |_| match expected {
        TextLang::En => TextLang::from(course),
        _ => TextLang::En,
    })
}

fn stemmer(lang: Lang) -> Stemmer {
    Stemmer::create(match lang {
        Lang::Es => Algorithm::Spanish,
        Lang::Fr => Algorithm::French,
        Lang::De => Algorithm::German,
    })
}

/// The Snowball stem of one lower-cased word (rust-stemmers: Spanish, French, German).
pub fn stem(lang: Lang, word: &str) -> String {
    let lower = word.to_lowercase();
    match stemmer(lang).stem(&lower) {
        Cow::Borrowed(text) => text.to_string(),
        Cow::Owned(text) => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Project-written sentences of known language, four to twelve words, in the course a lesson
    /// would use them in: the measurements the rule and the threshold were chosen from.
    const SENTENCES: &[(&str, TextLang, Lang)] = &[
        ("Quiero agua, por favor.", TextLang::Es, Lang::Es),
        ("Quiero un café con leche.", TextLang::Es, Lang::Es),
        ("Ana quiere pan con agua.", TextLang::Es, Lang::Es),
        ("Yo quiero un té sin azúcar.", TextLang::Es, Lang::Es),
        ("Buenos días, ¿qué tal estás hoy?", TextLang::Es, Lang::Es),
        (
            "Mi hermana vive en Madrid con su perro.",
            TextLang::Es,
            Lang::Es,
        ),
        (
            "Hoy no tengo tiempo para comer en casa.",
            TextLang::Es,
            Lang::Es,
        ),
        (
            "Je voudrais un café, s'il vous plaît.",
            TextLang::Fr,
            Lang::Fr,
        ),
        (
            "Nous habitons dans une petite maison.",
            TextLang::Fr,
            Lang::Fr,
        ),
        (
            "Elle mange du pain avec du fromage.",
            TextLang::Fr,
            Lang::Fr,
        ),
        (
            "Bonjour, je m'appelle Paul et j'ai vingt ans.",
            TextLang::Fr,
            Lang::Fr,
        ),
        ("Ich möchte einen Kaffee mit Milch.", TextLang::De, Lang::De),
        ("Wir wohnen in einem kleinen Haus.", TextLang::De, Lang::De),
        ("Ich will Wasser, bitte.", TextLang::De, Lang::De),
        (
            "Meine Schwester trinkt gern Tee am Morgen.",
            TextLang::De,
            Lang::De,
        ),
        ("I want water, please.", TextLang::En, Lang::Es),
        ("I want a coffee with milk.", TextLang::En, Lang::Es),
        ("I'd like a white coffee.", TextLang::En, Lang::Es),
        (
            "My sister lives in Madrid with her dog.",
            TextLang::En,
            Lang::Es,
        ),
        ("Where is the station, please?", TextLang::En, Lang::De),
        (
            "We have no time to eat at home today.",
            TextLang::En,
            Lang::Fr,
        ),
    ];

    #[test]
    fn project_sentences_are_in_their_language() {
        for (text, lang, course) in SENTENCES {
            assert!(is_language_in(text, *lang, *course), "{text}");
            if *lang != TextLang::En {
                assert!(is_language(text, *lang), "{text}");
            }
        }
    }

    #[test]
    fn the_wrong_language_is_refused() {
        assert!(!is_language("I want water please now", TextLang::Es));
        assert!(!is_language_in(
            "I want water please now",
            TextLang::Es,
            Lang::Es
        ));
        assert!(!is_language_in(
            "Quiero un café con leche.",
            TextLang::En,
            Lang::Es
        ));
        assert!(!is_language_in(
            "Hoy no tengo tiempo para comer en casa.",
            TextLang::En,
            Lang::Es
        ));
        assert!(!is_language(
            "Je voudrais un café, s'il vous plaît.",
            TextLang::Es
        ));
        assert!(!is_language(
            "The black cat sleeps on the bed.",
            TextLang::De
        ));
        assert!(is_language(
            "The black cat sleeps on the bed.",
            TextLang::En
        ));
    }

    #[test]
    fn identification_names_clear_sentences() {
        assert_eq!(
            identify("Hoy no tengo tiempo para comer en casa.").map(|(l, _)| l),
            Some(TextLang::Es)
        );
        assert_eq!(
            identify("Das Wetter ist heute sehr schön.").map(|(l, _)| l),
            Some(TextLang::De)
        );
        assert_eq!(
            identify("Nous habitons dans une petite maison.").map(|(l, _)| l),
            Some(TextLang::Fr)
        );
        assert_eq!(
            identify("The black cat sleeps on the bed.").map(|(l, _)| l),
            Some(TextLang::En)
        );
        assert_eq!(identify("123 456"), None);
    }

    #[test]
    fn fewer_than_four_words_always_pass() {
        assert!(is_language("I want water", TextLang::Es));
        assert!(is_language("el café", TextLang::De));
        assert!(is_language("", TextLang::Fr));
        assert_eq!(word_count("Est-ce que c'est bon ?"), 5);
    }

    #[test]
    fn stems_follow_snowball() {
        assert_eq!(stem(Lang::Es, "Quiero"), stem(Lang::Es, "quieres"));
        assert_eq!(stem(Lang::Es, "aguas"), stem(Lang::Es, "agua"));
        assert_eq!(stem(Lang::Fr, "maisons"), stem(Lang::Fr, "maison"));
        assert_eq!(stem(Lang::De, "Häuser"), stem(Lang::De, "Haus"));
        assert_ne!(stem(Lang::Es, "agua"), stem(Lang::Es, "hielo"));
    }
}
