//! Acceptance tests of module 07 in the core (docs/modules/07-lesson-generation.md, Acceptance
//! tests 1 to 22 and 24, except the parts that are UI only): fresh lessons (P1) with validators L1
//! to L9, template fill and the cache rule; the lesson start with a provider, Use the course pack
//! instead and Prepare next lesson; graded stories (P2) with S1 to S6; course packs; the validity
//! rate; the replay of gate G1; stories in the path and in backups.
//!
//! References: the spec, docs/PUBLIK-INTEGRATION.md §6.1, §6.3, §6.4 and §9, and fixtures true by
//! construction: the course in tests/fixtures/course07/es/ (whose lemmas are those of the §6.3
//! example), the answers in tests/fixtures/ai/P1/ and P2/ (each README states the plan, the context
//! and the one rule each file breaks, with the word counts worked out by hand) and the course packs
//! in tests/fixtures/coursepack/. No test reaches the network: AI answers come from a scripted
//! client that records every request.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use parley_core::ai::{
    AiClient, AiError, AiStatus, AudioReply, Cache, ChatReply, ChatRequest,
    FRESH_LESSON_UNAVAILABLE, LessonAiInfo, LessonCtx, LessonDraft, LessonPrompt, OFFLINE_LESSON,
    ProviderKind, SpeechRequest, StoryCtx, StoryPrompt, TextLang, TranscribeRequest,
    TranscriptReply, allowed_from_list, allowed_lemmas, cache_key_json, check_story,
    coverage_with_stems, generate_lesson, generate_story, idempotency_key, identify, is_language,
    lesson_checks, lesson_prompt_file, story_xp,
};
use parley_core::api::COMMANDS;
use parley_core::app::{AiAccess, Config, CourseFiles, Env, LessonBegin, Parley};
use parley_core::content::{
    Cefr, Forms, Item, Lang, Syllabus, derive_items, load_numbers, parse_syllabus,
};
use parley_core::coursepack::{PackCheck, export_course_pack, import_course_pack};
use parley_core::env::{Clock, FailingNet, Fs, FsError, LocalDate, SplitMix64};
use parley_core::error::{AppError, ErrorCode};
use parley_core::exercise::{
    Body, Exercise, ExerciseType, Lesson, LessonRun, Origin, Plan, PlanEntry,
};
use parley_core::grading::{Answer, Verdict};
use parley_core::store::Store;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SKILL: &str = "es.a1.u03.s01";
const BASICS: &str = "es.a1.u01.s01";
const INVALID: &str = "The AI answer could not be used.";
/// The allowed lemma list of the §6.3 example: function words in syllabus order, then the lemmas
/// of es.a1.u01.s01 (introduced) and es.a1.u03.s01 by frequency rank.
const EXAMPLE_ALLOWED: &str = "el, la, un, una, y, de, en, a, no, sí, yo, tú, usted, ser, estar, tener, querer, por favor, gracias, café, té, agua, leche, pan, zumo, bocadillo, con, sin, azúcar";

// ---------------------------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------------------------

/// The repository root: the first ancestor of this crate that holds the workspace Cargo.toml.
fn repo_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if let Ok(text) = fs::read_to_string(dir.join("Cargo.toml"))
            && text.contains("[workspace]")
        {
            return dir;
        }
        assert!(dir.pop(), "no workspace Cargo.toml above the crate");
    }
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()))
}

fn json_file(relative: &str) -> Value {
    serde_json::from_str(&read(relative)).unwrap_or_else(|e| panic!("{relative}: {e}"))
}

/// The reply text of a P1 or P2 fixture: a JSON string is the reply itself (prose); anything else
/// is sent as the JSON text it is.
fn reply_text(relative: &str) -> String {
    match json_file(relative) {
        Value::String(text) => text,
        other => other.to_string(),
    }
}

fn p1(name: &str) -> String {
    reply_text(&format!("tests/fixtures/ai/P1/{name}"))
}

fn p2(name: &str) -> String {
    reply_text(&format!("tests/fixtures/ai/P2/{name}"))
}

/// 2026-11-06T12:00:00Z, a Friday.
const NOW: i64 = 1_793_966_400_000;

struct FixedClock;

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        NOW
    }
    fn utc_offset_minutes(&self) -> i32 {
        0
    }
}

fn clock() -> Box<dyn Clock + Send + Sync> {
    Box::new(FixedClock)
}

/// One course of the repository, parsed.
struct Course {
    syllabus: Syllabus,
    forms: Forms,
    numbers: Vec<String>,
    items: Vec<Item>,
}

fn load_course(dir: &str) -> Course {
    let syllabus = parse_syllabus("syllabus.yaml", &read(&format!("{dir}/syllabus.yaml")))
        .unwrap_or_else(|e| panic!("{dir}/syllabus.yaml parses: {e:?}"));
    let forms = Forms::load_tsv(read(&format!("{dir}/forms.tsv")).as_bytes())
        .unwrap_or_else(|e| panic!("{dir}/forms.tsv loads: {e:?}"));
    let numbers_path = repo_root().join(format!("{dir}/numbers.yaml"));
    let numbers = if numbers_path.exists() {
        load_numbers(&read(&format!("{dir}/numbers.yaml"))).expect("numbers.yaml loads")
    } else {
        Vec::new()
    };
    let items = derive_items(&syllabus, &forms);
    Course {
        syllabus,
        forms,
        numbers,
        items,
    }
}

/// The test course whose lemmas are those of the §6.3 example.
fn course07() -> Course {
    load_course("tests/fixtures/course07/es")
}

fn entry(n: u32, item: &str, kind: ExerciseType, focus: &str) -> PlanEntry {
    PlanEntry {
        n,
        item_id: format!("{SKILL}#{item}"),
        facet: kind.facet(),
        kind,
        focus: focus.to_string(),
        new: true,
    }
}

/// The five-entry plan of the §6.3 example (tests/fixtures/ai/P1/README.md).
fn example_plan() -> Plan {
    Plan {
        entries: vec![
            entry(1, "l:café", ExerciseType::PickMeaning, "el café"),
            entry(2, "l:agua", ExerciseType::TranslateToTarget, "el agua"),
            entry(
                3,
                "e:2",
                ExerciseType::TranslateToEn,
                "Quiero un café con leche.",
            ),
            entry(4, "l:pan", ExerciseType::FillGap, "el pan"),
            entry(5, "l:té", ExerciseType::MatchPairs, "el té"),
        ],
    }
}

fn example_prompt(course: &Course, seq: u32) -> LessonPrompt {
    LessonPrompt::new(
        &course.syllabus,
        &course.items,
        &[BASICS.to_string()],
        SKILL,
        seq,
        &example_plan(),
    )
    .expect("es.a1.u03.s01 is a skill of the test course")
}

fn lesson_ctx(course: &Course, prompt: LessonPrompt) -> LessonCtx<'_> {
    LessonCtx {
        prompt,
        syllabus: &course.syllabus,
        forms: &course.forms,
        items: &course.items,
        numbers: &course.numbers,
        sentences: &[],
    }
}

/// An `AiClient` that answers chat requests from a queue (or one answer for every request) and
/// records every request it is sent.
struct Scripted {
    kind: ProviderKind,
    needs_credit: bool,
    replies: Mutex<VecDeque<Result<ChatReply, AiError>>>,
    always: Option<String>,
    sent: Mutex<Vec<ChatRequest>>,
}

impl Scripted {
    fn new(kind: ProviderKind) -> Scripted {
        Scripted {
            kind,
            needs_credit: false,
            replies: Mutex::new(VecDeque::new()),
            always: None,
            sent: Mutex::new(Vec::new()),
        }
    }

    /// Answers every request with `content`.
    fn always(content: &str) -> Arc<Scripted> {
        let mut client = Scripted::new(ProviderKind::Publik);
        client.always = Some(content.to_string());
        Arc::new(client)
    }

    fn reply(self, content: &str) -> Scripted {
        self.replies.lock().unwrap().push_back(Ok(ChatReply {
            content: content.to_string(),
            charge_micros: Some(15_000),
            request_id: Some("req_test".into()),
        }));
        self
    }

    fn fail(self, error: AiError) -> Scripted {
        self.replies.lock().unwrap().push_back(Err(error));
        self
    }

    fn sent(&self) -> Vec<ChatRequest> {
        self.sent.lock().unwrap().clone()
    }

    fn requests(&self) -> usize {
        self.sent.lock().unwrap().len()
    }
}

