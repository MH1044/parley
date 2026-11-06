//! Module 05 tests of the library (docs/modules/05-course-content-a1-a2.md, Acceptance tests):
//! the real course in content/ read through the library API of modules 01 to 03 and with plain
//! YAML and TOML parsing. Each test names the bullet it covers; tests without a number cover a
//! Scope item or an Interfaces promise the bullets do not name. The new commands and rules are
//! tested through the binary in crates/parley-cli/tests/m05_course_content_a1_a2.rs, and the
//! fixture cases also run as golden cases under tests/cases/05-course-content-a1-a2.
//!
//! Expected values come from the spec, the schemas' wording in its Interfaces and the fixture
//! READMEs (tests/fixtures/scenarios, tests/fixtures/course05), never from running the code.
//! Tests over the real course hold once the course is complete; the pack checks of bullet 9 are
//! skipped, with the spec's message, when a language's pack is not in data-cache/packs.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use parley_core::content::{
    Cefr, Forms, Item, Lang, Lemma, Syllabus, derive_items, load_numbers, pack_info,
    parse_syllabus, read_wiktextract,
};
use parley_core::env::{LocalDate, SplitMix64};
use parley_core::exercise::{
    Allowed, Capabilities, CheckCtx, ComposeRequest, ExerciseType, Lesson, Plan, TemplateCtx,
    check_exercise, compose, max_words, template_lesson,
};
use parley_core::grading::normalise;
use parley_core::store::Store;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// What a test over a real pack prints when the pack has not been built on this machine.
const SKIPPED: &str = "pack not found in data-cache/packs; run parley pack build";

const SCENARIOS: [&str; 6] = [
    "cafe",
    "shop",
    "directions",
    "hotel",
    "doctor",
    "job_interview",
];
const META_JSONL: &str = "tests/fixtures/course05/wiktextract-meta.jsonl";
/// The scaled-down G3 of `verify`: this many seeded template lessons over the full course.
const G3_LESSONS: usize = 50;

// ---------------------------------------------------------------------------------------------
// Helpers
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

fn read(rel: &str) -> String {
    fs::read_to_string(repo_root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn exists(rel: &str) -> bool {
    repo_root().join(rel).is_file()
}

/// A YAML file as plain data.
fn yaml(rel: &str) -> Value {
    serde_yaml_ng::from_str(&read(rel)).unwrap_or_else(|e| panic!("{rel} is not YAML: {e}"))
}

fn json_schema(rel: &str) -> jsonschema::Validator {
    let schema: Value =
        serde_json::from_str(&read(rel)).unwrap_or_else(|e| panic!("{rel} is not JSON: {e}"));
    jsonschema::validator_for(&schema).unwrap_or_else(|e| panic!("{rel} is not a JSON Schema: {e}"))
}

/// Every schema violation of `value`, one line each, for a failure message.
fn violations(validator: &jsonschema::Validator, value: &Value) -> Vec<String> {
    validator
        .iter_errors(value)
        .map(|e| format!("{}: {e}", e.instance_path().as_str()))
        .collect()
}

fn syllabus_at(rel: &str) -> Syllabus {
    parse_syllabus(rel, &read(rel)).unwrap_or_else(|errors| {
        let messages: Vec<&str> = errors.iter().map(|e| e.0.as_str()).collect();
        panic!("{rel} does not parse:\n{}", messages.join("\n"))
    })
}

fn forms_at(rel: &str) -> Forms {
    Forms::load_tsv(read(rel).as_bytes()).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn str_field<'a>(value: &'a Value, key: &str, what: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("{what}: {key} is not a string: {value}"))
}

fn array<'a>(value: &'a Value, key: &str, what: &str) -> &'a Vec<Value> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("{what}: {key} is not a list: {value}"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn is_hex64(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The skills of a syllabus by id, with their level.
fn skill_levels(syllabus: &Syllabus) -> BTreeMap<String, Cefr> {
    syllabus
        .skills()
        .map(|(_, skill)| (skill.id.clone(), skill.cefr))
        .collect()
}

