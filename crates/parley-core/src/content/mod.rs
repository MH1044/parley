//! The content model (docs/modules/01-content-model-and-syllabus.md): the syllabus format and
//! its validator, the tokeniser, the forms table, item derivation, the id lock and sentence
//! packs.
//!
//! Content files and packs are inputs and build outputs, not learner data, so this module reads
//! them by path; the `Fs` trait of [`crate::env`] stays the only way to the learner's data
//! directory. Paths in messages use `/` as the separator and the path as typed, so the same input
//! gives the same output on every platform.

use std::fmt;
use std::path::Path;
use std::str::FromStr;
use std::sync::OnceLock;

use jsonschema::error::ValidationErrorKind;
use jsonschema::{JsonType, Validator};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

pub mod forms;
pub mod items;
pub mod lock;
pub mod numbers;
pub mod pack;
pub mod syllabus;
pub mod tokenise;
pub mod validate;
pub mod wiktextract;

pub use forms::{FormRow, Forms};
pub use items::{Accepted, Item, ItemKind, TaughtForms, derive_items};
pub use lock::{IdsLock, LockReport, lock_items};
pub use numbers::{load_exclude, load_numbers};
pub use pack::{
    Dropped, PackEntry, PackError, PackInfo, PackInput, PackReport, PacksLock, build_pack,
    pack_info, parse_packs_lock, verify_pack,
};
pub use syllabus::{Cefr, Example, Gender, Lemma, Pos, Skill, SkillKind, Syllabus, Unit};
pub use tokenise::{Token, TokenKind, tokenise};
pub use validate::{Context, Report, validate};
pub use wiktextract::{Extract, read_wiktextract, write_forms_tsv};

/// A course language. Serialises as its two-letter code (`"es"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Es,
    Fr,
    De,
}

impl Lang {
    /// Every course language, in the order the tools list them.
    pub const ALL: [Lang; 3] = [Lang::Es, Lang::Fr, Lang::De];

    /// The two-letter code: `es`, `fr` or `de`.
    pub fn as_str(self) -> &'static str {
        match self {
            Lang::Es => "es",
            Lang::Fr => "fr",
            Lang::De => "de",
        }
    }

    /// The three-letter code Tatoeba uses: `spa`, `fra` or `deu`.
    pub fn iso3(self) -> &'static str {
        match self {
            Lang::Es => "spa",
            Lang::Fr => "fra",
            Lang::De => "deu",
        }
    }
}

impl FromStr for Lang {
    type Err = ContentError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "es" => Ok(Lang::Es),
            "fr" => Ok(Lang::Fr),
            "de" => Ok(Lang::De),
            other => Err(ContentError(format!(
                "unknown language {other}; use es, fr or de"
            ))),
        }
    }
}

impl fmt::Display for Lang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One content error. The text is the whole message, as the validator prints it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContentError(pub String);

impl fmt::Display for ContentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ContentError {}

/// `s` in Unicode NFC.
pub fn nfc(s: &str) -> String {
    s.nfc().collect()
}

/// `s` in NFC and lower case: the form every comparison of words uses.
pub fn nfc_lower(s: &str) -> String {
    nfc(s).to_lowercase()
}

/// A path as it appears in messages and JSON: as typed, with `/` as the separator and no
/// trailing separator.
pub fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let trimmed = text.trim_end_matches('/');
    if trimmed.is_empty() {
        text
    } else {
        trimmed.to_string()
    }
}

const SYLLABUS_SCHEMA: &str = include_str!("../../../../schemas/syllabus.schema.json");
const ITEM_SCHEMA: &str = include_str!("../../../../schemas/item.schema.json");

fn validator_from(schema: &str) -> Validator {
    let schema: serde_json::Value =
        serde_json::from_str(schema).expect("a schema under schemas/ is valid JSON");
    jsonschema::validator_for(&schema).expect("a schema under schemas/ is a valid JSON Schema")
}

/// The compiled `schemas/syllabus.schema.json`.
pub fn syllabus_validator() -> &'static Validator {
    static VALIDATOR: OnceLock<Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| validator_from(SYLLABUS_SCHEMA))
}

