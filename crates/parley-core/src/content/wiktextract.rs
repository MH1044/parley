//! Reading a wiktextract extract (kaikki.org: one JSON object per line) into forms rows, and
//! writing a forms table (docs/modules/01-content-model-and-syllabus.md, Interfaces).
//!
//! An entry is used when `lang_code` is the language and, when a lemma list is given, `word` is
//! on it. Its gender is the first `masculine`, `feminine` or `neuter` found in, in order, the
//! entry's `tags`, the tags of a `forms` entry tagged `canonical`, then the `senses` entries'
//! `tags`; the gender column carries it for nouns and is empty for other parts of speech. Rows:
//! one for the headword with tags `canonical`, then one per `forms` entry whose `form` differs
//! from the headword, with that entry's tags joined by single spaces. Rows are sorted by
//! (form, lemma, pos) in byte order; a repeated (form, lemma, pos) keeps the first tags.

use std::collections::HashSet;
use std::io::{self, BufRead, Write};

use serde::Deserialize;

use super::forms::FormRow;
use super::syllabus::Gender;
use super::{ContentError, Lang};

/// What `read_wiktextract` found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extract {
    pub rows: Vec<FormRow>,
    /// The requested lemmas with at least one entry (every headword, when none were requested).
    pub found: usize,
    /// The requested lemmas without an entry, in the order requested.
    pub missing: Vec<String>,
}

#[derive(Deserialize)]
struct Entry {
    word: Option<String>,
    pos: Option<String>,
    lang_code: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    forms: Vec<FormEntry>,
    #[serde(default)]
    senses: Vec<Sense>,
}