/// One language of the course, as the app and `parley lesson template` read it from
/// content/<l>/.
struct RealCourse {
    lang: Lang,
    syllabus: Syllabus,
    forms: Forms,
    numbers: Vec<String>,
    items: Vec<Item>,
    store: Store,
}

fn real_course(lang: Lang) -> RealCourse {
    let dir = format!("content/{}", lang.as_str());
    let syllabus = syllabus_at(&format!("{dir}/syllabus.yaml"));
    let forms = forms_at(&format!("{dir}/forms.tsv"));
    let numbers = load_numbers(&read(&format!("{dir}/numbers.yaml")))
        .unwrap_or_else(|e| panic!("{dir}/numbers.yaml: {e}"));
    let items = derive_items(&syllabus, &forms);
    let mut store = Store::open_in_memory().expect("an in-memory store");
    store
        .start_course(&syllabus, &forms, 0)
        .unwrap_or_else(|e| panic!("{dir}: {e}"));
    RealCourse {
        lang,
        syllabus,
        forms,
        numbers,
        items,
        store,
    }
}

// ---------------------------------------------------------------------------------------------
// Scope: units and skills
// ---------------------------------------------------------------------------------------------

// Scope (and bullet 2, which the binary's tests check through the report): each language has the
// sixteen units of the unit plan, A1 u01 to u08 and A2 u01 to u08, at least 8 per level, every
// unit with 3 to 6 skills; the seed units of module 01 stay.
#[test]
fn scope_units_and_skills_per_language() {
    for lang in Lang::ALL {
        let l = lang.as_str();
        let syllabus = syllabus_at(&format!("content/{l}/syllabus.yaml"));
        let units: BTreeMap<&str, (Cefr, usize)> = syllabus
            .units
            .iter()
            .map(|unit| (unit.id.as_str(), (unit.cefr, unit.skills.len())))
            .collect();
        let a1 = units.values().filter(|(c, _)| *c == Cefr::A1).count();
        let a2 = units.values().filter(|(c, _)| *c == Cefr::A2).count();
        assert!(a1 >= 8, "{l}: {a1} A1 units; at least 8");
        assert!(a2 >= 8, "{l}: {a2} A2 units; at least 8");
        for level in ["a1", "a2"] {
            for n in 1..=8 {
                let id = format!("{l}.{level}.u{n:02}");
                assert!(
                    units.contains_key(id.as_str()),
                    "{l}: unit {id} of the unit plan"
                );
            }
        }
        for unit in &syllabus.units {
            assert!(
                (3..=6).contains(&unit.skills.len()),
                "{}: {} skills; a unit has 3 to 6",
                unit.id,
                unit.skills.len()
            );
        }
    }
    let syllabus = syllabus_at("content/es/syllabus.yaml");
    assert!(
        syllabus.units.iter().any(|u| u.id == "es.a1.u02"),
        "the Spanish seed unit 2"
    );
}

// ---------------------------------------------------------------------------------------------
// 3. Checklists
// ---------------------------------------------------------------------------------------------

