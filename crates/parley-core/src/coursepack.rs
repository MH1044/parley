//! Course packs, `parley.coursepack/1` (docs/modules/07-lesson-generation.md): the AI lessons and
//! stories a learner has, exported as one JSON file, and imported from someone else.
//!
//! An imported pack is untrusted input. Every exercise goes through the exercise shape and the
//! checks L4 to L9 against the local syllabus and forms table, exactly as an AI answer does, and an
//! exercise whose item the local syllabus does not have is dropped (invariant I11). Stories go
//! through the story schema and S2 to S4 and S6. Nothing is stored unless the whole import
//! succeeds.

use std::collections::HashSet;
use std::fmt;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::ai::{
    AiCheck, Story, StoryCtx, StoryPrompt, check_ai_exercise, check_story, has_story,
    lesson_prompt_file, stems_of, story_schema_valid,
};
use crate::api::{VERSION, language_name};
use crate::content::{Cefr, Forms, Item, Lang, Syllabus};
use crate::env::{LocalDate, Rng, SplitMix64};
use crate::exercise::template::{focus_of, seen_push};
use crate::exercise::{
    Allowed, Body, Exercise, ExerciseType, Lesson, Origin, max_words, word_bank,
};
use crate::store::{Store, StoreError};

pub const COURSE_PACK_SCHEMA: &str = "parley.coursepack/1";
pub const COURSE_PACK_MAX_BYTES: u64 = 5_000_000;
/// The licence of course content (ADR-0003), which a pack made from it carries.
pub const COURSE_PACK_LICENCE: &str = "CC-BY-SA-4.0";

const ENVELOPE_SCHEMA: &str = include_str!("../../../schemas/coursepack.schema.json");

