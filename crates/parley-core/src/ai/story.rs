//! P2, the graded story (docs/PUBLIK-INTEGRATION.md §6.4; docs/modules/07-lesson-generation.md):
//! the prompt's values for the unit a skill ends, the call, and the checks S2 to S6.
//!
//! A story is one unit of validation: any failure discards it, stores and caches nothing and counts
//! one attempt, so Try again is a new request (§6.1). Stories award XP and never change cards.

use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use unicode_segmentation::UnicodeSegmentation;

use super::cache::Cache;
use super::call::{Called, RuleFailure, call};
use super::client::{AiClient, AiError};
use super::langid::{TextLang, is_language_in, stem};
use super::lesson::{allowed_lemmas, allowed_sha256};
use super::prompts::Prompt;
use super::validate::{allowed_from_list, coverage_counts, stems_of};
use crate::api::language_name;
use crate::content::{Cefr, Forms, Lang, Pos, Syllabus, TaughtForms, nfc, nfc_lower, tokenise};
use crate::grading::normalise;

pub use crate::game::story_xp;

const STORY_PROMPT: &str = include_str!("../../../../prompts/story.v1.md");
const STORY_SCHEMA: &str = include_str!("../../../../schemas/ai/story.v1.schema.json");

/// The prompt id stored with an AI story.
pub const STORY_PROMPT_ID: &str = "story.v1";

/// `prompts/story.v1.md` and `schemas/ai/story.v1.schema.json`, compiled in.
pub fn story_prompt_file() -> &'static Prompt {
    static PROMPT: OnceLock<Prompt> = OnceLock::new();
    PROMPT.get_or_init(|| {
        Prompt::parse(STORY_PROMPT, STORY_SCHEMA).expect("prompts/story.v1.md is a valid prompt")
    })
}

/// Whether `value` has the shape of a P2 answer (the story schema), as a course pack's story must.
pub(crate) fn story_schema_valid(value: &Value) -> bool {
    story_prompt_file().schema_valid(value)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoryQuestion {
    pub question_en: String,
    pub options: Vec<String>,
    pub answer_index: u32,
    pub paragraph: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Story {
    pub title: String,
    pub title_en: String,
    pub paragraphs: Vec<String>,
    pub questions: Vec<StoryQuestion>,
}

/// What P2 sends, as data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoryPrompt {
    pub lang: Lang,
    pub cefr: Cefr,
    pub skill_id: String,
    pub seq: u32,
    pub theme: String,
    pub focus: Vec<String>,
    pub variety: String,
    pub allowed_lemmas: Vec<String>,
    pub names: Vec<String>,
}

/// The shape of a story at a level (§6.4): paragraphs, the word range, questions.
struct Shape {
    paragraphs: u32,
    min_words: u32,
    max_words: u32,
    questions: u32,
}

fn shape(cefr: Cefr) -> Shape {
    match cefr {
        Cefr::A1 => Shape {
            paragraphs: 3,
            min_words: 80,
            max_words: 120,
            questions: 3,
        },
        Cefr::A2 => Shape {
            paragraphs: 4,
            min_words: 120,
            max_words: 180,
            questions: 4,
        },
    }
}

fn tenses(lang: Lang, cefr: Cefr) -> &'static str {
    match (cefr, lang) {
        (Cefr::A1, _) => "present",
        (Cefr::A2, Lang::Es) => "present, pretérito perfecto, pretérito indefinido and near future",
        (Cefr::A2, Lang::Fr) => "present, passé composé and near future",
        (Cefr::A2, Lang::De) => "present, Perfekt and near future",
    }
}

/// How many focus words a story asks for.
const FOCUS_WORDS: usize = 4;

/// The unit whose last skill is `skill_id`, when it is one: (unit title, the unit's skills).
fn unit_ended_by<'a>(syllabus: &'a Syllabus, skill_id: &str) -> Option<&'a crate::content::Unit> {
    syllabus
        .units
        .iter()
        .find(|unit| unit.skills.last().is_some_and(|last| last.id == skill_id))
}

/// Whether `skill_id` is the last skill of its unit, so it has a story.
pub(crate) fn has_story(syllabus: &Syllabus, skill_id: &str) -> bool {
    unit_ended_by(syllabus, skill_id).is_some()
}

