//! The due queue, skill mastery and skill states (docs/modules/02-srs-and-learner-model.md,
//! Design notes).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::fsrs::{Card, Facet, retrievability};
use crate::content::Lang;
use crate::store::{Store, StoreError};

/// A due card as the review queue lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardRef {
    pub item_id: String,
    pub facet: Facet,
    pub due: i64,
    pub retrievability: f64,
}

/// The due cards among `cards` (`due ≤ now_ms`), most at risk first: by retrievability
/// ascending, then due, then item id, then receptive before productive; at most `limit`.
pub fn queue_order(cards: &[Card], now_ms: i64, limit: usize) -> Vec<CardRef> {
    let mut queue: Vec<CardRef> = cards
        .iter()
        .filter_map(|card| {
            let due = card.due.filter(|&due| due <= now_ms)?;
            Some(CardRef {
                item_id: card.item_id.clone(),
                facet: card.facet,
                due,
                retrievability: retrievability(card, now_ms),
            })
        })
        .collect();
    queue.sort_by(|a, b| {
        a.retrievability
            .total_cmp(&b.retrievability)
            .then(a.due.cmp(&b.due))
            .then_with(|| a.item_id.cmp(&b.item_id))
            .then(a.facet.cmp(&b.facet))
    });
    queue.truncate(limit);
    queue
}

/// The review queue of a course: its due cards of items not retired, in [`queue_order`].
pub fn due_queue(
    store: &Store,
    lang: Lang,
    now_ms: i64,
    limit: usize,
) -> Result<Vec<CardRef>, StoreError> {
    Ok(queue_order(&store.due_cards(lang, now_ms)?, now_ms, limit))
}

/// Mastery of a skill with `item_count` items: the sum of the cards' retrievability over twice
/// the item count, so cards of items not yet introduced count 0. No items: 0.
pub fn mastery_of(cards: &[Card], item_count: usize, now_ms: i64) -> f64 {
    if item_count == 0 {
        return 0.0;
    }
    let sum: f64 = cards.iter().map(|card| retrievability(card, now_ms)).sum();
    sum / (2 * item_count) as f64
}

/// Mastery of one skill over its items that are not retired.
pub fn skill_mastery(store: &Store, skill_id: &str, now_ms: i64) -> Result<f64, StoreError> {
    let items = store.item_ids(skill_id)?;
    let cards = store.skill_cards(skill_id)?;
    Ok(mastery_of(&cards, items.len(), now_ms))
}

/// Where a skill is on the learning path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillState {
    Locked,
    Available,
    Learning,
    Practised,
    Mastered,
}

/// The mastery at and above which a practised skill is mastered.
const MASTERED_AT: f64 = 0.85;

/// What the cards of a skill say: whether every item has a reviewed card, and whether any item
/// has been introduced.
struct Progress {
    practised: bool,
    introduced: bool,
}

fn progress(items: &[String], cards: &[Card]) -> Progress {
    let reviewed: HashSet<&str> = cards
        .iter()
        .filter(|card| card.last_review.is_some())
        .map(|card| card.item_id.as_str())
        .collect();
    Progress {
        practised: items.iter().all(|item| reviewed.contains(item.as_str())),
        introduced: !cards.is_empty(),
    }
}

/// Whether a prerequisite is met: the skill is tested out, or every one of its items has a
/// reviewed card (practised or mastered). A prerequisite the store does not know is not met.
fn prerequisite_met(store: &Store, skill_id: &str) -> Result<bool, StoreError> {
    let row = match store.skill_row(skill_id) {
        Ok(row) => row,
        Err(StoreError::UnknownSkill(_)) => return Ok(false),
        Err(other) => return Err(other),
    };
    if row.tested_out {
        return Ok(true);
    }
    let items = store.item_ids(skill_id)?;
    let cards = store.skill_cards(skill_id)?;
    Ok(progress(&items, &cards).practised)
}

