//! `numbers.yaml` (the words for 0 to 100) and `exclude.txt` (lemmas never taught or shown).

use serde::Deserialize;
use unicode_segmentation::UnicodeSegmentation;

use super::{ContentError, nfc, nfc_lower};

/// The schema tag of `numbers.yaml`.
pub const NUMBERS_SCHEMA: &str = "parley.numbers/1";

#[derive(Deserialize)]
struct NumbersFile {
    schema: String,
    words: Vec<String>,
}

/// The words for 0 to 100 of `numbers.yaml` as written, NFC; entry n is the number n. A course
/// without the file has none, and digits then stay as written (module 10's speaking checks).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Numbers {
    pub words: Vec<String>,
}

impl Numbers {
    /// The words of the number `n` (`70` → `soixante-dix`), when the file has an entry for it.
    pub fn spell(&self, n: u64) -> Option<&str> {
        usize::try_from(n)
            .ok()
            .and_then(|n| self.words.get(n))
            .map(String::as_str)
    }
}

/// Reads and checks `numbers.yaml`: its schema tag and exactly 101 entries.
fn read_numbers_file(text: &str) -> Result<NumbersFile, ContentError> {
    let file: NumbersFile =
        serde_yaml_ng::from_str(text).map_err(|e| ContentError(format!("numbers.yaml: {e}")))?;
    if file.schema != NUMBERS_SCHEMA {
        return Err(ContentError(format!(
            "numbers.yaml: schema must be {NUMBERS_SCHEMA}"
        )));
    }
    if file.words.len() != 101 {
        return Err(ContentError(format!(
            "numbers.yaml: words has {} entries; 0 to 100 need 101",
            file.words.len()
        )));
    }
    Ok(file)
}

/// The same checks as [`load_numbers`] (schema, 101 entries); the entries in order, NFC.
pub fn load_number_words(text: &str) -> Result<Numbers, ContentError> {
    let file = read_numbers_file(text)?;
    Ok(Numbers {
        words: file.words.iter().map(|entry| nfc(entry.trim())).collect(),
    })
}

/// Reads `numbers.yaml` and returns its number words: every word of every entry (`treinta y
/// uno` gives `treinta`, `y`, `uno`), normalised, in first-seen order without repeats.
pub fn load_numbers(text: &str) -> Result<Vec<String>, ContentError> {
    let file = read_numbers_file(text)?;
    let mut words: Vec<String> = Vec::new();
    for entry in &file.words {
        for word in entry.unicode_words() {
            let word = nfc_lower(word);
            if !words.contains(&word) {
                words.push(word);
            }
        }
    }
    Ok(words)
}

/// Reads `exclude.txt`: one lemma per line; blank lines and `#` comments are ignored.
pub fn load_exclude(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbers_yaml(count: usize) -> String {
        let mut text = String::from("schema: parley.numbers/1\nlang: es\nwords:\n");
        for n in 0..count {
            let word = match n {
                0 => "cero".to_string(),
                31 => "treinta y uno".to_string(),
                _ => format!("n{n}"),
            };
            text.push_str(&format!("  - {word}\n"));
        }
        text
    }

    #[test]
    fn entries_are_split_into_words() {
        let words = load_numbers(&numbers_yaml(101)).unwrap();
        assert!(words.contains(&"cero".to_string()));
        assert!(words.contains(&"treinta".to_string()));
        assert!(words.contains(&"y".to_string()));
        assert!(words.contains(&"uno".to_string()));
        assert!(!words.contains(&"treinta y uno".to_string()));
    }

    #[test]
    fn the_count_must_be_101() {
        let err = load_numbers(&numbers_yaml(100)).unwrap_err();
        assert_eq!(
            err.0,
            "numbers.yaml: words has 100 entries; 0 to 100 need 101"
        );
    }

    #[test]
    fn exclude_skips_blank_lines_and_comments() {
        assert_eq!(
            load_exclude("# never taught\n\nputo \n mierda\n"),
            ["puto", "mierda"]
        );
    }

    #[test]
    fn number_words_keep_each_entry_as_written() {
        let numbers = load_number_words(&numbers_yaml(101)).unwrap();
        assert_eq!(numbers.words.len(), 101);
        assert_eq!(numbers.spell(0), Some("cero"));
        assert_eq!(numbers.spell(31), Some("treinta y uno"));
        assert_eq!(numbers.spell(100), Some("n100"));
        assert_eq!(numbers.spell(101), None);
        assert_eq!(Numbers::default().spell(3), None);
        assert!(load_number_words(&numbers_yaml(100)).is_err());
        let decomposed = numbers_yaml(101).replace("n16", "diecise\u{301}is");
        assert_eq!(
            load_number_words(&decomposed).unwrap().spell(16),
            Some("diecis\u{e9}is"),
            "entries are NFC"
        );
    }

    #[test]
    fn a_wrong_schema_is_refused() {
        let text = numbers_yaml(101).replace("parley.numbers/1", "parley.numbers/2");
        assert!(load_number_words(&text).is_err());
        let err = load_numbers(&text).expect_err("another schema is refused");
        assert_eq!(err.0, "numbers.yaml: schema must be parley.numbers/1");
    }
}
