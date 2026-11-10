//! P1, the fresh lesson (docs/PUBLIK-INTEGRATION.md §6.3; docs/modules/07-lesson-generation.md):
//! the prompt's values from the course and the composer's plan, the call through `ai::call`, the
//! validators L1 to L9, the template fill and the notice.
//!
//! Nothing the model writes reaches the learner before it passes: the schema (L1) through the call
//! pipeline, then L2 to L9 per exercise (invariant I2). An exercise that fails is dropped alone and
//! its plan entry gets the template exercise; when more than half the entries have no AI exercise,
//! the whole lesson is the template lesson with the notice. Every AI exercise refers to an item of
//! the plan the core composed (L2 and L3; invariant I11).

use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::cache::Cache;
use super::call::{Called, RuleFailure, call};
use super::client::{AiClient, AiError, ProviderKind};
use super::prompts::Prompt;
use super::validate::{AiCheck, allowed_from_list, check_ai_exercise, stems_of};
use crate::api::language_name;
use crate::content::{Cefr, Example, Forms, Item, ItemKind, Lang, Syllabus, nfc};
use crate::env::Rng;
use crate::exercise::template::{focus_of, seen_push, template_for_entry};
use crate::exercise::{
    Allowed, Body, Exercise, ExerciseType, Lesson, Origin, Plan, TemplateCtx, max_words,
    template_lesson, word_bank,
};
use crate::grading::normalise;

pub const FRESH_LESSON_UNAVAILABLE: &str =
    "A fresh lesson was not available, so this one comes from the course pack.";
pub const OFFLINE_LESSON: &str = "You're offline, so this lesson comes from the course pack.";

/// The prompt id stored with a lesson a fresh-lesson answer was received for.
pub const LESSON_PROMPT_ID: &str = "lesson.v1";

/// The most entries the allowed lemma list has (§6.3).
const ALLOWED_CAP: usize = 600;

const LESSON_PROMPT: &str = include_str!("../../../../prompts/lesson.v1.md");
const LESSON_SCHEMA: &str = include_str!("../../../../schemas/ai/lesson.v1.schema.json");

/// `prompts/lesson.v1.md` and `schemas/ai/lesson.v1.schema.json`, compiled in.
pub fn lesson_prompt_file() -> &'static Prompt {
    static PROMPT: OnceLock<Prompt> = OnceLock::new();
    PROMPT.get_or_init(|| {
        Prompt::parse(LESSON_PROMPT, LESSON_SCHEMA).expect("prompts/lesson.v1.md is a valid prompt")
    })
}

/// One plan entry as the prompt shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptEntry {
    pub n: u32,
    pub item_id: String,
    #[serde(rename = "type")]
    pub kind: ExerciseType,
    pub focus: String,
    /// The item's first English gloss for a vocabulary item; none for phrases and patterns.
    pub gloss: Option<String>,
    pub new: bool,
}

/// What P1 sends, as data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LessonPrompt {
    pub lang: Lang,
    pub cefr: Cefr,
    pub skill_id: String,
    pub seq: u32,
    pub skill_title: String,
    pub can_do: String,
    pub grammar_note: Option<String>,
    pub variety: String,
    pub allowed_lemmas: Vec<String>,
    pub names: Vec<String>,
    pub plan: Vec<PromptEntry>,
}