impl AiClient for Scripted {
    fn chat(&self, req: ChatRequest) -> Result<ChatReply, AiError> {
        self.sent.lock().unwrap().push(req);
        if let Some(content) = &self.always {
            return Ok(ChatReply {
                content: content.clone(),
                charge_micros: Some(15_000),
                request_id: None,
            });
        }
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("a scripted reply for every request")
    }

    fn chat_stream(
        &self,
        req: ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<ChatReply, AiError> {
        let reply = self.chat(req)?;
        on_delta(&reply.content);
        Ok(reply)
    }

    fn transcribe(&self, _req: TranscribeRequest) -> Result<TranscriptReply, AiError> {
        Err(AiError::Offline)
    }

    fn speech(&self, _req: SpeechRequest) -> Result<AudioReply, AiError> {
        Err(AiError::Offline)
    }

    fn kind(&self) -> ProviderKind {
        self.kind
    }

    fn status(&self) -> AiStatus {
        AiStatus {
            needs_credit: self.needs_credit,
            ..AiStatus::default()
        }
    }

    fn chat_model(&self, tier: &str) -> String {
        tier.to_string()
    }
}

fn access(client: &Arc<Scripted>) -> Option<AiAccess> {
    let client: Arc<dyn AiClient> = client.clone();
    Some(Ok(client))
}

fn generate(course: &Course, prompt: LessonPrompt, reply: &str) -> (LessonDraft, Scripted, Cache) {
    let client = Scripted::new(ProviderKind::Publik).reply(reply);
    let cache = Cache::in_memory(clock());
    let mut rng = SplitMix64::new(7);
    let ctx = lesson_ctx(course, prompt);
    let draft = generate_lesson(&client, &cache, &example_plan(), &ctx, &mut rng)
        .expect("a received answer always gives a lesson");
    (draft, client, cache)
}

/// The answer of the §6.3 plan in fixture `name`, with the course at A1.
fn example_draft(name: &str) -> LessonDraft {
    let course = course07();
    let prompt = example_prompt(&course, 1);
    generate(&course, prompt, &p1(name)).0
}

/// (position, rule) of every dropped exercise.
fn dropped(draft: &LessonDraft) -> Vec<(usize, String)> {
    draft
        .dropped
        .iter()
        .map(|(n, failure)| (*n, failure.rule.clone()))
        .collect()
}

fn origins(lesson: &Lesson) -> Vec<Origin> {
    lesson.exercises.iter().map(|ex| ex.origin).collect()
}

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// The app over the test course.

struct DirFs(PathBuf);

fn fs_error(path: &Path, e: std::io::Error) -> FsError {
    if e.kind() == std::io::ErrorKind::NotFound {
        FsError::NotFound(path.to_path_buf())
    } else {
        FsError::Io(e.to_string())
    }
}

impl DirFs {
    fn full(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.0.join(path)
        }
    }
}

impl Fs for DirFs {
    fn read(&self, path: &Path) -> Result<Vec<u8>, FsError> {
        fs::read(self.full(path)).map_err(|e| fs_error(path, e))
    }
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), FsError> {
        let full = self.full(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(|e| fs_error(path, e))?;
        }
        fs::write(full, bytes).map_err(|e| fs_error(path, e))
    }
    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        fs::rename(self.full(from), self.full(to)).map_err(|e| fs_error(from, e))
    }
    fn remove(&self, path: &Path) -> Result<(), FsError> {
        fs::remove_file(self.full(path)).map_err(|e| fs_error(path, e))
    }
}

/// The app's service over `dir` with the test course as its Spanish course, the course started.
fn course_app(dir: &Path) -> Parley {
    let mut courses = BTreeMap::new();
    courses.insert(
        Lang::Es,
        CourseFiles {
            syllabus_yaml: read("tests/fixtures/course07/es/syllabus.yaml"),
            forms_tsv: read("tests/fixtures/course07/es/forms.tsv"),
            numbers_yaml: None,
        },
    );
    let mut app = Parley::new(
        Config {
            data_dir: dir.to_path_buf(),
            courses,
            packs_lock: "schema = \"parley.packs/1\"\n".to_string(),
        },
        Env {
            clock: clock(),
            rng: Box::new(SplitMix64::new(11)),
            fs: Box::new(DirFs(dir.to_path_buf())),
            net: Box::new(FailingNet),
        },
    );
    app.course_start("es").expect("the test course starts");
    app
}

/// The same with `ai.provider` set to `provider`.
fn app_with(dir: &Path, provider: &str) -> Parley {
    let mut app = course_app(dir);
    let mut ai = app.settings_get().ai;
    ai.provider = provider.to_string();
    app.save_ai_settings(&ai).expect("the provider is saved");
    app
}

fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("a temporary directory")
}

/// Some answer for every exercise, right or wrong; each one reviews its card.
fn answer_all(app: &mut Parley, session: &str, exercises: &[Exercise]) {
    for ex in exercises {
        let answer = match &ex.body {
            Body::PickMeaning { answer_index, .. } => Answer::Choice(*answer_index),
            Body::MatchPairs { pairs } => {
                Answer::Pairs((0..pairs.len() as u32).map(|i| [i, i]).collect())
            }
            Body::FillGap { answer, .. } => Answer::Text(answer.clone()),
            Body::TranslateToTarget { target, .. } => Answer::Text(target.clone()),
            Body::TranslateToEn { accepted, .. } => Answer::Text(accepted[0].clone()),
            Body::ListenType { text, .. } => Answer::Text(text.clone()),
            Body::Speak { text, .. } => Answer::Text(text.clone()),
        };
        app.exercise_submit(session, &ex.id, &answer, Some(4_000), false)
            .expect("the answer is graded");
    }
}

/// The type and body of each exercise as JSON, without id and origin, to compare lessons.
fn bodies(exercises: &[Exercise]) -> Vec<Value> {
    exercises
        .iter()
        .map(|ex| {
            let mut value = serde_json::to_value(ex).unwrap();
            let object = value.as_object_mut().unwrap();
            object.remove("id");
            object.remove("origin");
            object.remove("tiles");
            value
        })
        .collect()
}

/// A finished lesson of es.a1.u01.s01, so its lemmas are in the allowed list of later lessons
/// and stories (the P2 fixtures are written for that list).
fn introduce_basics(app: &mut Parley) {
    let lesson = app.lesson_start(BASICS, "learn").unwrap();
    answer_all(app, &lesson.session_id, &lesson.exercises);
    app.lesson_finish(&lesson.session_id).unwrap();
}

