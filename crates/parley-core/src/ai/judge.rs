//! P3, the answer judge (docs/PUBLIK-INTEGRATION.md §6.5; docs/modules/08-judging-and-explanations.md):
//! the prompt's values, the call with the rules J2 and J3, J4, the effects of a verdict, and the
//! grade a verdict gives.
//!
//! A verdict is one unit of validation: any failure stores and caches nothing and counts one
//! attempt, so Try again is a new request (§6.1). A valid verdict is learning data kept in the
//! `judgements` table under a provider-independent key, so the same answer to the same prompt is
//! decided offline from then on, whichever provider judged it.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use unicode_segmentation::UnicodeSegmentation;

use super::cache::Cache;
use super::call::{RuleFailure, call};
use super::client::{AiClient, AiError, ProviderKind};
use super::langid::{TextLang, is_language, is_language_in};
use super::prompts::Prompt;
use crate::api::language_name;
use crate::content::{Cefr, Lang, nfc};
use crate::exercise::ExerciseType;
use crate::grading::{Direction, Grade, StoredJudgement, Tag, normalise};
use crate::srs::{self, Outcome, rating_for};
use crate::store::{AnswerSource, Store, StoreError};

pub use crate::grading::judgement_key;

const JUDGE_PROMPT: &str = include_str!("../../../../prompts/judge.v1.md");
const JUDGE_SCHEMA: &str = include_str!("../../../../schemas/ai/judge.v1.schema.json");

/// The prompt id of the answer judge.
pub const JUDGE_PROMPT_ID: &str = "judge.v1";

/// "You're offline, so Parley can't check free answers."
pub const JUDGE_OFFLINE: &str = "You're offline, so Parley can't check free answers.";

/// The most words an explanation of a verdict may have (J2).
const MAX_EXPLANATION_WORDS: usize = 25;

/// `prompts/judge.v1.md` and `schemas/ai/judge.v1.schema.json`, compiled in.
pub fn judge_prompt_file() -> &'static Prompt {
    static PROMPT: OnceLock<Prompt> = OnceLock::new();
    PROMPT.get_or_init(|| {
        Prompt::parse(JUDGE_PROMPT, JUDGE_SCHEMA).expect("prompts/judge.v1.md is a valid prompt")
    })
}

/// What P3 judges. `item_id` and `own_prompt` default to "" and false when absent (the CLI's
/// request file and the gate set leave them out).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgeRequest {
    /// The course language.
    pub lang: Lang,
    /// `to_target`: the answer is in `lang`; `to_en`: the answer is English.
    pub direction: Direction,
    /// The item's level.
    pub cefr: Cefr,
    /// The exercise's prompt: `prompt_en` (to_target) or `prompt_target` (to_en).
    pub source: String,
    /// The exercise's own accepted answers, in order.
    pub accepted: Vec<String>,
    /// The learner's text as typed.
    pub answer: String,
    #[serde(default)]
    pub item_id: String,
    /// The prompt is the item's own (module 03's rule), so a correct answer joins the item's
    /// accepted answers.
    #[serde(default)]
    pub own_prompt: bool,
}

impl JudgeRequest {
    /// Whether `text` is in the answer's language: the course language for `to_target`, English
    /// for `to_en` (told apart from the course language when the identification is unsure).
    fn in_answer_language(&self, text: &str) -> bool {
        match self.direction {
            Direction::ToTarget => is_language(text, TextLang::from(self.lang)),
            Direction::ToEn => is_language_in(text, TextLang::En, self.lang),
        }
    }

