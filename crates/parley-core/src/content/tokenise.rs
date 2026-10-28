//! The tokeniser: UAX #29 words plus the per-language rules of the module 01 Design notes, with
//! lemma keys from the forms table.
//!
//! - `norm` is the surface in NFC and lower case (a typographic ’ read as ').
//! - Digits become [`TokenKind::Number`].
//! - French elisions (`l' d' j' m' t' s' n' c' qu' jusqu' lorsqu' puisqu'`) split after the
//!   apostrophe into a token whose surface keeps the apostrophe and whose lemma keys are the full
//!   word; `aujourd'hui` stays whole. A `t` between two hyphens (`a-t-il`) is dropped.
//! - Spanish `al`, `del` and German `am im ins zum zur vom beim` stay one token with two lemma
//!   keys.
//! - Every other word's lemma keys are `lemmas_of(surface)` and `lemmas_of(norm)` merged, sorted
//!   and deduplicated; a capitalised word not at sentence start, unknown to the forms table, is a
//!   [`TokenKind::Name`].

use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

use super::forms::Forms;
use super::{Lang, nfc};

/// What kind of word a token is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenKind {
    Word,
    Number,
    Name,
}

/// One word of a sentence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    /// The word as written.
    pub surface: String,
    /// The surface in NFC and lower case.
    pub norm: String,
    pub kind: TokenKind,
    /// The lemmas the word is a form of, sorted and deduplicated.
    pub lemma_keys: Vec<String>,
}

/// French elision prefixes (without the apostrophe) and the words they stand for.
const FR_ELISIONS: &[(&str, &[&str])] = &[
    ("l", &["le", "la"]),
    ("d", &["de"]),
    ("j", &["je"]),
    ("m", &["me"]),
    ("t", &["te"]),
    ("s", &["se"]),
    ("n", &["ne"]),
    ("c", &["ce"]),
    ("qu", &["que"]),
    ("jusqu", &["jusque"]),
    ("lorsqu", &["lorsque"]),
    ("puisqu", &["puisque"]),
];

/// Spanish contractions and their two lemma keys.
const ES_CONTRACTIONS: &[(&str, &[&str])] = &[("al", &["a", "el"]), ("del", &["de", "el"])];

/// German contractions and their two lemma keys.
const DE_CONTRACTIONS: &[(&str, &[&str])] = &[
    ("am", &["an", "dem"]),
    ("im", &["in", "dem"]),
    ("ins", &["in", "das"]),
    ("zum", &["zu", "dem"]),
    ("zur", &["zu", "der"]),
    ("vom", &["von", "dem"]),
    ("beim", &["bei", "dem"]),
];

/// Punctuation that may stand between a sentence boundary and its first word.
const OPENING: [char; 7] = ['¿', '¡', '"', '«', '(', '‘', '“'];

/// The normalised form of a surface: NFC, lower case, ASCII apostrophe.
pub fn normalise(surface: &str) -> String {
    nfc(surface).to_lowercase().replace('’', "'")
}

/// Whether the word starting at byte `offset` is at sentence start: scanning back over spaces
/// and opening punctuation, the text begins or the previous character is `.`, `!` or `?`.
pub fn at_sentence_start(text: &str, offset: usize) -> bool {
    let before = &text[..offset];
    match before
        .chars()
        .rev()
        .find(|c| !c.is_whitespace() && !OPENING.contains(c))
    {
        None => true,
        Some(c) => matches!(c, '.' | '!' | '?'),
    }
}

fn is_number(norm: &str) -> bool {
    norm.chars().next().is_some_and(char::is_numeric)
        && norm.chars().all(|c| c.is_numeric() || c == ',' || c == '.')
}

fn contraction(lang: Lang, norm: &str) -> Option<&'static [&'static str]> {
    let table: &[(&str, &[&str])] = match lang {
        Lang::Es => ES_CONTRACTIONS,
        Lang::De => DE_CONTRACTIONS,
        Lang::Fr => return None,
    };
    table
        .iter()
        .find(|(word, _)| *word == norm)
        .map(|(_, keys)| *keys)
}

