//! Item derivation: vocabulary items from lemmas, phrase or pattern items from examples, with
//! deterministic ids and accepted answers (docs/modules/01-content-model-and-syllabus.md, Design
//! notes), and the taught-forms rule the validator and the items share.

use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

use super::forms::Forms;
use super::syllabus::{Cefr, Gender, Lemma, Pos, Skill, SkillKind, Syllabus};
use super::tokenise::{Token, tokenise};
use super::{Lang, nfc, nfc_lower};

/// What an item is: a word, a sentence from a vocabulary or function skill, or a sentence that
/// shows a grammar pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Vocab,
    Phrase,
    Pattern,
}

/// The answers accepted in each direction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Accepted {
    pub to_target: Vec<String>,
    pub to_en: Vec<String>,
}

/// A learnable item. Serialises to the object of `schemas/item.schema.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// `<skill>#l:<lemma>` or `<skill>#e:<example id>`.
    pub id: String,
    pub lang: Lang,
    pub skill: String,
    pub kind: ItemKind,
    pub target: String,
    pub en: Vec<String>,
    pub accepted: Accepted,
    /// The syllabus lemmas the target uses, in token order without repeats.
    pub lemmas: Vec<String>,
    pub cefr: Cefr,
}

/// The id of a lemma's vocabulary item.
pub fn lemma_item_id(skill: &str, lemma: &str) -> String {
    format!("{skill}#l:{lemma}")
}

/// The id of an example's item.
pub fn example_item_id(skill: &str, example_id: i64) -> String {
    format!("{skill}#e:{example_id}")
}

/// The forms a lemma teaches beyond the forms table: a word matches a lemma when `lemmas_of`
/// maps its surface or its normalised form to that lemma, when it equals the lemma's own citation
/// form, or when it is one of the words of a multi-word lemma (`por favor` teaches `por` and
/// `favor`). A German lemma marked `separable: auf` also allows its base verb (`stehen`) and the
/// particle `auf` on its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaughtForms {
    /// Normalised words that match on their own.
    words: Vec<String>,
    /// Lemma keys (as the forms table spells them) that match.
    keys: Vec<String>,
}

impl TaughtForms {
    pub fn of(lemma: &Lemma) -> TaughtForms {
        let citation = nfc(&lemma.lemma);
        let mut keys = vec![citation.clone()];
        let mut words = vec![nfc_lower(&lemma.lemma)];
        for word in lemma.lemma.unicode_words() {
            words.push(nfc_lower(word));
        }
        if let Some(prefix) = &lemma.separable {
            let prefix = nfc(prefix);
            if let Some(base) = citation.strip_prefix(prefix.as_str())
                && !base.is_empty()
            {
                keys.push(base.to_string());
                words.push(base.to_lowercase());
            }
            words.push(prefix.to_lowercase());
        }
        words.sort_unstable();
        words.dedup();
        keys.sort_unstable();
        keys.dedup();
        TaughtForms { words, keys }
    }

    /// Whether `token` is a form of the lemma.
    pub fn matches(&self, token: &Token) -> bool {
        self.words.contains(&token.norm)
            || self.keys.iter().any(|key| token.lemma_keys.contains(key))
    }
}

/// Whether a French noun takes `l'`: it starts with a vowel or `h`.
fn french_elides(lemma: &str) -> bool {
    nfc_lower(lemma)
        .chars()
        .next()
        .is_some_and(|c| "aeiouàâäéèêëîïôöùûüœæh".contains(c))
}

/// The definite article of a noun, when its gender gives one for the language.
fn article(lang: Lang, lemma: &Lemma) -> Option<String> {
    if let Some(article) = &lemma.article {
        return Some(article.clone());
    }
    let gender = lemma.gender?;
    let article = match (lang, gender) {
        (Lang::Es, Gender::M) => "el",
        (Lang::Es, Gender::F) => "la",
        (Lang::Fr, Gender::M | Gender::F) if french_elides(&lemma.lemma) => "l'",
        (Lang::Fr, Gender::M) => "le",
        (Lang::Fr, Gender::F) => "la",
        (Lang::De, Gender::M) => "der",
        (Lang::De, Gender::F) => "die",
        (Lang::De, Gender::N) => "das",
        (Lang::Es | Lang::Fr, Gender::N) => return None,
    };
    Some(article.to_string())
}

