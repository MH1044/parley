//! The syllabus validator: rules V02 to V21 of docs/modules/01-content-model-and-syllabus.md
//! (V01, YAML and schema, is [`super::parse_syllabus`]). Errors are ordered by rule number, then
//! in document order; V17 is a warning.

use std::collections::{HashMap, HashSet};

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use unicode_normalization::is_nfc;

use super::forms::Forms;
use super::items::{TaughtForms, derive_items, example_item_id};
use super::lock::IdsLock;
use super::syllabus::{Cefr, Gender, Pos, Skill, Syllabus};
use super::tokenise::{Token, TokenKind, tokenise};
use super::{Lang, nfc, nfc_lower};

/// What the validator knows besides the syllabus.
pub struct Context<'a> {
    /// The syllabus path as typed, for V02 messages.
    pub file: &'a str,
    /// The name of the syllabus's folder when it is `es`, `fr` or `de` (V02).
    pub folder: Option<&'a str>,
    pub forms: &'a Forms,
    /// Number words (`numbers.yaml`).
    pub numbers: &'a [String],
    /// Lemmas never taught (`exclude.txt`).
    pub exclude: &'a [String],
    /// `ids.lock` when it exists beside the syllabus (V20, V21).
    pub lock: Option<&'a IdsLock>,
}

/// The validator's result. Serialises as `{"ok": bool, "errors": [...], "warnings": [...]}`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl Report {
    /// No errors (warnings are allowed).
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// Appends another file's report.
    pub fn extend(&mut self, other: Report) {
        self.errors.extend(other.errors);
        self.warnings.extend(other.warnings);
    }
}

impl Serialize for Report {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Report", 3)?;
        state.serialize_field("ok", &self.ok())?;
        state.serialize_field("errors", &self.errors)?;
        state.serialize_field("warnings", &self.warnings)?;
        state.end()
    }
}

/// `<lang>.<a1|a2>.uNN`
fn unit_id_ok(lang: Lang, id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    parts.len() == 3 && parts[0] == lang.as_str() && level_ok(parts[1]) && numbered(parts[2], 'u')
}

/// `<unit id>.sNN`
fn skill_id_ok(lang: Lang, unit_id: &str, id: &str) -> bool {
    unit_id_ok(lang, unit_id)
        && id
            .strip_prefix(unit_id)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|rest| numbered(rest, 's'))
}

fn level_ok(part: &str) -> bool {
    part == "a1" || part == "a2"
}

