//! The grammar and functions checklist (`content/<lang>/checklist.yaml`, docs/modules/
//! 05-course-content-a1-a2.md): the grammar points and communicative functions of each level, in
//! the project's own words, each mapped to the skills that teach it.
//!
//! Rules, in this order: **CL01** YAML or schema (`{file}: {pointer}: {message}`, worded as V01),
//! which stops the others; **CL02** `{file}: duplicate entry {id}`; **CL03**
//! `{file}: {id}: skill {skill} does not exist`; **CL04**
//! `{file}: {id}: skill {skill} is {level}, but the entry is {cefr}`. Within a rule, messages
//! follow the file.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::syllabus::{Cefr, Syllabus};
use super::validate::Report;
use super::{ContentError, Lang, checklist_validator, load_yaml, parse_yaml};

/// Whether an entry is a grammar point or a communicative function.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Grammar,
    Function,
}

/// One grammar point or function of a level.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecklistEntry {
    /// `a1.` or `a2.`, `g.` or `f.`, then lower case letters, digits and `_`.
    pub id: String,
    pub cefr: Cefr,
    pub kind: EntryKind,
    pub text_en: String,
    /// The skills that teach it; empty when no skill does yet (the entry is uncovered).
    pub skills: Vec<String>,
}

/// A language's checklist.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checklist {
    pub schema: String,
    pub lang: Lang,
    pub entries: Vec<ChecklistEntry>,
}

impl Checklist {
    /// The ids of the entries no skill teaches, in file order.
    pub fn uncovered(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|entry| entry.skills.is_empty())
            .map(|entry| entry.id.clone())
            .collect()
    }
}

/// Parses checklist text named `file` (the path as typed): CL01 messages only.
pub fn parse_checklist(file: &str, text: &str) -> Result<Checklist, Vec<ContentError>> {
    parse_yaml(file, text, checklist_validator())
}

/// Reads and parses a checklist file: CL01 messages only.
pub fn load_checklist(path: &Path) -> Result<Checklist, Vec<ContentError>> {
    load_yaml(path, checklist_validator())
}