/// For a French word with an elision prefix (`l'homme`): the byte length of the prefix with its
/// apostrophe and the lemma keys the prefix stands for.
fn elision(word: &str) -> Option<(usize, &'static [&'static str])> {
    let apostrophe = word.find(['\'', '’'])?;
    let head = normalise(&word[..apostrophe]);
    let keys = FR_ELISIONS
        .iter()
        .find(|(prefix, _)| *prefix == head)
        .map(|(_, keys)| *keys)?;
    let apostrophe_len = word[apostrophe..].chars().next().map_or(1, char::len_utf8);
    Some((apostrophe + apostrophe_len, keys))
}

fn char_before(text: &str, offset: usize) -> Option<char> {
    text[..offset].chars().next_back()
}

fn char_after(text: &str, offset: usize) -> Option<char> {
    text[offset..].chars().next()
}

fn classify(text: &str, offset: usize, surface: &str, lang: Lang, forms: &Forms) -> Token {
    let norm = normalise(surface);
    if is_number(&norm) {
        return Token {
            surface: surface.to_string(),
            norm,
            kind: TokenKind::Number,
            lemma_keys: Vec::new(),
        };
    }
    if let Some(keys) = contraction(lang, &norm) {
        return Token {
            surface: surface.to_string(),
            norm,
            kind: TokenKind::Word,
            lemma_keys: keys.iter().map(|k| k.to_string()).collect(),
        };
    }
    let mut lemma_keys: Vec<String> = forms
        .lemmas_of(surface)
        .into_iter()
        .chain(forms.lemmas_of(&norm))
        .map(str::to_string)
        .collect();
    lemma_keys.sort_unstable();
    lemma_keys.dedup();
    let capitalised = surface.chars().next().is_some_and(char::is_uppercase);
    let kind = if lemma_keys.is_empty() && capitalised && !at_sentence_start(text, offset) {
        TokenKind::Name
    } else {
        TokenKind::Word
    };
    Token {
        surface: surface.to_string(),
        norm,
        kind,
        lemma_keys,
    }
}

