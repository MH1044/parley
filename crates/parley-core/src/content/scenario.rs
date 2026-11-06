//! Role-play scenarios (`content/scenarios/<id>.yaml`, docs/modules/05-course-content-a1-a2.md):
//! a situation with goals, a turn cap and, per language, a setting, the partner's opening line
//! and the skills it builds on.
//!
//! Rules, in this order; `{file}` is the path as typed with `/` separators and `{id}` the
//! scenario's `id`:
//!
//! - **SC01** YAML or schema (`{file}: {pointer}: {message}`, worded as V01), which stops the
//!   others.
//! - **SC02** `{file}: id must match the file name`.
//! - **SC03** `{id}: {n} goals; a scenario has 3 to 6`.
//! - **SC04** `{id}: turn_cap {n} is outside 6 to 20`.
//! - **SC05** `{id}: {lang}: skill {skill} does not exist`, once per skill in list order.
//! - **SC06** `{id}: {lang}: opening uses "{word}", which the listed skills do not teach`, once
//!   per offending word in order, for a language whose skills all exist. A word is taught when it
//!   is a function word or a name of the language's syllabus, a number, or a form of a lemma of
//!   any skill up to and including the latest listed skill in syllabus order: V12's rule, through
//!   the same [`TaughtForms`].
//!
//! Languages are checked in the order es, fr, de.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::forms::Forms;
use super::items::TaughtForms;
use super::syllabus::{Cefr, Syllabus};
use super::tokenise::tokenise;
use super::validate::{Known, Report};
use super::{ContentError, Lang, display_path, load_yaml, parse_yaml, scenario_validator};

/// A goal the learner works towards in the conversation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Goal {
    /// Lower case letters, digits and `_`.
    pub id: String,
    pub text_en: String,
}

/// A scenario's text for one language.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioLanguage {
    pub setting_en: String,
    /// The partner's first line, in the target language.
    pub opening: String,
    pub opening_en: String,
    /// The skills the scenario builds on, at least one.
    pub skills: Vec<String>,
}

/// A role-play scenario.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scenario {
    pub schema: String,
    pub id: String,
    pub cefr: Cefr,
    pub title_en: String,
    pub partner_role_en: String,
    pub learner_role_en: String,
    /// The most turns a conversation takes (6 to 20, SC04).
    pub turn_cap: i64,
    pub goals: Vec<Goal>,
    /// At least one of es, fr and de, in that order.
    pub languages: BTreeMap<Lang, ScenarioLanguage>,
}

/// What SC05 and SC06 check a language's section against: its syllabus, forms table and number
/// words. A language whose syllabus is missing (or does not load) has none.
#[derive(Clone, Copy, Debug)]
pub struct ScenarioCourse<'a> {
    pub syllabus: &'a Syllabus,
    pub forms: &'a Forms,
    pub numbers: &'a [String],
}

/// Parses scenario text named `file` (the path as typed): SC01 messages only.
pub fn parse_scenario(file: &str, text: &str) -> Result<Scenario, Vec<ContentError>> {
    parse_yaml(file, text, scenario_validator())
}

/// Reads and parses a scenario file: SC01 messages only.
pub fn load_scenario(path: &Path) -> Result<Scenario, Vec<ContentError>> {
    load_yaml(path, scenario_validator())
}

