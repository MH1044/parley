//! The offline quality gates (docs/TESTING.md §6; docs/modules/03-exercise-engine-and-grading.md,
//! bullet 20). Each gate is an ignored test named `gate_*`, run by `scripts/gates/run` as
//! `cargo test --release -- --ignored gate_`; its numbers come from
//! scripts/gates/thresholds.toml. Timings are measured here with std::time::Instant; the core
//! itself never reads the clock. Test 10 of crates/parley-core/tests/m03_exercise_engine_and_grading.rs
//! is G3's scaled-down version in the normal suite.
//!
//! - `gate_grading_p99`: the 99th percentile of grading one answer (lookups and grade) over
//!   `[grading] answers` seeded answers to exercises made from the seed content's items is at
//!   most `[grading] p99_ms` milliseconds.
//! - `gate_template_lesson_time`: composing and building any template lesson of the seed content
//!   takes at most `[template_lesson] max_ms` milliseconds.
//! - `gate_g3_templates`: `[g3] lessons` seeded template lessons over the real syllabi; the share
//!   of plan entries that give an exercise passing check_exercise is at least
//!   `[g3] pass_share`, and every lesson validates against schemas/lesson.schema.json.

use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use parley_core::content::{
    Forms, Item, Lang, Lemma, Syllabus, derive_items, load_numbers, parse_syllabus,
};
use parley_core::env::{Rng, SplitMix64};
use parley_core::exercise::{
    Allowed, Body, Capabilities, CheckCtx, ComposeRequest, Exercise, ExerciseType, Lesson, Origin,
    Plan, TemplateCtx, check_exercise, compose, max_words, template_lesson,
};
use parley_core::grading::{AccentLeniency, Answer, fold_marks, grade, grade_ctx, normalise};
use parley_core::srs::Facet;
use parley_core::store::Store;
use serde_json::Value;

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

