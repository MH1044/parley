//! FSRS cards, scheduling, the review log and its replay, and retrievability
//! (docs/modules/02-srs-and-learner-model.md, Design notes).
//!
//! Scheduling goes through the `fsrs` crate with its default FSRS-6 parameters and no fuzz. The
//! crate computes in f32; stability and difficulty are kept widened to f64, so narrowing them back
//! is exact and a replay of the log reproduces every card bit for bit (invariant I6).

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use std::sync::OnceLock;

use ::fsrs::{DEFAULT_PARAMETERS, FSRS, MemoryState};
use serde::{Deserialize, Deserializer, Serialize};

use super::rating::Verdict;

/// One day in milliseconds.
pub const DAY_MS: i64 = 86_400_000;

/// The longest interval a review can schedule, in days.
pub const MAX_INTERVAL_DAYS: u32 = 365;

/// Which way an item is practised: recognising it (target to English) or producing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Facet {
    Receptive,
    Productive,
}

impl Facet {
    /// Both facets, receptive first: the order cards are listed in.
    pub const ALL: [Facet; 2] = [Facet::Receptive, Facet::Productive];

    pub fn as_str(self) -> &'static str {
        match self {
            Facet::Receptive => "receptive",
            Facet::Productive => "productive",
        }
    }
}

impl FromStr for Facet {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "receptive" => Ok(Facet::Receptive),
            "productive" => Ok(Facet::Productive),
            other => Err(format!("unknown facet {other}")),
        }
    }
}

impl fmt::Display for Facet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a card is in its life: never reviewed, remembered at its last review, or forgotten at
/// its last review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CardState {
    New,
    Review,
    Relearning,
}

impl CardState {
    pub fn as_str(self) -> &'static str {
        match self {
            CardState::New => "new",
            CardState::Review => "review",
            CardState::Relearning => "relearning",
        }
    }
}

impl FromStr for CardState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "new" => Ok(CardState::New),
            "review" => Ok(CardState::Review),
            "relearning" => Ok(CardState::Relearning),
            other => Err(format!("unknown card state {other}")),
        }
    }
}

/// The FSRS rating of a review. Serialises as the integer 1 to 4, as the review log stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(into = "i64", try_from = "i64")]
pub enum Rating {
    Again = 1,
    Hard = 2,
    Good = 3,
    Easy = 4,
}

impl Rating {
    /// The rating as FSRS numbers it, 1 to 4.
    pub fn as_u32(self) -> u32 {
        self as u32
    }
}

impl From<Rating> for i64 {
    fn from(rating: Rating) -> i64 {
        rating as i64
    }
}

impl TryFrom<i64> for Rating {
    type Error = String;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Rating::Again),
            2 => Ok(Rating::Hard),
            3 => Ok(Rating::Good),
            4 => Ok(Rating::Easy),
            other => Err(format!("rating must be 1, 2, 3 or 4, not {other}")),
        }
    }
}

/// The learner's scheduling settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SrsParams {
    /// The probability of recall a review is scheduled at, 0.80 to 0.95.
    pub desired_retention: f64,
}

impl SrsParams {
    /// The lowest desired retention the settings allow.
    pub const MIN_RETENTION: f64 = 0.80;
    /// The highest desired retention the settings allow.
    pub const MAX_RETENTION: f64 = 0.95;
    /// The desired retention when the learner has not chosen one.
    pub const DEFAULT_RETENTION: f64 = 0.90;

    /// Parameters at `r`, clamped to 0.80..=0.95; a value that is not a number gives the default.
    pub fn with_retention(r: f64) -> SrsParams {
        let desired_retention = if r.is_nan() {
            Self::DEFAULT_RETENTION
        } else {
            r.clamp(Self::MIN_RETENTION, Self::MAX_RETENTION)
        };
        SrsParams { desired_retention }
    }
}

impl Default for SrsParams {
    fn default() -> Self {
        SrsParams {
            desired_retention: Self::DEFAULT_RETENTION,
        }
    }
}

/// The crate's default FSRS-6 parameter vector, widened to f64 (exactly).
pub fn default_weights() -> [f64; 21] {
    DEFAULT_PARAMETERS.map(f64::from)
}

/// The FSRS model at the default parameters, built once.
fn model() -> &'static FSRS {
    static MODEL: OnceLock<FSRS> = OnceLock::new();
    MODEL.get_or_init(|| FSRS::new(&DEFAULT_PARAMETERS).expect("the default parameters are valid"))
}

