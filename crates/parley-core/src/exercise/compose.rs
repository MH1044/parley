//! The lesson composer: which items a lesson practises, in which order, and with which exercise
//! type (docs/modules/03-exercise-engine-and-grading.md, Design notes). Deterministic: the plan
//! depends only on the store and `now_ms`.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::types::ExerciseType;
use crate::content::{ItemKind, Lang};
use crate::grading::normalise;
use crate::grading::normalise::words;
use crate::srs::{Facet, due_queue, retrievability};
use crate::store::{ItemRow, Store, StoreError};

/// What the device can do: play audio, record from a microphone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Capabilities {
    pub audio_out: bool,
    pub mic: bool,
}

/// What a lesson is composed for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeRequest {
    pub lang: Lang,
    pub skill_id: String,
    /// The most entries of the plan (the `lesson_length` setting).
    pub lesson_length: usize,
    /// The most items the lesson introduces (the `new_items_per_lesson` setting).
    pub new_items_per_lesson: usize,
    pub capabilities: Capabilities,
}

/// One planned exercise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanEntry {
    /// The entry's place in the plan, from 1.
    pub n: u32,
    pub item_id: String,
    pub facet: Facet,
    pub kind: ExerciseType,
    /// The item's target.
    pub focus: String,
    /// The item is introduced by this lesson.
    pub new: bool,
}

/// A lesson plan.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Plan {
    pub entries: Vec<PlanEntry>,
}

/// How many entries a capability turns into its audio type.
const AUDIO_ENTRIES: usize = 2;
/// The longest focus, in words, a `speak` entry may have.
const SPEAK_MAX_WORDS: usize = 8;
/// The fewest distinct vocabulary items a `match_pairs` entry needs.
const PAIR_ITEMS: usize = 4;

/// The exercise type for a card: receptive vocabulary `pick_meaning`; receptive phrase or
/// pattern `translate_to_en`; productive vocabulary `fill_gap`, or `translate_to_target` when
/// its lemma has a space; productive phrase or pattern `translate_to_target`.
fn kind_for(item: &ItemRow, facet: Facet) -> ExerciseType {
    match (item.kind, facet) {
        (ItemKind::Vocab, Facet::Receptive) => ExerciseType::PickMeaning,
        (_, Facet::Receptive) => ExerciseType::TranslateToEn,
        (ItemKind::Vocab, Facet::Productive) => {
            let multiword = item
                .lemmas
                .first()
                .is_some_and(|lemma| lemma.chars().any(char::is_whitespace));
            if multiword {
                ExerciseType::TranslateToTarget
            } else {
                ExerciseType::FillGap
            }
        }
        (_, Facet::Productive) => ExerciseType::TranslateToTarget,
    }
}

/// The store's items, read once each.
struct Items<'a> {
    store: &'a Store,
    rows: HashMap<String, ItemRow>,
}

impl Items<'_> {
    fn get(&mut self, id: &str) -> Result<&ItemRow, StoreError> {
        if !self.rows.contains_key(id) {
            let row = self.store.item(id)?;
            self.rows.insert(id.to_string(), row);
        }
        Ok(&self.rows[id])
    }
}

/// A plan entry before numbering.
struct Draft {
    item_id: String,
    facet: Facet,
    kind: ExerciseType,
    focus: String,
    new: bool,
}

