//! The template generator: every exercise type from the syllabus and sentence packs, without AI,
//! deterministic for a seed (docs/modules/03-exercise-engine-and-grading.md, Design notes). It is
//! the offline course, and fills the gaps an AI lesson leaves.

use super::check::{Allowed, CheckCtx, check_exercise, max_words};
use super::compose::{Plan, PlanEntry};
use super::types::{Body, Exercise, ExerciseType, Lesson, Origin};
use super::wordbank::word_bank;
use crate::content::{
    Cefr, Example, Forms, Item, ItemKind, Lang, Lemma, Syllabus, TaughtForms, Token, nfc, tokenise,
};
use crate::env::{Rng, below, shuffle};
use crate::grading::normalise;
use crate::grading::normalise::words;

/// What the generator draws on.
#[derive(Clone, Copy, Debug)]
pub struct TemplateCtx<'a> {
    pub syllabus: &'a Syllabus,
    pub forms: &'a Forms,
    /// Every item of the syllabus (`derive_items`), for distractors and pairs.
    pub items: &'a [Item],
    pub allowed: &'a Allowed,
    /// Sentence-pack sentences a `fill_gap` may use, besides the syllabus examples.
    pub sentences: &'a [Example],
    /// The lesson's skill.
    pub skill_id: &'a str,
    /// The item ids of the lesson plan, in order.
    pub plan_items: &'a [String],
}

impl TemplateCtx<'_> {
    /// The position of a skill in syllabus order.
    fn skill_position(&self, skill_id: &str) -> Option<usize> {
        self.syllabus
            .skills()
            .position(|(_, skill)| skill.id == skill_id)
    }

    /// The CEFR level of the lesson's skill, which sets the longest main text.
    fn level(&self) -> Option<Cefr> {
        self.syllabus
            .skills()
            .find(|(_, skill)| skill.id == self.skill_id)
            .map(|(_, skill)| skill.cefr)
    }

    /// The syllabus lemma written `text` (its first occurrence).
    fn lemma(&self, text: &str) -> Option<&Lemma> {
        let text = nfc(text);
        self.syllabus
            .skills()
            .flat_map(|(_, skill)| skill.lemmas.iter())
            .find(|lemma| nfc(&lemma.lemma) == text)
    }
}

/// Vocabulary goes with vocabulary; phrases and patterns go together.
fn same_group(a: ItemKind, b: ItemKind) -> bool {
    (a == ItemKind::Vocab) == (b == ItemKind::Vocab)
}

/// Three distractors and the gloss, shuffled: the first glosses of other items of the same group
/// in the item's skill, then in earlier skills, never equal (normal form) to an accepted English
/// answer of the item or to each other; up to 3 at random from the same skill, the rest at
/// random from earlier skills. Fewer than 3 in all: none.
fn pick_meaning(item: &Item, ctx: &TemplateCtx, rng: &mut dyn Rng) -> Option<Body> {
    let gloss = item.en.first()?;
    let lang = item.lang;
    let own = ctx.skill_position(&item.skill)?;
    let mut taken: Vec<String> = item
        .accepted
        .to_en
        .iter()
        .chain(&item.en)
        .map(|en| normalise(lang, en))
        .collect();
    let mut pool = |earlier: bool| -> Vec<String> {
        let mut found = Vec::new();
        for other in ctx.items {
            if other.id == item.id || !same_group(other.kind, item.kind) {
                continue;
            }
            let Some(position) = ctx.skill_position(&other.skill) else {
                continue;
            };
            let wanted = if earlier {
                position < own
            } else {
                position == own
            };
            let Some(first) = other.en.first() else {
                continue;
            };
            let norm = normalise(lang, first);
            if wanted && !norm.is_empty() && !taken.contains(&norm) {
                taken.push(norm);
                found.push(first.clone());
            }
        }
        found
    };
    let mut same = pool(false);
    let mut earlier = pool(true);
    if same.len() + earlier.len() < 3 {
        return None;
    }
    shuffle(&mut same, rng);
    same.truncate(3);
    let rest = 3 - same.len();
    if rest > 0 {
        shuffle(&mut earlier, rng);
        earlier.truncate(rest);
    } else {
        earlier.clear();
    }
    let mut options: Vec<String> = vec![gloss.clone()];
    options.extend(same);
    options.extend(earlier);
    shuffle(&mut options, rng);
    let answer_index = options.iter().position(|option| option == gloss)? as u32;
    Some(Body::PickMeaning {
        prompt_target: item.target.clone(),
        options,
        answer_index,
    })
}