// Bullet 3: every checklist validates against schemas/checklist.schema.json (which checks the
// shape of the ids), names its language, has unique ids, covers every entry, maps each entry only
// to existing skills of the entry's level, and has at least 10 grammar and 8 function entries per
// level.
#[test]
fn t03_checklists_cover_both_levels() {
    let validator = json_schema("schemas/checklist.schema.json");
    for lang in Lang::ALL {
        let l = lang.as_str();
        let file = format!("content/{l}/checklist.yaml");
        let checklist = yaml(&file);
        let problems = violations(&validator, &checklist);
        assert!(problems.is_empty(), "{file}: {problems:#?}");
        assert_eq!(str_field(&checklist, "lang", &file), l);
        let levels = skill_levels(&syllabus_at(&format!("content/{l}/syllabus.yaml")));
        let mut ids = BTreeSet::new();
        let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
        for entry in array(&checklist, "entries", &file) {
            let id = str_field(entry, "id", &file);
            let cefr = str_field(entry, "cefr", &file);
            let kind = str_field(entry, "kind", &file);
            assert!(ids.insert(id.to_string()), "{file}: {id} twice");
            assert!(
                kind == "grammar" || kind == "function",
                "{file}: {id}: kind {kind}"
            );
            let skills = array(entry, "skills", &file);
            assert!(!skills.is_empty(), "{file}: {id} is uncovered");
            for skill in skills {
                let skill = skill
                    .as_str()
                    .unwrap_or_else(|| panic!("{file}: {id}: {skill}"));
                let level = levels
                    .get(skill)
                    .unwrap_or_else(|| panic!("{file}: {id}: skill {skill} does not exist"));
                assert_eq!(
                    level.as_str(),
                    cefr,
                    "{file}: {id}: skill {skill} is {}, but the entry is {cefr}",
                    level.as_str()
                );
            }
            *counts
                .entry((cefr.to_string(), kind.to_string()))
                .or_default() += 1;
        }
        for cefr in ["A1", "A2"] {
            let grammar = counts
                .get(&(cefr.to_string(), "grammar".to_string()))
                .copied()
                .unwrap_or(0);
            let function = counts
                .get(&(cefr.to_string(), "function".to_string()))
                .copied()
                .unwrap_or(0);
            assert!(
                grammar >= 10,
                "{file}: {grammar} {cefr} grammar entries; at least 10"
            );
            assert!(
                function >= 8,
                "{file}: {function} {cefr} function entries; at least 8"
            );
        }
    }
}

// Interfaces (checklist format): the schema accepts the fixture checklists, whose faults in
// bad-CL are rule faults (CL02 to CL04), not schema faults.
#[test]
fn the_checklist_schema_accepts_the_fixtures() {
    let validator = json_schema("schemas/checklist.schema.json");
    for file in [
        "tests/fixtures/course05/course/es/checklist.yaml",
        "tests/fixtures/course05/bad-CL/es/checklist.yaml",
    ] {
        let problems = violations(&validator, &yaml(file));
        assert!(problems.is_empty(), "{file}: {problems:#?}");
    }
}

// ---------------------------------------------------------------------------------------------
// 6. Scenarios
// ---------------------------------------------------------------------------------------------

// Bullet 6: the six scenarios exist, validate against schemas/scenario.schema.json, carry the id
// of their file name, 3 to 6 goals and a turn cap of 6 to 20, and have a
// setting, an opening with its English and at least one skill for each of es, fr and de.
#[test]
fn t06_each_scenario_opens_in_three_languages() {
    let validator = json_schema("schemas/scenario.schema.json");
    for id in SCENARIOS {
        let file = format!("content/scenarios/{id}.yaml");
        assert!(exists(&file), "{file} exists");
        let scenario = yaml(&file);
        let problems = violations(&validator, &scenario);
        assert!(problems.is_empty(), "{file}: {problems:#?}");
        assert_eq!(str_field(&scenario, "id", &file), id);
        let cefr = str_field(&scenario, "cefr", &file);
        assert!(cefr == "A1" || cefr == "A2", "{file}: cefr {cefr}");
        let goals = array(&scenario, "goals", &file);
        assert!(
            (3..=6).contains(&goals.len()),
            "{file}: {} goals",
            goals.len()
        );
        let cap = scenario["turn_cap"]
            .as_u64()
            .unwrap_or_else(|| panic!("{file}: turn_cap"));
        assert!((6..=20).contains(&cap), "{file}: turn_cap {cap}");
        for lang in ["es", "fr", "de"] {
            let section = &scenario["languages"][lang];
            assert!(section.is_object(), "{file}: no {lang} section");
            for key in ["setting_en", "opening", "opening_en"] {
                assert!(
                    !str_field(section, key, &file).trim().is_empty(),
                    "{file}: {lang}: an empty {key}"
                );
            }
            let skills = array(section, "skills", &file);
            assert!(!skills.is_empty(), "{file}: {lang}: no skills");
            for skill in skills {
                let skill = skill.as_str().unwrap_or_default();
                assert!(
                    skill.starts_with(&format!("{lang}.")),
                    "{file}: {lang}: skill {skill} of another language"
                );
            }
        }
    }
}