/// The plan of a lesson of `req.skill_id` at `now_ms`:
///
/// 1. New items: the skill's first `new_items_per_lesson` items (syllabus order) with no cards,
///    each with its receptive intro, in order.
/// 2. When at least 4 distinct vocabulary items are among the new items and the due cards'
///    items, a receptive `match_pairs` entry for the first vocabulary item among the new items,
///    then among the due cards in queue order.
/// 3. The due cards (`due_queue`, at most `lesson_length`), alternating with the productive
///    entries of the new items (in order) until both run out.
/// 4. Fill: the cards of the skill's introduced items not already planned as (item, facet), by
///    retrievability at `now_ms` ascending, then the item's syllabus position, receptive first.
///
/// The plan stops at `lesson_length` entries. With `audio_out` the first 2 `translate_to_en`
/// entries become `listen_type`; with `mic` the first 2 `translate_to_target` entries of phrase or
/// pattern items whose focus has at most 8 words become `speak`. An entry is `new` when its item
/// is one of the lesson's new items.
pub fn compose(req: &ComposeRequest, store: &Store, now_ms: i64) -> Result<Plan, StoreError> {
    let mut items = Items {
        store,
        rows: HashMap::new(),
    };
    let skill_items = store.item_ids(&req.skill_id)?;
    let mut new_items: Vec<String> = Vec::new();
    for id in &skill_items {
        if new_items.len() == req.new_items_per_lesson {
            break;
        }
        if store.card(id, Facet::Receptive)?.is_none()
            && store.card(id, Facet::Productive)?.is_none()
        {
            new_items.push(id.clone());
        }
    }
    let due = due_queue(store, req.lang, now_ms, req.lesson_length)?;

    let mut drafts: Vec<Draft> = Vec::new();
    let draft = |items: &mut Items, id: &str, facet: Facet, kind: Option<ExerciseType>| {
        let row = items.get(id)?;
        let new = new_items.iter().any(|n| n == id);
        Ok::<Draft, StoreError>(Draft {
            item_id: id.to_string(),
            facet,
            kind: kind.unwrap_or_else(|| kind_for(row, facet)),
            focus: row.target.clone(),
            new,
        })
    };

    // 1. The receptive intros of the new items.
    for id in &new_items {
        drafts.push(draft(&mut items, id, Facet::Receptive, None)?);
    }

    // 2. A match_pairs entry when there are enough vocabulary items.
    let mut vocab: Vec<String> = Vec::new();
    for id in new_items.iter().chain(due.iter().map(|card| &card.item_id)) {
        if items.get(id)?.kind == ItemKind::Vocab && !vocab.contains(id) {
            vocab.push(id.clone());
        }
    }
    if vocab.len() >= PAIR_ITEMS {
        drafts.push(draft(
            &mut items,
            &vocab[0],
            Facet::Receptive,
            Some(ExerciseType::MatchPairs),
        )?);
    }

    // 3. Due cards, alternating with the productive entries of the new items.
    let mut due_iter = due.iter();
    let mut new_iter = new_items.iter();
    loop {
        let card = due_iter.next();
        if let Some(card) = card {
            drafts.push(draft(&mut items, &card.item_id, card.facet, None)?);
        }
        let id = new_iter.next();
        if let Some(id) = id {
            drafts.push(draft(&mut items, id, Facet::Productive, None)?);
        }
        if card.is_none() && id.is_none() {
            break;
        }
    }

    // 4. Fill with the skill's other introduced cards, the least remembered first.
    let planned: HashSet<(String, Facet)> = drafts
        .iter()
        .map(|d| (d.item_id.clone(), d.facet))
        .collect();
    let mut fill: Vec<(f64, usize, Facet, String)> = Vec::new();
    for (position, id) in skill_items.iter().enumerate() {
        for facet in Facet::ALL {
            if planned.contains(&(id.clone(), facet)) {
                continue;
            }
            if let Some(card) = store.card(id, facet)? {
                fill.push((retrievability(&card, now_ms), position, facet, id.clone()));
            }
        }
    }
    fill.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    for (_, _, facet, id) in &fill {
        if drafts.len() >= req.lesson_length {
            break;
        }
        drafts.push(draft(&mut items, id, *facet, None)?);
    }
    drafts.truncate(req.lesson_length);

    // Capabilities.
    if req.capabilities.audio_out {
        for d in drafts
            .iter_mut()
            .filter(|d| d.kind == ExerciseType::TranslateToEn)
            .take(AUDIO_ENTRIES)
        {
            d.kind = ExerciseType::ListenType;
        }
    }
    if req.capabilities.mic {
        let mut turned = 0;
        for d in drafts.iter_mut() {
            if turned == AUDIO_ENTRIES {
                break;
            }
            if d.kind != ExerciseType::TranslateToTarget {
                continue;
            }
            let phrase = items.get(&d.item_id)?.kind != ItemKind::Vocab;
            if phrase && words(&normalise(req.lang, &d.focus)).len() <= SPEAK_MAX_WORDS {
                d.kind = ExerciseType::Speak;
                turned += 1;
            }
        }
    }

    Ok(Plan {
        entries: drafts
            .into_iter()
            .enumerate()
            .map(|(i, d)| PlanEntry {
                n: i as u32 + 1,
                item_id: d.item_id,
                facet: d.facet,
                kind: d.kind,
                focus: d.focus,
                new: d.new,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::parse_syllabus;
    use crate::content::test_support::{EXAMPLE_SYLLABUS, example_forms};
    use crate::srs::{DAY_MS, Rating, SrsParams, Verdict};
    use crate::store::ReviewContext;

    const SKILL: &str = "es.a1.u01.s01";

    fn started() -> Store {
        let mut store = Store::open_in_memory().unwrap();
        let syllabus = parse_syllabus("valid-es.yaml", EXAMPLE_SYLLABUS).unwrap();
        store.start_course(&syllabus, &example_forms(), 0).unwrap();
        store
    }

    fn request(audio_out: bool, mic: bool) -> ComposeRequest {
        ComposeRequest {
            lang: Lang::Es,
            skill_id: SKILL.into(),
            lesson_length: 12,
            new_items_per_lesson: 5,
            capabilities: Capabilities { audio_out, mic },
        }
    }

    /// (item, facet, kind, new) of each entry, the item without its skill prefix.
    fn summary(plan: &Plan) -> Vec<(String, Facet, ExerciseType, bool)> {
        plan.entries
            .iter()
            .map(|e| {
                let short = e.item_id.trim_start_matches("es.a1.u01.s01#").to_string();
                (short, e.facet, e.kind, e.new)
            })
            .collect()
    }

    fn entry(
        id: &str,
        facet: Facet,
        kind: ExerciseType,
        new: bool,
    ) -> (String, Facet, ExerciseType, bool) {
        (id.to_string(), facet, kind, new)
    }

    #[test]
    fn a_first_lesson_introduces_five_words_with_pairs() {
        use ExerciseType::*;
        use Facet::*;
        let store = started();
        let plan = compose(&request(false, false), &store, 0).unwrap();
        assert_eq!(
            summary(&plan),
            [
                entry("l:hola", Receptive, PickMeaning, true),
                entry("l:adiós", Receptive, PickMeaning, true),
                entry("l:bueno", Receptive, PickMeaning, true),
                entry("l:día", Receptive, PickMeaning, true),
                entry("l:noche", Receptive, PickMeaning, true),
                entry("l:hola", Receptive, MatchPairs, true),
                entry("l:hola", Productive, FillGap, true),
                entry("l:adiós", Productive, FillGap, true),
                entry("l:bueno", Productive, FillGap, true),
                entry("l:día", Productive, FillGap, true),
                entry("l:noche", Productive, FillGap, true),
            ]
        );
        let ns: Vec<u32> = plan.entries.iter().map(|e| e.n).collect();
        assert_eq!(ns, (1..=11).collect::<Vec<u32>>());
        assert_eq!(plan.entries[3].focus, "el día");
        assert_eq!(compose(&request(true, true), &store, 0).unwrap(), plan);
    }

    #[test]
    fn a_second_lesson_fills_with_the_skill_s_cards_and_audio_turns_two_entries() {
        use ExerciseType::*;
        use Facet::*;
        let mut store = started();
        for id in store.item_ids(SKILL).unwrap().iter().take(5) {
            store.introduce(id, 0).unwrap();
        }
        let plan = compose(&request(false, false), &store, 0).unwrap();
        let expected = vec![
            entry("l:tal", Receptive, PickMeaning, true),
            entry("l:gracias", Receptive, PickMeaning, true),
            entry("e:1", Receptive, TranslateToEn, true),
            entry("e:2", Receptive, TranslateToEn, true),
            entry("e:3", Receptive, TranslateToEn, true),
            entry("l:tal", Productive, FillGap, true),
            entry("l:gracias", Productive, FillGap, true),
            entry("e:1", Productive, TranslateToTarget, true),
            entry("e:2", Productive, TranslateToTarget, true),
            entry("e:3", Productive, TranslateToTarget, true),
            entry("l:hola", Receptive, PickMeaning, false),
            entry("l:hola", Productive, FillGap, false),
        ];
        assert_eq!(summary(&plan), expected);
        let both = compose(&request(true, true), &store, 0).unwrap();
        let mut turned = expected;
        turned[2].2 = ListenType;
        turned[3].2 = ListenType;
        turned[7].2 = Speak;
        turned[8].2 = Speak;
        assert_eq!(summary(&both), turned);
    }

    #[test]
    fn due_cards_alternate_with_the_new_productive_entries() {
        use ExerciseType::*;
        use Facet::*;
        let mut store = started();
        let ctx = ReviewContext {
            exercise_type: "pick_meaning".into(),
            verdict: Verdict::Correct,
            response_ms: None,
            session_id: "s".into(),
        };
        let p = SrsParams::default();
        let ids = store.item_ids(SKILL).unwrap();
        for id in ids.iter().take(5) {
            store.introduce(id, 0).unwrap();
        }
        // Two cards reviewed Again on day 0 are due on day 1.
        store
            .review(&ids[3], Receptive, Rating::Again, 0, &ctx, &p)
            .unwrap();
        store
            .review(&ids[4], Productive, Rating::Again, 0, &ctx, &p)
            .unwrap();
        let mut req = request(false, false);
        req.new_items_per_lesson = 2;
        req.lesson_length = 7;
        let plan = compose(&req, &store, DAY_MS).unwrap();
        // tal and gracias are new and día and noche due: four vocabulary items give a
        // match_pairs entry for tal. The due cards tie on retrievability and due time, so día
        // comes before noche by item id; they alternate with the productive entries.
        assert_eq!(
            summary(&plan),
            [
                entry("l:tal", Receptive, PickMeaning, true),
                entry("l:gracias", Receptive, PickMeaning, true),
                entry("l:tal", Receptive, MatchPairs, true),
                entry("l:día", Receptive, PickMeaning, false),
                entry("l:tal", Productive, FillGap, true),
                entry("l:noche", Productive, FillGap, false),
                entry("l:gracias", Productive, FillGap, true),
            ]
        );
        req.lesson_length = 4;
        assert_eq!(compose(&req, &store, DAY_MS).unwrap().entries.len(), 4);
    }
}