/// One facet of one item, with its FSRS memory and schedule. Serialises its nine fields in this
/// order, `null` for an empty option.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Card {
    pub item_id: String,
    pub facet: Facet,
    pub state: CardState,
    /// FSRS stability in days: the interval at which recall falls to 90%.
    pub stability: Option<f64>,
    /// FSRS difficulty, 1 to 10.
    pub difficulty: Option<f64>,
    /// Milliseconds since the epoch when the card is next due.
    pub due: Option<i64>,
    /// Milliseconds since the epoch of the last review.
    pub last_review: Option<i64>,
    pub reps: u32,
    pub lapses: u32,
}

impl Card {
    /// A card that has never been reviewed: state `new`, every option empty, counts 0.
    pub fn new(item_id: &str, facet: Facet) -> Card {
        Card {
            item_id: item_id.to_string(),
            facet,
            state: CardState::New,
            stability: None,
            difficulty: None,
            due: None,
            last_review: None,
            reps: 0,
            lapses: 0,
        }
    }

    /// The card's memory state as FSRS takes it, when it has one.
    fn memory(&self) -> Option<MemoryState> {
        match (self.stability, self.difficulty) {
            (Some(stability), Some(difficulty))
                if stability.is_finite() && difficulty.is_finite() =>
            {
                Some(MemoryState {
                    stability: stability as f32,
                    difficulty: difficulty as f32,
                })
            }
            _ => None,
        }
    }
}

/// The result of one review: the updated card, the whole days FSRS was given, and the interval.
#[derive(Debug, Clone, PartialEq)]
pub struct Scheduled {
    pub card: Card,
    pub elapsed_days: f64,
    pub interval_days: u32,
}

/// Whole days from `from` to `to`, floored and never below 0.
fn whole_days(from: i64, to: i64) -> u32 {
    let days = to.saturating_sub(from).div_euclid(DAY_MS).max(0);
    u32::try_from(days).unwrap_or(u32::MAX)
}

/// Schedules one review of `card` rated `rating` at `now_ms`.
///
/// Days elapsed is whole days since the last review, 0 for a card without memory, as FSRS and
/// py-fsrs count them, so a second review on the same day takes FSRS's short-term formula. The
/// interval is the crate's interval at the desired retention, rounded, at least 1 day and at most
/// [`MAX_INTERVAL_DAYS`]; there is no fuzz.
pub fn schedule(card: &Card, rating: Rating, now_ms: i64, p: &SrsParams) -> Scheduled {
    let memory = card.memory();
    let days = match (memory, card.last_review) {
        (Some(_), Some(last)) => whole_days(last, now_ms),
        _ => 0,
    };
    let next = model()
        .next_states(memory, p.desired_retention as f32, days)
        .expect("FSRS gives finite states for a finite memory state");
    let state = match rating {
        Rating::Again => next.again,
        Rating::Hard => next.hard,
        Rating::Good => next.good,
        Rating::Easy => next.easy,
    };
    let rounded = f64::from(state.interval).round();
    let interval_days = if rounded >= f64::from(MAX_INTERVAL_DAYS) {
        MAX_INTERVAL_DAYS
    } else if rounded >= 1.0 {
        rounded as u32
    } else {
        1
    };
    let lapsed = rating == Rating::Again && card.state == CardState::Review;
    let updated = Card {
        item_id: card.item_id.clone(),
        facet: card.facet,
        state: if rating == Rating::Again {
            CardState::Relearning
        } else {
            CardState::Review
        },
        stability: Some(f64::from(state.memory.stability)),
        difficulty: Some(f64::from(state.memory.difficulty)),
        due: Some(now_ms.saturating_add(i64::from(interval_days) * DAY_MS)),
        last_review: Some(now_ms),
        reps: card.reps.saturating_add(1),
        lapses: card.lapses.saturating_add(u32::from(lapsed)),
    };
    Scheduled {
        card: updated,
        elapsed_days: f64::from(days),
        interval_days,
    }
}

/// Reads a number that may also be `null` or absent (0).
fn number_or_zero<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    Ok(Option::<f64>::deserialize(deserializer)?.unwrap_or(0.0))
}