/// The compiled `schemas/item.schema.json`.
pub fn item_validator() -> &'static Validator {
    static VALIDATOR: OnceLock<Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| validator_from(ITEM_SCHEMA))
}

/// The article for a JSON type in a V01 message: `a string`, `an integer`, `null`.
fn type_name(json_type: JsonType) -> &'static str {
    match json_type {
        JsonType::String => "a string",
        JsonType::Integer => "an integer",
        JsonType::Number => "a number",
        JsonType::Boolean => "a boolean",
        JsonType::Array => "an array",
        JsonType::Object => "an object",
        JsonType::Null => "null",
    }
}

/// The order several allowed types are listed in: the order the rule in the spec lists them.
const TYPE_ORDER: [JsonType; 7] = [
    JsonType::String,
    JsonType::Integer,
    JsonType::Number,
    JsonType::Boolean,
    JsonType::Array,
    JsonType::Object,
    JsonType::Null,
];

/// A JSON value as a V01 message shows it: strings bare, everything else as JSON.
fn bare(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The V01 wording for one schema violation; `additionalProperties` names every unknown field.
fn schema_messages(kind: &ValidationErrorKind, fallback: &str) -> Vec<String> {
    match kind {
        ValidationErrorKind::Type { kind } => {
            let names: Vec<&str> = match kind {
                jsonschema::error::TypeKind::Single(t) => vec![type_name(*t)],
                jsonschema::error::TypeKind::Multiple(set) => TYPE_ORDER
                    .iter()
                    .filter(|t| set.contains(**t))
                    .map(|t| type_name(*t))
                    .collect(),
            };
            vec![format!("expected {}", names.join(" or "))]
        }
        ValidationErrorKind::Required { property } => {
            vec![format!("{} is required", bare(property))]
        }
        ValidationErrorKind::AdditionalProperties { unexpected } => unexpected
            .iter()
            .map(|name| format!("unknown field {name}"))
            .collect(),
        ValidationErrorKind::Enum { options } => {
            let list: Vec<String> = options
                .as_array()
                .map(|values| values.iter().map(bare).collect())
                .unwrap_or_default();
            vec![format!("must be one of {}", list.join(", "))]
        }
        ValidationErrorKind::Constant { expected_value } => {
            vec![format!("must be {}", bare(expected_value))]
        }
        _ => vec![fallback.to_string()],
    }
}

/// Every schema violation of `instance` as `{pointer}: {message}`, the pointer a JSON Pointer
/// into the instance (`/` for the root).
pub fn schema_errors(validator: &Validator, instance: &serde_json::Value) -> Vec<String> {
    let mut messages = Vec::new();
    for error in validator.iter_errors(instance) {
        let pointer = error.instance_path().as_str();
        let pointer = if pointer.is_empty() { "/" } else { pointer };
        let fallback = error.to_string();
        for message in schema_messages(error.kind(), &fallback) {
            messages.push(format!("{pointer}: {message}"));
        }
    }
    messages
}

/// Reads and parses a syllabus file. Errors are V01 messages only: the file could not be read,
/// is not YAML, or does not match `schemas/syllabus.schema.json`.
pub fn load_syllabus(path: &Path) -> Result<Syllabus, Vec<ContentError>> {
    let file = display_path(path);
    let text = read_text(path, &file).map_err(|e| vec![e])?;
    parse_syllabus(&file, &text)
}

/// Reads a UTF-8 text file, dropping a byte-order mark; the error names the file as typed.
pub(crate) fn read_text(path: &Path, file: &str) -> Result<String, ContentError> {
    let bytes = std::fs::read(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ContentError(format!("{file} not found"))
        } else {
            ContentError(format!("{file}: {e}"))
        }
    })?;
    let text =
        String::from_utf8(bytes).map_err(|_| ContentError(format!("{file}: not valid UTF-8")))?;
    Ok(text
        .strip_prefix('\u{feff}')
        .map_or(text.clone(), str::to_string))
}