/// The target of a lemma's vocabulary item: a noun with its definite article, anything else the
/// lemma itself.
pub fn vocab_target(lang: Lang, lemma: &Lemma) -> String {
    if lemma.pos != Pos::Noun {
        return lemma.lemma.clone();
    }
    match article(lang, lemma) {
        Some(article) if article.ends_with('\'') || article.ends_with('’') => {
            format!("{article}{}", lemma.lemma)
        }
        Some(article) => format!("{article} {}", lemma.lemma),
        None => lemma.lemma.clone(),
    }
}

/// The syllabus lemmas `tokens` use, in token order without repeats; `taught` pairs every
/// syllabus lemma, in syllabus order, with its forms.
fn lemmas_used(tokens: &[Token], taught: &[(&str, TaughtForms)]) -> Vec<String> {
    let mut used: Vec<String> = Vec::new();
    for token in tokens {
        for (lemma, forms) in taught {
            if forms.matches(token) && !used.iter().any(|u| u == lemma) {
                used.push(lemma.to_string());
            }
        }
    }
    used
}

fn example_kind(skill: &Skill) -> ItemKind {
    match skill.kind {
        SkillKind::Grammar => ItemKind::Pattern,
        SkillKind::Vocab | SkillKind::Function => ItemKind::Phrase,
    }
}