/// One row of the review log. When read from a file, `elapsed_days`, `stability_after` and
/// `difficulty_after` may be absent (a replay recomputes them) and `response_ms` may be `null` or
/// absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewLogEntry {
    pub item_id: String,
    pub facet: Facet,
    /// Milliseconds since the epoch.
    pub reviewed_at: i64,
    pub rating: Rating,
    pub exercise_type: String,
    pub verdict: Verdict,
    #[serde(default)]
    pub response_ms: Option<i64>,
    /// The whole days FSRS was given.
    #[serde(default, deserialize_with = "number_or_zero")]
    pub elapsed_days: f64,
    #[serde(default, deserialize_with = "number_or_zero")]
    pub stability_after: f64,
    #[serde(default, deserialize_with = "number_or_zero")]
    pub difficulty_after: f64,
    pub session_id: String,
}

/// A review log as a file holds it: `{"schema": "parley.review-log/1", "entries": [...]}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewLog {
    pub schema: String,
    pub entries: Vec<ReviewLogEntry>,
}

impl ReviewLog {
    /// The schema name of the file format.
    pub const SCHEMA: &'static str = "parley.review-log/1";

    /// A log of `entries` with the current schema name.
    pub fn new(entries: Vec<ReviewLogEntry>) -> ReviewLog {
        ReviewLog {
            schema: Self::SCHEMA.to_string(),
            entries,
        }
    }

    /// Parses a review log file. The error is the parser's message, or
    /// `schema must be parley.review-log/1`.
    pub fn parse(text: &str) -> Result<ReviewLog, String> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let log: ReviewLog = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if log.schema != Self::SCHEMA {
            return Err(format!("schema must be {}", Self::SCHEMA));
        }
        Ok(log)
    }
}

/// Every card the log mentions, rebuilt from `Card::new` by scheduling its entries in log order.
///
/// Cards come ordered by item id in byte order, receptive before productive. Stability,
/// difficulty, state, counts and last review depend on the log alone; `due` also depends on the
/// desired retention.
pub fn replay(log: &[ReviewLogEntry], p: &SrsParams) -> Vec<Card> {
    let mut cards: BTreeMap<(&str, Facet), Card> = BTreeMap::new();
    for entry in log {
        let card = cards
            .entry((entry.item_id.as_str(), entry.facet))
            .or_insert_with(|| Card::new(&entry.item_id, entry.facet));
        *card = schedule(card, entry.rating, entry.reviewed_at, p).card;
    }
    cards.into_values().collect()
}

