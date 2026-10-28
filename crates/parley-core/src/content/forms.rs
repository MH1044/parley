//! The forms table: inflected form to lemma (`content/<lang>/forms.tsv` and a pack's `forms`
//! table), with the lookups the tokeniser and the validator need.
//!
//! Format: the first line is the comment `# source: {file}; sha256: {hex}; date: {YYYY-MM-DD}`,
//! then one row per form, no column-name line: `form<TAB>lemma<TAB>pos<TAB>gender<TAB>tags`.
//! Blank lines and `#` lines are skipped.

use std::collections::HashMap;
use std::io::Read;

use serde::{Deserialize, Serialize};

use super::ContentError;
use super::syllabus::Gender;

/// One row of a forms table.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FormRow {
    pub form: String,
    pub lemma: String,
    pub pos: String,
    /// The lemma's gender, for nouns only.
    pub gender: Option<Gender>,
    /// The form's tags, joined by single spaces (`feminine plural`).
    pub tags: String,
}

/// A loaded forms table with indexes by form and by lemma.
#[derive(Clone, Debug, Default)]
pub struct Forms {
    rows: Vec<FormRow>,
    by_form: HashMap<String, Vec<usize>>,
    by_lemma: HashMap<String, Vec<usize>>,
}

impl Forms {
    /// Reads a TSV forms table. A line without exactly five columns is refused:
    /// `forms.tsv: line {n}: expected 5 tab-separated columns`.
    pub fn load_tsv(mut r: impl Read) -> Result<Forms, ContentError> {
        let mut text = String::new();
        r.read_to_string(&mut text)
            .map_err(|e| ContentError(format!("forms.tsv: {e}")))?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let mut rows = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let n = index + 1;
            let line = line.trim_end_matches('\r');
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let columns: Vec<&str> = line.split('\t').collect();
            if columns.len() != 5 {
                return Err(ContentError(format!(
                    "forms.tsv: line {n}: expected 5 tab-separated columns"
                )));
            }
            let gender = match columns[3] {
                "" => None,
                text => Some(Gender::parse(text).ok_or_else(|| {
                    ContentError(format!(
                        "forms.tsv: line {n}: gender must be m, f, n or empty"
                    ))
                })?),
            };
            rows.push(FormRow {
                form: columns[0].to_string(),
                lemma: columns[1].to_string(),
                pos: columns[2].to_string(),
                gender,
                tags: columns[4].to_string(),
            });
        }
        Ok(Forms::from_rows(rows))
    }

    /// A table from rows in the order given.
    pub fn from_rows(rows: Vec<FormRow>) -> Forms {
        let mut by_form: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_lemma: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, row) in rows.iter().enumerate() {
            by_form.entry(row.form.clone()).or_default().push(index);
            by_lemma.entry(row.lemma.clone()).or_default().push(index);
        }
        Forms {
            rows,
            by_form,
            by_lemma,
        }
    }

    /// Every row, in the order loaded.
    pub fn rows(&self) -> &[FormRow] {
        &self.rows
    }

    /// Whether the table has no rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The lemmas `form` is a form of, sorted and deduplicated; empty when the form is unknown.
    pub fn lemmas_of(&self, form: &str) -> Vec<&str> {
        let mut lemmas: Vec<&str> = self
            .by_form
            .get(form)
            .map(|indexes| {
                indexes
                    .iter()
                    .map(|&i| self.rows[i].lemma.as_str())
                    .collect()
            })
            .unwrap_or_default();
        lemmas.sort_unstable();
        lemmas.dedup();
        lemmas
    }

    /// The gender of the noun rows of `noun` (the lemma), when any carries one.
    pub fn gender_of(&self, noun: &str) -> Option<Gender> {
        self.by_lemma
            .get(noun)?
            .iter()
            .map(|&i| &self.rows[i])
            .filter(|row| row.pos == "noun")
            .find_map(|row| row.gender)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "# source: hand-written; sha256: -; date: 2026-10-01\n\
        buenas\tbueno\tadj\t\tfeminine plural\n\
        buenos\tbueno\tadj\t\tmasculine plural\n\
        noches\tnoche\tnoun\tf\tplural\n\
        fue\tir\tverb\t\tpreterite third-person singular\n\
        fue\tser\tverb\t\tpreterite third-person singular\n\
        mesa\tmesa\tnoun\tf\tcanonical\n\
        Tisch\tTisch\tnoun\tm\tcanonical\n";

    #[test]
    fn lookups_are_sorted_and_deduplicated() {
        let forms = Forms::load_tsv(TABLE.as_bytes()).unwrap();
        assert_eq!(forms.rows().len(), 7);
        assert_eq!(forms.lemmas_of("buenas"), ["bueno"]);
        assert_eq!(forms.lemmas_of("fue"), ["ir", "ser"]);
        assert!(forms.lemmas_of("zzz").is_empty());
        assert_eq!(forms.gender_of("mesa"), Some(Gender::F));
        assert_eq!(forms.gender_of("Tisch"), Some(Gender::M));
        assert_eq!(forms.gender_of("bueno"), None);
    }

    #[test]
    fn a_short_line_is_refused_with_its_number() {
        let err = Forms::load_tsv("# header\n\nbuenas\tbueno\tadj\n".as_bytes()).unwrap_err();
        assert_eq!(err.0, "forms.tsv: line 3: expected 5 tab-separated columns");
    }

    #[test]
    fn default_is_empty() {
        let forms = Forms::default();
        assert!(forms.is_empty());
        assert!(forms.lemmas_of("x").is_empty());
    }

    #[test]
    fn a_gender_outside_m_f_n_is_refused_with_its_line() {
        let bad =
            "# source: hand-written; sha256: -; date: 2026-10-01\nmesa\tmesa\tnoun\tx\tcanonical\n";
        let err = Forms::load_tsv(bad.as_bytes()).expect_err("gender x is refused");
        assert_eq!(err.0, "forms.tsv: line 2: gender must be m, f, n or empty");
    }
}