fn envelope() -> &'static jsonschema::Validator {
    static VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| {
        let schema: Value =
            serde_json::from_str(ENVELOPE_SCHEMA).expect("the course pack schema is valid JSON");
        jsonschema::validator_for(&schema).expect("the course pack schema is a valid JSON Schema")
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackLesson {
    pub skill_id: String,
    #[serde(default)]
    pub prompt: Option<String>,
    pub exercises: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackStory {
    pub skill_id: String,
    #[serde(default)]
    pub prompt: Option<String>,
    /// The story's own fields (title, title_en, paragraphs, questions).
    #[serde(flatten)]
    pub story: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoursePack {
    pub schema: String,
    pub lang: Lang,
    pub syllabus_version: u32,
    pub app_version: String,
    pub created_at: LocalDate,
    pub licence: String,
    pub lessons: Vec<PackLesson>,
    pub stories: Vec<PackStory>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PackError {
    TooLarge,
    NotACoursePack,
    WrongLanguage { pack: Lang, active: Lang },
    Store(StoreError),
}

impl PackError {
    pub fn message(&self) -> String {
        match self {
            PackError::TooLarge => "This course pack is larger than 5 MB.".into(),
            PackError::NotACoursePack => "This is not a Parley course pack.".into(),
            PackError::WrongLanguage { pack, active } => format!(
                "This course pack is for {}; your active course is {}.",
                language_name(*pack),
                language_name(*active)
            ),
            PackError::Store(e) => e.to_string(),
        }
    }
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl From<StoreError> for PackError {
    fn from(e: StoreError) -> Self {
        PackError::Store(e)
    }
}

/// What an export wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoursePackSummary {
    pub bytes: u64,
    pub lessons: usize,
    pub stories: usize,
    pub exercises: usize,
}

/// What an import kept and dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoursePackReport {
    pub lessons: usize,
    pub stories: usize,
    pub dropped: usize,
    pub message: String,
}

/// The pack of a language: every stored lesson with origin `ai` or `course_pack`, with only its
/// exercises of those origins (as stored, ids kept), by skill then number, a lesson with none left
/// out; every stored story; `prompt` from the stored prompt id.
pub fn export_course_pack(
    store: &Store,
    lang: Lang,
    syllabus_version: u32,
    created_at: LocalDate,
) -> Result<CoursePack, StoreError> {
    let mut lessons = Vec::new();
    for (lesson, prompt) in store.lessons_of(lang)? {
        if lesson.origin == Origin::Template {
            continue;
        }
        let exercises: Vec<Value> = lesson
            .exercises
            .iter()
            .filter(|ex| matches!(ex.origin, Origin::Ai | Origin::CoursePack))
            .map(|ex| serde_json::to_value(ex).expect("an exercise serialises"))
            .collect();
        if exercises.is_empty() {
            continue;
        }
        lessons.push(PackLesson {
            skill_id: lesson.skill_id,
            prompt,
            exercises,
        });
    }
    let stories = store
        .stories_of(lang)?
        .into_iter()
        .map(|(skill_id, story, prompt)| PackStory {
            skill_id,
            prompt,
            story: serde_json::to_value(story).expect("a story serialises"),
        })
        .collect();
    Ok(CoursePack {
        schema: COURSE_PACK_SCHEMA.into(),
        lang,
        syllabus_version,
        app_version: VERSION.into(),
        created_at,
        licence: COURSE_PACK_LICENCE.into(),
        lessons,
        stories,
    })
}

/// Size, JSON and schemas/coursepack.schema.json (the envelope only).
pub fn parse_course_pack(bytes: &[u8]) -> Result<CoursePack, PackError> {
    if bytes.len() as u64 > COURSE_PACK_MAX_BYTES {
        return Err(PackError::TooLarge);
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| PackError::NotACoursePack)?;
    if value.get("schema").and_then(Value::as_str) != Some(COURSE_PACK_SCHEMA)
        || !envelope().is_valid(&value)
    {
        return Err(PackError::NotACoursePack);
    }
    serde_json::from_value(value).map_err(|_| PackError::NotACoursePack)
}

/// What an import checks against: the active course and its content.
pub struct PackCheck<'a> {
    pub active: Lang,
    pub syllabus: &'a Syllabus,
    pub forms: &'a Forms,
    pub items: &'a [Item],
    pub numbers: &'a [String],
}

/// An exercise of a pack as an `Exercise`, when it has the shape of a P1 exercise plus `id`,
/// `item_id`, `facet` and `origin` (and optional `tiles`).
fn pack_exercise(value: &Value) -> Option<Exercise> {
    let object = value.as_object()?;
    let mut answer: Map<String, Value> = object.clone();
    for field in ["id", "facet", "origin", "tiles"] {
        answer.remove(field);
    }
    if !lesson_prompt_file().schema_valid(&json!({ "exercises": [Value::Object(answer)] })) {
        return None;
    }
    let mut ex: Exercise = serde_json::from_value(value.clone()).ok()?;
    // Imported tiles are never kept: no check looks at them, so they are rebuilt from the target.
    if let Body::TranslateToTarget { tiles, .. } = &mut ex.body {
        *tiles = None;
    }
    (!ex.id.is_empty() && ex.kind().facet() == ex.facet).then_some(ex)
}

/// The skill and level of a skill id, when the syllabus has it.
fn skill_level(syllabus: &Syllabus, skill_id: &str) -> Option<Cefr> {
    syllabus
        .skills()
        .find(|(_, skill)| skill.id == skill_id)
        .map(|(_, skill)| skill.cefr)
}

/// Every lemma of the skills up to and including `skill_id`, in syllabus order.
fn lemmas_up_to(syllabus: &Syllabus, skill_id: &str) -> Vec<String> {
    let mut lemmas = Vec::new();
    for (_, skill) in syllabus.skills() {
        lemmas.extend(skill.lemmas.iter().map(|lemma| lemma.lemma.clone()));
        if skill.id == skill_id {
            break;
        }
    }
    lemmas
}

/// The exercises of one pack lesson that pass, in order, and how many were dropped.
fn checked_lesson(
    pack: &PackLesson,
    check: &PackCheck,
    rng: &mut dyn Rng,
) -> (Vec<Exercise>, usize) {
    let Some(cefr) = skill_level(check.syllabus, &pack.skill_id) else {
        return (Vec::new(), pack.exercises.len());
    };
    let lang = check.syllabus.lang;
    let allowed = Allowed::for_skill(check.syllabus, check.numbers, &pack.skill_id);
    let stems = stems_of(lang, &allowed);
    let ai_check = AiCheck {
        lang,
        forms: check.forms,
        allowed: &allowed,
        stems: &stems,
        max_words: max_words(cefr),
    };
    let mut kept: Vec<Exercise> = Vec::new();
    let mut ids: HashSet<String> = HashSet::new();
    let mut seen: Vec<(ExerciseType, String)> = Vec::new();
    let mut dropped = 0;
    for value in &pack.exercises {
        let passed = pack_exercise(value).filter(|ex| {
            let Some(item) = check.items.iter().find(|item| item.id == ex.item_id) else {
                return false;
            };
            !ids.contains(&ex.id)
                && check_ai_exercise(ex, &focus_of(check.syllabus, item), &seen, &ai_check).is_ok()
        });
        match passed {
            Some(mut ex) => {
                ex.origin = Origin::CoursePack;
                if let Body::TranslateToTarget { target, tiles, .. } = &mut ex.body {
                    *tiles = Some(word_bank(target, lang, &allowed, check.forms, rng));
                }
                ids.insert(ex.id.clone());
                seen_push(&mut seen, lang, &ex);
                kept.push(ex);
            }
            None => dropped += 1,
        }
    }
    (kept, dropped)
}

/// A pack story, when it has the story shape and passes S2 to S4 and S6 for a skill that ends
/// its unit.
fn checked_story(pack: &PackStory, check: &PackCheck) -> Option<Story> {
    if !story_schema_valid(&pack.story) || !has_story(check.syllabus, &pack.skill_id) {
        return None;
    }
    let story: Story = serde_json::from_value(pack.story.clone()).ok()?;
    let mut prompt = StoryPrompt::new(check.syllabus, &[], &pack.skill_id, 1)?;
    prompt.focus = Vec::new();
    prompt.allowed_lemmas = lemmas_up_to(check.syllabus, &pack.skill_id);
    let ctx = StoryCtx {
        prompt,
        syllabus: check.syllabus,
        forms: check.forms,
        numbers: check.numbers,
    };
    check_story(&story, &ctx).is_ok().then_some(story)
}

/// Checks every exercise and story of `pack` against the active course and stores what passes:
/// each lesson with an exercise left at its skill's next number, origin `course_pack`; each story
/// at its skill's next story number. A pack for another language is refused.
pub fn import_course_pack(
    store: &mut Store,
    pack: &CoursePack,
    check: &PackCheck,
    now_ms: i64,
) -> Result<CoursePackReport, PackError> {
    if pack.lang != check.active {
        return Err(PackError::WrongLanguage {
            pack: pack.lang,
            active: check.active,
        });
    }
    let mut lessons: Vec<(Lesson, Option<String>)> = Vec::new();
    let mut dropped = 0;
    // The word-bank tiles of imported exercises, made as a template exercise's are; seeded by the
    // time of import.
    let mut rng = SplitMix64::new(now_ms as u64);
    for pack_lesson in &pack.lessons {
        let (exercises, lost) = checked_lesson(pack_lesson, check, &mut rng);
        dropped += lost;
        if exercises.is_empty() {
            continue;
        }
        lessons.push((
            Lesson {
                schema: Lesson::SCHEMA.into(),
                lang: pack.lang,
                skill_id: pack_lesson.skill_id.clone(),
                seq: 0,
                origin: Origin::CoursePack,
                exercises,
                ai: None,
            },
            pack_lesson.prompt.clone(),
        ));
    }
    let stories: Vec<(String, Story, Option<String>)> = pack
        .stories
        .iter()
        .filter_map(|pack_story| {
            checked_story(pack_story, check).map(|story| {
                (
                    pack_story.skill_id.clone(),
                    story,
                    pack_story.prompt.clone(),
                )
            })
        })
        .collect();
    store.import_pack_rows(pack.lang, &lessons, &stories, now_ms)?;
    let message = format!(
        "Imported {} lessons and {} stories; {dropped} exercises were dropped because they did not pass Parley's checks.",
        lessons.len(),
        stories.len()
    );
    Ok(CoursePackReport {
        lessons: lessons.len(),
        stories: stories.len(),
        dropped,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_envelope_is_checked_before_anything_else() {
        assert_eq!(
            parse_course_pack(b"{\"schema\": \"x\"}"),
            Err(PackError::NotACoursePack)
        );
        assert_eq!(parse_course_pack(b"[1, 2]"), Err(PackError::NotACoursePack));
        let missing = br#"{"schema": "parley.coursepack/1", "lang": "es"}"#;
        assert_eq!(parse_course_pack(missing), Err(PackError::NotACoursePack));
        let big = vec![b' '; 5_000_001];
        assert_eq!(parse_course_pack(&big), Err(PackError::TooLarge));
        let empty = br#"{"schema": "parley.coursepack/1", "lang": "de", "syllabus_version": 1,
            "app_version": "0.7.0", "created_at": "2026-11-02", "licence": "CC-BY-SA-4.0",
            "lessons": [], "stories": []}"#;
        let pack = parse_course_pack(empty).unwrap();
        assert_eq!(pack.lang, Lang::De);
        assert_eq!(
            PackError::WrongLanguage {
                pack: Lang::De,
                active: Lang::Fr
            }
            .message(),
            "This course pack is for German; your active course is French."
        );
    }
}