/// The state of a skill, the first that holds: mastered (practised and mastery ≥ 0.85);
/// practised (every item has a reviewed card); learning (an item is introduced); available
/// (every `requires` skill, or when there are none the previous skill in syllabus order, is
/// practised, mastered or tested out; the first skill always); otherwise locked.
pub fn skill_state(store: &Store, skill_id: &str, now_ms: i64) -> Result<SkillState, StoreError> {
    let row = store.skill_row(skill_id)?;
    let items = store.item_ids(skill_id)?;
    let cards = store.skill_cards(skill_id)?;
    let progress = progress(&items, &cards);
    if progress.practised {
        if mastery_of(&cards, items.len(), now_ms) >= MASTERED_AT {
            return Ok(SkillState::Mastered);
        }
        return Ok(SkillState::Practised);
    }
    if progress.introduced {
        return Ok(SkillState::Learning);
    }
    let prerequisites = if row.requires.is_empty() {
        store
            .previous_skill(&row.lang, row.ord)?
            .into_iter()
            .collect()
    } else {
        row.requires
    };
    for prerequisite in &prerequisites {
        if !prerequisite_met(store, prerequisite)? {
            return Ok(SkillState::Locked);
        }
    }
    Ok(SkillState::Available)
}

/// The first prerequisite of a skill that is not met, by [`skill_state`]'s rule: the skill a
/// locked skill waits for. None when every prerequisite is met.
pub(crate) fn first_unmet_prerequisite(
    store: &Store,
    skill_id: &str,
) -> Result<Option<String>, StoreError> {
    let row = store.skill_row(skill_id)?;
    let prerequisites: Vec<String> = if row.requires.is_empty() {
        store
            .previous_skill(&row.lang, row.ord)?
            .into_iter()
            .collect()
    } else {
        row.requires
    };
    for prerequisite in prerequisites {
        if !prerequisite_met(store, &prerequisite)? {
            return Ok(Some(prerequisite));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::srs::{CardState, DAY_MS, default_weights};

    /// A card reviewed `days` ago with stability `s`.
    fn card(item: &str, facet: Facet, s: f64, days: f64, now: i64) -> Card {
        Card {
            item_id: item.into(),
            facet,
            state: CardState::Review,
            stability: Some(s),
            difficulty: Some(5.0),
            due: Some(now - DAY_MS),
            last_review: Some(now - (days * DAY_MS as f64) as i64),
            reps: 1,
            lapses: 0,
        }
    }

    /// The days after which a card of stability `s` has retrievability `r`.
    fn days_for(r: f64, s: f64) -> f64 {
        let d = default_weights()[20];
        let f = 0.9f64.powf(-1.0 / d) - 1.0;
        s / f * (r.powf(-1.0 / d) - 1.0)
    }

    #[test]
    fn the_queue_puts_the_most_forgotten_first() {
        let now = 100 * DAY_MS;
        let cards = vec![
            card("a", Facet::Receptive, 10.0, 1.0, now),
            card("b", Facet::Receptive, 2.0, 9.0, now),
            card("c", Facet::Productive, 30.0, 30.0, now),
            card("d", Facet::Receptive, 5.0, 20.0, now),
            Card {
                due: Some(now + 1),
                ..card("e", Facet::Receptive, 1.0, 50.0, now)
            },
            Card::new("f", Facet::Receptive),
        ];
        let queue = queue_order(&cards, now, 10);
        let order: Vec<&str> = queue.iter().map(|c| c.item_id.as_str()).collect();
        assert_eq!(
            order,
            ["b", "d", "c", "a"],
            "not due and new cards are left out"
        );
        assert!(
            queue
                .windows(2)
                .all(|w| w[0].retrievability <= w[1].retrievability)
        );
        assert_eq!(queue_order(&cards, now, 2).len(), 2);
    }

    #[test]
    fn ties_go_by_due_then_item_then_facet() {
        let now = 10 * DAY_MS;
        let mut a = card("a", Facet::Productive, 3.0, 2.0, now);
        let mut b = card("a", Facet::Receptive, 3.0, 2.0, now);
        let mut c = card("0", Facet::Receptive, 3.0, 2.0, now);
        a.due = Some(now - 5);
        b.due = Some(now - 5);
        c.due = Some(now - 1);
        let order: Vec<(String, Facet)> = queue_order(&[c, a, b], now, 5)
            .into_iter()
            .map(|r| (r.item_id, r.facet))
            .collect();
        assert_eq!(
            order,
            [
                ("a".to_string(), Facet::Receptive),
                ("a".to_string(), Facet::Productive),
                ("0".to_string(), Facet::Receptive),
            ]
        );
    }

    #[test]
    fn mastery_divides_by_twice_the_items() {
        let now = 1_000 * DAY_MS;
        let cards: Vec<Card> = [0.9, 0.8, 0.7, 0.6]
            .iter()
            .enumerate()
            .map(|(i, &r)| {
                let s = 4.0 + i as f64;
                let mut c = card(
                    if i < 2 { "x" } else { "y" },
                    Facet::ALL[i % 2],
                    s,
                    0.0,
                    now,
                );
                c.last_review = None;
                let t = days_for(r, s);
                c.last_review = Some(now - (t * DAY_MS as f64).round() as i64);
                c
            })
            .collect();
        let mastery = mastery_of(&cards, 4, now);
        assert!((mastery - 0.375).abs() < 1e-6, "{mastery}");
        assert_eq!(mastery_of(&cards, 0, now), 0.0);
        assert_eq!(mastery_of(&[], 3, now), 0.0);
    }

    #[test]
    fn skill_states_follow_the_path() {
        use crate::content::parse_syllabus;
        use crate::content::test_support::{EXAMPLE_SYLLABUS, example_forms};
        use crate::srs::{Rating, SrsParams, Verdict};
        use crate::store::ReviewContext;

        let mut syllabus = parse_syllabus("valid-es.yaml", EXAMPLE_SYLLABUS).unwrap();
        let mut second = syllabus.units[0].skills[0].clone();
        second.id = "es.a1.u01.s02".into();
        second.requires = vec![];
        syllabus.units[0].skills.push(second);
        let mut store = Store::open_in_memory().unwrap();
        store.start_course(&syllabus, &example_forms(), 0).unwrap();
        let (s1, s2) = ("es.a1.u01.s01", "es.a1.u01.s02");
        assert_eq!(skill_state(&store, s1, 0).unwrap(), SkillState::Available);
        assert_eq!(skill_state(&store, s2, 0).unwrap(), SkillState::Locked);
        let ctx = ReviewContext {
            exercise_type: "translate".into(),
            verdict: Verdict::Correct,
            response_ms: None,
            session_id: "s".into(),
        };
        let p = SrsParams::default();
        let items = store.item_ids(s1).unwrap();
        store
            .review(&items[0], Facet::Receptive, Rating::Good, 10, &ctx, &p)
            .unwrap();
        assert_eq!(skill_state(&store, s1, 10).unwrap(), SkillState::Learning);
        assert_eq!(skill_state(&store, s2, 10).unwrap(), SkillState::Locked);
        for item in &items[1..] {
            store
                .review(item, Facet::Productive, Rating::Good, 20, &ctx, &p)
                .unwrap();
        }
        // Half the cards are fresh (R = 1) and half never reviewed: mastery 0.5.
        assert_eq!(skill_state(&store, s1, 20).unwrap(), SkillState::Practised);
        assert!((skill_mastery(&store, s1, 20).unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(skill_state(&store, s2, 20).unwrap(), SkillState::Available);
        for item in &items {
            for facet in Facet::ALL {
                store
                    .review(item, facet, Rating::Good, 30, &ctx, &p)
                    .unwrap();
            }
        }
        assert_eq!(skill_state(&store, s1, 30).unwrap(), SkillState::Mastered);
        assert_eq!(
            skill_state(&store, "nope", 0).unwrap_err(),
            StoreError::UnknownSkill("nope".into())
        );
        assert!(due_queue(&store, Lang::Es, 30, 100).unwrap().is_empty());
        let later = 30 + 400 * DAY_MS;
        assert_eq!(due_queue(&store, Lang::Es, later, 100).unwrap().len(), 22);
        assert_eq!(due_queue(&store, Lang::Fr, later, 100).unwrap().len(), 0);
    }
}