/// The byte offset of `tokens[index]` in `text`: tokens come in text order and each surface is a
/// substring of the text, so each is found after the one before it.
fn token_offset(text: &str, tokens: &[Token], index: usize) -> Option<usize> {
    let mut cursor = 0;
    for (i, token) in tokens.iter().enumerate() {
        let at = cursor + text[cursor..].find(&token.surface)?;
        if i == index {
            return Some(at);
        }
        cursor = at + token.surface.len();
    }
    None
}

/// A sentence with a form of the item's lemma gapped: one at random among the examples of every
/// skill up to and including the lesson's and the pack sentences that contain a form of the
/// lemma, whose tokens are all allowed and that have at most `max_words` words. The first such
/// token becomes `___`. Single-word vocabulary lemmas only.
fn fill_gap(item: &Item, ctx: &TemplateCtx, rng: &mut dyn Rng) -> Option<Body> {
    if item.kind != ItemKind::Vocab {
        return None;
    }
    let lemma_text = item.lemmas.first()?;
    if lemma_text.chars().any(char::is_whitespace) {
        return None;
    }
    let taught = TaughtForms::of(ctx.lemma(lemma_text)?);
    let limit = max_words(ctx.level().unwrap_or(item.cefr));
    let lang = item.lang;
    let examples: Vec<&str> = match ctx.skill_position(ctx.skill_id) {
        Some(last) => ctx
            .syllabus
            .skills()
            .take(last + 1)
            .flat_map(|(_, skill)| skill.examples.iter())
            .map(|example| example.target.as_str())
            .collect(),
        None => Vec::new(),
    };
    let candidates: Vec<(&str, Vec<Token>)> = examples
        .into_iter()
        .chain(ctx.sentences.iter().map(|example| example.target.as_str()))
        .filter_map(|text| {
            let tokens = tokenise(lang, text, ctx.forms);
            let usable = tokens.iter().any(|token| taught.matches(token))
                && tokens.iter().all(|token| ctx.allowed.allows(token))
                && words(&normalise(lang, text)).len() <= limit;
            usable.then_some((text, tokens))
        })
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let (text, tokens) = &candidates[below(rng, candidates.len())];
    let index = tokens.iter().position(|token| taught.matches(token))?;
    let surface = &tokens[index].surface;
    let at = token_offset(text, tokens, index)?;
    let sentence = format!("{}___{}", &text[..at], &text[at + surface.len()..]);
    Some(Body::FillGap {
        sentence,
        answer: surface.clone(),
        full_sentence: text.to_string(),
        hint_en: item.en.first()?.clone(),
    })
}

/// Four pairs: the item and the next vocabulary items of the plan, with distinct targets and
/// glosses, in random order. Fewer than 4: none.
fn match_pairs(item: &Item, ctx: &TemplateCtx, rng: &mut dyn Rng) -> Option<Body> {
    const PAIRS: usize = 4;
    if item.kind != ItemKind::Vocab {
        return None;
    }
    let lang = item.lang;
    let start = ctx
        .plan_items
        .iter()
        .position(|id| *id == item.id)
        .map_or(0, |i| i + 1);
    let mut pairs: Vec<[String; 2]> = Vec::new();
    let mut seen: Vec<(String, String)> = Vec::new();
    let mut add = |candidate: &Item, pairs: &mut Vec<[String; 2]>| {
        let Some(gloss) = candidate.en.first() else {
            return;
        };
        let target = normalise(lang, &candidate.target);
        let english = normalise(lang, gloss);
        if target.is_empty() || english.is_empty() {
            return;
        }
        if seen.iter().any(|(t, e)| *t == target || *e == english) {
            return;
        }
        seen.push((target, english));
        pairs.push([candidate.target.clone(), gloss.clone()]);
    };
    add(item, &mut pairs);
    for id in &ctx.plan_items[start.min(ctx.plan_items.len())..] {
        if pairs.len() == PAIRS {
            break;
        }
        if let Some(other) = ctx
            .items
            .iter()
            .find(|other| other.id == *id && other.kind == ItemKind::Vocab)
        {
            add(other, &mut pairs);
        }
    }
    if pairs.len() < PAIRS {
        return None;
    }
    shuffle(&mut pairs, rng);
    Some(Body::MatchPairs { pairs })
}

