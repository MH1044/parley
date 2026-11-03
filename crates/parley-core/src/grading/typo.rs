//! Typo tolerance: optimal string alignment distance (Damerau 1964; Levenshtein 1966) of at most
//! one per word, for a few words of an answer (docs/modules/03-exercise-engine-and-grading.md,
//! Design notes, grading rule 4).

use crate::content::{Forms, Lang};

use super::normalise::{accent_equal, fold_marks};

/// The optimal string alignment distance between `a` and `b`, counted over chars: insertions,
/// deletions, substitutions and transpositions of two adjacent chars, no substring edited twice.
pub fn osa_distance(a: &str, b: &str) -> usize {
    strsim::osa_distance(a, b)
}

/// The shortest word, in chars, that a typo may be forgiven in: below it, one letter changes the
/// word (`le`, `la`).
const MIN_TYPO_CHARS: usize = 4;

/// A word with its first letter in upper case, as German nouns are keyed in a forms table.
fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Whether the learner's `word` is itself a form in the forms table: then it is a different
/// word (`pero` for `perro`), not a slip.
fn is_known_form(forms: &Forms, word: &str) -> bool {
    !forms.lemmas_of(word).is_empty() || !forms.lemmas_of(&capitalised(word)).is_empty()
}

/// What the typo rule found against one candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypoMatch {
    /// Every position whose words are not equal (typos and accent slips), in order.
    pub positions: Vec<usize>,
    /// Another word differs only in accents.
    pub accent: bool,
}

/// The typo rule for the answer's words against one candidate's words (both normalised): the
/// same word count n > 0; the differing words (not equal and not accent-equal) number 1 to
/// max(1, n / 4); each is at OSA distance exactly 1 between the F2 forms, at least 4 chars long
/// in the candidate, and not itself a form in the forms table.
pub(crate) fn typo_match(
    lang: Lang,
    forms: &Forms,
    answer: &[&str],
    candidate: &[&str],
) -> Option<TypoMatch> {
    let n = candidate.len();
    if n == 0 || answer.len() != n {
        return None;
    }
    let mut positions = Vec::new();
    let mut typos = 0;
    let mut accent = false;
    for (i, (&given, &expected)) in answer.iter().zip(candidate).enumerate() {
        if given == expected {
            continue;
        }
        positions.push(i);
        if accent_equal(lang, given, expected) {
            accent = true;
            continue;
        }
        typos += 1;
        let slip = osa_distance(&fold_marks(expected), &fold_marks(given)) == 1
            && expected.chars().count() >= MIN_TYPO_CHARS
            && !is_known_form(forms, given);
        if !slip {
            return None;
        }
    }
    let allowed = (n / 4).max(1);
    (1..=allowed)
        .contains(&typos)
        .then_some(TypoMatch { positions, accent })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::FormRow;

    fn forms() -> Forms {
        let row = |form: &str, lemma: &str| FormRow {
            form: form.into(),
            lemma: lemma.into(),
            pos: "noun".into(),
            gender: None,
            tags: String::new(),
        };
        Forms::from_rows(vec![row("pero", "pero"), row("Hund", "Hund")])
    }

    fn split(s: &str) -> Vec<&str> {
        s.split(' ').collect()
    }

    #[test]
    fn osa_counts_a_transposition_as_one() {
        assert_eq!(osa_distance("leche", "lecje"), 1);
        assert_eq!(osa_distance("leche", "lehce"), 1);
        assert_eq!(osa_distance("ca", "abc"), 3);
        assert_eq!(osa_distance("día", "dia"), 1);
        assert_eq!(osa_distance("", "abc"), 3);
    }

    #[test]
    fn one_slip_in_a_long_word_is_a_typo() {
        let found = typo_match(
            Lang::Es,
            &forms(),
            &split("quiero un café con lecje"),
            &split("quiero un café con leche"),
        );
        assert_eq!(
            found,
            Some(TypoMatch {
                positions: vec![4],
                accent: false
            })
        );
    }

    #[test]
    fn a_known_form_or_a_short_word_is_not_a_typo() {
        assert_eq!(
            typo_match(Lang::Es, &forms(), &split("el pero"), &split("el perro")),
            None
        );
        assert_eq!(
            typo_match(Lang::Fr, &forms(), &split("la livre"), &split("le livre")),
            None
        );
        assert_eq!(
            typo_match(Lang::De, &forms(), &split("der hund"), &split("der hand")),
            None,
            "hund is a form once capitalised"
        );
    }

    #[test]
    fn the_number_of_slips_is_limited_by_the_length() {
        let expected = split("mañana quiero comprar leche caliente para nuestra abuela");
        let two = split("mañana quiero comprar lecje caliente para nuestra abuel");
        let three = split("mañana quiero comprar lecje caliente pora nuestra abuel");
        assert!(typo_match(Lang::Es, &forms(), &two, &expected).is_some());
        assert!(typo_match(Lang::Es, &forms(), &three, &expected).is_none());
        assert!(typo_match(Lang::Es, &forms(), &split("un"), &split("uno")).is_none());
        assert!(
            typo_match(
                Lang::Es,
                &forms(),
                &split("buenas noxhes senorx"),
                &split("buenas noches señora")
            )
            .is_none(),
            "two typos in three words are too many"
        );
    }

    #[test]
    fn an_accent_slip_beside_a_typo_is_reported() {
        let found = typo_match(
            Lang::Es,
            &forms(),
            &split("buenos dias señorra"),
            &split("buenos días señora"),
        );
        assert_eq!(
            found,
            Some(TypoMatch {
                positions: vec![1, 2],
                accent: true
            })
        );
    }

    #[test]
    fn different_word_counts_never_match() {
        assert_eq!(
            typo_match(Lang::Es, &forms(), &split("hola"), &split("hola amigo")),
            None
        );
        assert_eq!(typo_match(Lang::Es, &forms(), &[], &[]), None);
    }
}
