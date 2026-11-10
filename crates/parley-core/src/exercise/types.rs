//! Exercise and lesson types, serialised exactly as `schemas/lesson.schema.json` describes them
//! (docs/modules/03-exercise-engine-and-grading.md, Interfaces).
//!
//! An exercise is one flat JSON object: the common fields (`id`, `item_id`, `facet`, `origin`),
//! then `type` and that type's own fields.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::content::Lang;
use crate::grading::Direction;
use crate::srs::Facet;

/// The seven exercise types. Serialised in snake case; [`ExerciseType::as_str`] gives the same
/// strings, which are also what the review log records as `exercise_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExerciseType {
    PickMeaning,
    TranslateToTarget,
    TranslateToEn,
    ListenType,
    Speak,
    FillGap,
    MatchPairs,
}

impl ExerciseType {
    /// Every type, in the order the spec lists them.
    pub const ALL: [ExerciseType; 7] = [
        ExerciseType::PickMeaning,
        ExerciseType::TranslateToTarget,
        ExerciseType::TranslateToEn,
        ExerciseType::ListenType,
        ExerciseType::Speak,
        ExerciseType::FillGap,
        ExerciseType::MatchPairs,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ExerciseType::PickMeaning => "pick_meaning",
            ExerciseType::TranslateToTarget => "translate_to_target",
            ExerciseType::TranslateToEn => "translate_to_en",
            ExerciseType::ListenType => "listen_type",
            ExerciseType::Speak => "speak",
            ExerciseType::FillGap => "fill_gap",
            ExerciseType::MatchPairs => "match_pairs",
        }
    }

    /// The card an exercise of this type practises: recognising (`pick_meaning`,
    /// `translate_to_en`, `listen_type`, `match_pairs`) or producing (the others).
    pub fn facet(self) -> Facet {
        match self {
            ExerciseType::PickMeaning
            | ExerciseType::TranslateToEn
            | ExerciseType::ListenType
            | ExerciseType::MatchPairs => Facet::Receptive,
            ExerciseType::TranslateToTarget | ExerciseType::Speak | ExerciseType::FillGap => {
                Facet::Productive
            }
        }
    }

    /// The direction of the answer: into the target language for `translate_to_target`,
    /// `listen_type`, `speak` and `fill_gap`, into English for `translate_to_en`, none for the
    /// two choice types.
    pub fn direction(self) -> Option<Direction> {
        match self {
            ExerciseType::TranslateToTarget
            | ExerciseType::ListenType
            | ExerciseType::Speak
            | ExerciseType::FillGap => Some(Direction::ToTarget),
            ExerciseType::TranslateToEn => Some(Direction::ToEn),
            ExerciseType::PickMeaning | ExerciseType::MatchPairs => None,
        }
    }
}

impl fmt::Display for ExerciseType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where an exercise or lesson came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Template,
    Ai,
    CoursePack,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Template => "template",
            Origin::Ai => "ai",
            Origin::CoursePack => "course_pack",
        }
    }
}

/// One exercise: which card it practises, where it came from, and its body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exercise {
    /// `e1`, `e2`, … within a lesson.
    pub id: String,
    pub item_id: String,
    pub facet: Facet,
    pub origin: Origin,
    #[serde(flatten)]
    pub body: Body,
}

/// What an exercise shows and expects, by type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Body {
    /// A target-language text and four English meanings; `answer_index` is the right one.
    PickMeaning {
        prompt_target: String,
        options: Vec<String>,
        answer_index: u32,
    },
    /// Translate English into the target language, typed or from word-bank `tiles`.
    TranslateToTarget {
        prompt_en: String,
        target: String,
        accepted: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tiles: Option<Vec<String>>,
    },
    /// Translate a target-language text into English.
    TranslateToEn {
        prompt_target: String,
        accepted: Vec<String>,
    },
    /// Hear `text` and type it.
    ListenType { text: String, accepted: Vec<String> },
    /// Say `text` (graded as typed text until speaking checks exist).
    Speak { text: String, prompt_en: String },
    /// Type the word missing at `___` in `sentence`.
    FillGap {
        sentence: String,
        answer: String,
        full_sentence: String,
        hint_en: String,
    },
    /// Match each target-language text with its English meaning: `[target, English]`, 4 or 5.
    MatchPairs { pairs: Vec<[String; 2]> },
}

