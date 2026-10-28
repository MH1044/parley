//! `numbers.yaml` (the words for 0 to 100) and `exclude.txt` (lemmas never taught or shown).

use serde::Deserialize;
use unicode_segmentation::UnicodeSegmentation;

use super::{ContentError, nfc_lower};

/// The schema tag of `numbers.yaml`.
pub const NUMBERS_SCHEMA: &str = "parley.numbers/1";

#[derive(Deserialize)]
struct NumbersFile {
    schema: String,
    words: Vec<String>,
}

/// Reads `numbers.yaml` and returns its number words: every word of every entry (`treinta y
/// uno` gives `treinta`, `y`, `uno`), normalised, in first-seen order without repeats.
pub fn load_numbers(text: &str) -> Result<Vec<String>, ContentError> {
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
    fn a_wrong_schema_is_refused() {
        let text = numbers_yaml(101).replace("parley.numbers/1", "parley.numbers/2");
        let err = load_numbers(&text).expect_err("another schema is refused");
        assert_eq!(err.0, "numbers.yaml: schema must be parley.numbers/1");
    }
}