/// Runs SC02 to SC06 over a scenario that parsed. `name` is the file name without `.yaml`.
pub fn check_scenario(
    file: &str,
    name: &str,
    scenario: &Scenario,
    courses: &BTreeMap<Lang, ScenarioCourse<'_>>,
) -> Report {
    let mut errors = Vec::new();
    let id = &scenario.id;

    // SC02
    if scenario.id != name {
        errors.push(format!("{file}: id must match the file name"));
    }

    // SC03
    let goals = scenario.goals.len();
    if !(3..=6).contains(&goals) {
        errors.push(format!("{id}: {goals} goals; a scenario has 3 to 6"));
    }

    // SC04
    if !(6..=20).contains(&scenario.turn_cap) {
        errors.push(format!(
            "{id}: turn_cap {} is outside 6 to 20",
            scenario.turn_cap
        ));
    }

    // SC05; a language passes on to SC06 only when every listed skill exists.
    let mut complete: Vec<(Lang, &ScenarioLanguage, &ScenarioCourse<'_>, usize)> = Vec::new();
    for (lang, section) in &scenario.languages {
        let course = courses.get(lang);
        let mut latest: Option<usize> = None;
        let mut all_exist = true;
        for skill in &section.skills {
            let position = course.and_then(|course| {
                course
                    .syllabus
                    .skills()
                    .position(|(_, listed)| listed.id == *skill)
            });
            match position {
                Some(index) => latest = Some(latest.map_or(index, |l| l.max(index))),
                None => {
                    all_exist = false;
                    errors.push(format!(
                        "{id}: {}: skill {skill} does not exist",
                        lang.as_str()
                    ));
                }
            }
        }
        if let (true, Some(course), Some(latest)) = (all_exist, course, latest) {
            complete.push((*lang, section, course, latest));
        }
    }

    // SC06
    for (lang, section, course, latest) in complete {
        let known = Known::new(course.syllabus, course.numbers);
        let taught: Vec<TaughtForms> = course
            .syllabus
            .skills()
            .take(latest + 1)
            .flat_map(|(_, skill)| skill.lemmas.iter().map(TaughtForms::of))
            .collect();
        for token in tokenise(lang, &section.opening, course.forms) {
            let allowed = known.is_number(&token)
                || known.is_name(&token)
                || known.is_function(&token)
                || taught.iter().any(|forms| forms.matches(&token));
            if !allowed {
                errors.push(format!(
                    "{id}: {}: opening uses \"{}\", which the listed skills do not teach",
                    lang.as_str(),
                    token.surface
                ));
            }
        }
    }

    Report {
        errors,
        warnings: Vec::new(),
    }
}

/// SC01 to SC06 for the scenario file at `path`: the file name in messages is the path as typed
/// with `/` separators, and SC02 compares the id with the file name without `.yaml`.
pub fn validate_scenario_file(path: &Path, courses: &BTreeMap<Lang, ScenarioCourse<'_>>) -> Report {
    let file = display_path(path);
    match load_scenario(path) {
        Ok(scenario) => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let name = name.strip_suffix(".yaml").unwrap_or(&name);
            check_scenario(&file, name, &scenario, courses)
        }
        Err(errors) => Report {
            errors: errors.into_iter().map(|e| e.0).collect(),
            warnings: Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::parse_syllabus;
    use crate::content::test_support::{EXAMPLE_SYLLABUS, example_forms};

    const CAFE: &str = "\
schema: parley.scenario/1
id: cafe
cefr: A1
title_en: At a café
partner_role_en: the waiter
learner_role_en: a customer
turn_cap: 12
goals:
  - {id: greet, text_en: Greet the waiter}
  - {id: order_drink, text_en: Order a drink}
  - {id: pay_leave, text_en: Pay and say goodbye}
languages:
  es:
    setting_en: a small café in Madrid in the morning
    opening: \"¡Buenos días, Ana! ¿Qué tal?\"
    opening_en: \"Good morning, Ana! How are you?\"
    skills: [es.a1.u01.s01]
";

    fn check(text: &str) -> Report {
        let syllabus = parse_syllabus("s.yaml", EXAMPLE_SYLLABUS).unwrap();
        let forms = example_forms();
        let mut courses = BTreeMap::new();
        courses.insert(
            Lang::Es,
            ScenarioCourse {
                syllabus: &syllabus,
                forms: &forms,
                numbers: &[],
            },
        );
        let scenario = parse_scenario("scenarios/cafe.yaml", text).unwrap();
        check_scenario("scenarios/cafe.yaml", "cafe", &scenario, &courses)
    }

    #[test]
    fn a_valid_scenario_passes() {
        assert_eq!(check(CAFE), Report::default());
        let scenario = parse_scenario("cafe.yaml", CAFE).unwrap();
        assert_eq!(scenario.languages[&Lang::Es].skills, ["es.a1.u01.s01"]);
    }

    #[test]
    fn sc01_uses_the_v01_wording_for_the_schema() {
        let text = CAFE.replace("turn_cap: 12", "turn_cap: twelve");
        assert_eq!(
            parse_scenario("cafe.yaml", &text).unwrap_err(),
            [ContentError(
                "cafe.yaml: /turn_cap: expected an integer".into()
            )]
        );
        let text = CAFE.replace("  es:\n", "  it:\n");
        let errors: Vec<String> = parse_scenario("cafe.yaml", &text)
            .unwrap_err()
            .into_iter()
            .map(|e| e.0)
            .collect();
        assert!(
            errors.contains(&"cafe.yaml: /languages: unknown field it".to_string()),
            "{errors:?}"
        );
        let text = CAFE.replace("cefr: A1", "cefr: B1");
        assert_eq!(
            parse_scenario("cafe.yaml", &text).unwrap_err()[0].0,
            "cafe.yaml: /cefr: must be one of A1, A2"
        );
        let text = CAFE.replace("    opening_en: \"Good morning, Ana! How are you?\"\n", "");
        assert_eq!(
            parse_scenario("cafe.yaml", &text).unwrap_err()[0].0,
            "cafe.yaml: /languages/es: opening_en is required"
        );
        let errors = parse_scenario("cafe.yaml", "goals: [\n").unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].0.starts_with("cafe.yaml: "), "{errors:?}");
    }

    #[test]
    fn sc01_needs_a_language_and_a_skill() {
        let text = "schema: parley.scenario/1\nid: cafe\ncefr: A1\ntitle_en: t\npartner_role_en: p\nlearner_role_en: l\nturn_cap: 12\ngoals: []\nlanguages: {}\n";
        let errors = parse_scenario("cafe.yaml", text).unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].0.starts_with("cafe.yaml: /languages: "),
            "{errors:?}"
        );
        let text = CAFE.replace("skills: [es.a1.u01.s01]", "skills: []");
        let errors = parse_scenario("cafe.yaml", &text).unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].0.starts_with("cafe.yaml: /languages/es/skills: "),
            "{errors:?}"
        );
    }

    #[test]
    fn sc02_compares_the_id_with_the_file_name() {
        let text = CAFE.replace("id: cafe", "id: café");
        assert_eq!(
            check(&text).errors,
            ["scenarios/cafe.yaml: id must match the file name"]
        );
    }

    #[test]
    fn sc03_and_sc04_count_goals_and_turns() {
        let text = CAFE.replace("  - {id: pay_leave, text_en: Pay and say goodbye}\n", "");
        assert_eq!(
            check(&text).errors,
            ["cafe: 2 goals; a scenario has 3 to 6"]
        );
        let text = CAFE.replace("turn_cap: 12", "turn_cap: 21");
        assert_eq!(
            check(&text).errors,
            ["cafe: turn_cap 21 is outside 6 to 20"]
        );
        let text = CAFE.replace("turn_cap: 12", "turn_cap: 5");
        assert_eq!(check(&text).errors, ["cafe: turn_cap 5 is outside 6 to 20"]);
        assert!(check(&CAFE.replace("turn_cap: 12", "turn_cap: 6")).ok());
        assert!(check(&CAFE.replace("turn_cap: 12", "turn_cap: 20")).ok());
    }

    #[test]
    fn sc05_names_each_missing_skill_and_a_missing_syllabus() {
        let text = CAFE.replace(
            "skills: [es.a1.u01.s01]",
            "skills: [es.a1.u07.s01, es.a1.u01.s01, es.a1.u08.s02]",
        );
        assert_eq!(
            check(&text).errors,
            [
                "cafe: es: skill es.a1.u07.s01 does not exist",
                "cafe: es: skill es.a1.u08.s02 does not exist",
            ]
        );
        let text = format!(
            "{CAFE}  fr:\n    setting_en: s\n    opening: \"Bonjour !\"\n    opening_en: \"Hello!\"\n    skills: [fr.a1.u01.s01]\n"
        );
        assert_eq!(
            check(&text).errors,
            ["cafe: fr: skill fr.a1.u01.s01 does not exist"]
        );
    }

    #[test]
    fn sc06_names_each_untaught_word_of_the_opening() {
        let text = CAFE.replace(
            "opening: \"¡Buenos días, Ana! ¿Qué tal?\"",
            "opening: \"¡Buenos días! ¿Qué le pongo, Ana? Dos cafés.\"",
        );
        assert_eq!(
            check(&text).errors,
            [
                "cafe: es: opening uses \"pongo\", which the listed skills do not teach",
                "cafe: es: opening uses \"Dos\", which the listed skills do not teach",
                "cafe: es: opening uses \"cafés\", which the listed skills do not teach",
            ]
        );
    }

    #[test]
    fn sc06_does_not_run_while_a_skill_is_missing() {
        let text = CAFE.replace("¿Qué tal?", "¿Qué le pongo?").replace(
            "skills: [es.a1.u01.s01]",
            "skills: [es.a1.u01.s01, es.a1.u09.s01]",
        );
        assert_eq!(
            check(&text).errors,
            ["cafe: es: skill es.a1.u09.s01 does not exist"]
        );
    }

    #[test]
    fn a_scenario_file_is_checked_against_its_own_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shop.yaml");
        std::fs::write(&path, CAFE).unwrap();
        let report = validate_scenario_file(&path, &BTreeMap::new());
        let file = display_path(&path);
        assert_eq!(
            report.errors,
            [
                format!("{file}: id must match the file name"),
                "cafe: es: skill es.a1.u01.s01 does not exist".to_string(),
            ]
        );
        std::fs::write(&path, "id: [\n").unwrap();
        let report = validate_scenario_file(&path, &BTreeMap::new());
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].starts_with(&format!("{file}: ")));
    }
}