/// Splits `text` into tokens for `lang`, with lemma keys from `forms`.
pub fn tokenise(lang: Lang, text: &str, forms: &Forms) -> Vec<Token> {
    let mut tokens = Vec::new();
    for (offset, word) in text.unicode_word_indices() {
        if lang == Lang::Fr {
            if word == "t"
                && char_before(text, offset) == Some('-')
                && char_after(text, offset + word.len()) == Some('-')
            {
                continue;
            }
            if let Some((prefix_len, keys)) = elision(word) {
                let (head, tail) = word.split_at(prefix_len);
                tokens.push(Token {
                    surface: head.to_string(),
                    norm: normalise(head),
                    kind: TokenKind::Word,
                    lemma_keys: keys.iter().map(|k| k.to_string()).collect(),
                });
                if !tail.is_empty() {
                    tokens.push(classify(text, offset + prefix_len, tail, lang, forms));
                }
                continue;
            }
        }
        tokens.push(classify(text, offset, word, lang, forms));
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::forms::FormRow;

    fn row(form: &str, lemma: &str, pos: &str) -> FormRow {
        FormRow {
            form: form.into(),
            lemma: lemma.into(),
            pos: pos.into(),
            gender: None,
            tags: String::new(),
        }
    }

    fn mini() -> Forms {
        Forms::from_rows(vec![
            row("voy", "ir", "verb"),
            row("mercado", "mercado", "noun"),
            row("pueblo", "pueblo", "noun"),
            row("gehe", "gehen", "verb"),
            row("Bahnhof", "Bahnhof", "noun"),
            row("homme", "homme", "noun"),
            row("est", "être", "verb"),
            row("viens", "venir", "verb"),
            row("tu", "tu", "pron"),
            row("que", "que", "conj"),
            row("ce", "ce", "pron"),
            row("tengo", "tener", "verb"),
            row("gatos", "gato", "noun"),
            row("fue", "ir", "verb"),
            row("fue", "ser", "verb"),
        ])
    }

    fn surfaces(tokens: &[Token]) -> Vec<&str> {
        tokens.iter().map(|t| t.surface.as_str()).collect()
    }

    #[test]
    fn french_elisions_split_and_names_are_recognised() {
        let tokens = tokenise(Lang::Fr, "L'homme est aujourd'hui à Paris.", &mini());
        assert_eq!(
            surfaces(&tokens),
            ["L'", "homme", "est", "aujourd'hui", "à", "Paris"]
        );
        assert_eq!(tokens[0].lemma_keys, ["le", "la"]);
        assert_eq!(tokens[0].norm, "l'");
        assert_eq!(tokens[5].kind, TokenKind::Name);
        assert_eq!(tokens[3].kind, TokenKind::Word);
        assert_eq!(tokens[2].lemma_keys, ["être"]);
    }

    #[test]
    fn french_inversion_and_the_euphonic_t() {
        let tokens = tokenise(Lang::Fr, "Est-ce que tu viens ?", &mini());
        assert_eq!(surfaces(&tokens), ["Est", "ce", "que", "tu", "viens"]);
        let tokens = tokenise(
            Lang::Fr,
            "A-t-il faim ? Qu'est-ce qu'il a dit, d’accord ?",
            &mini(),
        );
        assert_eq!(
            surfaces(&tokens),
            [
                "A", "il", "faim", "Qu'", "est", "ce", "qu'", "il", "a", "dit", "d’", "accord"
            ]
        );
        assert_eq!(tokens[10].norm, "d'");
        assert_eq!(tokens[10].lemma_keys, ["de"]);
    }

    #[test]
    fn contractions_keep_one_token_with_two_keys() {
        let tokens = tokenise(Lang::Es, "Voy al mercado del pueblo.", &mini());
        let keys: Vec<Vec<String>> = tokens.iter().map(|t| t.lemma_keys.clone()).collect();
        assert_eq!(
            keys,
            vec![
                vec!["ir".to_string()],
                vec!["a".to_string(), "el".to_string()],
                vec!["mercado".to_string()],
                vec!["de".to_string(), "el".to_string()],
                vec!["pueblo".to_string()],
            ]
        );
        let tokens = tokenise(Lang::De, "Ich gehe zum Bahnhof.", &mini());
        assert_eq!(tokens[2].lemma_keys, ["zu", "dem"]);
        assert_eq!(tokens[3].lemma_keys, ["Bahnhof"]);
        assert_eq!(tokens[3].kind, TokenKind::Word);
        let tokens = tokenise(Lang::De, "Die Straße ist lang.", &mini());
        assert_eq!(surfaces(&tokens), ["Die", "Straße", "ist", "lang"]);
    }

    #[test]
    fn digits_are_numbers_and_sentence_start_is_not_a_name() {
        let tokens = tokenise(Lang::Es, "Tengo 3 gatos.", &mini());
        assert_eq!(tokens[1].kind, TokenKind::Number);
        assert_eq!(tokens[1].norm, "3");
        let tokens = tokenise(
            Lang::Es,
            "Hola, ¿qué tal? ¡Buenas! \"Ana\" dijo Luis.",
            &mini(),
        );
        let kinds: Vec<(&str, TokenKind)> = tokens
            .iter()
            .map(|t| (t.surface.as_str(), t.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("Hola", TokenKind::Word),
                ("qué", TokenKind::Word),
                ("tal", TokenKind::Word),
                ("Buenas", TokenKind::Word),
                ("Ana", TokenKind::Word),
                ("dijo", TokenKind::Word),
                ("Luis", TokenKind::Name),
            ]
        );
        assert_eq!(tokens[1].norm, "qué");
    }

    #[test]
    fn keys_merge_surface_and_norm_lookups() {
        let tokens = tokenise(Lang::Es, "Fue", &mini());
        assert_eq!(tokens[0].lemma_keys, ["ir", "ser"]);
        assert_eq!(tokens[0].kind, TokenKind::Word);
    }
}