fn numbered(part: &str, letter: char) -> bool {
    part.strip_prefix(letter)
        .is_some_and(|digits| digits.len() == 2 && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// The words of the syllabus every example may use: function words, names and numbers.
struct Known {
    function: HashSet<String>,
    names: HashSet<String>,
    names_lower: HashSet<String>,
    numbers: HashSet<String>,
}

impl Known {
    fn is_function(&self, token: &Token) -> bool {
        self.function.contains(&token.norm)
            || self.function.contains(&nfc(&token.surface))
            || token
                .lemma_keys
                .iter()
                .any(|key| self.function.contains(&nfc_lower(key)))
    }

    fn is_name(&self, token: &Token) -> bool {
        self.names.contains(&nfc(&token.surface)) || self.names_lower.contains(&token.norm)
    }

    fn is_number(&self, token: &Token) -> bool {
        token.kind == TokenKind::Number || self.numbers.contains(&token.norm)
    }
}

fn gender_rule(lang: Lang) -> &'static str {
    match lang {
        Lang::Es | Lang::Fr => "(m or f)",
        Lang::De => "(m, f or n)",
    }
}

fn gender_ok(lang: Lang, gender: Option<Gender>) -> bool {
    matches!(
        (lang, gender),
        (Lang::Es | Lang::Fr, Some(Gender::M | Gender::F)) | (Lang::De, Some(_))
    )
}

/// Runs V02 to V21 over `s`.
pub fn validate(s: &Syllabus, ctx: &Context) -> Report {
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let lang = s.lang;
    let skills: Vec<&Skill> = s.skills().map(|(_, skill)| skill).collect();

    // V02
    if let Some(folder) = ctx.folder
        && folder != lang.as_str()
    {
        errors.push(format!(
            "{}: lang is \"{}\" but the folder is \"{folder}\"",
            ctx.file,
            lang.as_str()
        ));
    }

    // V03
    for unit in &s.units {
        if !unit_id_ok(lang, &unit.id) {
            errors.push(format!("{}: ids look like es.a1.u01.s01", unit.id));
        }
        for skill in &unit.skills {
            if !skill_id_ok(lang, &unit.id, &skill.id) {
                errors.push(format!("{}: ids look like es.a1.u01.s01", skill.id));
            }
        }
    }

    // V04
    let mut seen_ids: HashSet<&str> = HashSet::new();
    for unit in &s.units {
        if !seen_ids.insert(&unit.id) {
            errors.push(format!("duplicate id {}", unit.id));
        }
        for skill in &unit.skills {
            if !seen_ids.insert(&skill.id) {
                errors.push(format!("duplicate id {}", skill.id));
            }
        }
    }

    // V05
    let mut seen_a2 = false;
    for unit in &s.units {
        match unit.cefr {
            Cefr::A2 => seen_a2 = true,
            Cefr::A1 if seen_a2 => errors.push(format!("unit {} is A1 after an A2 unit", unit.id)),
            Cefr::A1 => {}
        }
    }

    // V06
    let mut earlier: HashSet<&str> = HashSet::new();
    for skill in &skills {
        for other in &skill.requires {
            if !earlier.contains(other.as_str()) {
                errors.push(format!(
                    "{}: requires {other}, which is not an earlier skill",
                    skill.id
                ));
            }
        }
        earlier.insert(&skill.id);
    }

    // V07
    for skill in &skills {
        if !(skill.can_do.starts_with("I can ") && skill.can_do.ends_with('.')) {
            errors.push(format!(
                "{}: can_do must start with \"I can\" and end with a full stop",
                skill.id
            ));
        }
    }

    // V08
    for skill in &skills {
        if let Some(note) = &skill.grammar_note {
            let n = note.split_whitespace().count();
            if n > 120 {
                errors.push(format!(
                    "{}: grammar_note has {n} words; the limit is 120",
                    skill.id
                ));
            }
        }
    }

    // V09
    for skill in &skills {
        let n = skill.lemmas.len();
        if !(3..=20).contains(&n) {
            errors.push(format!("{}: {n} lemmas; a skill has 3 to 20", skill.id));
        }
    }

    // V10
    for skill in &skills {
        let n = skill.examples.len();
        if !(4..=15).contains(&n) {
            errors.push(format!("{}: {n} examples; a skill has 4 to 15", skill.id));
        }
    }

    // V11
    for skill in &skills {
        for lemma in &skill.lemmas {
            if lemma.pos == Pos::Noun && !gender_ok(lang, lemma.gender) {
                errors.push(format!(
                    "{}: noun {} needs a gender {}",
                    skill.id,
                    lemma.lemma,
                    gender_rule(lang)
                ));
            }
        }
    }

    // Shared by V12 to V14: the known words, each skill's taught forms and each example's tokens.
    let known = Known {
        function: s.function_words.iter().map(|w| nfc_lower(w)).collect(),
        names: s.names.iter().map(|n| nfc(n)).collect(),
        names_lower: s.names.iter().map(|n| nfc_lower(n)).collect(),
        numbers: ctx.numbers.iter().map(|w| nfc_lower(w)).collect(),
    };
    let taught: Vec<Vec<TaughtForms>> = skills
        .iter()
        .map(|skill| skill.lemmas.iter().map(TaughtForms::of).collect())
        .collect();
    let tokens: Vec<Vec<Vec<Token>>> = skills
        .iter()
        .map(|skill| {
            skill
                .examples
                .iter()
                .map(|example| tokenise(lang, &example.target, ctx.forms))
                .collect()
        })
        .collect();

    // V12
    for (i, skill) in skills.iter().enumerate() {
        for (j, example) in skill.examples.iter().enumerate() {
            for token in &tokens[i][j] {
                let allowed = known.is_number(token)
                    || known.is_name(token)
                    || known.is_function(token)
                    || taught[..=i]
                        .iter()
                        .flatten()
                        .any(|forms| forms.matches(token));
                if !allowed {
                    errors.push(format!(
                        "{}: example {} uses \"{}\", which no earlier skill teaches",
                        skill.id, example.id, token.surface
                    ));
                }
            }
        }
    }

    // V13
    for (i, skill) in skills.iter().enumerate() {
        for (j, example) in skill.examples.iter().enumerate() {
            let uses_own = tokens[i][j]
                .iter()
                .any(|token| taught[i].iter().any(|forms| forms.matches(token)));
            if !uses_own {
                errors.push(format!(
                    "{}: example {} uses none of this skill's lemmas",
                    skill.id, example.id
                ));
            }
        }
    }

    // V14
    let mut seen_targets: HashMap<String, (&str, i64)> = HashMap::new();
    for (i, skill) in skills.iter().enumerate() {
        for (j, example) in skill.examples.iter().enumerate() {
            let norm: Vec<&str> = tokens[i][j].iter().map(|t| t.norm.as_str()).collect();
            let norm = norm.join(" ");
            match seen_targets.get(&norm) {
                Some((other, m)) => errors.push(format!(
                    "{}: example {} repeats {other} example {m}",
                    skill.id, example.id
                )),
                None => {
                    seen_targets.insert(norm, (&skill.id, example.id));
                }
            }
        }
    }

    // V15
    for skill in &skills {
        for example in &skill.examples {
            if example.en.is_empty() {
                errors.push(format!(
                    "{}: example {} needs at least one English translation",
                    skill.id, example.id
                ));
            }
        }
    }

    // V16
    let exclude: HashSet<String> = ctx.exclude.iter().map(|w| nfc(w)).collect();
    for skill in &skills {
        for lemma in &skill.lemmas {
            if exclude.contains(&nfc(&lemma.lemma)) {
                errors.push(format!(
                    "{}: {} is on the exclude list",
                    skill.id, lemma.lemma
                ));
            }
        }
    }

    // V17 (warning)
    let mut first_skill: HashMap<String, &str> = HashMap::new();
    for skill in &skills {
        for lemma in &skill.lemmas {
            let key = nfc(&lemma.lemma);
            match first_skill.get(&key) {
                Some(first) if *first != skill.id => warnings.push(format!(
                    "warning: {} is taught in {first} and {}",
                    lemma.lemma, skill.id
                )),
                Some(_) => {}
                None => {
                    first_skill.insert(key, &skill.id);
                }
            }
        }
    }

    // V18
    for skill in &skills {
        for example in &skill.examples {
            if !is_nfc(&example.target) || example.en.iter().any(|en| !is_nfc(en)) {
                errors.push(format!(
                    "{}: example {} is not in Unicode NFC; save the file as NFC",
                    skill.id, example.id
                ));
            }
        }
    }

    // V19
    for skill in &skills {
        let mut seen: HashSet<i64> = HashSet::new();
        for example in &skill.examples {
            if example.id <= 0 {
                errors.push(format!(
                    "{}: example id {} must be a positive integer",
                    skill.id, example.id
                ));
            }
            if !seen.insert(example.id) {
                errors.push(format!(
                    "{}: example id {} is used twice",
                    skill.id, example.id
                ));
            }
        }
    }

    // V20 and V21
    if let Some(lock) = ctx.lock {
        let mut retired: HashSet<String> = HashSet::new();
        for skill in &skills {
            for example in &skill.examples {
                let item_id = example_item_id(&skill.id, example.id);
                if lock.is_retired(&item_id) {
                    errors.push(format!("{item_id} was retired; give the example a new id"));
                    retired.insert(item_id);
                }
            }
        }
        let ids: Vec<String> = derive_items(s, ctx.forms)
            .into_iter()
            .map(|item| item.id)
            .collect();
        for id in &ids {
            if !lock.is_active(id) && !retired.contains(id) {
                errors.push(format!(
                    "{id} is not in ids.lock; run parley content lock --lang {}",
                    lang.as_str()
                ));
            }
        }
        let in_syllabus: HashSet<&str> = ids.iter().map(String::as_str).collect();
        for id in lock.active_ids() {
            if !in_syllabus.contains(id) {
                errors.push(format!(
                    "{id} is in ids.lock but not in the syllabus; run parley content lock --lang {} to retire it",
                    lang.as_str()
                ));
            }
        }
    }

    Report { errors, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::lock::lock_items;
    use crate::content::parse_syllabus;
    use crate::content::test_support::{EXAMPLE_SYLLABUS, example_forms};

    fn check(text: &str, forms: &Forms) -> Report {
        let syllabus = parse_syllabus("s.yaml", text).unwrap();
        let ctx = Context {
            file: "s.yaml",
            folder: None,
            forms,
            numbers: &[],
            exclude: &[],
            lock: None,
        };
        validate(&syllabus, &ctx)
    }

    #[test]
    fn the_example_syllabus_is_valid() {
        let report = check(EXAMPLE_SYLLABUS, &example_forms());
        assert_eq!(report, Report::default(), "{report:?}");
        assert!(report.ok());
        assert_eq!(
            serde_json::to_string(&report).unwrap(),
            r#"{"ok":true,"errors":[],"warnings":[]}"#
        );
    }

    #[test]
    fn without_a_forms_table_v12_names_the_surface_forms() {
        let report = check(EXAMPLE_SYLLABUS, &Forms::default());
        assert_eq!(
            report.errors,
            [
                "es.a1.u01.s01: example 2 uses \"Buenos\", which no earlier skill teaches",
                "es.a1.u01.s01: example 2 uses \"días\", which no earlier skill teaches",
                "es.a1.u01.s01: example 3 uses \"Buenas\", which no earlier skill teaches",
                "es.a1.u01.s01: example 3 uses \"noches\", which no earlier skill teaches",
                "es.a1.u01.s01: example 2 uses none of this skill's lemmas",
                "es.a1.u01.s01: example 3 uses none of this skill's lemmas",
            ]
        );
    }

    #[test]
    fn v02_compares_lang_with_the_folder() {
        let syllabus = parse_syllabus("s.yaml", EXAMPLE_SYLLABUS).unwrap();
        let forms = example_forms();
        let ctx = Context {
            file: "bad-V02/fr/syllabus.yaml",
            folder: Some("fr"),
            forms: &forms,
            numbers: &[],
            exclude: &[],
            lock: None,
        };
        let report = validate(&syllabus, &ctx);
        assert_eq!(
            report.errors,
            ["bad-V02/fr/syllabus.yaml: lang is \"es\" but the folder is \"fr\""]
        );
    }

    #[test]
    fn ids_shapes_and_uniqueness() {
        let text = EXAMPLE_SYLLABUS.replace("id: es.a1.u01.s01", "id: es.a1.u01.s1");
        let report = check(&text, &example_forms());
        assert_eq!(report.errors, ["es.a1.u01.s1: ids look like es.a1.u01.s01"]);
        let text = EXAMPLE_SYLLABUS.replace("  - id: es.a1.u01\n", "  - id: fr.a1.u01\n");
        let report = check(&text, &example_forms());
        assert_eq!(
            report.errors,
            [
                "fr.a1.u01: ids look like es.a1.u01.s01",
                "es.a1.u01.s01: ids look like es.a1.u01.s01"
            ]
        );
    }

    #[test]
    fn can_do_wording_grammar_note_length_and_counts() {
        let text = EXAMPLE_SYLLABUS.replace("can_do: I can say hello", "can_do: I say hello");
        let report = check(&text, &example_forms());
        assert_eq!(
            report.errors,
            ["es.a1.u01.s01: can_do must start with \"I can\" and end with a full stop"]
        );
        let words = (0..121)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let text = EXAMPLE_SYLLABUS.replace(
            "        grammar_note: >-\n          Questions and exclamations open with an upside-down mark: ¿Qué tal? ¡Hola!\n          Say buenos días until lunch, buenas tardes until dark, then buenas noches.\n",
            &format!("        grammar_note: {words}\n"),
        );
        let report = check(&text, &example_forms());
        assert_eq!(
            report.errors,
            ["es.a1.u01.s01: grammar_note has 121 words; the limit is 120"]
        );
        let text = EXAMPLE_SYLLABUS.replace(
            "          - {id: 4, target: \"Adiós, gracias.\", en: [\"Goodbye, thank you.\", \"Bye, thanks.\"]}\n",
            "",
        );
        let report = check(&text, &example_forms());
        assert_eq!(
            report.errors,
            ["es.a1.u01.s01: 3 examples; a skill has 4 to 15"]
        );
    }

    #[test]
    fn nouns_need_a_gender_of_the_language() {
        let text =
            EXAMPLE_SYLLABUS.replace("pos: noun, gender: m, en: [day]", "pos: noun, en: [day]");
        let report = check(&text, &example_forms());
        assert_eq!(
            report.errors,
            ["es.a1.u01.s01: noun día needs a gender (m or f)"]
        );
        assert_eq!(gender_rule(Lang::De), "(m, f or n)");
        assert!(!gender_ok(Lang::Es, Some(Gender::N)));
        assert!(gender_ok(Lang::De, Some(Gender::N)));
    }

    #[test]
    fn repeated_targets_translations_and_example_ids() {
        let text = EXAMPLE_SYLLABUS.replace(
            "{id: 4, target: \"Adiós, gracias.\", en: [\"Goodbye, thank you.\", \"Bye, thanks.\"]}",
            "{id: 4, target: \"buenos DÍAS\", en: []}",
        );
        let report = check(&text, &example_forms());
        assert_eq!(
            report.errors,
            [
                "es.a1.u01.s01: example 4 repeats es.a1.u01.s01 example 2",
                "es.a1.u01.s01: example 4 needs at least one English translation",
            ]
        );
        let text = EXAMPLE_SYLLABUS.replace("{id: 4, target", "{id: 3, target");
        let report = check(&text, &example_forms());
        assert_eq!(report.errors, ["es.a1.u01.s01: example id 3 is used twice"]);
    }

    #[test]
    fn exclude_list_duplicate_lemmas_and_nfc() {
        let syllabus = parse_syllabus("s.yaml", EXAMPLE_SYLLABUS).unwrap();
        let forms = example_forms();
        let exclude = vec!["tal".to_string()];
        let ctx = Context {
            file: "s.yaml",
            folder: None,
            forms: &forms,
            numbers: &[],
            exclude: &exclude,
            lock: None,
        };
        let report = validate(&syllabus, &ctx);
        assert_eq!(report.errors, ["es.a1.u01.s01: tal is on the exclude list"]);

        let text = EXAMPLE_SYLLABUS.replace(
            "          - {id: 4, target: \"Adiós, gracias.\", en: [\"Goodbye, thank you.\", \"Bye, thanks.\"]}\n",
            "          - {id: 4, target: \"Adiós, gracias.\", en: [\"Goodbye, thank you.\"]}\n      - id: es.a1.u01.s02\n        kind: vocab\n        title: Again\n        cefr: A1\n        can_do: I can greet again.\n        requires: [es.a1.u01.s01]\n        lemmas:\n          - {lemma: hola, pos: intj, en: [hello], rank: null}\n          - {lemma: tal, pos: adv, en: [such], rank: null}\n          - {lemma: gracias, pos: intj, en: [thanks], rank: null}\n        examples:\n          - {id: 1, target: \"Hola, Luis.\", en: [\"Hi, Luis.\"]}\n          - {id: 2, target: \"Gracias, Ana.\", en: [\"Thanks, Ana.\"]}\n          - {id: 3, target: \"Hola, ¿qué tal, Ana?\", en: [\"Hi, how are you, Ana?\"]}\n          - {id: 4, target: \"Gracias, Luis.\", en: [\"Thanks, Luis.\"]}\n",
        );
        let text = text.replace(
            "names: [Ana, Luis, Carmen, Pablo, Madrid, Sevilla]",
            "names: [Ana, Luis]",
        );
        let report = check(&text, &forms);
        assert_eq!(report.errors, Vec::<String>::new());
        assert_eq!(
            report.warnings,
            [
                "warning: hola is taught in es.a1.u01.s01 and es.a1.u01.s02",
                "warning: tal is taught in es.a1.u01.s01 and es.a1.u01.s02",
                "warning: gracias is taught in es.a1.u01.s01 and es.a1.u01.s02",
            ]
        );

        let text = EXAMPLE_SYLLABUS.replace(
            "target: \"Adiós, gracias.\"",
            "target: \"Adio\u{0301}s, gracias.\"",
        );
        let report = check(&text, &forms);
        assert_eq!(
            report.errors,
            ["es.a1.u01.s01: example 4 is not in Unicode NFC; save the file as NFC"]
        );
    }

    #[test]
    fn requires_must_name_an_earlier_skill() {
        let text = EXAMPLE_SYLLABUS.replace("requires: []", "requires: [es.a1.u01.s01]");
        let report = check(&text, &example_forms());
        assert_eq!(
            report.errors,
            ["es.a1.u01.s01: requires es.a1.u01.s01, which is not an earlier skill"]
        );
    }

    #[test]
    fn the_lock_rules_follow_deletions_and_new_ids() {
        let syllabus = parse_syllabus("s.yaml", EXAMPLE_SYLLABUS).unwrap();
        let forms = example_forms();
        let (lock, _) = lock_items(&syllabus, &forms, &IdsLock::default());
        let ctx = |lock: &IdsLock, s: &Syllabus| {
            let c = Context {
                file: "s.yaml",
                folder: None,
                forms: &forms,
                numbers: &[],
                exclude: &[],
                lock: Some(lock),
            };
            validate(s, &c)
        };
        assert!(ctx(&lock, &syllabus).ok());

        // Example 2 goes; a new example 6 keeps the skill at four examples.
        let without_2 = EXAMPLE_SYLLABUS
            .replace(
                "          - {id: 2, target: \"Buenos días.\", en: [\"Good morning.\"]}\n",
                "",
            )
            .replace(
                "en: [\"Goodbye, thank you.\", \"Bye, thanks.\"]}\n",
                "en: [\"Goodbye, thank you.\", \"Bye, thanks.\"]}\n          - {id: 6, target: \"Buenos días, Luis.\", en: [\"Good morning, Luis.\"]}\n",
            );
        let smaller = parse_syllabus("s.yaml", &without_2).unwrap();
        assert_eq!(smaller.units[0].skills[0].examples.len(), 4);
        let report = ctx(&lock, &smaller);
        assert_eq!(
            report.errors,
            [
                "es.a1.u01.s01#e:6 is not in ids.lock; run parley content lock --lang es",
                "es.a1.u01.s01#e:2 is in ids.lock but not in the syllabus; run parley content lock --lang es to retire it",
            ]
        );
        let (lock2, report) = lock_items(&smaller, &forms, &lock);
        assert_eq!(report.added, ["es.a1.u01.s01#e:6"]);
        assert_eq!(report.retired, ["es.a1.u01.s01#e:2"]);
        assert!(ctx(&lock2, &smaller).ok());

        let reused = without_2.replace(
            "          - {id: 3, target: \"Buenas noches, Ana.\"",
            "          - {id: 2, target: \"Buenos días.\", en: [\"Good morning.\"]}\n          - {id: 3, target: \"Buenas noches, Ana.\"",
        );
        let reused = parse_syllabus("s.yaml", &reused).unwrap();
        assert_eq!(
            ctx(&lock2, &reused).errors,
            ["es.a1.u01.s01#e:2 was retired; give the example a new id"]
        );

        let fresh = without_2.replace(
            "          - {id: 3, target: \"Buenas noches, Ana.\"",
            "          - {id: 5, target: \"Buenos días.\", en: [\"Good morning.\"]}\n          - {id: 3, target: \"Buenas noches, Ana.\"",
        );
        let fresh = parse_syllabus("s.yaml", &fresh).unwrap();
        assert_eq!(
            ctx(&lock2, &fresh).errors,
            ["es.a1.u01.s01#e:5 is not in ids.lock; run parley content lock --lang es"]
        );
        let (lock3, report) = lock_items(&fresh, &forms, &lock2);
        assert_eq!(report.added, ["es.a1.u01.s01#e:5"]);
        assert!(ctx(&lock3, &fresh).ok());
    }

    #[test]
    fn a_separable_verb_allows_its_parts() {
        let text = "schema: parley.syllabus/1\nlang: de\nname: German\nvariety: Standard\nversion: 1\nfunction_words: [ich, um]\nnames: []\nunits:\n  - id: de.a1.u01\n    title: Morning\n    cefr: A1\n    skills:\n      - id: de.a1.u01.s01\n        kind: vocab\n        title: Getting up\n        cefr: A1\n        can_do: I can say when I get up.\n        requires: []\n        lemmas:\n          - {lemma: aufstehen, pos: verb, en: [to get up], rank: null, separable: auf}\n          - {lemma: sieben, pos: num, en: [seven], rank: null}\n          - {lemma: Uhr, pos: noun, gender: f, en: [clock], rank: null}\n        examples:\n          - {id: 1, target: \"Ich stehe um sieben auf.\", en: [\"I get up at seven.\"]}\n          - {id: 2, target: \"Ich stehe um sieben Uhr auf.\", en: [\"I get up at seven o'clock.\"]}\n          - {id: 3, target: \"Um sieben Uhr.\", en: [\"At seven o'clock.\"]}\n          - {id: 4, target: \"Sieben Uhr.\", en: [\"Seven o'clock.\"]}\n";
        let forms = Forms::from_rows(vec![crate::content::forms::FormRow {
            form: "stehe".into(),
            lemma: "stehen".into(),
            pos: "verb".into(),
            gender: None,
            tags: String::new(),
        }]);
        assert!(check(text, &forms).ok());
        let without = text.replace(
            "          - {lemma: aufstehen, pos: verb, en: [to get up], rank: null, separable: auf}\n",
            "          - {lemma: Tag, pos: noun, gender: m, en: [day], rank: null}\n",
        );
        let report = check(&without, &forms);
        assert_eq!(
            report.errors,
            [
                "de.a1.u01.s01: example 1 uses \"stehe\", which no earlier skill teaches",
                "de.a1.u01.s01: example 1 uses \"auf\", which no earlier skill teaches",
                "de.a1.u01.s01: example 2 uses \"stehe\", which no earlier skill teaches",
                "de.a1.u01.s01: example 2 uses \"auf\", which no earlier skill teaches",
            ]
        );
    }

    #[test]
    fn v19_reports_an_example_id_that_is_not_positive() {
        let text = EXAMPLE_SYLLABUS.replace("{id: 4, target", "{id: 0, target");
        assert_ne!(text, EXAMPLE_SYLLABUS, "the example syllabus has example 4");
        let report = check(&text, &example_forms());
        assert_eq!(
            report.errors,
            vec!["es.a1.u01.s01: example id 0 must be a positive integer".to_string()],
            "{report:?}"
        );
    }
}