/// Every item of the syllabus in syllabus order: for each unit, each skill, its lemma items in
/// lemma order, then its example items in example order.
pub fn derive_items(s: &Syllabus, forms: &Forms) -> Vec<Item> {
    let taught: Vec<(&str, TaughtForms)> = s
        .skills()
        .flat_map(|(_, skill)| skill.lemmas.iter())
        .map(|lemma| (lemma.lemma.as_str(), TaughtForms::of(lemma)))
        .collect();
    let mut items = Vec::new();
    for (_, skill) in s.skills() {
        for lemma in &skill.lemmas {
            let target = vocab_target(s.lang, lemma);
            items.push(Item {
                id: lemma_item_id(&skill.id, &lemma.lemma),
                lang: s.lang,
                skill: skill.id.clone(),
                kind: ItemKind::Vocab,
                target: target.clone(),
                en: lemma.en.clone(),
                accepted: Accepted {
                    to_target: vec![target],
                    to_en: lemma.en.clone(),
                },
                lemmas: vec![lemma.lemma.clone()],
                cefr: skill.cefr,
            });
        }
        for example in &skill.examples {
            let tokens = tokenise(s.lang, &example.target, forms);
            items.push(Item {
                id: example_item_id(&skill.id, example.id),
                lang: s.lang,
                skill: skill.id.clone(),
                kind: example_kind(skill),
                target: example.target.clone(),
                en: example.en.clone(),
                accepted: Accepted {
                    to_target: vec![example.target.clone()],
                    to_en: example.en.clone(),
                },
                lemmas: lemmas_used(&tokens, &taught),
                cefr: skill.cefr,
            });
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::forms::FormRow;
    use crate::content::parse_syllabus;
    use crate::content::test_support::{EXAMPLE_SYLLABUS, example_forms};

    fn lemma(text: &str, pos: Pos, gender: Option<Gender>) -> Lemma {
        Lemma {
            lemma: text.into(),
            pos,
            gender,
            en: vec!["x".into()],
            rank: None,
            separable: None,
            article: None,
        }
    }

    #[test]
    fn nouns_carry_their_article() {
        assert_eq!(
            vocab_target(Lang::Es, &lemma("día", Pos::Noun, Some(Gender::M))),
            "el día"
        );
        assert_eq!(
            vocab_target(Lang::Es, &lemma("noche", Pos::Noun, Some(Gender::F))),
            "la noche"
        );
        assert_eq!(
            vocab_target(Lang::Fr, &lemma("homme", Pos::Noun, Some(Gender::M))),
            "l'homme"
        );
        assert_eq!(
            vocab_target(Lang::Fr, &lemma("école", Pos::Noun, Some(Gender::F))),
            "l'école"
        );
        assert_eq!(
            vocab_target(Lang::Fr, &lemma("pain", Pos::Noun, Some(Gender::M))),
            "le pain"
        );
        assert_eq!(
            vocab_target(Lang::De, &lemma("Haus", Pos::Noun, Some(Gender::N))),
            "das Haus"
        );
        assert_eq!(
            vocab_target(Lang::Es, &lemma("hola", Pos::Intj, None)),
            "hola"
        );
        let mut agua = lemma("agua", Pos::Noun, Some(Gender::F));
        agua.article = Some("el".into());
        assert_eq!(vocab_target(Lang::Es, &agua), "el agua");
    }

    #[test]
    fn separable_verbs_teach_their_base_and_particle() {
        let mut aufstehen = lemma("aufstehen", Pos::Verb, None);
        aufstehen.separable = Some("auf".into());
        let taught = TaughtForms::of(&aufstehen);
        let forms = Forms::from_rows(vec![FormRow {
            form: "stehe".into(),
            lemma: "stehen".into(),
            pos: "verb".into(),
            gender: None,
            tags: String::new(),
        }]);
        let tokens = tokenise(Lang::De, "Ich stehe um sieben auf.", &forms);
        assert!(taught.matches(&tokens[1]), "stehe");
        assert!(taught.matches(&tokens[4]), "auf");
        assert!(!taught.matches(&tokens[0]), "Ich");
        let por_favor = lemma("por favor", Pos::Expr, None);
        let taught = TaughtForms::of(&por_favor);
        let tokens = tokenise(Lang::Es, "Un café, por favor.", &Forms::default());
        assert!(taught.matches(&tokens[2]));
        assert!(taught.matches(&tokens[3]));
        assert!(!taught.matches(&tokens[1]));
    }

    #[test]
    fn the_example_syllabus_derives_the_expected_items() {
        let syllabus = parse_syllabus("valid-es.yaml", EXAMPLE_SYLLABUS).unwrap();
        let items = derive_items(&syllabus, &example_forms());
        assert_eq!(items.len(), 11);
        let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "es.a1.u01.s01#l:hola",
                "es.a1.u01.s01#l:adiós",
                "es.a1.u01.s01#l:bueno",
                "es.a1.u01.s01#l:día",
                "es.a1.u01.s01#l:noche",
                "es.a1.u01.s01#l:tal",
                "es.a1.u01.s01#l:gracias",
                "es.a1.u01.s01#e:1",
                "es.a1.u01.s01#e:2",
                "es.a1.u01.s01#e:3",
                "es.a1.u01.s01#e:4",
            ]
        );
        let dia = &items[3];
        assert_eq!(
            serde_json::to_value(dia).unwrap(),
            serde_json::json!({
                "id": "es.a1.u01.s01#l:día", "lang": "es", "skill": "es.a1.u01.s01",
                "kind": "vocab", "target": "el día", "en": ["day"],
                "accepted": {"to_target": ["el día"], "to_en": ["day"]},
                "lemmas": ["día"], "cefr": "A1"
            })
        );
        assert_eq!(items[4].target, "la noche");
        assert_eq!(items[7].kind, ItemKind::Phrase);
        assert_eq!(items[7].lemmas, ["hola", "tal"]);
        assert_eq!(items[8].lemmas, ["bueno", "día"]);
        assert_eq!(items[9].lemmas, ["bueno", "noche"]);
        assert_eq!(items[10].lemmas, ["adiós", "gracias"]);
        assert_eq!(
            items[10].accepted.to_en,
            ["Goodbye, thank you.", "Bye, thanks."]
        );
    }

    #[test]
    fn every_derived_item_matches_the_item_schema() {
        let syllabus = parse_syllabus("valid-es.yaml", EXAMPLE_SYLLABUS).unwrap();
        for item in derive_items(&syllabus, &example_forms()) {
            let value = serde_json::to_value(&item).unwrap();
            let errors = crate::content::schema_errors(crate::content::item_validator(), &value);
            assert!(errors.is_empty(), "{}: {errors:?}", item.id);
        }
    }

    #[test]
    fn a_grammar_skill_gives_pattern_items() {
        let text = EXAMPLE_SYLLABUS.replace("kind: function", "kind: grammar");
        let syllabus = parse_syllabus("s.yaml", &text).unwrap();
        let items = derive_items(&syllabus, &example_forms());
        assert_eq!(items[7].kind, ItemKind::Pattern);
    }
}