/// SHA-256 hex of the allowed lemma list as the prompt shows it.
pub(crate) fn allowed_sha256(allowed: &[String]) -> String {
    Sha256::digest(allowed.join(", ").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The allowed lemma list of P1 and P2: the syllabus's function words in syllabus order; then
/// every lemma of each skill in `introduced_skills` and of `skill_id`, by frequency rank (lemmas
/// without a rank after the others, each group in syllabus order), each once, a lemma equal to a
/// function word left out. Over 600 entries, lemmas of other skills are removed from the end
/// until it is 600; function words and this skill's lemmas stay.
pub fn allowed_lemmas(
    syllabus: &Syllabus,
    introduced_skills: &[String],
    skill_id: &str,
) -> Vec<String> {
    let function: HashSet<String> = syllabus.function_words.iter().map(|w| nfc(w)).collect();
    let mut seen: HashSet<String> = HashSet::new();
    // (rank, own skill, lemma) in syllabus order.
    let mut lemmas: Vec<(Option<u32>, bool, String)> = Vec::new();
    for (_, skill) in syllabus.skills() {
        let own = skill.id == skill_id;
        if !own && !introduced_skills.contains(&skill.id) {
            continue;
        }
        for lemma in &skill.lemmas {
            let key = nfc(&lemma.lemma);
            if function.contains(&key) || !seen.insert(key) {
                continue;
            }
            lemmas.push((lemma.rank, own, lemma.lemma.clone()));
        }
    }
    // A stable sort keeps syllabus order within equal ranks and among the unranked.
    lemmas.sort_by_key(|(rank, _, _)| rank.map_or((1, 0), |r| (0, r)));
    let budget = ALLOWED_CAP.saturating_sub(syllabus.function_words.len());
    while lemmas.len() > budget {
        match lemmas.iter().rposition(|(_, own, _)| !own) {
            Some(at) => {
                lemmas.remove(at);
            }
            None => break,
        }
    }
    syllabus
        .function_words
        .iter()
        .cloned()
        .chain(lemmas.into_iter().map(|(_, _, lemma)| lemma))
        .collect()
}

impl LessonPrompt {
    /// From the course: the skill's title, can-do, grammar note and level; the syllabus's variety
    /// and names; the allowed lemmas; one entry per plan entry, `gloss` the item's first English
    /// gloss for a vocab item and None for phrase and pattern items. None: the skill is not in the
    /// syllabus.
    pub fn new(
        syllabus: &Syllabus,
        items: &[Item],
        introduced_skills: &[String],
        skill_id: &str,
        seq: u32,
        plan: &Plan,
    ) -> Option<LessonPrompt> {
        let (_, skill) = syllabus.skills().find(|(_, skill)| skill.id == skill_id)?;
        let entries = plan
            .entries
            .iter()
            .map(|entry| {
                let gloss = items
                    .iter()
                    .find(|item| item.id == entry.item_id)
                    .filter(|item| item.kind == ItemKind::Vocab)
                    .and_then(|item| item.en.first().cloned());
                PromptEntry {
                    n: entry.n,
                    item_id: entry.item_id.clone(),
                    kind: entry.kind,
                    focus: entry.focus.clone(),
                    gloss,
                    new: entry.new,
                }
            })
            .collect();
        Some(LessonPrompt {
            lang: syllabus.lang,
            cefr: skill.cefr,
            skill_id: skill_id.to_string(),
            seq,
            skill_title: skill.title.clone(),
            can_do: skill.can_do.clone(),
            grammar_note: skill.grammar_note.clone(),
            variety: syllabus.variety.clone(),
            allowed_lemmas: allowed_lemmas(syllabus, introduced_skills, skill_id),
            names: syllabus.names.clone(),
            plan: entries,
        })
    }

    /// The PLAN block: one line per entry.
    fn plan_text(&self) -> String {
        self.plan
            .iter()
            .map(|entry| {
                let gloss = entry
                    .gloss
                    .as_ref()
                    .map(|g| format!(" ({g})"))
                    .unwrap_or_default();
                format!(
                    "{}. item_id={} type={} focus=\"{}\"{gloss} new={}",
                    entry.n,
                    entry.item_id,
                    entry.kind.as_str(),
                    entry.focus,
                    entry.new
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The placeholder values of PUBLIK-INTEGRATION §6.3.
    pub fn vars(&self) -> BTreeMap<String, String> {
        let pairs = [
            ("target_language_name", language_name(self.lang).to_string()),
            ("cefr", self.cefr.as_str().to_string()),
            ("variety", self.variety.clone()),
            ("max_words", max_words(self.cefr).to_string()),
            ("skill_title", self.skill_title.clone()),
            ("can_do", self.can_do.clone()),
            (
                "grammar_note",
                self.grammar_note.clone().unwrap_or_else(|| "none".into()),
            ),
            ("allowed_lemmas", self.allowed_lemmas.join(", ")),
            ("allowed_names", self.names.join(", ")),
            ("plan", self.plan_text()),
        ];
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    /// {"lang", "skill_id", "seq", "plan", "allowed_sha256", "names"}.
    pub fn key_input(&self) -> Value {
        json!({
            "lang": self.lang.as_str(),
            "skill_id": self.skill_id,
            "seq": self.seq,
            "plan": self.plan_text(),
            "allowed_sha256": allowed_sha256(&self.allowed_lemmas),
            "names": self.names,
        })
    }
}

/// What a lesson is built in: the prompt and the course, with the pack sentences a template
/// lesson would get.
pub struct LessonCtx<'a> {
    pub prompt: LessonPrompt,
    pub syllabus: &'a Syllabus,
    pub forms: &'a Forms,
    pub items: &'a [Item],
    pub numbers: &'a [String],
    pub sentences: &'a [Example],
}

/// The AI summary a lesson keeps (`Lesson::ai`): who wrote it and how many of its planned
/// exercises passed L2 to L9.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LessonAiInfo {
    pub provider: ProviderKind,
    pub valid: u32,
    pub planned: u32,
}

/// A schema-valid P1 answer: its exercises as they came, each checked on its own afterwards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LessonResponse {
    pub exercises: Vec<Value>,
}

/// A lesson built from a P1 answer.
#[derive(Debug, Clone, PartialEq)]
pub struct LessonDraft {
    /// Origin Ai when any exercise is Ai, else Template.
    pub lesson: Lesson,
    /// (position in the answer, from 1; the first rule broken).
    pub dropped: Vec<(usize, RuleFailure)>,
    /// FRESH_LESSON_UNAVAILABLE or None.
    pub notice: Option<String>,
    pub cached: bool,
    pub charge_micros: Option<i64>,
}

/// The P1 call: the schema is its only rule (L1), so a schema-valid answer is cached even when
/// some of its exercises are dropped, and the same input never spends twice (§6.1).
pub fn request_lesson(
    client: &dyn AiClient,
    cache: &Cache,
    prompt: &LessonPrompt,
) -> Result<Called<LessonResponse>, AiError> {
    call(
        client,
        cache,
        lesson_prompt_file(),
        &prompt.vars(),
        &prompt.key_input(),
        &|_: &LessonResponse| Ok(()),
    )
}

fn rule(rule: &str, detail: impl Into<String>) -> RuleFailure {
    RuleFailure {
        rule: rule.to_string(),
        detail: detail.into(),
    }
}

/// An answer's exercise as an `Exercise` of the plan entry: L2 and L3, then the body.
fn exercise_of(value: &Value, position: usize, plan: &Plan) -> Result<Exercise, RuleFailure> {
    let Some(entry) = plan.entries.get(position - 1) else {
        return Err(rule("L2", format!("the plan has no entry {position}")));
    };
    let item_id = value.get("item_id").and_then(Value::as_str).unwrap_or("");
    let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
    if item_id != entry.item_id || kind != entry.kind.as_str() {
        return Err(rule(
            "L3",
            format!(
                "entry {position} is {} {}, not {item_id} {kind}",
                entry.item_id,
                entry.kind.as_str()
            ),
        ));
    }
    let mut body = value.clone();
    if let Some(object) = body.as_object_mut() {
        object.remove("item_id");
    }
    let body: Body = serde_json::from_value(body)
        .map_err(|e| rule("L1", format!("the exercise does not read: {e}")))?;
    Ok(Exercise {
        id: format!("e{}", entry.n),
        item_id: entry.item_id.clone(),
        facet: entry.facet,
        origin: Origin::Ai,
        body,
    })
}

/// L7's allowance: duplicates inside `accepted` are removed (the first kept), not failed.
fn dedup_accepted(lang: Lang, ex: &mut Exercise) {
    let accepted = match &mut ex.body {
        Body::TranslateToTarget { accepted, .. }
        | Body::TranslateToEn { accepted, .. }
        | Body::ListenType { accepted, .. } => accepted,
        _ => return,
    };
    let mut seen: HashSet<String> = HashSet::new();
    accepted.retain(|answer| seen.insert(normalise(lang, answer)));
}

/// Builds the lesson from a schema-valid answer, or from none when L1 failed: L2 to L9 per
/// exercise, template fill, the notice. `provider` is the client's, for the lesson's AI summary.
pub fn build_lesson(
    response: Option<&LessonResponse>,
    provider: ProviderKind,
    plan: &Plan,
    ctx: &LessonCtx,
    rng: &mut dyn Rng,
) -> LessonDraft {
    let prompt = &ctx.prompt;
    let lang = prompt.lang;
    let skill_allowed = Allowed::for_skill(ctx.syllabus, ctx.numbers, &prompt.skill_id);
    let plan_items: Vec<String> = plan.entries.iter().map(|e| e.item_id.clone()).collect();
    let template_ctx = TemplateCtx {
        syllabus: ctx.syllabus,
        forms: ctx.forms,
        items: ctx.items,
        allowed: &skill_allowed,
        sentences: ctx.sentences,
        skill_id: &prompt.skill_id,
        plan_items: &plan_items,
    };
    let planned = plan.entries.len();

    // L2 to L9 per exercise, in the order of the answer.
    let allowed = allowed_from_list(ctx.syllabus, ctx.numbers, &prompt.allowed_lemmas);
    let stems = stems_of(lang, &allowed);
    let check = AiCheck {
        lang,
        forms: ctx.forms,
        allowed: &allowed,
        stems: &stems,
        max_words: max_words(prompt.cefr),
    };
    let mut kept: Vec<Option<Exercise>> = vec![None; planned];
    let mut dropped: Vec<(usize, RuleFailure)> = Vec::new();
    let mut seen: Vec<(ExerciseType, String)> = Vec::new();
    for (index, value) in response
        .map_or(&[][..], |r| &r.exercises)
        .iter()
        .enumerate()
    {
        let position = index + 1;
        let checked = exercise_of(value, position, plan).and_then(|mut ex| {
            dedup_accepted(lang, &mut ex);
            let focus = ctx
                .items
                .iter()
                .find(|item| item.id == ex.item_id)
                .map(|item| focus_of(ctx.syllabus, item))
                .unwrap_or_default();
            check_ai_exercise(&ex, &focus, &seen, &check).map(|()| ex)
        });
        match checked {
            Ok(mut ex) => {
                if let Body::TranslateToTarget { target, tiles, .. } = &mut ex.body {
                    *tiles = Some(word_bank(target, lang, &skill_allowed, ctx.forms, rng));
                }
                seen_push(&mut seen, lang, &ex);
                kept[index] = Some(ex);
            }
            Err(failure) => dropped.push((position, failure)),
        }
    }

    let valid = kept.iter().flatten().count();
    // An answer was received (one that failed L1 included), so the lesson counts towards the
    // provider's validity rate.
    let ai = Some(LessonAiInfo {
        provider,
        valid: valid as u32,
        planned: planned as u32,
    });
    let missing = planned - valid;
    if response.is_none() || 2 * missing > planned {
        let mut lesson = template_lesson(plan, &template_ctx, prompt.seq, rng);
        lesson.ai = ai;
        return LessonDraft {
            lesson,
            dropped,
            notice: Some(FRESH_LESSON_UNAVAILABLE.to_string()),
            cached: false,
            charge_micros: None,
        };
    }

    // The lesson in plan order: each entry's AI exercise, or the template exercise checked against
    // the exercises before it.
    let mut exercises: Vec<Exercise> = Vec::new();
    let mut before: Vec<(ExerciseType, String)> = Vec::new();
    for (entry, ai_exercise) in plan.entries.iter().zip(kept) {
        let ex = match ai_exercise {
            Some(ex) => Some(ex),
            None => template_for_entry(entry, &template_ctx, &before, rng),
        };
        if let Some(ex) = ex {
            seen_push(&mut before, lang, &ex);
            exercises.push(ex);
        }
    }
    let origin = if exercises.iter().any(|ex| ex.origin == Origin::Ai) {
        Origin::Ai
    } else {
        Origin::Template
    };
    LessonDraft {
        lesson: Lesson {
            schema: Lesson::SCHEMA.into(),
            lang,
            skill_id: prompt.skill_id.clone(),
            seq: prompt.seq,
            origin,
            exercises,
            ai,
        },
        dropped,
        notice: None,
        cached: false,
        charge_micros: None,
    }
}

/// P1 end to end: ai::call with the schema as the only rule (L1); then L2 to L9 per exercise,
/// template fill, the notice. An answer that fails L1 (AiError::Invalid) gives Ok: the whole
/// template lesson with the notice and nothing cached. Any other AiError is returned as it is.
pub fn generate_lesson(
    client: &dyn AiClient,
    cache: &Cache,
    plan: &Plan,
    ctx: &LessonCtx,
    rng: &mut dyn Rng,
) -> Result<LessonDraft, AiError> {
    match request_lesson(client, cache, &ctx.prompt) {
        Ok(called) => {
            let mut draft = build_lesson(Some(&called.value), client.kind(), plan, ctx, rng);
            draft.cached = called.cached;
            draft.charge_micros = called.charge_micros;
            Ok(draft)
        }
        Err(AiError::Invalid) => Ok(build_lesson(None, client.kind(), plan, ctx, rng)),
        Err(other) => Err(other),
    }
}

/// The validity rate of Settings > AI > Local model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LessonChecks {
    pub lessons: u32,
    pub valid: u32,
    pub planned: u32,
    pub message: Option<String>,
}

/// The most recent lessons the rate covers.
const CHECKS_WINDOW: usize = 10;

/// Over `recent` (newest first, at most 10): "Local model: {p}% of AI exercises passed Parley's
/// checks (last {n} lessons)." (`(last lesson)` when n is 1), p rounded half up in integers; no
/// message when there is no lesson or nothing was planned.
pub fn lesson_checks(recent: &[LessonAiInfo]) -> LessonChecks {
    let window = &recent[..recent.len().min(CHECKS_WINDOW)];
    let lessons = window.len() as u32;
    let valid: u32 = window.iter().map(|info| info.valid).sum();
    let planned: u32 = window.iter().map(|info| info.planned).sum();
    let message = (lessons > 0 && planned > 0).then(|| {
        let percent = (200 * u64::from(valid) + u64::from(planned)) / (2 * u64::from(planned));
        let last = if lessons == 1 {
            "last lesson".to_string()
        } else {
            format!("last {lessons} lessons")
        };
        format!("Local model: {percent}% of AI exercises passed Parley's checks ({last}).")
    });
    LessonChecks {
        lessons,
        valid,
        planned,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rate_rounds_half_up() {
        let info = |valid, planned| LessonAiInfo {
            provider: ProviderKind::Local,
            valid,
            planned,
        };
        assert_eq!(
            lesson_checks(&[info(1, 8)]).message.as_deref(),
            Some("Local model: 13% of AI exercises passed Parley's checks (last lesson).")
        );
        assert_eq!(lesson_checks(&[info(0, 0)]).message, None);
    }

    #[test]
    fn the_lesson_prompt_file_renders_every_placeholder() {
        let prompt = LessonPrompt {
            lang: Lang::Fr,
            cefr: Cefr::A2,
            skill_id: "fr.a2.u01.s01".into(),
            seq: 3,
            skill_title: "Travel".into(),
            can_do: "I can buy a ticket.".into(),
            grammar_note: None,
            variety: "Standard French of France".into(),
            allowed_lemmas: vec!["le".into(), "train".into()],
            names: vec!["Paul".into()],
            plan: vec![PromptEntry {
                n: 1,
                item_id: "fr.a2.u01.s01#l:train".into(),
                kind: ExerciseType::PickMeaning,
                focus: "le train".into(),
                gloss: Some("train".into()),
                new: false,
            }],
        };
        let messages = lesson_prompt_file().render(&prompt.vars()).unwrap();
        let system = messages[0].content.as_str().unwrap();
        assert!(system.contains("The target language is French. The learner's level is A2."));
        assert!(system.contains("at most 12 words"));
        let user = messages[1].content.as_str().unwrap();
        assert!(user.contains("GRAMMAR NOTE: none\n"));
        assert!(user.ends_with(
            "1. item_id=fr.a2.u01.s01#l:train type=pick_meaning focus=\"le train\" (train) new=false"
        ));
    }
}