fn read(rel: &str) -> String {
    fs::read_to_string(repo_root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn thresholds() -> toml::Table {
    read("scripts/gates/thresholds.toml")
        .parse()
        .unwrap_or_else(|e| panic!("scripts/gates/thresholds.toml: {e}"))
}

/// A number of thresholds.toml, integer or float.
fn setting(table: &toml::Table, section: &str, key: &str) -> f64 {
    let value = table
        .get(section)
        .and_then(|s| s.get(key))
        .unwrap_or_else(|| panic!("thresholds.toml: [{section}] needs {key}"));
    value
        .as_float()
        .or_else(|| value.as_integer().map(|n| n as f64))
        .unwrap_or_else(|| panic!("thresholds.toml: [{section}] {key} is not a number"))
}

/// One language of the seed content, as `parley lesson template` reads it from content/<l>/,
/// with an in-memory store started from it at time 0.
struct SeedCourse {
    lang: Lang,
    syllabus: Syllabus,
    forms: Forms,
    numbers: Vec<String>,
    items: Vec<Item>,
    store: Store,
}

fn seed_course(lang: Lang) -> SeedCourse {
    let dir = format!("content/{}", lang.as_str());
    let file = format!("{dir}/syllabus.yaml");
    let syllabus = parse_syllabus(&file, &read(&file))
        .unwrap_or_else(|errors| panic!("{file} does not parse: {errors:?}"));
    let forms = Forms::load_tsv(read(&format!("{dir}/forms.tsv")).as_bytes())
        .unwrap_or_else(|e| panic!("{dir}/forms.tsv: {e}"));
    let numbers = load_numbers(&read(&format!("{dir}/numbers.yaml")))
        .unwrap_or_else(|e| panic!("{dir}/numbers.yaml: {e}"));
    let items = derive_items(&syllabus, &forms);
    let mut store = Store::open_in_memory().expect("an in-memory store");
    store
        .start_course(&syllabus, &forms, 0)
        .unwrap_or_else(|e| panic!("{dir}: {e}"));
    SeedCourse {
        lang,
        syllabus,
        forms,
        numbers,
        items,
        store,
    }
}

fn seed_courses() -> Vec<SeedCourse> {
    Lang::ALL.into_iter().map(seed_course).collect()
}

fn skill_ids(course: &SeedCourse) -> Vec<String> {
    course
        .syllabus
        .skills()
        .map(|(_, skill)| skill.id.clone())
        .collect()
}

/// The plan of lesson 1 of a skill as `parley lesson template` composes it: at time 0, with 12
/// exercises, 5 new items and no capabilities.
fn template_plan(course: &SeedCourse, skill: &str) -> Plan {
    let request = ComposeRequest {
        lang: course.lang,
        skill_id: skill.to_string(),
        lesson_length: 12,
        new_items_per_lesson: 5,
        capabilities: Capabilities {
            audio_out: false,
            mic: false,
        },
    };
    compose(&request, &course.store, 0).unwrap_or_else(|e| panic!("{skill}: {e}"))
}

/// The template lesson of that plan: Allowed::for_skill with the language's number words, no
/// pack sentences, the plan's item ids, SplitMix64(seed).
fn seed_lesson(course: &SeedCourse, skill: &str, plan: &Plan, seed: u64) -> Lesson {
    let allowed = Allowed::for_skill(&course.syllabus, &course.numbers, skill);
    let plan_items: Vec<String> = plan.entries.iter().map(|e| e.item_id.clone()).collect();
    let ctx = TemplateCtx {
        syllabus: &course.syllabus,
        forms: &course.forms,
        items: &course.items,
        allowed: &allowed,
        sentences: &[],
        skill_id: skill,
        plan_items: &plan_items,
    };
    template_lesson(plan, &ctx, 1, &mut SplitMix64::new(seed))
}

/// The syllabus lemmas named by an item's `lemmas`, as the focus of L9.
fn focus_of(syllabus: &Syllabus, names: &[String]) -> Vec<Lemma> {
    names
        .iter()
        .filter_map(|name| {
            syllabus
                .skills()
                .flat_map(|(_, skill)| skill.lemmas.iter())
                .find(|lemma| lemma.lemma == *name)
                .cloned()
        })
        .collect()
}

/// How many of the plan's entries gave an exercise that passes check_exercise (focus the item's
/// lemmas, seen the earlier exercises, the skill's max_words), with one line per failure.
fn passing_entries(course: &SeedCourse, plan: &Plan, lesson: &Lesson) -> (usize, Vec<String>) {
    let skill = lesson.skill_id.as_str();
    let cefr = course
        .syllabus
        .skills()
        .find(|(_, s)| s.id == skill)
        .map(|(_, s)| s.cefr)
        .unwrap_or_else(|| panic!("no skill {skill}"));
    let allowed = Allowed::for_skill(&course.syllabus, &course.numbers, skill);
    let mut seen: Vec<(ExerciseType, String)> = Vec::new();
    let mut passed = 0;
    let mut failures = Vec::new();
    if lesson.exercises.len() != plan.entries.len() {
        failures.push(format!(
            "{skill}: {} exercises for {} plan entries",
            lesson.exercises.len(),
            plan.entries.len()
        ));
    }
    for ex in &lesson.exercises {
        let focus = course
            .items
            .iter()
            .find(|item| item.id == ex.item_id)
            .map(|item| focus_of(&course.syllabus, &item.lemmas))
            .unwrap_or_default();
        let ctx = CheckCtx {
            lang: course.lang,
            forms: &course.forms,
            allowed: &allowed,
            max_words: max_words(cefr),
            focus: &focus,
            seen: &seen,
        };
        match check_exercise(ex, &ctx) {
            Ok(()) => passed += 1,
            Err(failure) => failures.push(format!(
                "{skill} {}: {} fails {}",
                ex.id,
                ex.kind().as_str(),
                failure.rule()
            )),
        }
        let first = ex.targets().first().copied().unwrap_or("").to_string();
        seen.push((ex.kind(), normalise(course.lang, &first)));
    }
    (passed.min(plan.entries.len()), failures)
}

fn lesson_validator() -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(&read("schemas/lesson.schema.json"))
        .expect("schemas/lesson.schema.json is JSON");
    jsonschema::validator_for(&schema).expect("schemas/lesson.schema.json is a JSON Schema")
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

// ---------------------------------------------------------------------------------------------
// Grading p99
// ---------------------------------------------------------------------------------------------

fn exercise(item: &Item, facet: Facet, body: Body) -> Exercise {
    Exercise {
        id: "e1".to_string(),
        item_id: item.id.clone(),
        facet,
        origin: Origin::Template,
        body,
    }
}

/// An exercise of the item and the text a right answer would match: translate_to_target,
/// translate_to_en, listen_type or speak, by `kind`.
fn graded_exercise(item: &Item, kind: u64) -> (Exercise, String) {
    let first_en = item.en.first().cloned().unwrap_or_default();
    match kind {
        0 => (
            exercise(
                item,
                Facet::Productive,
                Body::TranslateToTarget {
                    prompt_en: first_en,
                    target: item.target.clone(),
                    accepted: item.accepted.to_target.clone(),
                    tiles: None,
                },
            ),
            item.target.clone(),
        ),
        1 => (
            exercise(
                item,
                Facet::Receptive,
                Body::TranslateToEn {
                    prompt_target: item.target.clone(),
                    accepted: item.accepted.to_en.clone(),
                },
            ),
            first_en,
        ),
        2 => (
            exercise(
                item,
                Facet::Receptive,
                Body::ListenType {
                    text: item.target.clone(),
                    accepted: item.accepted.to_target.clone(),
                },
            ),
            item.target.clone(),
        ),
        _ => (
            exercise(
                item,
                Facet::Productive,
                Body::Speak {
                    text: item.target.clone(),
                    prompt_en: first_en,
                },
            ),
            item.target.clone(),
        ),
    }
}

/// `text` with two neighbouring letters of its longest word swapped (one edit).
fn transposed(text: &str) -> String {
    let words: Vec<&str> = text.split(' ').collect();
    let longest = (0..words.len())
        .max_by_key(|&i| words[i].chars().count())
        .unwrap_or(0);
    let mut out: Vec<String> = words.iter().map(|w| w.to_string()).collect();
    let mut chars: Vec<char> = words[longest].chars().collect();
    if chars.len() >= 4 {
        let middle = chars.len() / 2;
        chars.swap(middle - 1, middle);
    }
    out[longest] = chars.into_iter().collect();
    out.join(" ")
}

// Bullet 20: grading p99 ≤ [grading] p99_ms over [grading] answers seeded answers: exact,
// unpunctuated, accent-stripped, one transposition, or another item's text.
#[test]
#[ignore = "quality gate; run by scripts/gates/run"]
fn gate_grading_p99() {
    let table = thresholds();
    let answers = setting(&table, "grading", "answers") as usize;
    let p99_ms = setting(&table, "grading", "p99_ms");
    let courses = seed_courses();
    let mut rng = SplitMix64::new(20);
    let mut times = Vec::with_capacity(answers);
    for n in 0..answers {
        let course = &courses[(rng.next_u64() % courses.len() as u64) as usize];
        let item = &course.items[(rng.next_u64() % course.items.len() as u64) as usize];
        let (ex, right) = graded_exercise(item, n as u64 % 4);
        let answer = match rng.next_u64() % 5 {
            0 => right,
            1 => normalise(course.lang, &right),
            2 => fold_marks(&right),
            3 => transposed(&right),
            _ => {
                let other = &course.items[(rng.next_u64() % course.items.len() as u64) as usize];
                other.target.clone()
            }
        };
        let answer = Answer::Text(answer);
        let start = Instant::now();
        let ctx = grade_ctx(
            &course.store,
            &course.forms,
            AccentLeniency::Lenient,
            &ex,
            &answer,
        )
        .unwrap_or_else(|e| panic!("{}: {e}", item.id));
        let graded = grade(&ctx, &ex, &answer);
        times.push(start.elapsed());
        black_box(graded);
    }
    times.sort();
    let p99 = millis(times[(answers * 99).div_ceil(100) - 1]);
    let worst = millis(times[answers - 1]);
    println!(
        "grading: p99 {p99:.3} ms over {answers} answers (slowest {worst:.3} ms); gate {p99_ms} ms"
    );
    assert!(
        p99 <= p99_ms,
        "grading p99 is {p99:.3} ms over {answers} answers (slowest {worst:.3} ms); the gate is {p99_ms} ms"
    );
}

// Bullet 20: composing and building a template lesson takes at most [template_lesson] max_ms,
// for every skill of the seed content and seeds 1 to 5.
#[test]
#[ignore = "quality gate; run by scripts/gates/run"]
fn gate_template_lesson_time() {
    let table = thresholds();
    let max_ms = setting(&table, "template_lesson", "max_ms");
    let courses = seed_courses();
    let mut worst = (0.0, String::new());
    for course in &courses {
        for skill in skill_ids(course) {
            for seed in 1..=5u64 {
                let start = Instant::now();
                let plan = template_plan(course, &skill);
                let lesson = seed_lesson(course, &skill, &plan, seed);
                let took = millis(start.elapsed());
                black_box(lesson);
                if took > worst.0 {
                    worst = (took, format!("{skill} seed {seed}"));
                }
            }
        }
    }
    println!(
        "template lesson: slowest {:.3} ms ({}); gate {max_ms} ms",
        worst.0, worst.1
    );
    assert!(
        worst.0 <= max_ms,
        "the slowest template lesson ({}) took {:.3} ms; the gate is {max_ms} ms",
        worst.1,
        worst.0
    );
}

// Bullet 20, gate G3: [g3] lessons seeded template lessons over the real syllabi (every skill of
// es, fr and de in turn, seeds from 1000); at least [g3] pass_share of the plan entries give an
// exercise that passes check_exercise, and every lesson validates against the lesson schema.
#[test]
#[ignore = "quality gate; run by scripts/gates/run"]
fn gate_g3_templates() {
    let table = thresholds();
    let lessons = setting(&table, "g3", "lessons") as usize;
    let pass_share = setting(&table, "g3", "pass_share");
    let validator = lesson_validator();
    let courses = seed_courses();
    let mut skills: Vec<(&SeedCourse, String, Plan)> = Vec::new();
    for course in &courses {
        for skill in skill_ids(course) {
            let plan = template_plan(course, &skill);
            skills.push((course, skill, plan));
        }
    }
    let mut entries = 0;
    let mut passed = 0;
    let mut failures = Vec::new();
    for i in 0..lessons {
        let (course, skill, plan) = &skills[i % skills.len()];
        let seed = 1_000 + i as u64;
        let lesson = seed_lesson(course, skill, plan, seed);
        let (ok, problems) = passing_entries(course, plan, &lesson);
        entries += plan.entries.len();
        passed += ok;
        failures.extend(problems.into_iter().map(|p| format!("seed {seed}: {p}")));
        let value = serde_json::to_value(&lesson).expect("a lesson serialises");
        failures.extend(
            validator
                .iter_errors(&value)
                .map(|e| format!("seed {seed}: {skill}: schema: {e}")),
        );
    }
    let share = passed as f64 / entries.max(1) as f64;
    let shown: Vec<&String> = failures.iter().take(30).collect();
    println!(
        "G3: {passed} of {entries} plan entries pass ({share:.4}) over {lessons} lessons; gate {pass_share}"
    );
    assert!(
        share >= pass_share,
        "{passed} of {entries} plan entries pass ({share:.4}); the gate is {pass_share}; first failures: {shown:#?}"
    );
    assert!(
        failures.iter().all(|f| !f.contains("schema:")),
        "every lesson validates against schemas/lesson.schema.json: {shown:#?}"
    );
}
