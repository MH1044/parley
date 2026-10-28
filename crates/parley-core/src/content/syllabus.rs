//! The syllabus types, named exactly as the YAML keys of `content/<lang>/syllabus.yaml`
//! (docs/modules/01-content-model-and-syllabus.md, Interfaces).

use serde::{Deserialize, Serialize};

use super::Lang;

/// A CEFR level a unit or skill is placed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Cefr {
    A1,
    A2,
}

impl Cefr {
    pub fn as_str(self) -> &'static str {
        match self {
            Cefr::A1 => "A1",
            Cefr::A2 => "A2",
        }
    }
}

/// What a skill teaches: a grammar point, vocabulary, or a communicative function.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillKind {
    Grammar,
    Vocab,
    Function,
}

impl SkillKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SkillKind::Grammar => "grammar",
            SkillKind::Vocab => "vocab",
            SkillKind::Function => "function",
        }
    }
}

/// A part of speech.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pos {
    Noun,
    Verb,
    Adj,
    Adv,
    Intj,
    Pron,
    Prep,
    Conj,
    Det,
    Num,
    Expr,
}

impl Pos {
    pub fn as_str(self) -> &'static str {
        match self {
            Pos::Noun => "noun",
            Pos::Verb => "verb",
            Pos::Adj => "adj",
            Pos::Adv => "adv",
            Pos::Intj => "intj",
            Pos::Pron => "pron",
            Pos::Prep => "prep",
            Pos::Conj => "conj",
            Pos::Det => "det",
            Pos::Num => "num",
            Pos::Expr => "expr",
        }
    }
}

/// A grammatical gender; `n` (neuter) exists in German only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Gender {
    M,
    F,
    N,
}

impl Gender {
    pub fn as_str(self) -> &'static str {
        match self {
            Gender::M => "m",
            Gender::F => "f",
            Gender::N => "n",
        }
    }

    /// The gender a forms table column names: `m`, `f` or `n`; anything else is none.
    pub fn parse(text: &str) -> Option<Gender> {
        match text {
            "m" => Some(Gender::M),
            "f" => Some(Gender::F),
            "n" => Some(Gender::N),
            _ => None,
        }
    }
}

/// A whole course for one language.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Syllabus {
    pub schema: String,
    pub lang: Lang,
    pub name: String,
    pub variety: String,
    pub version: u32,
    /// Words every example may use without a skill teaching them.
    pub function_words: Vec<String>,
    /// Proper names every example may use.
    pub names: Vec<String>,
    pub units: Vec<Unit>,
}

impl Syllabus {
    /// Every skill in document order, with its unit.
    pub fn skills(&self) -> impl Iterator<Item = (&Unit, &Skill)> {
        self.units
            .iter()
            .flat_map(|unit| unit.skills.iter().map(move |skill| (unit, skill)))
    }
}

/// A group of skills at one CEFR level.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Unit {
    pub id: String,
    pub title: String,
    pub cefr: Cefr,
    pub skills: Vec<Skill>,
}

/// One teachable step: its lemmas and example sentences.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub kind: SkillKind,
    pub title: String,
    pub cefr: Cefr,
    /// "I can ..." and a full stop.
    pub can_do: String,
    /// Ids of earlier skills this one builds on.
    pub requires: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grammar_note: Option<String>,
    pub lemmas: Vec<Lemma>,
    pub examples: Vec<Example>,
}

/// A dictionary word the skill teaches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lemma {
    pub lemma: String,
    pub pos: Pos,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gender: Option<Gender>,
    pub en: Vec<String>,
    /// Frequency rank from the sentence pack; filled in by a later module.
    pub rank: Option<u32>,
    /// A German separable prefix (`auf` for `aufstehen`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub separable: Option<String>,
    /// Overrides the definite article of the vocabulary item (`el` for `agua`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub article: Option<String>,
}

/// A sentence in the target language with its English translations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Example {
    /// The example's own id, unique within its skill and never reused.
    pub id: i64,
    pub target: String,
    pub en: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerations_serialise_as_the_yaml_values() {
        assert_eq!(serde_json::to_string(&Cefr::A1).unwrap(), "\"A1\"");
        assert_eq!(serde_json::to_string(&Gender::N).unwrap(), "\"n\"");
        assert_eq!(serde_json::to_string(&Pos::Intj).unwrap(), "\"intj\"");
        assert_eq!(
            serde_json::to_string(&SkillKind::Function).unwrap(),
            "\"function\""
        );
        assert!(Cefr::A1 < Cefr::A2);
        assert_eq!(Gender::parse("f"), Some(Gender::F));
        assert_eq!(Gender::parse(""), None);
    }
}
