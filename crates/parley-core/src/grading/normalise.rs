//! The normaliser and accent folding (docs/modules/03-exercise-engine-and-grading.md, Design
//! notes).
//!
//! Every comparison of an answer goes through [`normalise`], so two answers that differ only in
//! case, punctuation, apostrophe style or spacing compare equal, while accents, `ß` and every
//! letter are kept for the accent and typo rules to judge.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::content::Lang;

/// Typographic apostrophes read as `'`.
const APOSTROPHES: [char; 3] = ['\u{2019}', '\u{2018}', '\u{02BC}'];

/// No-break spaces read as a space.
const NO_BREAK_SPACES: [char; 2] = ['\u{00A0}', '\u{202F}'];

/// The normaliser's steps, with or without the lowercase step. With `decimals`, a `.` or `,`
/// between two ASCII digits also stays inside its word (`3,5`), so a speaking check keeps a
/// decimal as written instead of reading two numbers.
fn clean(s: &str, lower: bool, decimals: bool) -> String {
    let nfc: String = s
        .nfc()
        .map(|c| {
            if APOSTROPHES.contains(&c) {
                '\''
            } else if NO_BREAK_SPACES.contains(&c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    // `to_lowercase`, not case folding, so `ß` stays `ß` and German keeps its letter.
    let cased = if lower { nfc.to_lowercase() } else { nfc };
    let chars: Vec<char> = cased.chars().collect();
    let mut out = String::with_capacity(cased.len());
    for (i, &c) in chars.iter().enumerate() {
        let inner = (c == '\'' || c == '-')
            && i > 0
            && chars[i - 1].is_alphabetic()
            && chars.get(i + 1).is_some_and(|next| next.is_alphabetic());
        let decimal = decimals
            && (c == '.' || c == ',')
            && i > 0
            && chars[i - 1].is_ascii_digit()
            && chars.get(i + 1).is_some_and(char::is_ascii_digit);
        let kept =
            c.is_alphanumeric() || is_combining_mark(c) || c.is_whitespace() || inner || decimal;
        // A space, not nothing, so `Hola,¿qué` stays two words.
        out.push(if kept { c } else { ' ' });
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The normal form of an answer or an accepted answer: NFC; typographic apostrophes (’ ‘ ʼ) to
/// `'`; U+00A0 and U+202F to a space; lower case; every character that is not a letter or
/// number, a combining mark or white space becomes a space, except `'` or `-` between two
/// letters; white space collapsed to single spaces and trimmed. The rules are the same for every
/// language, English included.
pub fn normalise(lang: Lang, s: &str) -> String {
    let _ = lang;
    clean(s, true, false)
}

/// The normaliser's steps, with the lowercase step only when `lower`: the first step of the words
/// a speaking check compares (module 10), whose expected words keep their case for display.
pub(crate) fn normalise_cased(s: &str, lower: bool) -> String {
    clean(s, lower, true)
}

/// The words of `s` as written: the normaliser's steps without the lowercase step, so the i-th
/// display word is the i-th word of the normal form, in the case the text uses. Messages name
/// words in this form.
pub(crate) fn display_words(s: &str) -> Vec<String> {
    let display = clean(s, false, false);
    let normal = clean(s, true, false);
    let words: Vec<String> = display.split(' ').map(str::to_string).collect();
    // Lowercasing never adds or removes a word boundary; the fallback only guards that belief.
    if words.len() == normal.split(' ').count() {
        words
    } else {
        normal.split(' ').map(str::to_string).collect()
    }
}

/// The words of a normal form; none for the empty text.
pub(crate) fn words(norm: &str) -> Vec<&str> {
    if norm.is_empty() {
        Vec::new()
    } else {
        norm.split(' ').collect()
    }
}

/// F2: the text with every combining mark removed after NFD, then NFC again (`días` → `dias`,
/// `Mädchen` → `Madchen`; `ß` is not a mark and stays).
pub fn fold_marks(s: &str) -> String {
    let stripped: String = s.nfd().filter(|c| !is_combining_mark(*c)).collect();
    stripped.nfc().collect()
}

/// F1, the German spelling without umlauts or ß: ä → ae, ö → oe, ü → ue, ß → ss, applied to
/// normalised (lower-case) text.
pub fn fold_german(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        match c {
            'ä' => out.push_str("ae"),
            'ö' => out.push_str("oe"),
            'ü' => out.push_str("ue"),
            'ß' => out.push_str("ss"),
            other => out.push(other),
        }
    }
    out
}

/// Whether two normalised texts (or words) differ only in accents: they are not equal, but equal
/// under F2, or, in German, under F1 on both sides.
pub(crate) fn accent_equal(lang: Lang, a: &str, b: &str) -> bool {
    a != b
        && (fold_marks(a) == fold_marks(b)
            || (lang == Lang::De && fold_german(a) == fold_german(b)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spec_examples_normalise_as_written() {
        assert_eq!(normalise(Lang::Es, "  ¿Qué  TAL? "), "qué tal");
        assert_eq!(
            normalise(Lang::Fr, "Aujourd\u{2019}hui, c\u{2019}est lundi !"),
            "aujourd'hui c'est lundi"
        );
        assert_eq!(normalise(Lang::De, "Die Straße."), "die straße");
        assert_eq!(normalise(Lang::Es, "e\u{301}l"), "él");
        assert_eq!(normalise(Lang::Fr, "Est-ce que…"), "est-ce que");
    }

    #[test]
    fn punctuation_between_words_splits_them_and_inner_marks_stay() {
        assert_eq!(normalise(Lang::Es, "Hola,¿qué tal?"), "hola qué tal");
        assert_eq!(normalise(Lang::Fr, "l'homme"), "l'homme");
        assert_eq!(normalise(Lang::Fr, "l' homme"), "l homme");
        assert_eq!(normalise(Lang::Fr, "'quoi'"), "quoi");
        assert_eq!(normalise(Lang::Es, "a--b - c"), "a b c");
        assert_eq!(normalise(Lang::Es, "Tengo 3 gatos."), "tengo 3 gatos");
        assert_eq!(
            normalise(Lang::Es, "uno\u{A0}dos\u{202F}tres\t\ncuatro"),
            "uno dos tres cuatro"
        );
        assert_eq!(normalise(Lang::Es, "¡¿!?"), "");
        assert_eq!(normalise(Lang::De, "STRASSE"), "strasse");
    }

    #[test]
    fn display_words_keep_case_and_match_positions() {
        assert_eq!(display_words("Hola, ¿qué tal?"), ["Hola", "qué", "tal"]);
        assert_eq!(display_words("Buenos días."), ["Buenos", "días"]);
        assert_eq!(words(""), Vec::<&str>::new());
        assert_eq!(words("a b"), ["a", "b"]);
    }

    #[test]
    fn folding_removes_marks_and_spells_out_umlauts() {
        assert_eq!(fold_marks("días"), "dias");
        assert_eq!(fold_marks("mädchen"), "madchen");
        assert_eq!(fold_marks("straße"), "straße");
        assert_eq!(fold_marks("e\u{301}"), "e");
        assert_eq!(
            fold_german("mädchen straße über öl"),
            "maedchen strasse ueber oel"
        );
    }

    #[test]
    fn accent_equality_needs_a_difference() {
        assert!(accent_equal(Lang::Es, "dias", "días"));
        assert!(!accent_equal(Lang::Es, "días", "días"));
        assert!(accent_equal(Lang::De, "maedchen", "mädchen"));
        assert!(accent_equal(Lang::De, "madchen", "mädchen"));
        assert!(accent_equal(Lang::De, "strasse", "straße"));
        assert!(!accent_equal(Lang::Es, "strasse", "straße"));
        assert!(!accent_equal(Lang::Es, "dia", "días"));
    }
}