#[derive(Deserialize)]
struct FormEntry {
    form: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct Sense {
    #[serde(default)]
    tags: Vec<String>,
}

fn gender_in(tags: &[String]) -> Option<Gender> {
    tags.iter().find_map(|tag| match tag.as_str() {
        "masculine" => Some(Gender::M),
        "feminine" => Some(Gender::F),
        "neuter" => Some(Gender::N),
        _ => None,
    })
}

fn gender_of(entry: &Entry) -> Option<Gender> {
    gender_in(&entry.tags)
        .or_else(|| {
            entry
                .forms
                .iter()
                .filter(|form| form.tags.iter().any(|tag| tag == "canonical"))
                .find_map(|form| gender_in(&form.tags))
        })
        .or_else(|| entry.senses.iter().find_map(|sense| gender_in(&sense.tags)))
}

/// Reads an extract for `lang`, keeping every entry or only the entries of `lemmas`.
pub fn read_wiktextract(
    r: impl BufRead,
    lang: Lang,
    lemmas: Option<&[String]>,
) -> Result<Extract, ContentError> {
    let wanted: Option<HashSet<&str>> =
        lemmas.map(|list| list.iter().map(String::as_str).collect());
    let mut headwords: HashSet<String> = HashSet::new();
    let mut rows = Vec::new();
    for (index, line) in r.lines().enumerate() {
        let n = index + 1;
        let line = line.map_err(|e| ContentError(format!("wiktextract: line {n}: {e}")))?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: Entry = serde_json::from_str(&line)
            .map_err(|e| ContentError(format!("wiktextract: line {n}: {e}")))?;
        let (Some(word), Some(pos), Some(lang_code)) = (&entry.word, &entry.pos, &entry.lang_code)
        else {
            continue;
        };
        if lang_code != lang.as_str() {
            continue;
        }
        if let Some(wanted) = &wanted
            && !wanted.contains(word.as_str())
        {
            continue;
        }
        headwords.insert(word.clone());
        let gender = if pos == "noun" {
            gender_of(&entry)
        } else {
            None
        };
        rows.push(FormRow {
            form: word.clone(),
            lemma: word.clone(),
            pos: pos.clone(),
            gender,
            tags: "canonical".to_string(),
        });
        for form in &entry.forms {
            let Some(text) = &form.form else { continue };
            if text == word {
                continue;
            }
            rows.push(FormRow {
                form: text.clone(),
                lemma: word.clone(),
                pos: pos.clone(),
                gender,
                tags: form.tags.join(" "),
            });
        }
    }
    rows.sort_by(|a, b| {
        (a.form.as_str(), a.lemma.as_str(), a.pos.as_str()).cmp(&(
            b.form.as_str(),
            b.lemma.as_str(),
            b.pos.as_str(),
        ))
    });
    rows.dedup_by(|later, earlier| {
        later.form == earlier.form && later.lemma == earlier.lemma && later.pos == earlier.pos
    });
    let (found, missing) = match lemmas {
        Some(list) => {
            let mut found = 0;
            let mut missing = Vec::new();
            let mut seen: HashSet<&str> = HashSet::new();
            for lemma in list {
                if !seen.insert(lemma.as_str()) {
                    continue;
                }
                if headwords.contains(lemma) {
                    found += 1;
                } else {
                    missing.push(lemma.clone());
                }
            }
            (found, missing)
        }
        None => (headwords.len(), Vec::new()),
    };
    Ok(Extract {
        rows,
        found,
        missing,
    })
}

/// Writes a forms table: the header comment, then one row per form, LF line ends.
pub fn write_forms_tsv(
    w: &mut impl Write,
    source: &str,
    sha256: &str,
    date: &str,
    rows: &[FormRow],
) -> io::Result<()> {
    writeln!(w, "# source: {source}; sha256: {sha256}; date: {date}")?;
    for row in rows {
        writeln!(
            w,
            "{}\t{}\t{}\t{}\t{}",
            row.form,
            row.lemma,
            row.pos,
            row.gender.map_or("", Gender::as_str),
            row.tags
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::forms::Forms;

    const EXTRACT: &str = concat!(
        r#"{"word":"bueno","pos":"adj","lang_code":"es","forms":[{"form":"bueno","tags":["canonical"]},{"form":"buena","tags":["feminine","singular"]},{"form":"buenos","tags":["masculine","plural"]},{"form":"buenas","tags":["feminine","plural"]}],"senses":[{"glosses":["good"]}]}"#,
        "\n",
        r#"{"word":"noche","pos":"noun","lang_code":"es","forms":[{"form":"noche","tags":["canonical","feminine"]},{"form":"noches","tags":["plural"]}],"senses":[{"tags":["feminine"]}]}"#,
        "\n",
        r#"{"word":"Tisch","pos":"noun","lang_code":"de","tags":["masculine"],"forms":[{"form":"Tische","tags":["plural"]}]}"#,
        "\n",
        r#"{"word":"ir","pos":"verb","lang_code":"es","forms":[{"form":"fue","tags":["third-person","singular","preterite"]},{"form":"voy","tags":["first-person","singular","present"]}]}"#,
        "\n",
        r#"{"word":"ser","pos":"verb","lang_code":"es","forms":[{"form":"fue","tags":["third-person","singular","preterite"]}]}"#,
        "\n",
        r#"{"word":"ser","pos":"verb","lang_code":"es","forms":[{"form":"fue","tags":["repeated","later"]}]}"#,
        "\n",
        r#"{"title":"redirect","redirect":"x"}"#,
        "\n",
    );

    #[test]
    fn rows_are_sorted_with_the_headword_first_and_genders_for_nouns() {
        let extract = read_wiktextract(EXTRACT.as_bytes(), Lang::Es, None).unwrap();
        assert_eq!(extract.found, 4);
        assert!(extract.missing.is_empty());
        let forms = Forms::from_rows(extract.rows.clone());
        assert_eq!(forms.lemmas_of("buenas"), ["bueno"]);
        assert_eq!(forms.lemmas_of("fue"), ["ir", "ser"]);
        assert_eq!(forms.gender_of("noche"), Some(Gender::F));
        assert_eq!(forms.gender_of("bueno"), None);
        let keys: Vec<(&str, &str, &str)> = extract
            .rows
            .iter()
            .map(|r| (r.form.as_str(), r.lemma.as_str(), r.pos.as_str()))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(keys, sorted);
        let fue_ser = extract
            .rows
            .iter()
            .find(|r| r.form == "fue" && r.lemma == "ser")
            .unwrap();
        assert_eq!(fue_ser.tags, "third-person singular preterite");
        let bueno = extract.rows.iter().find(|r| r.form == "bueno").unwrap();
        assert_eq!(bueno.tags, "canonical");
    }

    #[test]
    fn a_lemma_list_reports_found_and_missing_in_order() {
        let lemmas: Vec<String> = ["bueno", "zzz", "noche", "bueno"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let extract = read_wiktextract(EXTRACT.as_bytes(), Lang::Es, Some(&lemmas)).unwrap();
        assert_eq!(extract.found, 2);
        assert_eq!(extract.missing, ["zzz"]);
        assert!(
            extract
                .rows
                .iter()
                .all(|r| r.lemma == "bueno" || r.lemma == "noche")
        );
    }

    #[test]
    fn entry_tags_give_the_gender_first() {
        let extract = read_wiktextract(EXTRACT.as_bytes(), Lang::De, None).unwrap();
        let forms = Forms::from_rows(extract.rows);
        assert_eq!(forms.gender_of("Tisch"), Some(Gender::M));
    }

    #[test]
    fn the_table_round_trips_through_tsv() {
        let extract = read_wiktextract(EXTRACT.as_bytes(), Lang::Es, None).unwrap();
        let mut out = Vec::new();
        write_forms_tsv(
            &mut out,
            "extract.jsonl",
            "abc",
            "2026-10-01",
            &extract.rows,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("# source: extract.jsonl; sha256: abc; date: 2026-10-01\n"));
        assert!(text.contains("noches\tnoche\tnoun\tf\tplural\n"));
        assert!(text.contains("buenas\tbueno\tadj\t\tfeminine plural\n"));
        let forms = Forms::load_tsv(text.as_bytes()).unwrap();
        assert_eq!(forms.rows(), &extract.rows[..]);
    }

    #[test]
    fn a_line_that_is_not_json_names_its_number() {
        let err = read_wiktextract("{}\nnot json\n".as_bytes(), Lang::Es, None).unwrap_err();
        assert!(err.0.starts_with("wiktextract: line 2: "), "{}", err.0);
    }
}