impl Body {
    pub fn kind(&self) -> ExerciseType {
        match self {
            Body::PickMeaning { .. } => ExerciseType::PickMeaning,
            Body::TranslateToTarget { .. } => ExerciseType::TranslateToTarget,
            Body::TranslateToEn { .. } => ExerciseType::TranslateToEn,
            Body::ListenType { .. } => ExerciseType::ListenType,
            Body::Speak { .. } => ExerciseType::Speak,
            Body::FillGap { .. } => ExerciseType::FillGap,
            Body::MatchPairs { .. } => ExerciseType::MatchPairs,
        }
    }
}

impl Exercise {
    pub fn kind(&self) -> ExerciseType {
        self.body.kind()
    }

    /// The direction of the answer (see [`ExerciseType::direction`]).
    pub fn direction(&self) -> Option<Direction> {
        self.kind().direction()
    }

    /// The main target-language texts, which the structural checks L7 to L9 look at:
    /// `pick_meaning` and `translate_to_en` their `prompt_target`; `translate_to_target` its
    /// `target`; `listen_type` and `speak` their `text`; `fill_gap` its `full_sentence`;
    /// `match_pairs` the target side of each pair.
    pub fn targets(&self) -> Vec<&str> {
        match &self.body {
            Body::PickMeaning { prompt_target, .. } | Body::TranslateToEn { prompt_target, .. } => {
                vec![prompt_target.as_str()]
            }
            Body::TranslateToTarget { target, .. } => vec![target.as_str()],
            Body::ListenType { text, .. } | Body::Speak { text, .. } => vec![text.as_str()],
            Body::FillGap { full_sentence, .. } => vec![full_sentence.as_str()],
            Body::MatchPairs { pairs } => pairs.iter().map(|pair| pair[0].as_str()).collect(),
        }
    }

    /// The text the learner is shown and answers from, which a cached judgement is keyed by:
    /// `prompt_en`, `prompt_target`, `text` or `sentence`; none for `match_pairs`.
    pub(crate) fn prompt(&self) -> Option<&str> {
        match &self.body {
            Body::TranslateToTarget { prompt_en, .. } => Some(prompt_en),
            Body::PickMeaning { prompt_target, .. } | Body::TranslateToEn { prompt_target, .. } => {
                Some(prompt_target)
            }
            Body::ListenType { text, .. } | Body::Speak { text, .. } => Some(text),
            Body::FillGap { sentence, .. } => Some(sentence),
            Body::MatchPairs { .. } => None,
        }
    }

    /// The answers the exercise itself accepts, in order: `accepted` for the two translations
    /// and `listen_type`, `text` for `speak`, `answer` for `fill_gap`; none for the choice types.
    pub(crate) fn own_answers(&self) -> Vec<&str> {
        match &self.body {
            Body::TranslateToTarget { accepted, .. }
            | Body::TranslateToEn { accepted, .. }
            | Body::ListenType { accepted, .. } => accepted.iter().map(String::as_str).collect(),
            Body::Speak { text, .. } => vec![text.as_str()],
            Body::FillGap { answer, .. } => vec![answer.as_str()],
            Body::PickMeaning { .. } | Body::MatchPairs { .. } => Vec::new(),
        }
    }
}