impl StoryPrompt {
    /// For the unit whose last skill is `skill_id`: the unit's title as the theme; focus words
    /// from the single-word nouns, verbs and adjectives of the unit's skills in syllabus order,
    /// story `seq` taking 4 of them from index 4 × (seq − 1), wrapping round (all of them when the
    /// unit has fewer than 4); the allowed lemmas of the skill. None: not the last skill of a unit.
    pub fn new(
        syllabus: &Syllabus,
        introduced_skills: &[String],
        skill_id: &str,
        seq: u32,
    ) -> Option<StoryPrompt> {
        let unit = unit_ended_by(syllabus, skill_id)?;
        let skill = unit.skills.last()?;
        let words: Vec<String> = unit
            .skills
            .iter()
            .flat_map(|skill| skill.lemmas.iter())
            .filter(|lemma| matches!(lemma.pos, Pos::Noun | Pos::Verb | Pos::Adj))
            .filter(|lemma| lemma.lemma.unicode_words().count() == 1 && !lemma.lemma.contains(' '))
            .map(|lemma| lemma.lemma.clone())
            .collect();
        let focus = if words.len() <= FOCUS_WORDS {
            words
        } else {
            let start = FOCUS_WORDS * (seq.max(1) as usize - 1);
            (0..FOCUS_WORDS)
                .map(|i| words[(start + i) % words.len()].clone())
                .collect()
        };
        Some(StoryPrompt {
            lang: syllabus.lang,
            cefr: skill.cefr,
            skill_id: skill_id.to_string(),
            seq,
            theme: unit.title.clone(),
            focus,
            variety: syllabus.variety.clone(),
            allowed_lemmas: allowed_lemmas(syllabus, introduced_skills, skill_id),
            names: syllabus.names.clone(),
        })
    }

    /// The placeholder values of PUBLIK-INTEGRATION §6.4.
    pub fn vars(&self) -> BTreeMap<String, String> {
        let shape = shape(self.cefr);
        let pairs = [
            ("target_language_name", language_name(self.lang).to_string()),
            ("cefr", self.cefr.as_str().to_string()),
            ("paragraphs", shape.paragraphs.to_string()),
            ("min_words", shape.min_words.to_string()),
            ("max_words", shape.max_words.to_string()),
            ("questions", shape.questions.to_string()),
            ("tenses", tenses(self.lang, self.cefr).to_string()),
            ("variety", self.variety.clone()),
            ("theme", self.theme.clone()),
            ("focus_lemmas", self.focus.join(", ")),
            ("allowed_lemmas", self.allowed_lemmas.join(", ")),
            ("allowed_names", self.names.join(", ")),
        ];
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    /// {"lang", "skill_id", "seq", "focus", "allowed_sha256"}.
    pub fn key_input(&self) -> Value {
        json!({
            "lang": self.lang.as_str(),
            "skill_id": self.skill_id,
            "seq": self.seq,
            "focus": self.focus,
            "allowed_sha256": allowed_sha256(&self.allowed_lemmas),
        })
    }
}

/// What a story is checked in.
pub struct StoryCtx<'a> {
    pub prompt: StoryPrompt,
    pub syllabus: &'a Syllabus,
    pub forms: &'a Forms,
    pub numbers: &'a [String],
}

fn failure(rule: &str, detail: impl Into<String>) -> RuleFailure {
    RuleFailure {
        rule: rule.to_string(),
        detail: detail.into(),
    }
}

/// S3: the share of covered words a story needs, 92 in 100.
const STORY_COVERAGE_PERCENT: usize = 92;