/// One template exercise of `kind` for `item`, or none when the item cannot give one (no
/// distractors, no sentence, too few pairs). Its facet is the type's; its id is left empty for
/// the lesson to number.
pub fn template(
    item: &Item,
    kind: ExerciseType,
    ctx: &TemplateCtx,
    rng: &mut dyn Rng,
) -> Option<Exercise> {
    let gloss = item.en.first();
    let body = match kind {
        ExerciseType::PickMeaning => pick_meaning(item, ctx, rng)?,
        ExerciseType::TranslateToTarget => Body::TranslateToTarget {
            prompt_en: gloss?.clone(),
            target: item.target.clone(),
            accepted: item.accepted.to_target.clone(),
            tiles: Some(word_bank(
                &item.target,
                item.lang,
                ctx.allowed,
                ctx.forms,
                rng,
            )),
        },
        ExerciseType::TranslateToEn => Body::TranslateToEn {
            prompt_target: item.target.clone(),
            accepted: item.accepted.to_en.clone(),
        },
        ExerciseType::ListenType => Body::ListenType {
            text: item.target.clone(),
            accepted: item.accepted.to_target.clone(),
        },
        ExerciseType::Speak => Body::Speak {
            text: item.target.clone(),
            prompt_en: gloss?.clone(),
        },
        ExerciseType::FillGap => fill_gap(item, ctx, rng)?,
        ExerciseType::MatchPairs => match_pairs(item, ctx, rng)?,
    };
    Some(Exercise {
        id: String::new(),
        item_id: item.id.clone(),
        facet: kind.facet(),
        origin: Origin::Template,
        body,
    })
}

/// The types tried for a plan entry: its own, then its fallbacks.
fn attempts(kind: ExerciseType) -> &'static [ExerciseType] {
    use ExerciseType::*;
    match kind {
        MatchPairs => &[MatchPairs, PickMeaning, TranslateToEn],
        PickMeaning => &[PickMeaning, TranslateToEn],
        FillGap => &[FillGap, TranslateToTarget],
        ListenType => &[ListenType, TranslateToEn],
        Speak => &[Speak, TranslateToTarget],
        TranslateToTarget => &[TranslateToTarget],
        TranslateToEn => &[TranslateToEn],
    }
}

/// The template lesson of a plan: one exercise per entry, id `e{n}`, the entry's type tried
/// first and then its fallbacks (`match_pairs` → `pick_meaning` → `translate_to_en`;
/// `pick_meaning` → `translate_to_en`; `fill_gap` → `translate_to_target`; `listen_type` →
/// `translate_to_en`; `speak` → `translate_to_target`), keeping the first that passes
/// `check_exercise` with the entry's item's lemmas as focus and the lesson's earlier exercises as
/// `seen`. An entry with none is dropped. `max_words` comes from the lesson skill's level.
pub fn template_lesson(plan: &Plan, ctx: &TemplateCtx, seq: u32, rng: &mut dyn Rng) -> Lesson {
    let mut exercises: Vec<Exercise> = Vec::new();
    let mut seen: Vec<(ExerciseType, String)> = Vec::new();
    for entry in &plan.entries {
        if let Some(ex) = template_for_entry(entry, ctx, &seen, rng) {
            seen_push(&mut seen, ctx.syllabus.lang, &ex);
            exercises.push(ex);
        }
    }
    Lesson {
        schema: Lesson::SCHEMA.to_string(),
        lang: ctx.syllabus.lang,
        skill_id: ctx.skill_id.to_string(),
        seq,
        origin: Origin::Template,
        exercises,
        ai: None,
    }
}

/// Adds an exercise's type and normalised first main text to `seen` (L7's memory).
pub(crate) fn seen_push(seen: &mut Vec<(ExerciseType, String)>, lang: Lang, ex: &Exercise) {
    if let Some(first) = ex.targets().first() {
        seen.push((ex.kind(), normalise(lang, first)));
    }
}

/// The lemmas of the syllabus an item practises, as L9's focus.
pub(crate) fn focus_of(syllabus: &Syllabus, item: &Item) -> Vec<Lemma> {
    item.lemmas
        .iter()
        .filter_map(|lemma| {
            let text = nfc(lemma);
            syllabus
                .skills()
                .flat_map(|(_, skill)| skill.lemmas.iter())
                .find(|found| nfc(&found.lemma) == text)
                .cloned()
        })
        .collect()
}