/// A lesson: its exercises for one skill, numbered by `seq` within the skill. Serialises to the
/// object of `schemas/lesson.schema.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lesson {
    /// Always [`Lesson::SCHEMA`].
    pub schema: String,
    pub lang: Lang,
    pub skill_id: String,
    pub seq: u32,
    pub origin: Origin,
    pub exercises: Vec<Exercise>,
    /// Who wrote the lesson and how many of its planned exercises passed L2 to L9, whenever a
    /// fresh-lesson answer was received (module 07); none for a lesson made offline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<crate::ai::LessonAiInfo>,
}

impl Lesson {
    /// The schema name of the lesson format.
    pub const SCHEMA: &'static str = "parley.lesson/1";
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exercise(body: Body) -> Exercise {
        Exercise {
            id: "e1".into(),
            item_id: "es.a1.u01.s01#l:hola".into(),
            facet: body.kind().facet(),
            origin: Origin::Template,
            body,
        }
    }

    #[test]
    fn an_exercise_is_one_flat_object_tagged_by_type() {
        let ex = exercise(Body::TranslateToTarget {
            prompt_en: "hello".into(),
            target: "hola".into(),
            accepted: vec!["hola".into()],
            tiles: None,
        });
        assert_eq!(
            serde_json::to_string(&ex).unwrap(),
            r#"{"id":"e1","item_id":"es.a1.u01.s01#l:hola","facet":"productive","origin":"template","type":"translate_to_target","prompt_en":"hello","target":"hola","accepted":["hola"]}"#
        );
        let back: Exercise = serde_json::from_str(&serde_json::to_string(&ex).unwrap()).unwrap();
        assert_eq!(back, ex);
        let pairs = exercise(Body::MatchPairs {
            pairs: vec![["el agua".into(), "water".into()]],
        });
        let value = serde_json::to_value(&pairs).unwrap();
        assert_eq!(value["type"], "match_pairs");
        assert_eq!(value["pairs"], serde_json::json!([["el agua", "water"]]));
        assert_eq!(value["facet"], "receptive");
    }

    #[test]
    fn type_strings_directions_and_facets_follow_the_spec() {
        for kind in ExerciseType::ALL {
            assert_eq!(
                serde_json::to_string(&kind).unwrap(),
                format!("\"{}\"", kind.as_str())
            );
        }
        assert_eq!(
            serde_json::to_string(&Origin::CoursePack).unwrap(),
            "\"course_pack\""
        );
        use ExerciseType::*;
        let directions: Vec<Option<Direction>> =
            ExerciseType::ALL.iter().map(|k| k.direction()).collect();
        assert_eq!(
            directions,
            [
                None,
                Some(Direction::ToTarget),
                Some(Direction::ToEn),
                Some(Direction::ToTarget),
                Some(Direction::ToTarget),
                Some(Direction::ToTarget),
                None
            ]
        );
        let receptive: Vec<ExerciseType> = ExerciseType::ALL
            .into_iter()
            .filter(|k| k.facet() == Facet::Receptive)
            .collect();
        assert_eq!(
            receptive,
            [PickMeaning, TranslateToEn, ListenType, MatchPairs]
        );
    }

    #[test]
    fn main_texts_are_the_target_language_side() {
        let gap = exercise(Body::FillGap {
            sentence: "Quiero un café con ___.".into(),
            answer: "leche".into(),
            full_sentence: "Quiero un café con leche.".into(),
            hint_en: "milk".into(),
        });
        assert_eq!(gap.targets(), ["Quiero un café con leche."]);
        assert_eq!(gap.prompt(), Some("Quiero un café con ___."));
        assert_eq!(gap.own_answers(), ["leche"]);
        let pairs = exercise(Body::MatchPairs {
            pairs: vec![["a".into(), "x".into()], ["b".into(), "y".into()]],
        });
        assert_eq!(pairs.targets(), ["a", "b"]);
        assert_eq!(pairs.prompt(), None);
        let speak = exercise(Body::Speak {
            text: "Hola".into(),
            prompt_en: "Hello".into(),
        });
        assert_eq!(speak.own_answers(), ["Hola"]);
        assert_eq!(speak.direction(), Some(Direction::ToTarget));
    }
}