    /// The placeholder values of PUBLIK-INTEGRATION §6.5, each the exact text the prompt shows:
    /// the answer's language name and the source's (`Spanish` and `English` for `to_target`, the
    /// other way round for `to_en`), the level, `source_json` and `answer_json` as JSON string
    /// literals, `accepted_json` the accepted answers as JSON string literals joined by `", "`
    /// inside `[` and `]`.
    pub fn vars(&self) -> BTreeMap<String, String> {
        let course = language_name(self.lang);
        let (answer_language, source_language) = match self.direction {
            Direction::ToTarget => (course, "English"),
            Direction::ToEn => ("English", course),
        };
        let accepted = serde_json::to_string(&self.accepted).expect("strings serialise");
        let pairs = [
            ("answer_language_name", answer_language.to_string()),
            ("source_language_name", source_language.to_string()),
            ("cefr", self.cefr.as_str().to_string()),
            ("source_json", self.source.clone()),
            ("accepted_json", accepted),
            ("answer_json", self.answer.clone()),
        ];
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    /// {"lang", "direction", "source", "accepted", "answer_norm"} (§6.5): the language and the
    /// direction as their strings, `accepted` the JSON array, `answer_norm` the answer's normal
    /// form.
    pub fn key_input(&self) -> Value {
        json!({
            "lang": self.lang.as_str(),
            "direction": self.direction.as_str(),
            "source": self.source,
            "accepted": self.accepted,
            "answer_norm": normalise(self.lang, &self.answer),
        })
    }

    /// The key of this answer's row in `judgements`.
    fn judgement_key(&self) -> String {
        judgement_key(
            self.lang,
            self.direction,
            &self.source,
            &normalise(self.lang, &self.answer),
        )
    }
}

/// A schema-valid P3 answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgeResponse {
    pub verdict: srs::Verdict,
    pub corrected: String,
    pub explanation: String,
    #[serde(default)]
    pub error_tags: Vec<String>,
}

/// A verdict, after J4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Judgement {
    pub verdict: srs::Verdict,
    pub corrected: String,
    pub explanation: String,
    /// The error tags; always empty for `correct` (J4).
    pub tags: Vec<String>,
    /// From the `judgements` table or the AI cache: no request was made.
    pub cached: bool,
    pub charge_micros: Option<i64>,
}

fn failure(rule: &str, detail: impl Into<String>) -> RuleFailure {
    RuleFailure {
        rule: rule.to_string(),
        detail: detail.into(),
    }
}

/// J2 and J3: the explanation has at most 25 words (UAX #29 words), and a corrected answer of
/// four words or more is in the answer's language.
fn check_verdict(response: &JudgeResponse, req: &JudgeRequest) -> Result<(), RuleFailure> {
    let words = response.explanation.unicode_words().count();
    if words > MAX_EXPLANATION_WORDS {
        return Err(failure(
            "J2",
            format!("the explanation has {words} words; the limit is {MAX_EXPLANATION_WORDS}"),
        ));
    }
    if !req.in_answer_language(&response.corrected) {
        return Err(failure(
            "J3",
            "the corrected answer is not in the answer's language",
        ));
    }
    Ok(())
}

/// J4: a `correct` verdict keeps no tags.
fn without_tags_if_correct(verdict: srs::Verdict, tags: Vec<String>) -> Vec<String> {
    if verdict == srs::Verdict::Correct {
        Vec::new()
    } else {
        tags
    }
}

/// P3 with no store: ai::call with the J2 and J3 rules, then J4. Any failure is the call's
/// AiError (a rule failure is AiError::Invalid, nothing cached, one attempt counted).
pub fn request_judgement(
    client: &dyn AiClient,
    cache: &Cache,
    req: &JudgeRequest,
) -> Result<Judgement, AiError> {
    let called = call::<JudgeResponse>(
        client,
        cache,
        judge_prompt_file(),
        &req.vars(),
        &req.key_input(),
        &|response: &JudgeResponse| check_verdict(response, req),
    )?;
    let JudgeResponse {
        verdict,
        corrected,
        explanation,
        error_tags,
    } = called.value;
    Ok(Judgement {
        verdict,
        corrected,
        explanation,
        tags: without_tags_if_correct(verdict, error_tags),
        cached: called.cached,
        charge_micros: called.charge_micros,
    })
}