/// The probability of recalling `card` at `now_ms`: the FSRS-6 forgetting curve
/// R(t, S) = (1 + f·t/S)^(−d), d = w[20], f = 0.9^(−1/d) − 1, in f64, with t the fractional days
/// since the last review (never below 0). A card without stability or last review has 0.
pub fn retrievability(card: &Card, now_ms: i64) -> f64 {
    let (Some(stability), Some(last)) = (card.stability, card.last_review) else {
        return 0.0;
    };
    if stability.is_nan() || stability <= 0.0 {
        return 0.0;
    }
    let decay = f64::from(DEFAULT_PARAMETERS[20]);
    let factor = 0.9f64.powf(-1.0 / decay) - 1.0;
    let t = (now_ms.saturating_sub(last) as f64 / DAY_MS as f64).max(0.0);
    (1.0 + factor * t / stability).powf(-decay)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> SrsParams {
        SrsParams::default()
    }

    #[test]
    fn a_new_card_takes_the_initial_stability_of_its_rating() {
        let w = default_weights();
        for (rating, expected) in [
            (Rating::Again, w[0]),
            (Rating::Hard, w[1]),
            (Rating::Good, w[2]),
            (Rating::Easy, w[3]),
        ] {
            let scheduled = schedule(&Card::new("i", Facet::Receptive), rating, 0, &params());
            let stability = scheduled.card.stability.unwrap();
            assert!(
                (stability - expected).abs() < 1e-9,
                "{rating:?}: {stability}"
            );
            assert_eq!(scheduled.elapsed_days, 0.0);
            assert_eq!(scheduled.card.reps, 1);
            assert_eq!(scheduled.card.last_review, Some(0));
        }
    }

    #[test]
    fn the_interval_is_the_rounded_stability_at_ninety_percent() {
        let card = Card::new("i", Facet::Productive);
        let good = schedule(&card, Rating::Good, 1_000, &params());
        assert_eq!(good.interval_days, 2);
        assert_eq!(good.card.due, Some(1_000 + 2 * DAY_MS));
        let again = schedule(&card, Rating::Again, 1_000, &params());
        assert_eq!(
            again.interval_days, 1,
            "0.212 days rounds up to the 1-day floor"
        );
        assert_eq!(again.card.state, CardState::Relearning);
        assert_eq!(again.card.lapses, 0, "a new card does not lapse");
        let easy = schedule(&card, Rating::Easy, 1_000, &params());
        assert_eq!(easy.interval_days, 8);
    }

    #[test]
    fn intervals_never_exceed_a_year() {
        let mut card = Card::new("i", Facet::Receptive);
        let mut now = 0;
        for _ in 0..12 {
            let scheduled = schedule(&card, Rating::Easy, now, &params());
            assert!(scheduled.interval_days <= MAX_INTERVAL_DAYS);
            now = scheduled.card.due.unwrap();
            card = scheduled.card;
        }
        assert!(card.stability.unwrap() > 365.0);
        let last = schedule(&card, Rating::Easy, now, &params());
        assert_eq!(last.interval_days, MAX_INTERVAL_DAYS);
    }

    #[test]
    fn again_on_a_review_card_is_a_lapse_and_days_are_whole() {
        let first = schedule(
            &Card::new("i", Facet::Receptive),
            Rating::Good,
            0,
            &params(),
        )
        .card;
        let later = schedule(&first, Rating::Again, 3 * DAY_MS - 1, &params());
        assert_eq!(later.elapsed_days, 2.0, "2.99 days floor to 2");
        assert_eq!(later.card.state, CardState::Relearning);
        assert_eq!(later.card.lapses, 1);
        let back = schedule(&later.card, Rating::Again, 3 * DAY_MS, &params());
        assert_eq!(
            back.card.lapses, 1,
            "a relearning card does not lapse again"
        );
        let early = schedule(&first, Rating::Good, -DAY_MS, &params());
        assert_eq!(
            early.elapsed_days, 0.0,
            "a clock that went back counts 0 days"
        );
    }

    #[test]
    fn widened_values_narrow_back_exactly() {
        let card = schedule(
            &Card::new("i", Facet::Receptive),
            Rating::Hard,
            0,
            &params(),
        )
        .card;
        let s = card.stability.unwrap();
        assert_eq!(f64::from(s as f32), s);
    }

    #[test]
    fn retention_is_clamped_and_changes_the_interval_only() {
        assert_eq!(SrsParams::with_retention(0.5).desired_retention, 0.80);
        assert_eq!(SrsParams::with_retention(0.99).desired_retention, 0.95);
        assert_eq!(SrsParams::with_retention(0.85).desired_retention, 0.85);
        assert_eq!(SrsParams::with_retention(f64::NAN).desired_retention, 0.90);
        assert_eq!(SrsParams::default().desired_retention, 0.90);
        let card = Card::new("i", Facet::Receptive);
        let low = schedule(&card, Rating::Easy, 0, &SrsParams::with_retention(0.80));
        let high = schedule(&card, Rating::Easy, 0, &SrsParams::with_retention(0.95));
        assert_eq!(low.card.stability, high.card.stability);
        assert!(low.interval_days > high.interval_days);
    }

    #[test]
    fn ratings_serialise_as_integers_and_refuse_others() {
        assert_eq!(serde_json::to_string(&Rating::Again).unwrap(), "1");
        assert_eq!(serde_json::to_string(&Rating::Easy).unwrap(), "4");
        assert_eq!(serde_json::from_str::<Rating>("3").unwrap(), Rating::Good);
        let err = serde_json::from_str::<Rating>("5").unwrap_err().to_string();
        assert!(err.contains("rating must be 1, 2, 3 or 4"), "{err}");
        assert!(serde_json::from_str::<Rating>("\"good\"").is_err());
    }

    #[test]
    fn a_card_serialises_its_nine_fields_in_order() {
        let json = serde_json::to_string(&Card::new("x#l:a", Facet::Receptive)).unwrap();
        assert_eq!(
            json,
            r#"{"item_id":"x#l:a","facet":"receptive","state":"new","stability":null,"difficulty":null,"due":null,"last_review":null,"reps":0,"lapses":0}"#
        );
        assert_eq!(
            serde_json::to_string(&CardState::Relearning).unwrap(),
            "\"relearning\""
        );
        assert_eq!("productive".parse::<Facet>(), Ok(Facet::Productive));
        assert_eq!("review".parse::<CardState>(), Ok(CardState::Review));
        assert!("other".parse::<Facet>().is_err());
    }

    #[test]
    fn a_log_entry_reads_without_the_recomputed_fields() {
        let text = r#"{"schema":"parley.review-log/1","entries":[
            {"item_id":"a","facet":"productive","reviewed_at":5,"rating":2,"exercise_type":"translate",
             "verdict":"almost","session_id":"s"},
            {"item_id":"a","facet":"productive","reviewed_at":6,"rating":3,"exercise_type":"translate",
             "verdict":"correct","response_ms":null,"elapsed_days":null,"stability_after":1.5,
             "difficulty_after":5,"session_id":"s"}]}"#;
        let log = ReviewLog::parse(text).unwrap();
        assert_eq!(log.entries.len(), 2);
        assert_eq!(log.entries[0].response_ms, None);
        assert_eq!(log.entries[0].elapsed_days, 0.0);
        assert_eq!(log.entries[1].stability_after, 1.5);
        assert_eq!(log.entries[1].verdict, Verdict::Correct);
    }

    #[test]
    fn a_log_with_another_schema_or_shape_is_refused() {
        let err = ReviewLog::parse(r#"{"schema":"parley.review-log/2","entries":[]}"#).unwrap_err();
        assert_eq!(err, "schema must be parley.review-log/1");
        let err = ReviewLog::parse(r#"{"schema":"parley.review-log/1"}"#).unwrap_err();
        assert!(err.starts_with("missing field `entries`"), "{err}");
        assert!(ReviewLog::parse("not json").is_err());
        let log = ReviewLog::new(Vec::new());
        assert_eq!(
            ReviewLog::parse(&serde_json::to_string(&log).unwrap()),
            Ok(log)
        );
    }

    #[test]
    fn replay_orders_by_item_id_bytes_then_facet() {
        let entry = |item: &str, facet: Facet, at: i64, rating: Rating| ReviewLogEntry {
            item_id: item.into(),
            facet,
            reviewed_at: at,
            rating,
            exercise_type: "translate".into(),
            verdict: Verdict::Correct,
            response_ms: None,
            elapsed_days: 0.0,
            stability_after: 0.0,
            difficulty_after: 0.0,
            session_id: "s".into(),
        };
        let log = vec![
            entry("b", Facet::Productive, 0, Rating::Good),
            entry("a", Facet::Productive, 0, Rating::Good),
            entry("b", Facet::Receptive, 0, Rating::Hard),
            entry("B", Facet::Receptive, 0, Rating::Easy),
            entry("b", Facet::Productive, 3 * DAY_MS, Rating::Again),
        ];
        let cards = replay(&log, &params());
        let keys: Vec<(&str, Facet)> = cards
            .iter()
            .map(|c| (c.item_id.as_str(), c.facet))
            .collect();
        assert_eq!(
            keys,
            [
                ("B", Facet::Receptive),
                ("a", Facet::Productive),
                ("b", Facet::Receptive),
                ("b", Facet::Productive),
            ]
        );
        let first = schedule(
            &Card::new("b", Facet::Productive),
            Rating::Good,
            0,
            &params(),
        )
        .card;
        let second = schedule(&first, Rating::Again, 3 * DAY_MS, &params()).card;
        assert_eq!(cards[3], second);
        assert_eq!(cards[3].reps, 2);
        assert_eq!(cards[3].lapses, 1);
    }

    #[test]
    fn retrievability_follows_the_forgetting_curve() {
        let mut card = Card::new("i", Facet::Receptive);
        assert_eq!(retrievability(&card, 0), 0.0, "never reviewed");
        card.stability = Some(10.0);
        card.last_review = Some(0);
        assert!((retrievability(&card, 0) - 1.0).abs() < 1e-12);
        // By definition R(S, S) = 0.9.
        assert!((retrievability(&card, 10 * DAY_MS) - 0.9).abs() < 1e-12);
        assert!(retrievability(&card, 20 * DAY_MS) < 0.9);
        assert!((retrievability(&card, -DAY_MS) - 1.0).abs() < 1e-12);
    }
}
