//! P4, Explain my answer (docs/PUBLIK-INTEGRATION.md §6.6; docs/modules/08-judging-and-explanations.md):
//! the prompt's values per exercise type, and the call with the rules E2 and E3.
//!
//! An explanation is one unit of validation: any failure stores and caches nothing and counts one
//! attempt, so Try again is a new request (§6.1); a valid one is cached, so a second tap on Explain
//! makes no request.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use unicode_segmentation::UnicodeSegmentation;

use super::cache::Cache;
use super::call::{RuleFailure, call_charged};
use super::client::{AiClient, AiError};
use super::langid::{TextLang, is_language_in};
use super::prompts::{Prompt, json_literal};
use crate::api::language_name;
use crate::content::{Cefr, Lang};
use crate::exercise::ExerciseType;
use crate::grading::normalise;
use crate::srs;

const EXPLAIN_PROMPT: &str = include_str!("../../../../prompts/explain.v1.md");
const EXPLAIN_SCHEMA: &str = include_str!("../../../../schemas/ai/explain.v1.schema.json");

/// The prompt id of Explain my answer.
pub const EXPLAIN_PROMPT_ID: &str = "explain.v1";

/// "Explanations need a connection or a local model."
pub const EXPLAIN_OFFLINE: &str = "Explanations need a connection or a local model.";

/// "Explanations are available after an answer that was almost right or wrong."
pub const EXPLAIN_NOT_AVAILABLE: &str =
    "Explanations are available after an answer that was almost right or wrong.";

/// The most words an explanation may have (E2).
const MAX_EXPLANATION_WORDS: usize = 80;
/// The most words a tip may have (E2).
const MAX_TIP_WORDS: usize = 15;

/// `prompts/explain.v1.md` and `schemas/ai/explain.v1.schema.json`, compiled in.
pub fn explain_prompt_file() -> &'static Prompt {
    static PROMPT: OnceLock<Prompt> = OnceLock::new();
    PROMPT.get_or_init(|| {
        Prompt::parse(EXPLAIN_PROMPT, EXPLAIN_SCHEMA)
            .expect("prompts/explain.v1.md is a valid prompt")
    })
}

/// What P4 explains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplainRequest {
    pub lang: Lang,
    pub cefr: Cefr,
    pub exercise_type: ExerciseType,
    /// What the exercise showed: `prompt_en` (translate_to_target, speak), `prompt_target`
    /// (translate_to_en, pick_meaning), `text` (listen_type) or `sentence` (fill_gap).
    pub prompt: String,
    /// The answer expected: the first accepted answer (the two translations), the correct option
    /// (pick_meaning), `text` (listen_type, speak) or `answer` (fill_gap).
    pub expected: String,
    /// The learner's text, or the text of the chosen option.
    pub answer: String,
    /// `almost` or `wrong`.
    pub verdict: srs::Verdict,
    /// The judge's error tags, else the grade's tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// The grammar note of the skill.
    #[serde(default)]
    pub grammar_note: Option<String>,
}

/// How the prompt names an exercise type.
fn exercise_type_name(kind: ExerciseType, lang: Lang) -> String {
    match kind {
        ExerciseType::TranslateToTarget => format!("translate into {}", language_name(lang)),
        ExerciseType::TranslateToEn => "translate into English".to_string(),
        ExerciseType::PickMeaning => "choose the meaning".to_string(),
        ExerciseType::ListenType => "type what you hear".to_string(),
        ExerciseType::Speak => "say the sentence".to_string(),
        ExerciseType::FillGap => "fill the gap".to_string(),
        // Never explained; named for completeness.
        ExerciseType::MatchPairs => "match the pairs".to_string(),
    }
}

impl ExplainRequest {
    /// The placeholder values of PUBLIK-INTEGRATION §6.6, each the exact text the prompt shows:
    /// the verdict, the course language's name, the level, the exercise type's name,
    /// `prompt_json`, `expected_json` and `answer_json` as JSON string literals, the tags joined by
    /// `", "` (or `none`), and the grammar note as a JSON string literal (or `none`).
    pub fn vars(&self) -> BTreeMap<String, String> {
        let tags = if self.tags.is_empty() {
            "none".to_string()
        } else {
            self.tags.join(", ")
        };
        let grammar_note = match &self.grammar_note {
            Some(note) => json_literal(note),
            None => "none".to_string(),
        };
        let pairs = [
            ("verdict", self.verdict.as_str().to_string()),
            ("target_language_name", language_name(self.lang).to_string()),
            ("cefr", self.cefr.as_str().to_string()),
            (
                "exercise_type_name",
                exercise_type_name(self.exercise_type, self.lang),
            ),
            ("prompt_json", self.prompt.clone()),
            ("expected_json", self.expected.clone()),
            ("answer_json", self.answer.clone()),
            ("tags", tags),
            ("grammar_note", grammar_note),
        ];
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    /// {"lang", "exercise_type", "prompt", "expected", "answer_norm", "verdict"} (§6.6).
    pub fn key_input(&self) -> Value {
        json!({
            "lang": self.lang.as_str(),
            "exercise_type": self.exercise_type.as_str(),
            "prompt": self.prompt,
            "expected": self.expected,
            "answer_norm": normalise(self.lang, &self.answer),
            "verdict": self.verdict.as_str(),
        })
    }
}

/// A schema-valid P4 answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplainResponse {
    pub explanation: String,
    #[serde(default)]
    pub tip: Option<String>,
}