/// The effects of a valid verdict: the `judgements` row under `judgement_key` (provider =
/// `provider.as_str()`), and for `correct` with `own_prompt`, an `accepted_answers` row of the item
/// in the request's direction (display = the answer, NFC and trimmed; source `judge`). An answer
/// with no words stores nothing.
pub fn apply_judgement(
    store: &mut Store,
    req: &JudgeRequest,
    judgement: &Judgement,
    provider: ProviderKind,
    now_ms: i64,
) -> Result<(), StoreError> {
    if normalise(req.lang, &req.answer).is_empty() {
        return Ok(());
    }
    let stored = StoredJudgement {
        verdict: judgement.verdict,
        corrected: judgement.corrected.clone(),
        explanation: judgement.explanation.clone(),
        tags: without_tags_if_correct(judgement.verdict, judgement.tags.clone()),
        provider: provider.as_str().to_string(),
    };
    store.put_judgement(&req.judgement_key(), &stored, now_ms)?;
    if judgement.verdict == srs::Verdict::Correct && req.own_prompt && !req.item_id.is_empty() {
        let display = nfc(&req.answer).trim().to_string();
        store.add_accepted_answer(
            &req.item_id,
            req.direction,
            &display,
            AnswerSource::Judge,
            now_ms,
        )?;
    }
    Ok(())
}

/// End to end: a stored judgement for the key is returned (cached: true) with no request;
/// otherwise request_judgement, then apply_judgement. A store that cannot be written loses only
/// the saving: the verdict is still returned.
pub fn judge(
    client: &dyn AiClient,
    cache: &Cache,
    store: &mut Store,
    req: &JudgeRequest,
    now_ms: i64,
) -> Result<Judgement, AiError> {
    if let Ok(Some(stored)) = store.judgement(&req.judgement_key()) {
        return Ok(Judgement {
            verdict: stored.verdict,
            corrected: stored.corrected,
            explanation: stored.explanation,
            tags: without_tags_if_correct(stored.verdict, stored.tags),
            cached: true,
            charge_micros: None,
        });
    }
    let judgement = request_judgement(client, cache, req)?;
    let _ = apply_judgement(store, req, &judgement, client.kind(), now_ms);
    Ok(judgement)
}