/// The exercise `template_lesson` gives one plan entry after the exercises of `seen`: the entry's
/// type, then its fallbacks, the first that passes `check_exercise`; id `e{n}` and the entry's
/// facet. None when the item is unknown or no type gives a passing exercise.
pub(crate) fn template_for_entry(
    entry: &PlanEntry,
    ctx: &TemplateCtx,
    seen: &[(ExerciseType, String)],
    rng: &mut dyn Rng,
) -> Option<Exercise> {
    let item = ctx.items.iter().find(|item| item.id == entry.item_id)?;
    let limit = max_words(ctx.level().unwrap_or(Cefr::A1));
    let focus = focus_of(ctx.syllabus, item);
    let check = CheckCtx {
        lang: ctx.syllabus.lang,
        forms: ctx.forms,
        allowed: ctx.allowed,
        max_words: limit,
        focus: &focus,
        seen,
    };
    let mut ex = attempts(entry.kind).iter().find_map(|&kind| {
        template(item, kind, ctx, rng).filter(|ex| check_exercise(ex, &check).is_ok())
    })?;
    ex.id = format!("e{}", entry.n);
    ex.facet = entry.facet;
    Some(ex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::test_support::{EXAMPLE_SYLLABUS, example_forms};
    use crate::content::{Lang, derive_items, parse_syllabus, schema_errors};
    use crate::env::SplitMix64;
    use crate::exercise::compose::{Capabilities, ComposeRequest, compose};
    use crate::srs::Facet;
    use crate::store::Store;

    const SKILL: &str = "es.a1.u01.s01";

    struct Fixture {
        syllabus: Syllabus,
        forms: Forms,
        items: Vec<Item>,
        allowed: Allowed,
    }

    fn fixture() -> Fixture {
        let syllabus = parse_syllabus("valid-es.yaml", EXAMPLE_SYLLABUS).unwrap();
        let forms = example_forms();
        let items = derive_items(&syllabus, &forms);
        let allowed = Allowed::for_skill(&syllabus, &[], SKILL);
        Fixture {
            syllabus,
            forms,
            items,
            allowed,
        }
    }

    impl Fixture {
        fn ctx<'a>(&'a self, plan_items: &'a [String]) -> TemplateCtx<'a> {
            TemplateCtx {
                syllabus: &self.syllabus,
                forms: &self.forms,
                items: &self.items,
                allowed: &self.allowed,
                sentences: &[],
                skill_id: SKILL,
                plan_items,
            }
        }

        fn item(&self, short: &str) -> &Item {
            let id = format!("{SKILL}#{short}");
            self.items.iter().find(|item| item.id == id).unwrap()
        }
    }

    #[test]
    fn pick_meaning_never_offers_an_accepted_gloss_twice() {
        let f = fixture();
        let item = f.item("l:gracias");
        for seed in 0..50 {
            let ex = template(
                item,
                ExerciseType::PickMeaning,
                &f.ctx(&[]),
                &mut SplitMix64::new(seed),
            )
            .unwrap();
            let Body::PickMeaning {
                prompt_target,
                options,
                answer_index,
            } = &ex.body
            else {
                panic!("{ex:?}");
            };
            assert_eq!(prompt_target, "gracias");
            assert_eq!(options[*answer_index as usize], "thank you");
            let others: Vec<&String> = options
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != *answer_index as usize)
                .map(|(_, o)| o)
                .collect();
            assert_eq!(others.len(), 3);
            for other in others {
                assert!(other != "thank you" && other != "thanks", "{options:?}");
            }
            assert_eq!(ex.facet, Facet::Receptive);
        }
        // A phrase takes its distractors from the skill's three other phrases.
        let phrase = f.item("e:1");
        let ex = template(
            phrase,
            ExerciseType::PickMeaning,
            &f.ctx(&[]),
            &mut SplitMix64::new(0),
        )
        .unwrap();
        let Body::PickMeaning { mut options, .. } = ex.body else {
            panic!()
        };
        options.sort();
        assert_eq!(
            options,
            [
                "Good morning.",
                "Good night, Ana.",
                "Goodbye, thank you.",
                "Hi, how are you?"
            ]
        );
        let mut first = f.items.clone();
        first.retain(|item| item.kind == ItemKind::Vocab && item.id != f.item("l:hola").id);
        first.truncate(2);
        let few = TemplateCtx {
            items: &first,
            ..f.ctx(&[])
        };
        assert!(
            template(
                f.item("l:hola"),
                ExerciseType::PickMeaning,
                &few,
                &mut SplitMix64::new(0)
            )
            .is_none(),
            "fewer than three distractors"
        );
    }

    #[test]
    fn fill_gap_blanks_the_first_form_of_the_lemma() {
        let f = fixture();
        let ex = template(
            f.item("l:día"),
            ExerciseType::FillGap,
            &f.ctx(&[]),
            &mut SplitMix64::new(3),
        )
        .unwrap();
        assert_eq!(
            ex.body,
            Body::FillGap {
                sentence: "Buenos ___.".into(),
                answer: "días".into(),
                full_sentence: "Buenos días.".into(),
                hint_en: "day".into(),
            }
        );
        assert_eq!(ex.facet, Facet::Productive);
        let none = template(
            f.item("e:1"),
            ExerciseType::FillGap,
            &f.ctx(&[]),
            &mut SplitMix64::new(3),
        );
        assert!(none.is_none(), "phrases have no gap");
        let ex = template(
            f.item("l:hola"),
            ExerciseType::FillGap,
            &f.ctx(&[]),
            &mut SplitMix64::new(3),
        )
        .unwrap();
        let Body::FillGap {
            sentence, answer, ..
        } = &ex.body
        else {
            panic!()
        };
        assert_eq!(
            (sentence.as_str(), answer.as_str()),
            ("___, ¿qué tal?", "Hola")
        );
    }

    #[test]
    fn match_pairs_takes_the_next_words_of_the_plan() {
        let f = fixture();
        let ids: Vec<String> = [
            "l:hola", "l:adiós", "e:1", "l:hola", "l:bueno", "l:día", "l:noche",
        ]
        .iter()
        .map(|s| format!("{SKILL}#{s}"))
        .collect();
        let ex = template(
            f.item("l:hola"),
            ExerciseType::MatchPairs,
            &f.ctx(&ids),
            &mut SplitMix64::new(1),
        )
        .unwrap();
        let Body::MatchPairs { pairs } = &ex.body else {
            panic!()
        };
        let mut targets: Vec<&str> = pairs.iter().map(|p| p[0].as_str()).collect();
        targets.sort();
        assert_eq!(targets, ["adiós", "bueno", "el día", "hola"]);
        assert!(pairs.contains(&["el día".to_string(), "day".to_string()]));
        let short: Vec<String> = ids[..3].to_vec();
        let none = template(
            f.item("l:hola"),
            ExerciseType::MatchPairs,
            &f.ctx(&short),
            &mut SplitMix64::new(1),
        );
        assert!(none.is_none(), "fewer than 4 words");
    }

    #[test]
    fn a_template_lesson_numbers_its_exercises_and_matches_the_schema() {
        let f = fixture();
        let mut store = Store::open_in_memory().unwrap();
        store.start_course(&f.syllabus, &f.forms, 0).unwrap();
        let request = ComposeRequest {
            lang: Lang::Es,
            skill_id: SKILL.into(),
            lesson_length: 12,
            new_items_per_lesson: 5,
            capabilities: Capabilities::default(),
        };
        let plan = compose(&request, &store, 0).unwrap();
        let ids: Vec<String> = plan.entries.iter().map(|e| e.item_id.clone()).collect();
        let lesson = template_lesson(&plan, &f.ctx(&ids), 1, &mut SplitMix64::new(9));
        assert_eq!(lesson.exercises.len(), plan.entries.len());
        for (ex, entry) in lesson.exercises.iter().zip(&plan.entries) {
            assert_eq!(ex.id, format!("e{}", entry.n));
            assert_eq!(
                (ex.item_id.as_str(), ex.facet),
                (entry.item_id.as_str(), entry.facet)
            );
        }
        assert_eq!(lesson.exercises[5].kind(), ExerciseType::MatchPairs);
        let again = template_lesson(&plan, &f.ctx(&ids), 1, &mut SplitMix64::new(9));
        assert_eq!(lesson, again);
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../../schemas/lesson.schema.json")).unwrap();
        let validator = jsonschema::validator_for(&schema).unwrap();
        let value = serde_json::to_value(&lesson).unwrap();
        assert_eq!(schema_errors(&validator, &value), Vec::<String>::new());
        let mut broken = value.clone();
        broken["exercises"][0]["options"] = serde_json::json!(["a", "b"]);
        assert!(!schema_errors(&validator, &broken).is_empty());
        let mut broken = value;
        broken["exercises"][0]["extra"] = serde_json::json!(1);
        assert!(!schema_errors(&validator, &broken).is_empty());
    }

    #[test]
    fn an_entry_falls_back_to_a_type_that_passes() {
        let f = fixture();
        let plan = Plan {
            entries: vec![crate::exercise::compose::PlanEntry {
                n: 1,
                item_id: format!("{SKILL}#l:hola"),
                facet: Facet::Receptive,
                kind: ExerciseType::MatchPairs,
                focus: "hola".into(),
                new: true,
            }],
        };
        let ids = vec![format!("{SKILL}#l:hola")];
        let lesson = template_lesson(&plan, &f.ctx(&ids), 1, &mut SplitMix64::new(0));
        assert_eq!(lesson.exercises.len(), 1);
        assert_eq!(lesson.exercises[0].kind(), ExerciseType::PickMeaning);
    }
}