/// An explanation that passed E1 to E3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Explanation {
    pub text: String,
    /// None when the answer gave no tip, or an empty one.
    pub tip: Option<String>,
    /// From the AI cache: no request was made.
    pub cached: bool,
    pub charge_micros: Option<i64>,
}

fn failure(rule: &str, detail: impl Into<String>) -> RuleFailure {
    RuleFailure {
        rule: rule.to_string(),
        detail: detail.into(),
    }
}

/// E2 and E3: at most 80 words of explanation and 15 of tip (UAX #29 words), and the explanation
/// is English, told apart from the course language `lang` when the identification is unsure (an
/// explanation quotes the learner's words and the expected ones).
fn check_explanation(response: &ExplainResponse, lang: Lang) -> Result<(), RuleFailure> {
    let words = response.explanation.unicode_words().count();
    if words > MAX_EXPLANATION_WORDS {
        return Err(failure(
            "E2",
            format!("the explanation has {words} words; the limit is {MAX_EXPLANATION_WORDS}"),
        ));
    }
    if let Some(tip) = &response.tip {
        let words = tip.unicode_words().count();
        if words > MAX_TIP_WORDS {
            return Err(failure(
                "E2",
                format!("the tip has {words} words; the limit is {MAX_TIP_WORDS}"),
            ));
        }
    }
    if !is_language_in(&response.explanation, TextLang::En, lang) {
        return Err(failure("E3", "the explanation is not English"));
    }
    Ok(())
}

/// P4: ai::call with the E2 and E3 rules. Any failure is the call's AiError (a rule failure is
/// AiError::Invalid, nothing cached, one attempt counted).
pub fn explain(
    client: &dyn AiClient,
    cache: &Cache,
    req: &ExplainRequest,
) -> Result<Explanation, AiError> {
    explain_charged(client, cache, req).0
}