fn backup(app: &mut Parley, dir: &Path, name: &str) -> Value {
    let path = dir.join(name);
    app.backup_export(&path).expect("the backup is written");
    serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap()
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 1: the request of the §6.3 example.
// ---------------------------------------------------------------------------------------------

/// Acceptance test 1: the prompt values of the §6.3 example come from the course (the allowed list
/// in the example's order) and the request sent is exactly the golden body.
#[test]
fn the_example_plan_sends_exactly_the_section_6_3_request() {
    let course = course07();
    let introduced = [BASICS.to_string()];
    let allowed = allowed_lemmas(&course.syllabus, &introduced, SKILL);
    assert_eq!(allowed.join(", "), EXAMPLE_ALLOWED);
    let prompt = example_prompt(&course, 1);
    assert_eq!(prompt.allowed_lemmas.join(", "), EXAMPLE_ALLOWED);
    assert_eq!(prompt.names, ["Ana", "Luis", "Madrid"]);
    assert_eq!((prompt.cefr, prompt.seq), (Cefr::A1, 1));
    assert_eq!(prompt.skill_title, "Food and drink");
    let glosses: Vec<Option<&str>> = prompt.plan.iter().map(|e| e.gloss.as_deref()).collect();
    assert_eq!(
        glosses,
        [
            Some("coffee"),
            Some("water"),
            None,
            Some("bread"),
            Some("tea")
        ]
    );

    let expected = json_file("tests/cases/07-lesson-generation/render-es-a1/expected.json");
    let (_, client, _) = generate(&course, prompt.clone(), &p1("valid.json"));
    let sent = client.sent();
    assert_eq!(sent.len(), 1, "one request");
    assert_eq!(sent[0].body(false), expected);
    assert_eq!(
        lesson_prompt_file().render(&prompt.vars()).unwrap()[1].content,
        expected["messages"][1]["content"]
    );
}

/// Acceptance test 13 (keys, §6.1 and §6.3): the cache key input and the idempotency key.
#[test]
fn the_cache_key_input_is_that_of_section_6_3() {
    let course = course07();
    let prompt = example_prompt(&course, 1);
    let plan_text = prompt.vars()["plan"].clone();
    assert!(plan_text.starts_with(
        "1. item_id=es.a1.u03.s01#l:café type=pick_meaning focus=\"el café\" (coffee) new=true\n"
    ));
    assert!(plan_text.contains(
        "3. item_id=es.a1.u03.s01#e:2 type=translate_to_en focus=\"Quiero un café con leche.\" new=true"
    ));
    let input = prompt.key_input();
    assert_eq!(
        input,
        json!({
            "lang": "es",
            "skill_id": SKILL,
            "seq": 1,
            "plan": plan_text,
            "allowed_sha256": sha256_hex(EXAMPLE_ALLOWED),
            "names": ["Ana", "Luis", "Madrid"],
        })
    );
    let (_, client, _) = generate(&course, prompt.clone(), &p1("valid.json"));
    let key_json = cache_key_json("lesson", 1, ProviderKind::Publik, "publik-balanced", &input);
    assert_eq!(
        client.sent()[0].idempotency_key,
        Some(idempotency_key(&key_json, 1))
    );
    assert_eq!(client.sent()[0].timeout_s, 45);
    assert_eq!(client.sent()[0].max_tokens, 2000);
}

// ---------------------------------------------------------------------------------------------
// Acceptance tests 2 to 12: validators L1 to L9 (PUBLIK-INTEGRATION test 21).
// ---------------------------------------------------------------------------------------------

/// Acceptance test 2: the example answer gives 5 AI exercises in plan order, none dropped.
#[test]
fn the_valid_answer_gives_five_ai_exercises() {
    let draft = example_draft("valid.json");
    assert!(draft.dropped.is_empty(), "{:?}", draft.dropped);
    assert_eq!(draft.notice, None);
    assert!(!draft.cached);
    assert_eq!(draft.charge_micros, Some(15_000));
    let lesson = &draft.lesson;
    assert_eq!(lesson.origin, Origin::Ai);
    assert_eq!(origins(lesson), vec![Origin::Ai; 5]);
    let ids: Vec<&str> = lesson.exercises.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["e1", "e2", "e3", "e4", "e5"]);
    let plan = example_plan();
    for (ex, entry) in lesson.exercises.iter().zip(&plan.entries) {
        assert_eq!(ex.item_id, entry.item_id);
        assert_eq!(ex.kind(), entry.kind);
        assert_eq!(ex.facet, entry.facet);
    }
    let Body::TranslateToTarget {
        target,
        accepted,
        tiles,
        ..
    } = &lesson.exercises[1].body
    else {
        panic!("exercise 2 is translate_to_target")
    };
    assert_eq!(target, "Quiero agua, por favor.");
    assert_eq!(accepted.len(), 2);
    assert!(tiles.is_some(), "a typed answer also gets word-bank tiles");
    assert_eq!(
        lesson.ai,
        Some(LessonAiInfo {
            provider: ProviderKind::Publik,
            valid: 5,
            planned: 5,
        })
    );
    assert_eq!((lesson.skill_id.as_str(), lesson.seq), (SKILL, 1));
}

/// Acceptance test 3: L1, a reply that is not JSON → the template lesson with the notice, nothing
/// cached.
#[test]
fn l1_a_reply_that_is_not_json_gives_the_template_lesson_with_the_notice() {
    let course = course07();
    let prompt = example_prompt(&course, 1);
    let (draft, client, cache) = generate(&course, prompt, &p1("L1-not-json.json"));
    assert_eq!(client.requests(), 1);
    assert_eq!(draft.notice.as_deref(), Some(FRESH_LESSON_UNAVAILABLE));
    assert_eq!(
        FRESH_LESSON_UNAVAILABLE,
        "A fresh lesson was not available, so this one comes from the course pack."
    );
    assert_eq!(draft.lesson.origin, Origin::Template);
    assert!(!draft.lesson.exercises.is_empty());
    assert!(
        draft
            .lesson
            .exercises
            .iter()
            .all(|ex| ex.origin == Origin::Template)
    );
    assert_eq!(cache.len(), 0, "an invalid answer is not cached");
    assert_eq!(
        draft.lesson.ai.as_ref().map(|ai| (ai.valid, ai.planned)),
        Some((0, 5))
    );
}

/// Acceptance test 4: L2, 7 exercises for a 5-entry plan → the last 2 dropped, 5 shown.
#[test]
fn l2_extra_exercises_are_dropped() {
    let draft = example_draft("L2-seven-exercises.json");
    assert_eq!(
        dropped(&draft),
        [(6, "L2".to_string()), (7, "L2".to_string())]
    );
    assert_eq!(origins(&draft.lesson), vec![Origin::Ai; 5]);
    assert_eq!(draft.notice, None);
}

/// Acceptance test 5: L3, a wrong item_id at position 2 → exercise 2 from the template.
#[test]
fn l3_a_wrong_item_is_replaced_by_a_template_exercise() {
    let draft = example_draft("L3-wrong-item-id.json");
    assert_eq!(dropped(&draft), [(2, "L3".to_string())]);
    let lesson = &draft.lesson;
    assert_eq!(lesson.exercises.len(), 5);
    assert_eq!(
        origins(lesson),
        [
            Origin::Ai,
            Origin::Template,
            Origin::Ai,
            Origin::Ai,
            Origin::Ai
        ]
    );
    assert_eq!(lesson.exercises[1].item_id, format!("{SKILL}#l:agua"));
    assert_eq!(lesson.exercises[1].id, "e2");
    assert_eq!(lesson.origin, Origin::Ai);
    assert_eq!(draft.notice, None);
    assert_eq!(lesson.ai.as_ref().map(|ai| ai.valid), Some(4));
}

/// Acceptance test 6: L4, an English sentence as the Spanish target → dropped.
#[test]
fn l4_a_target_in_the_wrong_language_is_dropped() {
    let draft = example_draft("L4-english-target.json");
    assert_eq!(dropped(&draft), [(2, "L4".to_string())]);
    assert_eq!(draft.lesson.exercises[1].origin, Origin::Template);

    // The identification itself: clear sentences of each language, and the four-word floor.
    assert!(!is_language("I want water please now", TextLang::Es));
    assert_eq!(
        identify("The children are playing in the garden behind the house.").map(|(l, _)| l),
        Some(TextLang::En)
    );
    assert_eq!(
        identify("Los niños juegan en el jardín detrás de la casa.").map(|(l, _)| l),
        Some(TextLang::Es)
    );
    assert_eq!(
        identify("Les enfants jouent dans le jardin derrière la maison.").map(|(l, _)| l),
        Some(TextLang::Fr)
    );
    assert_eq!(
        identify("Die Kinder spielen im Garten hinter dem Haus.").map(|(l, _)| l),
        Some(TextLang::De)
    );
    assert!(
        is_language("water please now", TextLang::Es),
        "under four words is not checked"
    );
}

/// Acceptance test 7: L5, 5 of 6 words covered (83%) → dropped; at A2 a 10-word sentence with one
/// uncovered word (90%) → kept.
#[test]
fn l5_coverage_under_ninety_percent_is_dropped() {
    let course = course07();
    let prompt = example_prompt(&course, 1);
    let allowed = allowed_from_list(&course.syllabus, &course.numbers, &prompt.allowed_lemmas);
    assert_eq!(
        coverage_with_stems(
            Lang::Es,
            "Quiero agua con hielo, por favor.",
            &allowed,
            &course.forms
        ),
        (5, 6)
    );
    assert_eq!(
        coverage_with_stems(
            Lang::Es,
            "Yo quiero agua con hielo y un té sin azúcar.",
            &allowed,
            &course.forms
        ),
        (9, 10)
    );

    let draft = example_draft("L5-uncovered-word.json");
    assert_eq!(dropped(&draft), [(2, "L5".to_string())]);

    let mut a2 = example_prompt(&course, 1);
    a2.cefr = Cefr::A2;
    let (draft, _, _) = generate(&course, a2, &p1("L5-ninety-percent-a2.json"));
    assert!(draft.dropped.is_empty(), "{:?}", draft.dropped);
    assert_eq!(origins(&draft.lesson), vec![Origin::Ai; 5]);
}