/// Parses syllabus text named `file` (the path as typed, used in every message): YAML first,
/// then the JSON Schema, then the typed structure.
pub fn parse_syllabus(file: &str, text: &str) -> Result<Syllabus, Vec<ContentError>> {
    let value: serde_json::Value =
        serde_yaml_ng::from_str(text).map_err(|e| vec![ContentError(format!("{file}: {e}"))])?;
    let errors = schema_errors(syllabus_validator(), &value);
    if !errors.is_empty() {
        return Err(errors
            .into_iter()
            .map(|message| ContentError(format!("{file}: {message}")))
            .collect());
    }
    serde_json::from_value(value).map_err(|e| vec![ContentError(format!("{file}: /: {e}"))])
}

/// The spec's example syllabus and a forms table for it, shared by the unit tests.
#[cfg(test)]
pub(crate) mod test_support {
    use super::forms::{FormRow, Forms};
    use super::syllabus::Gender;

    /// `tests/fixtures/syllabus/valid-es.yaml` as the spec prints it.
    pub const EXAMPLE_SYLLABUS: &str = "\
schema: parley.syllabus/1
lang: es
name: Spanish for English speakers
variety: Spain Spanish; usted and tú both taught
version: 1
function_words: [el, la, los, las, un, una, y, o, pero, de, a, en, con, sin, por, para, no, sí,
  yo, tú, él, ella, usted, nosotros, ellos, ellas, ustedes, me, te, se, lo, le, que, qué]
names: [Ana, Luis, Carmen, Pablo, Madrid, Sevilla]
units:
  - id: es.a1.u01
    title: Hello
    cefr: A1
    skills:
      - id: es.a1.u01.s01
        kind: function            # grammar | vocab | function
        title: Greetings
        cefr: A1
        can_do: I can say hello and goodbye and ask how someone is.
        requires: []
        grammar_note: >-
          Questions and exclamations open with an upside-down mark: ¿Qué tal? ¡Hola!
          Say buenos días until lunch, buenas tardes until dark, then buenas noches.
        lemmas:
          - {lemma: hola, pos: intj, en: [hello, hi], rank: null}
          - {lemma: adiós, pos: intj, en: [goodbye, bye], rank: null}
          - {lemma: bueno, pos: adj, en: [good], rank: null}
          - {lemma: día, pos: noun, gender: m, en: [day], rank: null}
          - {lemma: noche, pos: noun, gender: f, en: [night], rank: null}
          - {lemma: tal, pos: adv, en: [such], rank: null}
          - {lemma: gracias, pos: intj, en: [thank you, thanks], rank: null}
        examples:
          - {id: 1, target: \"Hola, ¿qué tal?\", en: [\"Hi, how are you?\", \"Hello, how are you?\"]}
          - {id: 2, target: \"Buenos días.\", en: [\"Good morning.\"]}
          - {id: 3, target: \"Buenas noches, Ana.\", en: [\"Good night, Ana.\", \"Good evening, Ana.\"]}
          - {id: 4, target: \"Adiós, gracias.\", en: [\"Goodbye, thank you.\", \"Bye, thanks.\"]}
";

    fn row(form: &str, lemma: &str, pos: &str, gender: Option<Gender>, tags: &str) -> FormRow {
        FormRow {
            form: form.into(),
            lemma: lemma.into(),
            pos: pos.into(),
            gender,
            tags: tags.into(),
        }
    }

    /// The forms the example's sentences need: `buenos`/`buenas` to `bueno`, `días` to `día`,
    /// `noches` to `noche`.
    pub fn example_forms() -> Forms {
        Forms::from_rows(vec![
            row("bueno", "bueno", "adj", None, "canonical"),
            row("buenos", "bueno", "adj", None, "masculine plural"),
            row("buenas", "bueno", "adj", None, "feminine plural"),
            row("día", "día", "noun", Some(Gender::M), "canonical"),
            row("días", "día", "noun", Some(Gender::M), "plural"),
            row("noche", "noche", "noun", Some(Gender::F), "canonical"),
            row("noches", "noche", "noun", Some(Gender::F), "plural"),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_round_trips_through_text_and_json() {
        for lang in Lang::ALL {
            assert_eq!(lang.as_str().parse::<Lang>().unwrap(), lang);
            assert_eq!(
                serde_json::to_string(&lang).unwrap(),
                format!("\"{}\"", lang.as_str())
            );
        }
        assert_eq!(Lang::Es.iso3(), "spa");
        assert_eq!(Lang::Fr.iso3(), "fra");
        assert_eq!(Lang::De.iso3(), "deu");
        assert!("en".parse::<Lang>().is_err());
    }

    #[test]
    fn display_path_uses_forward_slashes_and_no_trailing_separator() {
        assert_eq!(display_path(Path::new("a\\b\\c.yaml")), "a/b/c.yaml");
        assert_eq!(display_path(Path::new("content/")), "content");
        assert_eq!(display_path(Path::new("content\\")), "content");
    }

    #[test]
    fn wrong_type_gives_the_v01_wording() {
        let text = "schema: parley.syllabus/1\nlang: es\nname: n\nvariety: v\nversion: \"one\"\nfunction_words: []\nnames: []\nunits: []\n";
        let errors = parse_syllabus("s.yaml", text).unwrap_err();
        assert_eq!(
            errors,
            vec![ContentError("s.yaml: /version: expected an integer".into())]
        );
    }

    #[test]
    fn several_types_are_joined_by_or() {
        let text = "schema: parley.syllabus/1\nlang: es\nname: n\nvariety: v\nversion: 1\nfunction_words: []\nnames: []\nunits:\n  - id: es.a1.u01\n    title: t\n    cefr: A1\n    skills:\n      - id: es.a1.u01.s01\n        kind: vocab\n        title: t\n        cefr: A1\n        can_do: I can.\n        requires: []\n        lemmas:\n          - {lemma: a, pos: noun, en: [a], rank: \"x\"}\n        examples: []\n";
        let errors = parse_syllabus("s.yaml", text).unwrap_err();
        assert_eq!(
            errors[0].0,
            "s.yaml: /units/0/skills/0/lemmas/0/rank: expected an integer or null"
        );
    }

    #[test]
    fn required_unknown_enum_and_const_have_their_wording() {
        let text = "schema: parley.syllabus/2\nlang: en\nname: n\nversion: 1\nfunction_words: []\nnames: []\nunits: []\nextra: 1\n";
        let errors: Vec<String> = parse_syllabus("s.yaml", text)
            .unwrap_err()
            .into_iter()
            .map(|e| e.0)
            .collect();
        assert!(
            errors.contains(&"s.yaml: /: variety is required".to_string()),
            "{errors:?}"
        );
        assert!(
            errors.contains(&"s.yaml: /: unknown field extra".to_string()),
            "{errors:?}"
        );
        assert!(
            errors.contains(&"s.yaml: /lang: must be one of es, fr, de".to_string()),
            "{errors:?}"
        );
        assert!(
            errors.contains(&"s.yaml: /schema: must be parley.syllabus/1".to_string()),
            "{errors:?}"
        );
    }

    #[test]
    fn a_root_that_is_not_an_object_points_at_the_root() {
        let errors = parse_syllabus("s.yaml", "- a\n").unwrap_err();
        assert_eq!(
            errors,
            vec![ContentError("s.yaml: /: expected an object".into())]
        );
    }

    #[test]
    fn a_yaml_syntax_error_carries_the_parser_message() {
        let errors = parse_syllabus("s.yaml", "units:\n  - id: x\n   title: y\n").unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].0.starts_with("s.yaml: "), "{}", errors[0].0);
        assert!(!errors[0].0.contains('\n'), "{}", errors[0].0);
    }

    #[test]
    fn yaml_keeps_no_and_yes_as_strings() {
        let text = "schema: parley.syllabus/1\nlang: es\nname: n\nvariety: v\nversion: 1\nfunction_words: [no, sí, y, yes, on, off]\nnames: []\nunits: []\n";
        let syllabus = parse_syllabus("s.yaml", text).unwrap();
        assert_eq!(
            syllabus.function_words,
            ["no", "sí", "y", "yes", "on", "off"]
        );
    }

    #[test]
    fn a_file_that_is_not_utf8_is_refused_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.yaml");
        std::fs::write(&path, [0xff, 0xfe, b'a']).unwrap();
        let errors = load_syllabus(&path).expect_err("bytes that are not UTF-8 are refused");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].0.ends_with("bad.yaml: not valid UTF-8"),
            "{}",
            errors[0].0
        );
    }
}