/// [`explain`] with the request's charge beside its result, also for an answer that fails E1 to
/// E3, so the session's cost counts it.
pub(crate) fn explain_charged(
    client: &dyn AiClient,
    cache: &Cache,
    req: &ExplainRequest,
) -> (Result<Explanation, AiError>, Option<i64>) {
    let (called, charge) = call_charged::<ExplainResponse>(
        client,
        cache,
        explain_prompt_file(),
        &req.vars(),
        &req.key_input(),
        &|response: &ExplainResponse| check_explanation(response, req.lang),
    );
    let explained = called.map(|called| {
        let ExplainResponse { explanation, tip } = called.value;
        Explanation {
            text: explanation,
            tip: tip.filter(|tip| !tip.trim().is_empty()),
            cached: called.cached,
            charge_micros: called.charge_micros,
        }
    });
    (explained, charge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::test_client::Scripted;
    use crate::env::Clock;

    struct Fixed;

    impl Clock for Fixed {
        fn now_ms(&self) -> i64 {
            0
        }
        fn utc_offset_minutes(&self) -> i32 {
            0
        }
    }

    fn cache() -> Cache {
        Cache::in_memory(Box::new(Fixed))
    }

    /// The request of the §6.6 example.
    fn example() -> ExplainRequest {
        ExplainRequest {
            lang: Lang::Es,
            cefr: Cefr::A1,
            exercise_type: ExerciseType::TranslateToTarget,
            prompt: "I would like a coffee with milk.".into(),
            expected: "Quiero un café con leche.".into(),
            answer: "Me gustaría una café con leche".into(),
            verdict: srs::Verdict::Almost,
            tags: vec!["gender".into()],
            grammar_note: Some("Every noun is masculine or feminine.".into()),
        }
    }

    const VALID: &str = r#"{"explanation": "You wrote \"una café\". Café is a masculine noun, so it needs the masculine article: un café. \"Me gustaría\" is a polite, correct way to say \"I would like\".", "tip": "Learn each noun with its article: un café."}"#;

    #[test]
    fn the_prompt_file_carries_the_settings_of_6_6() {
        let p = explain_prompt_file();
        assert_eq!(
            (p.id.as_str(), p.version, p.tier.as_str()),
            ("explain", 1, "publik-fast")
        );
        assert_eq!((p.max_tokens, p.timeout_s, p.temperature), (300, 15, 0.3));
        assert_eq!(p.schema, "schemas/ai/explain.v1.schema.json");
        assert_eq!(EXPLAIN_PROMPT_ID, format!("{}.v{}", p.id, p.version));
    }

    #[test]
    fn the_example_renders_the_documented_user_content() {
        let messages = explain_prompt_file().render(&example().vars()).unwrap();
        assert_eq!(
            messages[1].content,
            json!(
                "LEVEL: A1\nEXERCISE: translate into Spanish\nPROMPT: \"I would like a coffee with milk.\"\nEXPECTED: \"Quiero un café con leche.\"\nLEARNER ANSWER: \"Me gustaría una café con leche\"\nVERDICT: almost; TAGS: gender\nGRAMMAR NOTE OF THIS SKILL: \"Every noun is masculine or feminine.\""
            )
        );
        let system = messages[0].content.as_str().unwrap();
        assert!(system.starts_with(
            "You are a patient language teacher. Explain to an adult learner whose own language is English\nwhy their answer was marked \"almost\", in at most 80 words of plain English."
        ));
        assert!(system.contains("with one short example in\nSpanish if it helps."));
        let bare = ExplainRequest {
            exercise_type: ExerciseType::FillGap,
            tags: vec![],
            grammar_note: None,
            verdict: srs::Verdict::Wrong,
            ..example()
        };
        let user = explain_prompt_file().render(&bare.vars()).unwrap()[1]
            .content
            .clone();
        let user = user.as_str().unwrap();
        assert!(user.contains("EXERCISE: fill the gap\n"));
        assert!(user.ends_with("VERDICT: wrong; TAGS: none\nGRAMMAR NOTE OF THIS SKILL: none"));
    }

    #[test]
    fn exercise_types_have_their_names() {
        let names: Vec<String> = [
            ExerciseType::TranslateToTarget,
            ExerciseType::TranslateToEn,
            ExerciseType::PickMeaning,
            ExerciseType::ListenType,
            ExerciseType::Speak,
            ExerciseType::FillGap,
        ]
        .into_iter()
        .map(|kind| exercise_type_name(kind, Lang::Fr))
        .collect();
        assert_eq!(
            names,
            [
                "translate into French",
                "translate into English",
                "choose the meaning",
                "type what you hear",
                "say the sentence",
                "fill the gap"
            ]
        );
    }

    #[test]
    fn the_key_input_has_the_six_fields() {
        assert_eq!(
            example().key_input(),
            json!({
                "lang": "es",
                "exercise_type": "translate_to_target",
                "prompt": "I would like a coffee with milk.",
                "expected": "Quiero un café con leche.",
                "answer_norm": "me gustaría una café con leche",
                "verdict": "almost",
            })
        );
    }

    #[test]
    fn a_valid_explanation_is_cached_and_shown() {
        let client = Scripted::new(vec![Scripted::reply(VALID, Some(220))]);
        let cache = cache();
        let first = explain(&client, &cache, &example()).unwrap();
        assert_eq!(
            (first.cached, first.charge_micros, first.tip.as_deref()),
            (
                false,
                Some(220),
                Some("Learn each noun with its article: un café.")
            )
        );
        assert!(first.text.starts_with("You wrote \"una café\"."));
        let again = explain(&client, &cache, &example()).unwrap();
        assert_eq!((again.cached, again.charge_micros), (true, None));
        assert_eq!(client.requests(), 1);
        let body = client.seen.lock().unwrap()[0].body(false);
        assert_eq!(
            (
                body["model"].clone(),
                body["max_tokens"].clone(),
                body["temperature"].clone()
            ),
            (json!("publik-fast"), json!(300), json!(0.3))
        );
        let no_tip = Scripted::new(vec![Scripted::reply(
            r#"{"explanation": "The noun is masculine, so it needs the masculine article.", "tip": " "}"#,
            None,
        )]);
        assert_eq!(
            explain(
                &no_tip,
                &cache,
                &ExplainRequest {
                    answer: "x".into(),
                    ..example()
                }
            )
            .unwrap()
            .tip,
            None
        );
    }

    #[test]
    fn rules_e1_to_e3_refuse_and_cache_nothing() {
        let words = |n: usize| {
            let base = [
                "You",
                "need",
                "the",
                "masculine",
                "article",
                "because",
                "this",
                "noun",
                "is",
                "masculine",
            ];
            (0..n)
                .map(|i| base[i % base.len()])
                .collect::<Vec<_>>()
                .join(" ")
        };
        let cases = [
            json!({"explanation": words(81)}).to_string(),
            json!({"explanation": "Café is masculine.", "tip": words(16)}).to_string(),
            json!({"explanation": "Tu as écrit une café, mais café est un nom masculin, alors il faut dire un café avec le bon article."}).to_string(),
            json!({"explanation": "Fine.", "note": "extra"}).to_string(),
            json!({"explanation": ""}).to_string(),
            json!({"tip": "Learn the article."}).to_string(),
        ];
        for content in &cases {
            let client = Scripted::new(vec![Scripted::reply(content, Some(220))]);
            let cache = cache();
            assert_eq!(
                explain(&client, &cache, &example()),
                Err(AiError::Invalid),
                "{content}"
            );
            assert_eq!(cache.len(), 0, "{content}");
        }
        let client = Scripted::new(vec![Scripted::reply(
            &json!({"explanation": words(80), "tip": words(15)}).to_string(),
            None,
        )]);
        assert!(explain(&client, &cache(), &example()).is_ok());
    }
}