// Interfaces (scenario format): the schema accepts valid-cafe.yaml and the fixtures whose fault is
// a rule's (SC02 to SC06), and refuses bad-SC01.yaml, which lacks title_en.
#[test]
fn the_scenario_schema_sorts_the_fixtures() {
    let validator = json_schema("schemas/scenario.schema.json");
    for name in [
        "valid-cafe",
        "valid-numbers",
        "bad-SC02",
        "bad-SC03",
        "bad-SC04",
        "bad-SC05",
        "bad-SC06",
    ] {
        let file = format!("tests/fixtures/scenarios/{name}.yaml");
        let problems = violations(&validator, &yaml(&file));
        assert!(problems.is_empty(), "{file}: {problems:#?}");
    }
    let file = "tests/fixtures/scenarios/content/scenarios/shop.yaml";
    let problems = violations(&validator, &yaml(file));
    assert!(problems.is_empty(), "{file}: {problems:#?}");

    let bad = yaml("tests/fixtures/scenarios/bad-SC01.yaml");
    assert!(!validator.is_valid(&bad), "bad-SC01.yaml lacks title_en");
    let mut mended = bad.clone();
    mended["title_en"] = Value::String("At a café".to_string());
    let problems = violations(&validator, &mended);
    assert!(
        problems.is_empty(),
        "only title_en was missing: {problems:#?}"
    );
}

// ---------------------------------------------------------------------------------------------
// 7. Gate G3, scaled down
// ---------------------------------------------------------------------------------------------