/// L5's stem fallback: a form missing from the forms table still counts when its Snowball stem is
/// an allowed lemma's stem. The table lists tengo, tienes, tiene, tenemos and tienen, not
/// "tenéis"; the Spanish Snowball stemmer gives "ten" for both "tenéis" (the verb ending -éis
/// removed from the region after "ten") and "tener" (the ending -er removed).
#[test]
fn l5_counts_a_form_by_its_stem_when_the_forms_table_lacks_it() {
    let course = course07();
    let prompt = example_prompt(&course, 1);
    let allowed = allowed_from_list(&course.syllabus, &course.numbers, &prompt.allowed_lemmas);
    assert_eq!(
        coverage_with_stems(Lang::Es, "Tenéis café.", &allowed, &course.forms),
        (2, 2),
        "tenéis by its stem, café as a lemma"
    );
    assert_eq!(
        parley_core::exercise::coverage(Lang::Es, "Tenéis café.", &allowed, &course.forms),
        (1, 2),
        "without the stems, tenéis is not covered"
    );
}

/// Acceptance test 8: L6, each broken answer is dropped alone.
#[test]
fn l6_answers_that_do_not_fit_are_dropped() {
    for (file, position) in [
        ("L6-target-not-accepted.json", 2),
        ("L6-two-gaps.json", 4),
        ("L6-two-word-answer.json", 4),
        ("L6-full-sentence-mismatch.json", 4),
        ("L6-duplicate-options.json", 1),
        ("L6-duplicate-pairs.json", 5),
    ] {
        let draft = example_draft(file);
        assert_eq!(dropped(&draft), [(position, "L6".to_string())], "{file}");
        assert_eq!(
            draft.lesson.exercises[position - 1].origin,
            Origin::Template,
            "{file}"
        );
        assert_eq!(draft.notice, None, "{file}");
    }
}

/// Acceptance test 9: L7, the second of two exercises with the same normalised sentence is dropped.
#[test]
fn l7_a_repeated_sentence_is_dropped() {
    let draft = example_draft("L7-duplicate-target.json");
    assert_eq!(dropped(&draft), [(4, "L7".to_string())]);
    assert_eq!(draft.lesson.exercises[2].origin, Origin::Ai);
}

/// L7: duplicates inside `accepted` are removed, not failed.
#[test]
fn l7_duplicate_accepted_answers_are_removed_not_failed() {
    let course = course07();
    let mut answer = json_file("tests/fixtures/ai/P1/valid.json");
    answer["exercises"][1]["accepted"] = json!([
        "Quiero agua, por favor.",
        "quiero agua por favor",
        "Yo quiero agua, por favor."
    ]);
    let (draft, _, _) = generate(&course, example_prompt(&course, 1), &answer.to_string());
    assert!(draft.dropped.is_empty(), "{:?}", draft.dropped);
    let Body::TranslateToTarget { accepted, .. } = &draft.lesson.exercises[1].body else {
        panic!("exercise 2 is translate_to_target")
    };
    assert_eq!(
        accepted,
        &["Quiero agua, por favor.", "Yo quiero agua, por favor."]
    );
}

/// Acceptance test 10: L8, a 9-word target at A1 (limit 8) → dropped.
#[test]
fn l8_a_sentence_too_long_for_the_level_is_dropped() {
    let draft = example_draft("L8-nine-words.json");
    assert_eq!(dropped(&draft), [(2, "L8".to_string())]);
}

/// Acceptance test 11: L9, an exercise without its focus lemma → dropped.
#[test]
fn l9_an_exercise_without_its_focus_is_dropped() {
    let draft = example_draft("L9-no-focus.json");
    assert_eq!(dropped(&draft), [(2, "L9".to_string())]);
}