/// The grade a verdict gives an exercise of `kind` (no hint, no response time; the run rates the
/// card again with both): Correct → rating Good, corrected None; Almost → Hard, corrected Some;
/// Wrong → Again, corrected Some and expected = `first_accepted`. explanation Some for all three;
/// tags: the error_tags that are module 03 tags (`accent`), others dropped; message None.
pub fn verdict_grade(
    kind: ExerciseType,
    judgement: &Judgement,
    first_accepted: Option<&str>,
) -> Grade {
    let verdict = judgement.verdict;
    let rating = rating_for(&Outcome {
        verdict,
        used_hint: false,
        exercise_type: kind.as_str().to_string(),
        response_ms: None,
        learner_override: false,
    });
    let mut tags: Vec<Tag> = Vec::new();
    for tag in judgement.tags.iter().filter_map(|t| Tag::parse(t)) {
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    Grade {
        verdict: verdict.into(),
        tags,
        message: None,
        corrected: (verdict != srs::Verdict::Correct).then(|| judgement.corrected.clone()),
        words: Vec::new(),
        rating: Some(rating),
        explanation: Some(judgement.explanation.clone()),
        expected: if verdict == srs::Verdict::Wrong {
            first_accepted.map(str::to_string)
        } else {
            None
        },
        judge_error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::test_client::Scripted;
    use crate::content::parse_syllabus;
    use crate::content::test_support::{EXAMPLE_SYLLABUS, example_forms};
    use crate::env::Clock;
    use crate::grading::Verdict;
    use crate::srs::Rating;

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

    /// The request of the §6.5 example.
    fn example() -> JudgeRequest {
        JudgeRequest {
            lang: Lang::Es,
            direction: Direction::ToTarget,
            cefr: Cefr::A1,
            source: "I would like a coffee with milk.".into(),
            accepted: vec![
                "Quiero un café con leche.".into(),
                "Quisiera un café con leche.".into(),
            ],
            answer: "Me gustaría una café con leche".into(),
            item_id: String::new(),
            own_prompt: false,
        }
    }

    const ALMOST: &str = r#"{"verdict": "almost", "corrected": "Me gustaría un café con leche", "explanation": "Café is masculine, so it takes un, not una.", "error_tags": ["gender"]}"#;

    /// The complete example request of docs/PUBLIK-INTEGRATION.md §6.5, as the document shows it.
    fn documented_example() -> Value {
        let doc = include_str!("../../../../docs/PUBLIK-INTEGRATION.md");
        let section = &doc[doc.find("### 6.5").unwrap()..];
        let start = section.find("```json\n").unwrap() + "```json\n".len();
        let end = start + section[start..].find("\n```").unwrap();
        serde_json::from_str(&section[start..end]).unwrap()
    }

    #[test]
    fn the_prompt_file_carries_the_settings_of_6_5() {
        let p = judge_prompt_file();
        assert_eq!(
            (p.id.as_str(), p.version, p.tier.as_str()),
            ("judge", 1, "publik-fast")
        );
        assert_eq!((p.max_tokens, p.timeout_s, p.temperature), (200, 10, 0.0));
        assert_eq!(p.schema, "schemas/ai/judge.v1.schema.json");
        assert_eq!(JUDGE_PROMPT_ID, format!("{}.v{}", p.id, p.version));
    }

    #[test]
    fn the_example_renders_the_documented_request() {
        let client = Scripted::new(vec![Scripted::reply(ALMOST, Some(150))]);
        let judgement = request_judgement(&client, &cache(), &example()).unwrap();
        let seen = client.seen.lock().unwrap();
        assert_eq!(seen[0].body(false), documented_example());
        assert_eq!(seen[0].timeout_s, 10);
        assert_eq!(
            judgement,
            Judgement {
                verdict: srs::Verdict::Almost,
                corrected: "Me gustaría un café con leche".into(),
                explanation: "Café is masculine, so it takes un, not una.".into(),
                tags: vec!["gender".into()],
                cached: false,
                charge_micros: Some(150),
            }
        );
    }

    #[test]
    fn values_name_the_languages_by_direction() {
        let vars = example().vars();
        assert_eq!(vars["answer_language_name"], "Spanish");
        assert_eq!(vars["source_language_name"], "English");
        // Text goes to the renderer raw, which writes it as JSON; the list as a JSON array.
        assert_eq!(vars["source_json"], "I would like a coffee with milk.");
        assert_eq!(
            vars["accepted_json"],
            r#"["Quiero un café con leche.","Quisiera un café con leche."]"#
        );
        assert_eq!(vars["answer_json"], "Me gustaría una café con leche");
        let to_en = JudgeRequest {
            lang: Lang::De,
            direction: Direction::ToEn,
            cefr: Cefr::A2,
            source: "Ich möchte einen Kaffee.".into(),
            accepted: vec![],
            answer: "I'd like a \"coffee\"".into(),
            ..example()
        };
        let vars = to_en.vars();
        assert_eq!(vars["answer_language_name"], "English");
        assert_eq!(vars["source_language_name"], "German");
        assert_eq!(vars["cefr"], "A2");
        assert_eq!(vars["accepted_json"], "[]");
        let messages = judge_prompt_file().render(&vars).unwrap();
        assert_eq!(
            messages[1].content,
            json!(
                "LEVEL: A2\nSOURCE (German): \"Ich möchte einen Kaffee.\"\nACCEPTED: []\nANSWER: \"I'd like a \\\"coffee\\\"\""
            )
        );
    }

    #[test]
    fn the_key_input_has_the_five_fields() {
        let req = JudgeRequest {
            answer: "  Me GUSTARÍA, una café con leche!".into(),
            ..example()
        };
        assert_eq!(
            req.key_input(),
            json!({
                "lang": "es",
                "direction": "to_target",
                "source": "I would like a coffee with milk.",
                "accepted": ["Quiero un café con leche.", "Quisiera un café con leche."],
                "answer_norm": "me gustaría una café con leche",
            })
        );
        let parsed: JudgeRequest = serde_json::from_value(json!({
            "lang": "es", "direction": "to_target", "cefr": "A1",
            "source": "I would like a coffee with milk.",
            "accepted": ["Quiero un café con leche.", "Quisiera un café con leche."],
            "answer": "Me gustaría una café con leche"
        }))
        .unwrap();
        assert_eq!(parsed, example());
    }

    fn reply(verdict: &str, corrected: &str, explanation: &str, tags: &[&str]) -> String {
        json!({"verdict": verdict, "corrected": corrected, "explanation": explanation, "error_tags": tags})
            .to_string()
    }

    #[test]
    fn rules_j1_to_j3_refuse_and_cache_nothing() {
        let words = |n: usize| vec!["word"; n].join(" ");
        let cases = [
            r#"{"corrected": "Me gustaría un café", "explanation": "Gender.", "error_tags": []}"#
                .to_string(),
            reply(
                "almost",
                "Me gustaría un café con leche",
                &words(26),
                &["gender"],
            ),
            reply(
                "almost",
                "I would like a coffee with milk",
                "Use Spanish.",
                &[],
            ),
            reply("maybe", "x", "y", &[]),
            reply("wrong", "x", "y", &["accent", "accent"]),
        ];
        for content in &cases {
            let client = Scripted::new(vec![Scripted::reply(content, Some(150))]);
            let cache = cache();
            let result = request_judgement(&client, &cache, &example());
            assert_eq!(result, Err(AiError::Invalid), "{content}");
            assert_eq!(cache.len(), 0, "{content}");
        }
        // 25 words pass, a corrected answer of three English words passes, and so does one in
        // the course language.
        for content in [
            reply(
                "almost",
                "Me gustaría un café con leche",
                &words(25),
                &["gender"],
            ),
            reply("wrong", "a coffee please", "Use Spanish.", &[]),
        ] {
            let client = Scripted::new(vec![Scripted::reply(&content, None)]);
            assert!(
                request_judgement(&client, &cache(), &example()).is_ok(),
                "{content}"
            );
        }
    }

    #[test]
    fn j3_follows_the_direction() {
        let to_en = JudgeRequest {
            direction: Direction::ToEn,
            source: "Quiero un café con leche.".into(),
            accepted: vec!["I want a coffee with milk.".into()],
            answer: "I want coffee with the milk".into(),
            ..example()
        };
        let spanish = reply("almost", "Quiero un café con leche.", "Use English.", &[]);
        let client = Scripted::new(vec![Scripted::reply(&spanish, None)]);
        assert_eq!(
            request_judgement(&client, &cache(), &to_en),
            Err(AiError::Invalid)
        );
        let english = reply("almost", "I want a coffee with milk.", "Say a coffee.", &[]);
        let client = Scripted::new(vec![Scripted::reply(&english, None)]);
        assert!(request_judgement(&client, &cache(), &to_en).is_ok());
    }

    #[test]
    fn j4_drops_the_tags_of_a_correct_verdict_on_every_use() {
        let content = reply(
            "correct",
            "Me gustaría un café con leche",
            "Fine.",
            &["gender"],
        );
        let client = Scripted::new(vec![Scripted::reply(&content, Some(150))]);
        let cache = cache();
        let first = request_judgement(&client, &cache, &example()).unwrap();
        assert_eq!(
            (first.verdict, first.tags.len()),
            (srs::Verdict::Correct, 0)
        );
        let again = request_judgement(&client, &cache, &example()).unwrap();
        assert_eq!(
            (again.cached, again.tags.len(), again.charge_micros),
            (true, 0, None)
        );
        assert_eq!(client.requests(), 1);
        let stored = cache.get(&crate::ai::cache_key(&crate::ai::cache_key_json(
            "judge",
            1,
            ProviderKind::Publik,
            "publik-fast",
            &example().key_input(),
        )));
        assert!(
            stored.unwrap().contains("gender"),
            "the cache keeps the tags"
        );
    }

    fn started_store() -> Store {
        let syllabus = parse_syllabus("valid-es.yaml", EXAMPLE_SYLLABUS).unwrap();
        let mut store = Store::open_in_memory().unwrap();
        store.start_course(&syllabus, &example_forms(), 0).unwrap();
        store
    }

    #[test]
    fn judge_stores_the_verdict_and_a_correct_answer_once() {
        let mut store = started_store();
        let item = "es.a1.u01.s01#e:2";
        let req = JudgeRequest {
            source: "Good morning.".into(),
            accepted: vec!["Buenos días.".into()],
            answer: " Buen día ".into(),
            item_id: item.into(),
            own_prompt: true,
            ..example()
        };
        let content = reply("correct", "Buen día", "Also fine.", &[]);
        let client = Scripted::new(vec![Scripted::reply(&content, Some(150))]);
        let cache = cache();
        let first = judge(&client, &cache, &mut store, &req, 7).unwrap();
        assert!(!first.cached);
        let row = store
            .judgement(&judgement_key(
                Lang::Es,
                Direction::ToTarget,
                "Good morning.",
                "buen día",
            ))
            .unwrap()
            .unwrap();
        assert_eq!(
            (row.verdict, row.provider.as_str()),
            (srs::Verdict::Correct, "publik")
        );
        let answers = store.accepted_answers(item, Direction::ToTarget).unwrap();
        let judged: Vec<_> = answers
            .iter()
            .filter(|a| a.source == AnswerSource::Judge)
            .collect();
        assert_eq!(judged.len(), 1);
        assert_eq!(judged[0].display, "Buen día");
        let again = judge(&client, &cache, &mut store, &req, 8).unwrap();
        assert_eq!((again.cached, again.verdict), (true, srs::Verdict::Correct));
        assert_eq!(client.requests(), 1, "a stored judgement makes no request");

        // Not the item's own prompt: only the judgement is stored.
        let other = JudgeRequest {
            source: "Morning!".into(),
            own_prompt: false,
            ..req.clone()
        };
        let client = Scripted::new(vec![Scripted::reply(&content, None)]);
        judge(&client, &cache, &mut store, &other, 9).unwrap();
        assert_eq!(
            store
                .accepted_answers(item, Direction::ToTarget)
                .unwrap()
                .len(),
            answers.len()
        );
    }

    #[test]
    fn verdicts_give_their_grades() {
        let judged = |verdict: srs::Verdict, tags: &[&str]| Judgement {
            verdict,
            corrected: "Me gustaría un café con leche".into(),
            explanation: "Café is masculine.".into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            cached: false,
            charge_micros: None,
        };
        let kind = ExerciseType::TranslateToTarget;
        let first = Some("Quiero un café con leche.");
        let correct = verdict_grade(kind, &judged(srs::Verdict::Correct, &[]), first);
        assert_eq!(
            (
                correct.verdict,
                correct.rating,
                correct.corrected,
                correct.expected
            ),
            (Verdict::Correct, Some(Rating::Good), None, None)
        );
        assert_eq!(correct.explanation.as_deref(), Some("Café is masculine."));
        let almost = verdict_grade(
            kind,
            &judged(srs::Verdict::Almost, &["accent", "gender"]),
            first,
        );
        assert_eq!(
            (almost.verdict, almost.rating),
            (Verdict::Almost, Some(Rating::Hard))
        );
        assert_eq!(
            almost.corrected.as_deref(),
            Some("Me gustaría un café con leche")
        );
        assert_eq!((almost.tags, almost.expected), (vec![Tag::Accent], None));
        let wrong = verdict_grade(kind, &judged(srs::Verdict::Wrong, &["meaning"]), first);
        assert_eq!(
            (wrong.verdict, wrong.rating),
            (Verdict::Wrong, Some(Rating::Again))
        );
        assert_eq!(wrong.expected.as_deref(), first);
        assert!(wrong.tags.is_empty());
        assert_eq!((wrong.message, wrong.judge_error), (None, None));
    }
}