/// The plan of lesson 1 of a skill as `parley lesson template` and gate G3 compose it: at time 0,
/// with 12 exercises, 5 new items and no capabilities.
fn template_plan(course: &RealCourse, skill: &str) -> Plan {
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
fn template(course: &RealCourse, skill: &str, plan: &Plan, seed: u64) -> Lesson {
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

/// The syllabus lemmas named by an item's `lemmas`, the focus of the checks.
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

/// One line per exercise of the lesson that fails check_exercise (focus the item's lemmas, seen
/// the earlier exercises, the skill's max_words), and per plan entry without an exercise.
fn lesson_failures(course: &RealCourse, plan: &Plan, lesson: &Lesson) -> Vec<String> {
    let skill = lesson.skill_id.as_str();
    let cefr = course
        .syllabus
        .skills()
        .find(|(_, s)| s.id == skill)
        .map(|(_, s)| s.cefr)
        .unwrap_or_else(|| panic!("no skill {skill}"));
    let allowed = Allowed::for_skill(&course.syllabus, &course.numbers, skill);
    let mut failures = Vec::new();
    if lesson.exercises.len() != plan.entries.len() {
        failures.push(format!(
            "{skill}: {} exercises for {} plan entries",
            lesson.exercises.len(),
            plan.entries.len()
        ));
    }
    let mut seen: Vec<(ExerciseType, String)> = Vec::new();
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
        if let Err(failure) = check_exercise(ex, &ctx) {
            failures.push(format!(
                "{skill} {}: {} fails {}",
                ex.id,
                ex.kind().as_str(),
                failure.rule()
            ));
        }
        let first = ex.targets().first().copied().unwrap_or("").to_string();
        seen.push((ex.kind(), normalise(course.lang, &first)));
    }
    failures
}

// Bullet 7, gate G3 as `verify` runs it: 50 seeded template lessons spread evenly over every skill
// of es, fr and de in syllabus order (lesson i takes skill ⌊i × skills / 50⌋, seed 1000 + i, as
// the gate's seeds start at 1000); every exercise passes check_exercise, there is one per plan
// entry, and every lesson validates against schemas/lesson.schema.json. The 1,000-lesson gate is
// gate_g3_templates in crates/parley-core/tests/gates.rs.
#[test]
fn t07_gate_g3_scaled_down() {
    let validator = json_schema("schemas/lesson.schema.json");
    let courses: Vec<RealCourse> = Lang::ALL.into_iter().map(real_course).collect();
    let skills: Vec<(usize, String)> = courses
        .iter()
        .enumerate()
        .flat_map(|(n, course)| {
            course
                .syllabus
                .skills()
                .map(move |(_, skill)| (n, skill.id.clone()))
        })
        .collect();
    assert!(
        skills.len() >= G3_LESSONS,
        "{} skills in the course",
        skills.len()
    );
    let mut failures = Vec::new();
    let mut languages = BTreeSet::new();
    for i in 0..G3_LESSONS {
        let (n, skill) = &skills[i * skills.len() / G3_LESSONS];
        let course = &courses[*n];
        languages.insert(course.lang.as_str());
        let seed = 1_000 + i as u64;
        let plan = template_plan(course, skill);
        assert!(!plan.entries.is_empty(), "{skill}: an empty plan");
        let lesson = template(course, skill, &plan, seed);
        failures.extend(
            lesson_failures(course, &plan, &lesson)
                .into_iter()
                .map(|f| format!("seed {seed}: {f}")),
        );
        let value = serde_json::to_value(&lesson).expect("a lesson serialises");
        failures.extend(
            violations(&validator, &value)
                .into_iter()
                .map(|f| format!("seed {seed}: {skill}: schema: {f}")),
        );
    }
    assert_eq!(languages.len(), 3, "the lessons reach es, fr and de");
    let shown: Vec<&String> = failures.iter().take(30).collect();
    assert!(
        failures.is_empty(),
        "{} failure(s) over {G3_LESSONS} lessons, the first: {shown:#?}",
        failures.len()
    );
}

// Bullet 7, every skill: module 03's every-skill template test runs over the frozen seed course
// (tests/fixtures/seed-course), so this one keeps every skill of the real course covered in
// `verify`, within its time. Every example fits its level's word limit for exercises (module 03's
// L8: max_words of the skill's level, counted as the words of the normal form), so a template
// lesson can always turn it into a translation exercise; and the first template lesson of every
// skill (seed 1) has one exercise per plan entry, each passing check_exercise.
#[test]
fn t07_every_skill_gives_full_template_lessons() {
    let mut failures = Vec::new();
    let mut lessons = 0;
    for lang in Lang::ALL {
        let course = real_course(lang);
        for (_, skill) in course.syllabus.skills() {
            for example in &skill.examples {
                let norm = normalise(lang, &example.target);
                let count = if norm.is_empty() {
                    0
                } else {
                    norm.split(' ').count()
                };
                if count > max_words(skill.cefr) {
                    failures.push(format!(
                        "{} example {}: {count} words; the limit is {}",
                        skill.id,
                        example.id,
                        max_words(skill.cefr)
                    ));
                }
            }
            let plan = template_plan(&course, &skill.id);
            let lesson = template(&course, &skill.id, &plan, 1);
            failures.extend(lesson_failures(&course, &plan, &lesson));
            lessons += 1;
        }
    }
    assert!(lessons >= 3 * 64, "{lessons} lessons built");
    let shown: Vec<&String> = failures.iter().take(30).collect();
    assert!(
        failures.is_empty(),
        "{} failure(s), the first: {shown:#?}",
        failures.len()
    );
}

// ---------------------------------------------------------------------------------------------
// 8. Forms
// ---------------------------------------------------------------------------------------------

/// The tags of wiktextract's table metadata, which are never forms of a word.
const METADATA_TAGS: [&str; 4] = ["table-tags", "inflection-template", "class", "auxiliary"];

// Bullet 8 (the real tables): each forms.tsv was extracted from a wiktextract file (its header
// names the file, its SHA-256 and a date), every syllabus lemma has at least one row, and no row
// carries the tag `table-tags`, `inflection-template`, `class` or `auxiliary`.
#[test]
fn t08_every_lemma_has_forms_and_no_row_is_metadata() {
    for lang in Lang::ALL {
        let l = lang.as_str();
        let file = format!("content/{l}/forms.tsv");
        let text = read(&file);
        let header = text.lines().next().unwrap_or_default();
        let fields: Vec<&str> = header
            .strip_prefix("# source: ")
            .unwrap_or_else(|| panic!("{file}: header {header:?}"))
            .split("; ")
            .collect();
        assert_eq!(fields.len(), 3, "{file}: header {header:?}");
        assert_ne!(
            fields[0], "hand-written",
            "{file} comes from a wiktextract extract"
        );
        let sha = fields[1]
            .strip_prefix("sha256: ")
            .unwrap_or_else(|| panic!("{file}: header {header:?}"));
        assert!(is_hex64(sha), "{file}: the extract's SHA-256 in {header:?}");
        let date = fields[2]
            .strip_prefix("date: ")
            .unwrap_or_else(|| panic!("{file}: header {header:?}"));
        assert!(LocalDate::parse(date).is_some(), "{file}: date {date:?}");

        let forms = Forms::load_tsv(text.as_bytes()).unwrap_or_else(|e| panic!("{file}: {e}"));
        let mut with_rows: BTreeSet<&str> = BTreeSet::new();
        for row in forms.rows() {
            with_rows.insert(row.lemma.as_str());
            for tag in row.tags.split_whitespace() {
                assert!(
                    !METADATA_TAGS.contains(&tag),
                    "{file}: {} → {} carries {tag}",
                    row.form,
                    row.lemma
                );
            }
        }
        let syllabus = syllabus_at(&format!("content/{l}/syllabus.yaml"));
        let missing: Vec<String> = syllabus
            .skills()
            .flat_map(|(_, skill)| {
                skill
                    .lemmas
                    .iter()
                    .map(move |lemma| (skill.id.clone(), lemma.lemma.clone()))
            })
            .filter(|(_, lemma)| !with_rows.contains(lemma.as_str()))
            .map(|(skill, lemma)| format!("{skill}: {lemma}"))
            .collect();
        assert!(
            missing.is_empty(),
            "{file}: lemmas without a row: {missing:#?}"
        );
    }
}

/// A forms row as plain text: form, lemma, pos, gender, tags.
fn row_text(form: &str, lemma: &str, pos: &str, gender: &str, tags: &str) -> String {
    format!("{form}\t{lemma}\t{pos}\t{gender}\t{tags}")
}

/// The rows `read_wiktextract` gives for the metadata fixture, as plain text, with `found` and
/// `missing`.
fn meta_rows(lang: Lang, lemmas: Option<&[String]>) -> (Vec<String>, usize, Vec<String>) {
    let text = read(META_JSONL);
    let extract = read_wiktextract(text.as_bytes(), lang, lemmas)
        .unwrap_or_else(|e| panic!("{META_JSONL}: {e}"));
    let rows = extract
        .rows
        .iter()
        .map(|row| {
            row_text(
                &row.form,
                &row.lemma,
                &row.pos,
                row.gender.map(|g| g.as_str()).unwrap_or(""),
                &row.tags,
            )
        })
        .collect();
    (rows, extract.found, extract.missing)
}

// Bullet 8 (the extract reader): a wiktextract line whose `forms` hold table metadata (tagged
// `table-tags`, `inflection-template`, `class` or `auxiliary`) gives no row for those entries;
// every other form gives its row as module 01 maps it (tests/fixtures/course05/README.md).
#[test]
fn t08_the_extract_reader_skips_table_metadata() {
    let spanish = [
        row_text(
            "habla",
            "hablar",
            "verb",
            "",
            "indicative present singular third-person",
        ),
        row_text("hablar", "hablar", "verb", "", "canonical"),
        row_text(
            "hablo",
            "hablar",
            "verb",
            "",
            "first-person indicative present singular",
        ),
        row_text("mesa", "mesa", "noun", "f", "canonical"),
        row_text("mesas", "mesa", "noun", "f", "plural"),
    ];
    let wanted = ["hablar".to_string(), "mesa".to_string()];
    for lemmas in [None, Some(&wanted[..])] {
        let (rows, found, missing) = meta_rows(Lang::Es, lemmas);
        assert_eq!(rows, spanish, "es, lemmas {lemmas:?}");
        assert_eq!(found, 2);
        assert!(missing.is_empty());
    }

    // German: `strong` (table-tags), `de-conj` (inflection-template), `5 strong` (class) and the
    // auxiliary `haben` give no row.
    let german = [
        row_text("gesehen", "sehen", "verb", "", "participle past"),
        row_text(
            "sah",
            "sehen",
            "verb",
            "",
            "first-person preterite singular",
        ),
        row_text("sehen", "sehen", "verb", "", "canonical"),
        row_text(
            "sieht",
            "sehen",
            "verb",
            "",
            "present singular third-person",
        ),
    ];
    let wanted = ["sehen".to_string()];
    for lemmas in [None, Some(&wanted[..])] {
        let (rows, found, missing) = meta_rows(Lang::De, lemmas);
        assert_eq!(rows, german, "de, lemmas {lemmas:?}");
        assert_eq!(found, 1);
        assert!(missing.is_empty());
    }
}

// ---------------------------------------------------------------------------------------------
// 9. packs.lock.toml
// ---------------------------------------------------------------------------------------------

// Bullet 9: content/packs.lock.toml has `version`, `url`, `sha256`, `bytes` and `content_sha256`
// for es, fr and de, the url naming this repository's release asset of that version; each
// matches the built file in data-cache/packs/<lang>-<version>.sqlite3: its SHA-256 and size, and
// the content_sha256, language and build date pack_info reads from it (skipped without the
// pack).
#[test]
fn t09_the_lock_pins_every_pack() {
    let lock: toml::Table = read("content/packs.lock.toml")
        .parse()
        .unwrap_or_else(|e| panic!("content/packs.lock.toml: {e}"));
    assert_eq!(
        lock.get("schema").and_then(|s| s.as_str()),
        Some("parley.packs/1")
    );
    for lang in Lang::ALL {
        let l = lang.as_str();
        let table = lock
            .get(l)
            .and_then(|t| t.as_table())
            .unwrap_or_else(|| panic!("packs.lock.toml has no [{l}]"));
        let text = |key: &str| -> String {
            table
                .get(key)
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("[{l}] needs {key} as a string"))
                .to_string()
        };
        let version = text("version");
        assert!(
            LocalDate::parse(&version).is_some(),
            "[{l}] version {version}"
        );
        assert_eq!(
            text("url"),
            format!(
                "https://github.com/MH1044/parley/releases/download/packs-{version}/{l}-{version}.sqlite3"
            ),
            "[{l}] url"
        );
        let sha256 = text("sha256");
        assert!(is_hex64(&sha256), "[{l}] sha256 {sha256}");
        let content_sha256 = text("content_sha256");
        assert!(
            is_hex64(&content_sha256),
            "[{l}] content_sha256 {content_sha256}"
        );
        let bytes = table
            .get("bytes")
            .and_then(|v| v.as_integer())
            .unwrap_or_else(|| panic!("[{l}] needs bytes as an integer"));
        assert!(bytes > 0, "[{l}] bytes {bytes}");

        let path = repo_root()
            .join("data-cache")
            .join("packs")
            .join(format!("{l}-{version}.sqlite3"));
        if !path.is_file() {
            println!("{SKIPPED}");
            continue;
        }
        let file = fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(sha256_hex(&file), sha256, "[{l}] sha256 of the built pack");
        assert_eq!(file.len() as i64, bytes, "[{l}] bytes of the built pack");
        let info = pack_info(&path).unwrap_or_else(|e| panic!("pack info: {e}"));
        assert_eq!(info.content_sha256, content_sha256, "[{l}] content_sha256");
        assert_eq!(info.lang, l);
        assert_eq!(
            info.built_at, version,
            "[{l}] version is the pack's built_at"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// 11. Review files
// ---------------------------------------------------------------------------------------------

// Bullet 11 (the real files): each review.yaml validates against schemas/review.schema.json
// (so no field but unit, status and date: no reviewer names), names its language, and lists
// every unit of its syllabus exactly once and nothing else, each with status unreviewed or
// reviewed and a real date.
#[test]
fn t11_every_unit_is_listed_once_in_review_yaml() {
    let validator = json_schema("schemas/review.schema.json");
    for lang in Lang::ALL {
        let l = lang.as_str();
        let file = format!("content/{l}/review.yaml");
        let review = yaml(&file);
        let problems = violations(&validator, &review);
        assert!(problems.is_empty(), "{file}: {problems:#?}");
        assert_eq!(str_field(&review, "lang", &file), l);
        let mut listed: BTreeMap<String, usize> = BTreeMap::new();
        for entry in array(&review, "units", &file) {
            let keys: BTreeSet<&str> = entry
                .as_object()
                .unwrap_or_else(|| panic!("{file}: {entry}"))
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(
                keys,
                BTreeSet::from(["date", "status", "unit"]),
                "{file}: {entry}"
            );
            let status = str_field(entry, "status", &file);
            assert!(
                status == "unreviewed" || status == "reviewed",
                "{file}: status {status}"
            );
            let date = str_field(entry, "date", &file);
            assert!(LocalDate::parse(date).is_some(), "{file}: date {date}");
            *listed
                .entry(str_field(entry, "unit", &file).to_string())
                .or_default() += 1;
        }
        let syllabus = syllabus_at(&format!("content/{l}/syllabus.yaml"));
        let units: BTreeMap<String, usize> = syllabus
            .units
            .iter()
            .map(|unit| (unit.id.clone(), 1))
            .collect();
        assert_eq!(listed, units, "{file}: every unit exactly once");
    }
}

// Interfaces (review format): the schema accepts the fixture review files, whose faults in bad-RV
// are rule faults (RV02 to RV04), and refuses a reviewer's name.
#[test]
fn the_review_schema_accepts_the_fixtures_and_no_names() {
    let validator = json_schema("schemas/review.schema.json");
    for folder in ["course", "ranked", "bad-V22", "bad-RV"] {
        let file = format!("tests/fixtures/course05/{folder}/es/review.yaml");
        let problems = violations(&validator, &yaml(&file));
        assert!(problems.is_empty(), "{file}: {problems:#?}");
    }
    let mut named = yaml("tests/fixtures/course05/ranked/es/review.yaml");
    named["units"][0]["reviewer"] = Value::String("A. Reviewer".to_string());
    assert!(!validator.is_valid(&named), "a reviewer's name is refused");
    let mut bad_status = yaml("tests/fixtures/course05/ranked/es/review.yaml");
    bad_status["units"][0]["status"] = Value::String("checked".to_string());
    assert!(
        !validator.is_valid(&bad_status),
        "status is unreviewed or reviewed"
    );
}
