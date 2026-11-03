//! Word-bank tiles for typing-free translation (docs/modules/03-exercise-engine-and-grading.md,
//! Design notes).

use crate::content::{Forms, Lang, nfc};
use crate::env::{Rng, below, shuffle};
use crate::grading::normalise;
use crate::grading::normalise::words;

use super::check::Allowed;

/// The most distractor tiles a word bank adds.
const DISTRACTORS: usize = 3;

/// The tiles of a word bank for `target`: the words of its normal form, plus up to 3 distractors
/// drawn at random from the allowed single-word forms (function words, allowed lemmas and the
/// forms table's forms of allowed lemmas; lower case, sorted, without repeats) that are not
/// target tiles; then all shuffled. The target tiles joined with spaces in the right order are
/// the target's normal form, so a word-bank answer grades like a typed one.
pub fn word_bank(
    target: &str,
    lang: Lang,
    allowed: &Allowed,
    forms: &Forms,
    rng: &mut dyn Rng,
) -> Vec<String> {
    let target_norm = normalise(lang, target);
    let mut tiles: Vec<String> = words(&target_norm)
        .into_iter()
        .map(str::to_string)
        .collect();

    let lemmas: Vec<String> = allowed.lemmas().iter().map(|lemma| nfc(lemma)).collect();
    let mut pool: Vec<String> = allowed
        .function_words()
        .iter()
        .chain(allowed.lemmas())
        .map(String::as_str)
        .chain(
            forms
                .rows()
                .iter()
                .filter(|row| lemmas.contains(&nfc(&row.lemma)))
                .map(|row| row.form.as_str()),
        )
        .map(|word| normalise(lang, word))
        .filter(|word| !word.is_empty() && !word.contains(' '))
        .collect();
    pool.sort_unstable();
    pool.dedup();
    pool.retain(|word| !tiles.contains(word));

    for _ in 0..DISTRACTORS.min(pool.len()) {
        let pick = below(rng, pool.len());
        tiles.push(pool.remove(pick));
    }
    shuffle(&mut tiles, rng);
    tiles
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::parse_syllabus;
    use crate::content::test_support::{EXAMPLE_SYLLABUS, example_forms};
    use crate::env::SplitMix64;

    #[test]
    fn the_target_tiles_and_three_allowed_distractors_are_shuffled() {
        let syllabus = parse_syllabus("valid-es.yaml", EXAMPLE_SYLLABUS).unwrap();
        let forms = example_forms();
        let allowed = Allowed::for_skill(&syllabus, &[], "es.a1.u01.s01");
        let tiles = word_bank(
            "Buenos días.",
            Lang::Es,
            &allowed,
            &forms,
            &mut SplitMix64::new(1),
        );
        assert_eq!(tiles.len(), 5);
        assert!(tiles.contains(&"buenos".to_string()));
        assert!(tiles.contains(&"días".to_string()));
        let tokens_allowed = tiles.iter().all(|tile| {
            crate::content::tokenise(Lang::Es, tile, &forms)
                .iter()
                .all(|token| allowed.allows(token))
        });
        assert!(tokens_allowed, "{tiles:?}");
        let again = word_bank(
            "Buenos días.",
            Lang::Es,
            &allowed,
            &forms,
            &mut SplitMix64::new(1),
        );
        assert_eq!(tiles, again);
        let mut sorted = tiles.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 5, "no tile twice: {tiles:?}");
    }

    #[test]
    fn a_small_pool_gives_fewer_distractors() {
        let text = "schema: parley.syllabus/1\nlang: es\nname: n\nvariety: v\nversion: 1\nfunction_words: [el]\nnames: []\nunits: []\n";
        let syllabus = parse_syllabus("s.yaml", text).unwrap();
        let allowed = Allowed::for_skill(&syllabus, &[], "none");
        let tiles = word_bank(
            "hola",
            Lang::Es,
            &allowed,
            &Forms::default(),
            &mut SplitMix64::new(0),
        );
        let mut sorted = tiles.clone();
        sorted.sort();
        assert_eq!(sorted, ["el", "hola"]);
    }
}