/// Acceptance test 12: 3 of 5 dropped → the whole lesson from templates with the notice; the
/// schema-valid answer is still cached (§6.1).
#[test]
fn more_than_half_dropped_gives_the_template_lesson_with_the_notice() {
    let course = course07();
    let (draft, _, cache) = generate(
        &course,
        example_prompt(&course, 1),
        &p1("three-dropped.json"),
    );
    assert_eq!(
        dropped(&draft),
        [
            (2, "L9".to_string()),
            (4, "L6".to_string()),
            (5, "L6".to_string())
        ]
    );
    assert_eq!(draft.notice.as_deref(), Some(FRESH_LESSON_UNAVAILABLE));
    assert_eq!(draft.lesson.origin, Origin::Template);
    assert!(
        draft
            .lesson
            .exercises
            .iter()
            .all(|ex| ex.origin == Origin::Template)
    );
    assert_eq!(cache.len(), 1, "a schema-valid answer is cached");
    assert_eq!(
        draft.lesson.ai.as_ref().map(|ai| (ai.valid, ai.planned)),
        Some((2, 5))
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 13: the cache (PUBLIK-INTEGRATION test 22).
// ---------------------------------------------------------------------------------------------

/// Acceptance test 13: a schema-valid answer is stored; the same seq again makes no request; seq + 1
/// does.
#[test]
fn the_same_lesson_number_is_served_from_the_cache_and_the_next_is_not() {
    let course = course07();
    let client = Scripted::new(ProviderKind::Publik)
        .reply(&p1("valid.json"))
        .reply(&p1("valid.json"));
    let cache = Cache::in_memory(clock());
    let mut rng = SplitMix64::new(1);
    let plan = example_plan();

    let first = generate_lesson(
        &client,
        &cache,
        &plan,
        &lesson_ctx(&course, example_prompt(&course, 1)),
        &mut rng,
    )
    .unwrap();
    assert_eq!(
        (client.requests(), cache.len(), first.cached),
        (1, 1, false)
    );

    let again = generate_lesson(
        &client,
        &cache,
        &plan,
        &lesson_ctx(&course, example_prompt(&course, 1)),
        &mut rng,
    )
    .unwrap();
    assert_eq!(client.requests(), 1, "a hit makes no request");
    assert!(again.cached);
    assert_eq!(again.charge_micros, None);
    assert_eq!(
        bodies(&again.lesson.exercises),
        bodies(&first.lesson.exercises)
    );

    let next = generate_lesson(
        &client,
        &cache,
        &plan,
        &lesson_ctx(&course, example_prompt(&course, 2)),
        &mut rng,
    )
    .unwrap();
    assert_eq!(
        client.requests(),
        2,
        "the next lesson number is a new request"
    );
    assert!(!next.cached);
    assert_eq!(next.lesson.seq, 2);
    assert_eq!(cache.len(), 2);
}

/// Errors other than an unusable answer are the caller's to show: generate_lesson returns them.
#[test]
fn a_transport_error_is_returned_as_it_is() {
    let course = course07();
    let client = Scripted::new(ProviderKind::Publik).fail(AiError::Busy);
    let cache = Cache::in_memory(clock());
    let result = generate_lesson(
        &client,
        &cache,
        &example_plan(),
        &lesson_ctx(&course, example_prompt(&course, 1)),
        &mut SplitMix64::new(1),
    );
    assert_eq!(result.unwrap_err(), AiError::Busy);
}

// ---------------------------------------------------------------------------------------------
// Acceptance tests 14, 15, 16 and 21: the lesson start with a provider.
// ---------------------------------------------------------------------------------------------

/// Acceptance test 14: with needs_credit set, lesson_start makes no call and returns a template
/// lesson.
#[test]
fn needs_credit_makes_no_call_and_gives_a_template_lesson() {
    let dir = tempdir();
    let mut app = app_with(dir.path(), "publik");
    let mut client = Scripted::new(ProviderKind::Publik).reply(&p1("valid.json"));
    client.needs_credit = true;
    let client = Arc::new(client);
    let started = app
        .lesson_start_with(SKILL, "learn", access(&client))
        .unwrap();
    assert_eq!(client.requests(), 0);
    assert_eq!(started.origin, Origin::Template);
    assert_eq!(
        (started.notice.as_deref(), started.ai_error.as_ref()),
        (None, None)
    );
    assert!(!started.exercises.is_empty());
}

/// Acceptance test 21: a 402 gives the template lesson and the error with its link; offline gives
/// the offline notice; with no provider chosen nothing is sent.
#[test]
fn p1_errors_give_a_template_lesson_and_say_why() {
    let dir = tempdir();
    let mut app = app_with(dir.path(), "publik");
    let client = Arc::new(
        Scripted::new(ProviderKind::Publik).fail(AiError::NeedsCredit {
            message: "Your publik balance is $0.00.".into(),
            top_up_url: "https://publikhq.com/claim/abc".into(),
        }),
    );
    let started = app
        .lesson_start_with(SKILL, "learn", access(&client))
        .unwrap();
    assert_eq!(client.requests(), 1);
    assert_eq!(started.origin, Origin::Template);
    assert_eq!(started.notice, None);
    let error = started.ai_error.expect("the 402 is shown");
    assert_eq!(error.code, ErrorCode::NeedsCredit);
    assert_eq!(error.message, "Your publik balance is $0.00.");
    assert_eq!(
        error.action,
        Some(json!({"top_up_url": "https://publikhq.com/claim/abc"}))
    );

    let dir = tempdir();
    let mut app = app_with(dir.path(), "publik");
    let client = Arc::new(Scripted::new(ProviderKind::Publik).fail(AiError::Offline));
    let started = app
        .lesson_start_with(SKILL, "learn", access(&client))
        .unwrap();
    assert_eq!(started.origin, Origin::Template);
    assert_eq!(started.notice.as_deref(), Some(OFFLINE_LESSON));
    assert_eq!(
        OFFLINE_LESSON,
        "You're offline, so this lesson comes from the course pack."
    );
    assert_eq!(started.ai_error, None);

    let dir = tempdir();
    let mut app = app_with(dir.path(), "publik");
    let started = app
        .lesson_start_with(SKILL, "learn", Some(Err(AiError::Disconnected)))
        .unwrap();
    assert_eq!(started.origin, Origin::Template);
    assert_eq!(
        started.ai_error.map(|e| e.code),
        Some(ErrorCode::Disconnected)
    );

    // No provider chosen (the default): a client handed in anyway is never used.
    let dir = tempdir();
    let mut app = course_app(dir.path());
    assert_eq!(app.settings_get().ai.provider, "unset");
    let client = Scripted::always(&p1("valid.json"));
    let started = app
        .lesson_start_with(SKILL, "learn", access(&client))
        .unwrap();
    assert_eq!(client.requests(), 0);
    assert_eq!(started.origin, Origin::Template);
    assert_eq!((started.notice, started.ai_error), (None, None));

    // Fresh lessons switched off: no request either.
    let dir = tempdir();
    let mut app = app_with(dir.path(), "publik");
    let mut ai = app.settings_get().ai;
    ai.fresh_lessons = false;
    app.save_ai_settings(&ai).unwrap();
    let started = app
        .lesson_start_with(SKILL, "learn", access(&client))
        .unwrap();
    assert_eq!(client.requests(), 0);
    assert_eq!(started.origin, Origin::Template);
}

/// Acceptance test 15: Use the course pack instead starts the template lesson; the answer that
/// arrives afterwards is cached and not shown.
#[test]
fn use_the_course_pack_instead_supersedes_the_pending_lesson() {
    let dir = tempdir();
    let mut app = app_with(dir.path(), "publik");
    let client = Scripted::always(&p1("valid.json"));
    let LessonBegin::Pending(job) = app.lesson_begin(SKILL, "learn", access(&client)).unwrap()
    else {
        panic!("a fresh lesson is asked for")
    };
    assert_eq!((job.skill_id(), job.seq()), (SKILL, 1));
    assert_eq!(client.requests(), 0, "begin sends nothing");

    let instead = app.lesson_use_course_pack(SKILL).unwrap();
    assert_eq!(instead.origin, Origin::Template);
    assert_eq!(instead.kind, "lesson");
    assert!(!instead.session_id.is_empty());

    let outcome = job.run();
    assert_eq!(client.requests(), 1);
    let error = app.lesson_complete(job, outcome).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(
        error.message,
        "This lesson was replaced by one from the course pack."
    );
    let cache = Cache::open(&dir.path().join("cache").join("ai.sqlite3"), clock()).unwrap();
    assert_eq!(cache.len(), 1, "the late answer is cached");
}

/// The begin, run and complete halves give a started lesson when nothing supersedes them.
#[test]
fn a_pending_lesson_completes_into_a_fresh_lesson_session() {
    let dir = tempdir();
    let mut app = app_with(dir.path(), "publik");
    let client = Scripted::always(&p1("valid.json"));
    let LessonBegin::Pending(job) = app.lesson_begin(SKILL, "learn", access(&client)).unwrap()
    else {
        panic!("a fresh lesson is asked for")
    };
    let outcome = job.run();
    let started = app.lesson_complete(job, outcome).unwrap();
    assert_eq!(started.kind, "lesson");
    assert_eq!(started.skill_id.as_deref(), Some(SKILL));
    assert!(!started.session_id.is_empty());
    assert!(!started.exercises.is_empty());
    // The plan is the composer's, so the example answer may fit it only in part; whatever is kept
    // is AI, and anything else came from the templates.
    assert!(
        started
            .exercises
            .iter()
            .all(|ex| matches!(ex.origin, Origin::Ai | Origin::Template))
    );
    // Review mode never calls.
    let review = app
        .lesson_start_with(SKILL, "review", access(&client))
        .unwrap();
    assert_eq!(client.requests(), 1);
    assert_eq!(review.origin, Origin::Template);
}

/// Acceptance test 16: after lesson_finish, exactly one P1 call for seq + 1, after which the next
/// lesson_start is served from the cache; none with the setting off or needs_credit set.
#[test]
fn prepare_next_lesson_makes_exactly_one_call_for_the_next_lesson() {
    let dir = tempdir();
    let mut app = app_with(dir.path(), "publik");
    let client = Scripted::always(&p1("valid.json"));
    let started = app
        .lesson_start_with(SKILL, "learn", access(&client))
        .unwrap();
    assert_eq!(client.requests(), 1);
    assert_eq!(app.run_skill(&started.session_id).as_deref(), Some(SKILL));
    answer_all(&mut app, &started.session_id, &started.exercises);
    app.lesson_finish(&started.session_id).unwrap();

    let job = app
        .next_lesson_job(SKILL, access(&client))
        .unwrap()
        .expect("the next lesson is prepared");
    assert_eq!((job.skill_id(), job.seq()), (SKILL, 2));
    let _ = job.run();
    assert_eq!(client.requests(), 2, "exactly one call");

    let next = app
        .lesson_start_with(SKILL, "learn", access(&client))
        .unwrap();
    assert_eq!(
        client.requests(),
        2,
        "the prepared lesson comes from the cache"
    );
    assert!(!next.exercises.is_empty());
    app.lesson_finish(&next.session_id).unwrap();

    // The setting off: nothing.
    let mut ai = app.settings_get().ai;
    ai.prepare_next_lesson = false;
    app.save_ai_settings(&ai).unwrap();
    assert!(
        app.next_lesson_job(SKILL, access(&client))
            .unwrap()
            .is_none()
    );

    // needs_credit: nothing.
    ai.prepare_next_lesson = true;
    app.save_ai_settings(&ai).unwrap();
    let mut broke = Scripted::new(ProviderKind::Publik);
    broke.needs_credit = true;
    let broke = Arc::new(broke);
    assert!(
        app.next_lesson_job(SKILL, access(&broke))
            .unwrap()
            .is_none()
    );
    assert_eq!(broke.requests(), 0);
    // No client: nothing.
    assert!(app.next_lesson_job(SKILL, None).unwrap().is_none());
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 17: stories (PUBLIK-INTEGRATION test 23).
// ---------------------------------------------------------------------------------------------

fn story_ctx(course: &Course) -> StoryCtx<'_> {
    let prompt = StoryPrompt::new(&course.syllabus, &[BASICS.to_string()], SKILL, 1)
        .expect("es.a1.u03.s01 ends its unit");
    StoryCtx {
        prompt,
        syllabus: &course.syllabus,
        forms: &course.forms,
        numbers: &course.numbers,
    }
}

/// Acceptance test 17: the story prompt's values for unit 3 at A1 (§6.4).
#[test]
fn the_story_prompt_has_the_values_of_section_6_4() {
    let course = course07();
    let ctx = story_ctx(&course);
    assert_eq!(ctx.prompt.theme, "Food and drink");
    assert_eq!(ctx.prompt.focus, ["café", "té", "agua", "leche"]);
    assert_eq!(ctx.prompt.allowed_lemmas.join(", "), EXAMPLE_ALLOWED);
    let vars = ctx.prompt.vars();
    for (name, value) in [
        ("target_language_name", "Spanish"),
        ("cefr", "A1"),
        ("paragraphs", "3"),
        ("min_words", "80"),
        ("max_words", "120"),
        ("questions", "3"),
        ("tenses", "present"),
        ("theme", "Food and drink"),
        ("focus_lemmas", "café, té, agua, leche"),
        ("allowed_lemmas", EXAMPLE_ALLOWED),
        ("allowed_names", "Ana, Luis, Madrid"),
        ("variety", "Spain Spanish, usted and tú both allowed"),
    ] {
        assert_eq!(vars.get(name).map(String::as_str), Some(value), "{name}");
    }
    assert_eq!(
        ctx.prompt.key_input(),
        json!({
            "lang": "es",
            "skill_id": SKILL,
            "seq": 1,
            "focus": ["café", "té", "agua", "leche"],
            "allowed_sha256": sha256_hex(EXAMPLE_ALLOWED),
        })
    );
    // Story 2 takes the next four focus words.
    let second = StoryPrompt::new(&course.syllabus, &[BASICS.to_string()], SKILL, 2).unwrap();
    assert_eq!(second.focus, ["pan", "zumo", "bocadillo", "azúcar"]);
    // A skill that does not end its unit has no story.
    assert!(StoryPrompt::new(&course.syllabus, &[], BASICS, 1).is_none());
}

/// Acceptance test 17: the valid story passes; each of S1 to S6 gives "The AI answer could not be
/// used." and stores nothing.
#[test]
fn each_broken_story_rule_discards_the_story() {
    let course = course07();
    let ctx = story_ctx(&course);
    let cache = Cache::in_memory(clock());
    let client = Scripted::new(ProviderKind::Publik).reply(&p2("valid.json"));
    let story = generate_story(&client, &cache, &ctx).unwrap();
    assert_eq!(story.value.title, "Un café en Madrid");
    assert_eq!(story.value.paragraphs.len(), 3);
    assert_eq!(story.value.questions.len(), 3);
    assert_eq!(cache.len(), 1);
    assert_eq!(client.sent()[0].max_tokens, 1200);
    assert_eq!(client.sent()[0].model, "publik-balanced");

    for (file, rule) in [
        ("S1-two-options.json", "S1"),
        ("S2-english-paragraph.json", "S2"),
        ("S3-uncovered-words.json", "S3"),
        ("S4-too-short.json", "S4"),
        ("S5-focus-word-missing.json", "S5"),
        ("S6-paragraph-out-of-range.json", "S6"),
    ] {
        let cache = Cache::in_memory(clock());
        let client = Scripted::new(ProviderKind::Publik).reply(&p2(file));
        let error = generate_story(&client, &cache, &ctx).unwrap_err();
        assert_eq!(error, AiError::Invalid, "{file}");
        assert_eq!(AppError::from(error).message, INVALID, "{file}");
        assert_eq!(cache.len(), 0, "{file}: nothing is stored");
        if rule != "S1" {
            let parsed = serde_json::from_str(&p2(file)).unwrap();
            assert_eq!(check_story(&parsed, &ctx).unwrap_err().rule, rule, "{file}");
        }
    }
    let valid = serde_json::from_str(&p2("valid.json")).unwrap();
    assert_eq!(check_story(&valid, &ctx), Ok(()));
}

/// S5's stem fallback (the spec's Scope: the Snowball stem fallback for L5, S3 and S5): the valid
/// story with every "café" written "cafe", a form no forms table lists, still uses the focus word
/// café, because the Spanish Snowball stemmer gives "caf" for both (the final vowel removed after
/// the third letter, then the accent).
#[test]
fn s5_finds_a_focus_word_by_its_stem() {
    let course = course07();
    let ctx = story_ctx(&course);
    let mut story: parley_core::ai::Story = serde_json::from_str(&p2("valid.json")).unwrap();
    for paragraph in &mut story.paragraphs {
        *paragraph = paragraph.replace("café", "cafe");
    }
    assert!(story.paragraphs.iter().all(|p| !p.contains("café")));
    assert_eq!(check_story(&story, &ctx), Ok(()));
}

/// Acceptance test 17: a story session; 3 correct answers of 3 → 16 XP; cards and the review log
/// unchanged.
#[test]
fn a_story_session_earns_xp_and_never_touches_cards() {
    assert_eq!(story_xp(3), 16);
    assert_eq!(story_xp(0), 10);
    let dir = tempdir();
    let mut app = app_with(dir.path(), "publik");
    // Some progress first, so there are cards to keep unchanged.
    introduce_basics(&mut app);
    let before = backup(&mut app, dir.path(), "before.json");

    let client = Arc::new(Scripted::new(ProviderKind::Publik).reply(&p2("valid.json")));
    let started = app
        .lesson_start_with(SKILL, "story", access(&client))
        .unwrap();
    assert_eq!(started.kind, "story");
    let story = started.story.clone().expect("the story is shown");
    assert_eq!(story.title, "Un café en Madrid");
    assert_eq!(story.title_en, "A coffee in Madrid");

    let session = started.session_id.as_str();
    let q1 = app
        .exercise_submit(session, "q1", &Answer::Choice(0), Some(2_000), false)
        .unwrap();
    assert_eq!((q1.verdict, q1.rating), (Verdict::Correct, None));
    let again = app
        .exercise_submit(session, "q1", &Answer::Choice(0), None, false)
        .unwrap_err();
    assert_eq!(again.message, "This exercise was already checked.");
    let text = app
        .exercise_submit(session, "q2", &Answer::Text("tea".into()), None, false)
        .unwrap_err();
    assert_eq!(text.message, "Choose an answer first.");
    assert_eq!(text.code, ErrorCode::InvalidInput);
    for q in ["q2", "q3"] {
        let grade = app
            .exercise_submit(session, q, &Answer::Choice(0), Some(2_000), false)
            .unwrap();
        assert_eq!(grade.verdict, Verdict::Correct);
    }
    assert_eq!(
        app.exercise_i_was_right(session, "q1").unwrap_err().message,
        "This exercise is no longer available."
    );
    let summary = app.lesson_finish(session).unwrap();
    assert_eq!((summary.xp, summary.correct, summary.total), (16, 3, 3));

    let after = backup(&mut app, dir.path(), "after.json");
    assert_eq!(after["cards"], before["cards"]);
    assert_eq!(after["review_log"], before["review_log"]);
}

/// Acceptance test 17: a wrong answer shows the right option.
#[test]
fn a_wrong_story_answer_shows_the_right_option() {
    let dir = tempdir();
    let mut app = app_with(dir.path(), "publik");
    introduce_basics(&mut app);
    let client = Arc::new(Scripted::new(ProviderKind::Publik).reply(&p2("valid.json")));
    let started = app
        .lesson_start_with(SKILL, "story", access(&client))
        .unwrap();
    let grade = app
        .exercise_submit(&started.session_id, "q2", &Answer::Choice(2), None, false)
        .unwrap();
    assert_eq!(grade.verdict, Verdict::Wrong);
    assert_eq!(grade.corrected.as_deref(), Some("Tea"));
    assert_eq!(grade.rating, None);
}

/// Acceptance tests 17 and 22: a story that fails its checks, no provider, a skill with no story.
#[test]
fn story_refusals_say_why() {
    let dir = tempdir();
    let mut app = app_with(dir.path(), "publik");
    let client = Arc::new(Scripted::new(ProviderKind::Publik).reply(&p2("S4-too-short.json")));
    let error = app
        .lesson_start_with(SKILL, "story", access(&client))
        .unwrap_err();
    assert_eq!(
        (error.code, error.message.as_str()),
        (ErrorCode::AiInvalid, INVALID)
    );

    let error = app.lesson_start(SKILL, "story").unwrap_err();
    assert_eq!(
        error.code,
        ErrorCode::NeedsConsent,
        "no client: the consent dialog"
    );

    let error = app
        .lesson_start_with(SKILL, "story", Some(Err(AiError::Disconnected)))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Disconnected);

    let client = Scripted::always(&p2("valid.json"));
    let error = app
        .lesson_start_with(BASICS, "story", access(&client))
        .unwrap_err();
    assert_eq!(
        (error.code, error.message.as_str()),
        (ErrorCode::NotFound, "This skill has no story.")
    );
    assert_eq!(client.requests(), 0);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 18: course packs.
// ---------------------------------------------------------------------------------------------

const VALID_PACK: &str = "tests/fixtures/coursepack/es-valid.json";

/// The five exercises of the valid pack, in order.
fn pack_exercises() -> Vec<Exercise> {
    let pack = json_file(VALID_PACK);
    serde_json::from_value(pack["lessons"][0]["exercises"].clone()).expect("pack exercises")
}

/// Acceptance test 18: an imported lesson is played as the skill's next lesson; export then import
/// into fresh data gives the same exercises with origin course_pack.
#[test]
fn a_course_pack_round_trip_keeps_the_exercises() {
    let dir = tempdir();
    let mut first = course_app(dir.path());
    let report = first
        .course_pack_import(&repo_root().join(VALID_PACK))
        .unwrap();
    assert_eq!((report.lessons, report.stories, report.dropped), (1, 1, 0));
    assert_eq!(
        report.message,
        "Imported 1 lessons and 1 stories; 0 exercises were dropped because they did not pass Parley's checks."
    );
    let played = first.lesson_start(SKILL, "learn").unwrap();
    assert_eq!(played.origin, Origin::CoursePack);
    assert_eq!(played.exercises.len(), 5);
    assert!(
        played
            .exercises
            .iter()
            .all(|ex| ex.origin == Origin::CoursePack)
    );
    assert_eq!(bodies(&played.exercises), bodies(&pack_exercises()));
    first.lesson_finish(&played.session_id).unwrap();

    // The imported story plays with no provider.
    let story = first.lesson_start(SKILL, "story").unwrap();
    assert_eq!(story.kind, "story");
    assert_eq!(
        story.story.map(|s| s.title),
        Some("Un café en Madrid".to_string())
    );

    let exported = dir.path().join("exported.json");
    let summary = first.course_pack_export("es", &exported).unwrap();
    assert_eq!(
        (summary.lessons, summary.stories, summary.exercises),
        (1, 1, 5)
    );
    assert_eq!(summary.bytes, fs::metadata(&exported).unwrap().len());
    let written: Value = serde_json::from_str(&fs::read_to_string(&exported).unwrap()).unwrap();
    assert_eq!(written["schema"], "parley.coursepack/1");
    assert_eq!(written["lang"], "es");
    assert_eq!(written["licence"], "CC-BY-SA-4.0");
    assert_eq!(written["created_at"], "2026-11-06");

    let other = tempdir();
    let mut second = course_app(other.path());
    let report = second.course_pack_import(&exported).unwrap();
    assert_eq!((report.lessons, report.stories, report.dropped), (1, 1, 0));
    let played = second.lesson_start(SKILL, "learn").unwrap();
    assert_eq!(played.origin, Origin::CoursePack);
    assert!(
        played
            .exercises
            .iter()
            .all(|ex| ex.origin == Origin::CoursePack)
    );
    assert_eq!(bodies(&played.exercises), bodies(&pack_exercises()));
}

/// Acceptance test 18: AI lessons are exported with their AI exercises only, and import gives each
/// its skill's next lesson number.
#[test]
fn export_takes_the_ai_exercises_and_import_numbers_them_next() {
    let course = course07();
    let mut store = Store::open_in_memory().unwrap();
    store
        .start_course(&course.syllabus, &course.forms, 0)
        .unwrap();
    let mut exercises = pack_exercises();
    let template = Exercise {
        id: "e6".into(),
        origin: Origin::Template,
        ..exercises[0].clone()
    };
    exercises.push(template);
    let lesson = Lesson {
        schema: "parley.lesson/1".into(),
        lang: Lang::Es,
        skill_id: SKILL.into(),
        seq: 1,
        origin: Origin::Ai,
        exercises,
        ai: Some(LessonAiInfo {
            provider: ProviderKind::Publik,
            valid: 5,
            planned: 6,
        }),
    };
    LessonRun::start_lesson(&mut store, &lesson, "s-1", &FixedClock).unwrap();
    let pack =
        export_course_pack(&store, Lang::Es, 1, LocalDate::parse("2026-11-02").unwrap()).unwrap();
    assert_eq!(pack.lessons.len(), 1);
    assert_eq!(pack.lessons[0].skill_id, SKILL);
    assert_eq!(
        pack.lessons[0].exercises.len(),
        5,
        "the template exercise stays home"
    );
    assert!(
        pack.lessons[0]
            .exercises
            .iter()
            .all(|ex| ex["origin"] == "ai")
    );
    assert!(pack.stories.is_empty());

    let mut fresh = Store::open_in_memory().unwrap();
    fresh
        .start_course(&course.syllabus, &course.forms, 0)
        .unwrap();
    let check = PackCheck {
        active: Lang::Es,
        syllabus: &course.syllabus,
        forms: &course.forms,
        items: &course.items,
        numbers: &course.numbers,
    };
    let report = import_course_pack(&mut fresh, &pack, &check, NOW).unwrap();
    assert_eq!((report.lessons, report.stories, report.dropped), (1, 0, 0));
    assert_eq!(fresh.next_lesson_seq(SKILL).unwrap(), 2);
    let again = import_course_pack(&mut fresh, &pack, &check, NOW).unwrap();
    assert_eq!(again.lessons, 1);
    assert_eq!(
        fresh.next_lesson_seq(SKILL).unwrap(),
        3,
        "the next seq again"
    );
}

/// Acceptance test 18: tampered exercises are dropped and counted; a pack for another language, a
/// file that is not a pack and one over 5 MB are refused with their messages.
#[test]
fn course_pack_imports_check_everything() {
    let dir = tempdir();
    let mut app = course_app(dir.path());
    let report = app
        .course_pack_import(&repo_root().join("tests/fixtures/coursepack/es-tampered.json"))
        .unwrap();
    assert_eq!((report.lessons, report.stories, report.dropped), (1, 1, 2));
    assert_eq!(
        report.message,
        "Imported 1 lessons and 1 stories; 2 exercises were dropped because they did not pass Parley's checks."
    );
    let played = app.lesson_start(SKILL, "learn").unwrap();
    assert_eq!(played.exercises.len(), 4);
    assert!(
        played
            .exercises
            .iter()
            .all(|ex| ex.item_id != format!("{SKILL}#l:agua") && !ex.item_id.contains("u09"))
    );

    let french = app
        .course_pack_import(&repo_root().join("tests/fixtures/coursepack/fr-empty.json"))
        .unwrap_err();
    assert_eq!(
        french.message,
        "This course pack is for French; your active course is Spanish."
    );
    let not_a_pack = app
        .course_pack_import(&repo_root().join("tests/fixtures/coursepack/not-a-pack.json"))
        .unwrap_err();
    assert_eq!(not_a_pack.message, "This is not a Parley course pack.");

    let big = dir.path().join("big.json");
    let padding = "x".repeat(6_000_000);
    fs::write(
        &big,
        format!(
            "{{\"schema\": \"parley.coursepack/1\", \"lang\": \"es\", \"pad\": \"{padding}\"}}"
        ),
    )
    .unwrap();
    let too_large = app.course_pack_import(&big).unwrap_err();
    assert_eq!(too_large.message, "This course pack is larger than 5 MB.");

    let prose = dir.path().join("prose.json");
    fs::write(&prose, "not json at all").unwrap();
    assert_eq!(
        app.course_pack_import(&prose).unwrap_err().message,
        "This is not a Parley course pack."
    );
    // The refusals left the course usable: a good pack still imports.
    let report = app
        .course_pack_import(&repo_root().join(VALID_PACK))
        .unwrap();
    assert_eq!(report.lessons, 1);
}

/// Acceptance test 18 (untrusted input): word-bank tiles in a pack are never kept; an imported
/// translation gets the tiles a template exercise would, from its own target.
#[test]
fn imported_tiles_are_rebuilt_not_kept() {
    let mut pack = json_file(VALID_PACK);
    pack["lessons"][0]["exercises"][1]["tiles"] = json!(["click", "here", "to", "win"]);
    let dir = tempdir();
    let path = dir.path().join("tiles.json");
    fs::write(&path, pack.to_string()).unwrap();
    let mut app = course_app(dir.path());
    let report = app.course_pack_import(&path).unwrap();
    assert_eq!((report.lessons, report.dropped), (1, 0));
    let played = app.lesson_start(SKILL, "learn").unwrap();
    let Body::TranslateToTarget { tiles, .. } = &played.exercises[1].body else {
        panic!("exercise 2 is translate_to_target")
    };
    let tiles = tiles.as_ref().expect("the word bank is rebuilt");
    assert!(
        tiles
            .iter()
            .all(|tile| !["click", "here", "win"].contains(&tile.as_str())),
        "{tiles:?}"
    );
    for word in ["Quiero", "agua", "por", "favor"] {
        assert!(
            tiles.iter().any(|tile| tile.eq_ignore_ascii_case(word)),
            "{word} is a tile: {tiles:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 19: the validity rate.
// ---------------------------------------------------------------------------------------------

/// Acceptance test 19: three local lessons with 12, 9 and 6 valid of 12 → 75%.
#[test]
fn the_validity_rate_of_the_last_lessons() {
    let info = |valid| LessonAiInfo {
        provider: ProviderKind::Local,
        valid,
        planned: 12,
    };
    let checks = lesson_checks(&[info(12), info(9), info(6)]);
    assert_eq!((checks.lessons, checks.valid, checks.planned), (3, 27, 36));
    assert_eq!(
        checks.message.as_deref(),
        Some("Local model: 75% of AI exercises passed Parley's checks (last 3 lessons).")
    );
    assert_eq!(
        lesson_checks(&[info(11)]).message.as_deref(),
        Some("Local model: 92% of AI exercises passed Parley's checks (last lesson).")
    );
    assert_eq!(lesson_checks(&[]).message, None);
    let eleven: Vec<LessonAiInfo> = (0..11).map(|_| info(12)).collect();
    assert_eq!(
        lesson_checks(&eleven).lessons,
        10,
        "the last 10 lessons at most"
    );

    let dir = tempdir();
    let app = course_app(dir.path());
    assert_eq!(app.ai_lesson_checks().unwrap().message, None);
}

/// Acceptance test 19: the app counts the lessons a local model wrote.
#[test]
fn the_app_reports_the_local_models_lessons() {
    let dir = tempdir();
    let mut app = app_with(dir.path(), "local");
    let mut client = Scripted::new(ProviderKind::Local);
    client.always = Some(p1("valid.json"));
    let client = Arc::new(client);
    let started = app
        .lesson_start_with(SKILL, "learn", access(&client))
        .unwrap();
    assert_eq!(client.requests(), 1);
    app.lesson_finish(&started.session_id).unwrap();
    let checks = app.ai_lesson_checks().unwrap();
    assert_eq!(checks.lessons, 1);
    assert!(checks.planned > 0);
    assert!(
        checks
            .message
            .as_deref()
            .is_some_and(|m| m.starts_with("Local model: ") && m.ends_with("(last lesson)."))
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 20: gate G1, scaled down.
// ---------------------------------------------------------------------------------------------

/// Acceptance test 20: every recorded P1 answer replayed through the validators; at least 80% of
/// the planned exercises pass.
#[test]
fn recorded_lessons_replay_through_the_validators() {
    let dir = repo_root().join("tests/fixtures/ai/P1/recorded");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    assert!(files.len() >= 3, "at least one request per language");
    let (mut valid, mut planned) = (0usize, 0usize);
    for file in files {
        let record: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        let prompt: LessonPrompt = serde_json::from_value(record["prompt"].clone()).unwrap();
        let plan: Plan = serde_json::from_value(record["plan"].clone()).unwrap();
        let course = load_course(&format!("content/{}", prompt.lang.as_str()));
        let client = Scripted::new(ProviderKind::Publik).reply(record["content"].as_str().unwrap());
        let cache = Cache::in_memory(clock());
        let ctx = lesson_ctx(&course, prompt);
        let draft = generate_lesson(&client, &cache, &plan, &ctx, &mut SplitMix64::new(3)).unwrap();
        let kept = draft
            .lesson
            .exercises
            .iter()
            .filter(|ex| ex.origin == Origin::Ai)
            .count();
        valid += kept;
        planned += plan.entries.len();
    }
    assert!(
        valid * 5 >= planned * 4,
        "{valid} of {planned} recorded exercises passed"
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 22: stories in backups and on the path.
// ---------------------------------------------------------------------------------------------

/// Acceptance test 22: stories are kept by a backup round trip; a backup without stories imports.
#[test]
fn stories_are_kept_in_backups() {
    let dir = tempdir();
    let mut app = app_with(dir.path(), "publik");
    introduce_basics(&mut app);
    let client = Arc::new(Scripted::new(ProviderKind::Publik).reply(&p2("valid.json")));
    let started = app
        .lesson_start_with(SKILL, "story", access(&client))
        .unwrap();
    app.lesson_finish(&started.session_id).unwrap();
    let exported = backup(&mut app, dir.path(), "backup.json");
    let stories = exported["stories"].as_array().expect("the stories array");
    assert_eq!(stories.len(), 1);
    assert_eq!(stories[0]["id"], format!("{SKILL}/story/1"));
    assert_eq!(stories[0]["origin"], "ai");
    assert_eq!(stories[0]["prompt_id"], "story.v1");
    assert_eq!(stories[0]["content"]["title"], "Un café en Madrid");

    let other = tempdir();
    let mut restored = app_with(other.path(), "publik");
    restored
        .backup_import(&dir.path().join("backup.json"))
        .unwrap();
    let again = backup(&mut restored, other.path(), "again.json");
    assert_eq!(again["stories"], exported["stories"]);

    let mut older = exported.clone();
    older.as_object_mut().unwrap().remove("stories");
    let older_path = other.path().join("older.json");
    fs::write(&older_path, older.to_string()).unwrap();
    let third = tempdir();
    let mut from_older = course_app(third.path());
    from_older.backup_import(&older_path).unwrap();
}

/// Acceptance test 22: the path ends each unit with its Story node while ai.stories is on, available
/// once the unit's last skill is practised.
#[test]
fn the_path_shows_story_nodes() {
    let dir = tempdir();
    let mut app = course_app(dir.path());
    let path = app.path_get("es").unwrap();
    let stories: Vec<_> = path
        .units
        .iter()
        .map(|unit| {
            unit.story
                .as_ref()
                .map(|s| (s.skill_id.clone(), s.available))
        })
        .collect();
    assert_eq!(
        stories,
        [
            Some(("es.a1.u01.s02".to_string(), false)),
            Some((SKILL.to_string(), false))
        ]
    );

    // Practise the last skill of unit 1: all four of its items in one lesson.
    app.settings_set(&json!({"new_items_per_lesson": 10}))
        .unwrap();
    let lesson = app.lesson_start("es.a1.u01.s02", "learn").unwrap();
    answer_all(&mut app, &lesson.session_id, &lesson.exercises);
    app.lesson_finish(&lesson.session_id).unwrap();
    let path = app.path_get("es").unwrap();
    assert_eq!(
        path.units[0].story.as_ref().map(|s| s.available),
        Some(true)
    );

    let mut ai = app.settings_get().ai;
    ai.stories = false;
    app.save_ai_settings(&ai).unwrap();
    let path = app.path_get("es").unwrap();
    assert!(path.units.iter().all(|unit| unit.story.is_none()));
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 24: the command list.
// ---------------------------------------------------------------------------------------------

/// Acceptance test 24: the four new commands follow ai_open_link, in order.
#[test]
fn the_new_commands_are_listed_in_order() {
    let at = COMMANDS
        .iter()
        .position(|c| *c == "ai_open_link")
        .expect("ai_open_link is listed");
    assert_eq!(
        &COMMANDS[at + 1..at + 5],
        [
            "lesson_use_course_pack",
            "course_pack_export",
            "course_pack_import",
            "ai_lesson_checks"
        ]
    );
}

/// The prompt files of P1 and P2 carry the settings of §6.3 and §6.4.
#[test]
fn the_prompt_files_carry_their_settings() {
    let lesson = lesson_prompt_file();
    assert_eq!(
        (lesson.id.as_str(), lesson.version, lesson.tier.as_str()),
        ("lesson", 1, "publik-balanced")
    );
    assert_eq!(
        (lesson.max_tokens, lesson.timeout_s, lesson.temperature),
        (2000, 45, 0.7)
    );
    assert_eq!(lesson.schema, "schemas/ai/lesson.v1.schema.json");
    let story = parley_core::ai::story_prompt_file();
    assert_eq!(
        (story.id.as_str(), story.version, story.tier.as_str()),
        ("story", 1, "publik-balanced")
    );
    assert_eq!(
        (story.max_tokens, story.timeout_s, story.temperature),
        (1200, 45, 0.8)
    );
    assert_eq!(story.schema, "schemas/ai/story.v1.schema.json");
}