/// Runs CL02 to CL04 over a checklist that parsed, against its language's syllabus. Without a
/// syllabus (it does not load) only CL02 runs: CL03 and CL04 need to know the skills.
pub fn check_checklist(file: &str, checklist: &Checklist, s: Option<&Syllabus>) -> Report {
    let mut errors = Vec::new();

    // CL02
    let mut seen: HashSet<&str> = HashSet::new();
    for entry in &checklist.entries {
        if !seen.insert(&entry.id) {
            errors.push(format!("{file}: duplicate entry {}", entry.id));
        }
    }

    let Some(s) = s else {
        return Report {
            errors,
            warnings: Vec::new(),
        };
    };
    let levels: HashMap<&str, Cefr> = s
        .skills()
        .map(|(_, skill)| (skill.id.as_str(), skill.cefr))
        .collect();

    // CL03
    for entry in &checklist.entries {
        for skill in &entry.skills {
            if !levels.contains_key(skill.as_str()) {
                errors.push(format!(
                    "{file}: {}: skill {skill} does not exist",
                    entry.id
                ));
            }
        }
    }

    // CL04
    for entry in &checklist.entries {
        for skill in &entry.skills {
            if let Some(level) = levels.get(skill.as_str())
                && *level != entry.cefr
            {
                errors.push(format!(
                    "{file}: {}: skill {skill} is {}, but the entry is {}",
                    entry.id,
                    level.as_str(),
                    entry.cefr.as_str()
                ));
            }
        }
    }

    Report {
        errors,
        warnings: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::parse_syllabus;
    use crate::content::test_support::EXAMPLE_SYLLABUS;

    const CHECKLIST: &str = "\
schema: parley.checklist/1
lang: es
entries:
  - {id: a1.f.greet, cefr: A1, kind: function, text_en: Greet people and say goodbye,
     skills: [es.a1.u01.s01]}
  - {id: a1.g.questions, cefr: A1, kind: grammar,
     text_en: \"Questions open with an upside-down mark\", skills: []}
";

    fn check(text: &str) -> Report {
        let syllabus = parse_syllabus("s.yaml", EXAMPLE_SYLLABUS).unwrap();
        let checklist = parse_checklist("c.yaml", text).unwrap();
        check_checklist("c.yaml", &checklist, Some(&syllabus))
    }

    #[test]
    fn a_valid_checklist_passes_and_lists_its_uncovered_entries() {
        assert_eq!(check(CHECKLIST), Report::default());
        let checklist = parse_checklist("c.yaml", CHECKLIST).unwrap();
        assert_eq!(checklist.uncovered(), ["a1.g.questions"]);
        assert_eq!(checklist.entries[0].kind, EntryKind::Function);
    }

    #[test]
    fn cl01_uses_the_v01_wording_for_the_schema() {
        let text = CHECKLIST.replace("kind: function", "kind: phrase");
        let errors = parse_checklist("c.yaml", &text).unwrap_err();
        assert_eq!(
            errors,
            [ContentError(
                "c.yaml: /entries/0/kind: must be one of grammar, function".into()
            )]
        );
        let text = CHECKLIST.replace("schema: parley.checklist/1", "schema: parley.checklist/2");
        assert_eq!(
            parse_checklist("c.yaml", &text).unwrap_err()[0].0,
            "c.yaml: /schema: must be parley.checklist/1"
        );
        let text = CHECKLIST.replace("text_en: Greet people and say goodbye,", "");
        assert_eq!(
            parse_checklist("c.yaml", &text).unwrap_err()[0].0,
            "c.yaml: /entries/0: text_en is required"
        );
        let text = CHECKLIST.replace("id: a1.f.greet", "id: b1.f.greet");
        let errors = parse_checklist("c.yaml", &text).unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].0.starts_with("c.yaml: /entries/0/id: "),
            "{errors:?}"
        );
        let errors = parse_checklist("c.yaml", "entries: [\n").unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].0.starts_with("c.yaml: "), "{errors:?}");
    }

    #[test]
    fn cl02_names_a_repeated_entry() {
        let text = CHECKLIST.replace("id: a1.g.questions", "id: a1.f.greet");
        assert_eq!(check(&text).errors, ["c.yaml: duplicate entry a1.f.greet"]);
    }

    #[test]
    fn cl03_names_a_skill_the_syllabus_lacks() {
        let text = CHECKLIST.replace("skills: []", "skills: [es.a1.u09.s01, es.a1.u01.s01]");
        assert_eq!(
            check(&text).errors,
            ["c.yaml: a1.g.questions: skill es.a1.u09.s01 does not exist"]
        );
    }

    #[test]
    fn cl04_names_a_skill_of_the_other_level() {
        let text = CHECKLIST.replace("{id: a1.f.greet, cefr: A1", "{id: a2.f.greet, cefr: A2");
        assert_eq!(
            check(&text).errors,
            ["c.yaml: a2.f.greet: skill es.a1.u01.s01 is A1, but the entry is A2"]
        );
    }

    #[test]
    fn without_a_syllabus_only_duplicates_are_reported() {
        let text = CHECKLIST
            .replace("skills: []", "skills: [es.a1.u05.s01]")
            .replace("id: a1.g.questions", "id: a1.f.greet");
        let checklist = parse_checklist("c.yaml", &text).unwrap();
        assert_eq!(
            check_checklist("c.yaml", &checklist, None).errors,
            ["c.yaml: duplicate entry a1.f.greet"]
        );
    }

    #[test]
    fn errors_come_in_rule_order() {
        let text = CHECKLIST
            .replace("{id: a1.f.greet, cefr: A1", "{id: a1.f.greet, cefr: A2")
            .replace("skills: []", "skills: [es.a1.u05.s01]")
            .replace("id: a1.g.questions", "id: a1.f.greet");
        assert_eq!(
            check(&text).errors,
            [
                "c.yaml: duplicate entry a1.f.greet",
                "c.yaml: a1.f.greet: skill es.a1.u05.s01 does not exist",
                "c.yaml: a1.f.greet: skill es.a1.u01.s01 is A1, but the entry is A2",
            ]
        );
    }
}