/// S2 to S6 against the context, the first failure (rule "S2" .. "S6"). With `focus` empty (an
/// imported story) S5 passes.
pub fn check_story(story: &Story, ctx: &StoryCtx) -> Result<(), RuleFailure> {
    let lang = ctx.prompt.lang;
    let course = TextLang::from(lang);
    // S2
    if let Some(n) = story
        .paragraphs
        .iter()
        .position(|p| !is_language_in(p, course, lang))
    {
        return Err(failure(
            "S2",
            format!("paragraph {} is not in the course language", n + 1),
        ));
    }
    // S3
    let allowed = allowed_from_list(ctx.syllabus, ctx.numbers, &ctx.prompt.allowed_lemmas);
    let stems = stems_of(lang, &allowed);
    let (mut covered, mut counted) = (0, 0);
    for paragraph in &story.paragraphs {
        let (c, n) = coverage_counts(lang, paragraph, &allowed, &stems, ctx.forms);
        covered += c;
        counted += n;
    }
    if counted > 0 && covered * 100 < counted * STORY_COVERAGE_PERCENT {
        return Err(failure(
            "S3",
            format!("{covered} of {counted} words are allowed"),
        ));
    }
    // S4: within min × 0.9 and max × 1.1, in integers.
    let shape = shape(ctx.prompt.cefr);
    let words: usize = story
        .paragraphs
        .iter()
        .map(|p| normalise(lang, p).unicode_words().count())
        .sum();
    let (low, high) = (shape.min_words as usize * 9, shape.max_words as usize * 11);
    if words * 10 < low || words * 10 > high {
        return Err(failure(
            "S4",
            format!(
                "{words} words; {} to {} asked for",
                shape.min_words, shape.max_words
            ),
        ));
    }
    // S5: a form of each focus word, through the forms table or, failing that, its Snowball stem.
    let tokens: Vec<_> = story
        .paragraphs
        .iter()
        .flat_map(|p| tokenise(lang, p, ctx.forms))
        .collect();
    for word in &ctx.prompt.focus {
        let key = nfc(word);
        let forms = ctx
            .syllabus
            .skills()
            .flat_map(|(_, skill)| skill.lemmas.iter())
            .find(|lemma| nfc(&lemma.lemma) == key)
            .map(TaughtForms::of);
        let focus_stem = stem(lang, &nfc_lower(word));
        let used = tokens.iter().any(|token| {
            forms.as_ref().is_some_and(|forms| forms.matches(token))
                || stem(lang, &token.norm) == focus_stem
        });
        if !used {
            return Err(failure("S5", format!("{word} is not used")));
        }
    }
    // S6
    for (n, question) in story.questions.iter().enumerate() {
        let options: HashSet<String> = question
            .options
            .iter()
            .map(|o| normalise(Lang::Es, o))
            .collect();
        if options.len() != question.options.len() || options.contains("") {
            return Err(failure(
                "S6",
                format!("question {} has repeated options", n + 1),
            ));
        }
        if question.paragraph as usize >= story.paragraphs.len()
            || question.answer_index as usize >= question.options.len()
        {
            return Err(failure(
                "S6",
                format!("question {} points outside the story", n + 1),
            ));
        }
        if !is_language_in(&question.question_en, TextLang::En, lang) {
            return Err(failure("S6", format!("question {} is not English", n + 1)));
        }
    }
    Ok(())
}

/// P2: ai::call with S2 to S6 as its rules, so any failure is AiError::Invalid, stores nothing and
/// counts one attempt (PUBLIK-INTEGRATION §6.1).
pub fn generate_story(
    client: &dyn AiClient,
    cache: &Cache,
    ctx: &StoryCtx,
) -> Result<Called<Story>, AiError> {
    call(
        client,
        cache,
        story_prompt_file(),
        &ctx.prompt.vars(),
        &ctx.prompt.key_input(),
        &|story: &Story| check_story(story, ctx),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a2_stories_are_longer_and_name_the_past_tense() {
        let prompt = StoryPrompt {
            lang: Lang::De,
            cefr: Cefr::A2,
            skill_id: "de.a2.u01.s04".into(),
            seq: 1,
            theme: "Travel".into(),
            focus: vec!["Zug".into()],
            variety: "Standard German".into(),
            allowed_lemmas: vec!["der".into()],
            names: vec!["Anna".into()],
        };
        let vars = prompt.vars();
        assert_eq!(vars["paragraphs"], "4");
        assert_eq!(vars["min_words"], "120");
        assert_eq!(vars["max_words"], "180");
        assert_eq!(vars["tenses"], "present, Perfekt and near future");
        let messages = story_prompt_file().render(&vars).unwrap();
        assert!(
            messages[0]
                .content
                .as_str()
                .unwrap()
                .contains("Use only these tenses: present, Perfekt and near future.")
        );
        assert_eq!(story_xp(4), 18);
    }
}
