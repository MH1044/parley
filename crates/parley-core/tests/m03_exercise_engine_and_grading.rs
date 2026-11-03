//! Module 03 tests of the library (docs/modules/03-exercise-engine-and-grading.md, Acceptance
//! tests). Each test names the bullet it covers in its name (`tNN_...`) and in a comment; tests
//! without a number cover a Scope item or an Interfaces promise the bullets do not name. The
//! grading bullets 2 to 8 and the streak bullets 16 and 17 also run as golden cases under
//! tests/cases/03-exercise-engine-and-grading through `parley grade` and `parley streak
//! simulate`; the command line's usage errors and runs of `parley lesson template` are in
//! crates/parley-cli/tests/m03_exercise_engine_and_grading.rs, and the quality gates of bullet
//! 20 in crates/parley-core/tests/gates.rs.
//!
//! Expected values come from the spec's rules worked by hand, UAX #15, ISO 8601 week numbering,
//! docs/ARCHITECTURE.md §4, SHA-256 digests computed with standard tools, and the SplitMix64
//! reference output, never from running the code under test. The hand-written fixture course is
//! tests/cases/03-exercise-engine-and-grading/course-es.yaml with course-es.forms.tsv; the
//! composer fixtures sit beside it.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use parley_core::content::{
    Cefr, Example, FormRow, Forms, Gender, Item, ItemKind, Lang, Lemma, Pos, Syllabus, Token,
    derive_items, load_numbers, parse_syllabus, tokenise,
};
use parley_core::env::{
    Clock, LocalDate, Rng, SplitMix64, below, local_date, local_date_at, shuffle,
};
use parley_core::exercise::{
    Allowed, Body, Capabilities, CheckCtx, CheckFailure, ComposeRequest, Exercise, ExerciseType,
    Lesson, LessonRun, LessonSummary, Origin, Plan, RunError, RunSettings, Submission, TemplateCtx,
    check_exercise, compose, coverage, max_words, template, template_lesson, word_bank,
};
use parley_core::game::{
    DayRow, FreezeKind, SessionKind, SessionResult, StreakState, WEEKLY_RECORD, streak,
    weekly_record, xp_for,
};
use parley_core::grading::{
    AccentLeniency, Answer, Direction, Grade, GradeCtx, StoredJudgement, Tag, Verdict,
    accept_learner_answer, fold_german, fold_marks, grade, grade_ctx, judgement_key, normalise,
    noun_info, osa_distance,
};
use parley_core::srs::{self, Facet, Rating, SrsParams};
use parley_core::store::{AnswerSource, ReviewContext, Store, StoreError};
use rusqlite::Connection;
use serde_json::{Value, json};

const CASES: &str = "tests/cases/03-exercise-engine-and-grading";
const COURSE: &str = "tests/cases/03-exercise-engine-and-grading/course-es.yaml";
const COURSE_FORMS: &str = "tests/cases/03-exercise-engine-and-grading/course-es.forms.tsv";
const GREETINGS: &str = "es.a1.u01.s01";
const CAFE: &str = "es.a1.u01.s02";
const DAY: i64 = 86_400_000;
const HOUR: i64 = 3_600_000;
const MINUTE: i64 = 60_000;
/// 2026-01-01T00:00:00Z, a Thursday.
const JAN_1: i64 = 1_767_225_600_000;

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

fn syllabus_at(rel: &str) -> Syllabus {
    parse_syllabus(rel, &read(rel))
        .unwrap_or_else(|errors| panic!("{rel} does not parse: {errors:?}"))
}

fn forms_at(rel: &str) -> Forms {
    Forms::load_tsv(read(rel).as_bytes()).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn course() -> Syllabus {
    syllabus_at(COURSE)
}

fn course_forms() -> Forms {
    forms_at(COURSE_FORMS)
}

fn find_item<'a>(items: &'a [Item], id: &str) -> &'a Item {
    items
        .iter()
        .find(|item| item.id == id)
        .unwrap_or_else(|| panic!("no item {id}"))
}

/// An item id of the fixture course: `es.a1.u01.s02` and `l:agua` give `es.a1.u01.s02#l:agua`.
fn id(skill: &str, suffix: &str) -> String {
    format!("{skill}#{suffix}")
}

fn ok<T>(result: Result<T, StoreError>) -> T {
    match result {
        Ok(value) => value,
        Err(e) => panic!("store error: {e}"),
    }
}

fn run_ok<T>(result: Result<T, RunError>) -> T {
    match result {
        Ok(value) => value,
        Err(e) => panic!("run error: {e}"),
    }
}

fn run_err<T>(result: Result<T, RunError>) -> RunError {
    match result {
        Ok(_) => panic!("expected a run error"),
        Err(e) => e,
    }
}

/// An in-memory store with the fixture course started at time 0.
fn course_store() -> Store {
    let mut store = ok(Store::open_in_memory());
    ok(store.start_course(&course(), &course_forms(), 0));
    store
}

fn ymd(year: i32, month: u32, day: u32) -> LocalDate {
    LocalDate { year, month, day }
}

fn jan(day: u32) -> LocalDate {
    ymd(2026, 1, day)
}

/// Milliseconds of 2026-01-<day> at `hour`:00 UTC.
fn jan_at(day: u32, hour: i64) -> i64 {
    JAN_1 + i64::from(day - 1) * DAY + hour * HOUR
}

struct FixedClock {
    ms: i64,
    offset: i32,
}

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.ms
    }
    fn utc_offset_minutes(&self) -> i32 {
        self.offset
    }
}

fn clock(ms: i64) -> FixedClock {
    FixedClock { ms, offset: 0 }
}

fn strings(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|text| text.to_string()).collect()
}

fn row(form: &str, lemma: &str, pos: &str, gender: Option<Gender>) -> FormRow {
    FormRow {
        form: form.to_string(),
        lemma: lemma.to_string(),
        pos: pos.to_string(),
        gender,
        tags: "canonical".to_string(),
    }
}

fn exercise(id: &str, item_id: &str, facet: Facet, body: Body) -> Exercise {
    Exercise {
        id: id.to_string(),
        item_id: item_id.to_string(),
        facet,
        origin: Origin::Template,
        body,
    }
}

/// A `translate_to_target` exercise whose target is the first accepted answer.
fn to_target(id: &str, item_id: &str, prompt_en: &str, accepted: &[&str]) -> Exercise {
    exercise(
        id,
        item_id,
        Facet::Productive,
        Body::TranslateToTarget {
            prompt_en: prompt_en.to_string(),
            target: accepted[0].to_string(),
            accepted: strings(accepted),
            tiles: None,
        },
    )
}

fn to_en(id: &str, item_id: &str, prompt_target: &str, accepted: &[&str]) -> Exercise {
    exercise(
        id,
        item_id,
        Facet::Receptive,
        Body::TranslateToEn {
            prompt_target: prompt_target.to_string(),
            accepted: strings(accepted),
        },
    )
}

fn listen(id: &str, item_id: &str, text: &str, accepted: &[&str]) -> Exercise {
    exercise(
        id,
        item_id,
        Facet::Receptive,
        Body::ListenType {
            text: text.to_string(),
            accepted: strings(accepted),
        },
    )
}

fn speak(id: &str, item_id: &str, text: &str, prompt_en: &str) -> Exercise {
    exercise(
        id,
        item_id,
        Facet::Productive,
        Body::Speak {
            text: text.to_string(),
            prompt_en: prompt_en.to_string(),
        },
    )
}

fn fill_gap(id: &str, item_id: &str, sentence: &str, answer: &str, full: &str) -> Exercise {
    exercise(
        id,
        item_id,
        Facet::Productive,
        Body::FillGap {
            sentence: sentence.to_string(),
            answer: answer.to_string(),
            full_sentence: full.to_string(),
            hint_en: "a hint".to_string(),
        },
    )
}

fn pick(id: &str, item_id: &str, prompt_target: &str, options: &[&str], answer: u32) -> Exercise {
    exercise(
        id,
        item_id,
        Facet::Receptive,
        Body::PickMeaning {
            prompt_target: prompt_target.to_string(),
            options: strings(options),
            answer_index: answer,
        },
    )
}

fn pairs(id: &str, item_id: &str, pairs: &[(&str, &str)]) -> Exercise {
    exercise(
        id,
        item_id,
        Facet::Receptive,
        Body::MatchPairs {
            pairs: pairs
                .iter()
                .map(|(target, en)| [target.to_string(), en.to_string()])
                .collect(),
        },
    )
}

/// The `pick_meaning` exercise for `el agua` whose answer is option 0.
fn agua_pick(id_text: &str) -> Exercise {
    pick(
        id_text,
        &id(CAFE, "l:agua"),
        "el agua",
        &["water", "coffee", "milk", "dog"],
        0,
    )
}

fn text(answer: &str) -> Answer {
    Answer::Text(answer.to_string())
}

fn lesson_of(exercises: Vec<Exercise>) -> Lesson {
    Lesson {
        schema: "parley.lesson/1".to_string(),
        lang: Lang::Es,
        skill_id: CAFE.to_string(),
        seq: 1,
        origin: Origin::Template,
        exercises,
    }
}

fn submission(exercise_id: &str, answer: Answer) -> Submission {
    Submission {
        exercise_id: exercise_id.to_string(),
        answer,
        response_ms: Some(4_000),
        used_hint: false,
    }
}

fn plain_ctx(lang: Lang, forms: &Forms) -> GradeCtx<'_> {
    GradeCtx {
        lang,
        leniency: AccentLeniency::Lenient,
        forms,
        noun: None,
        stored: Vec::new(),
        judgement: None,
    }
}

#[track_caller]
fn assert_grade(
    g: &Grade,
    verdict: Verdict,
    tags: &[Tag],
    message: Option<&str>,
    corrected: Option<&str>,
    words: &[&str],
    rating: Option<Rating>,
) {
    assert_eq!(g.verdict, verdict, "verdict");
    assert_eq!(g.tags.as_slice(), tags, "tags");
    assert_eq!(g.message.as_deref(), message, "message");
    assert_eq!(g.corrected.as_deref(), corrected, "corrected");
    assert_eq!(g.words, words, "words");
    assert_eq!(g.rating, rating, "rating");
}

fn source_name(source: &AnswerSource) -> &'static str {
    match source {
        AnswerSource::Content => "content",
        AnswerSource::Judge => "judge",
        AnswerSource::Learner => "learner",
    }
}

fn kind_name(kind: &FreezeKind) -> &'static str {
    match kind {
        FreezeKind::Earned => "earned",
        FreezeKind::Used => "used",
    }
}

/// One lesson of one `pick_meaning` exercise answered correctly at `ms` (UTC), finished at once.
fn one_lesson_at(store: &mut Store, forms: &Forms, session: &str, ms: i64) -> LessonSummary {
    let settings = RunSettings::default();
    let at = clock(ms);
    let lesson = lesson_of(vec![agua_pick("e1")]);
    let mut run = ok(LessonRun::start_lesson(store, &lesson, session, &at));
    let g = run_ok(run.submit(
        store,
        forms,
        &settings,
        &submission("e1", Answer::Choice(0)),
        &at,
    ));
    assert_eq!(g.verdict, Verdict::Correct);
    run_ok(run.finish(store, &settings, &at))
}

// ---------------------------------------------------------------------------------------------
// 1. The normaliser
// ---------------------------------------------------------------------------------------------

// Bullet 1: the spec's five cases. NFC follows UAX #15 (e + U+0301 composes to é); punctuation
// becomes a space; an apostrophe or hyphen between two letters stays.
#[test]
fn t01_normalise_the_spec_cases() {
    assert_eq!(normalise(Lang::Es, "  ¿Qué  TAL? "), "qué tal");
    assert_eq!(
        normalise(Lang::Fr, "Aujourd\u{2019}hui, c\u{2019}est lundi !"),
        "aujourd'hui c'est lundi"
    );
    assert_eq!(normalise(Lang::De, "Die Straße."), "die straße");
    assert_eq!(normalise(Lang::Es, "e\u{301}l"), "\u{e9}l");
    assert_eq!(normalise(Lang::Fr, "Est-ce que\u{2026}"), "est-ce que");
}

// Bullet 1 and Design notes: each step of the normaliser in turn.
#[test]
fn t01_normalise_each_step() {
    // The three typographic apostrophes become ', which stays between two letters.
    for apostrophe in ['\u{2019}', '\u{2018}', '\u{2bc}'] {
        assert_eq!(
            normalise(Lang::Fr, &format!("L{apostrophe}homme")),
            "l'homme",
            "{apostrophe}"
        );
    }
    // U+00A0 and U+202F are spaces.
    assert_eq!(
        normalise(Lang::Es, "buenos\u{a0}días\u{202f}Ana"),
        "buenos días ana"
    );
    // Lower case, not case folding: ß stays, and capital ẞ lowers to ß.
    assert_eq!(normalise(Lang::De, "STRA\u{1e9e}E"), "straße");
    assert_eq!(normalise(Lang::De, "Straße"), "straße");
    // Punctuation and symbols become spaces, so words written together stay two words.
    assert_eq!(normalise(Lang::Es, "Hola,¿qué tal?"), "hola qué tal");
    assert_eq!(normalise(Lang::Es, "café \u{2615} 2\u{20ac}"), "café 2");
    assert_eq!(normalise(Lang::Es, "¡Hola!\t¿Qué\ntal?"), "hola qué tal");
    assert_eq!(normalise(Lang::Es, "Hola\u{2013}adiós"), "hola adiós");
    // An apostrophe or hyphen-minus that is not between two letters is a space.
    assert_eq!(normalise(Lang::Fr, "'hola' - adiós -"), "hola adiós");
    assert_eq!(normalise(Lang::Fr, "c' est"), "c est");
    assert_eq!(normalise(Lang::Fr, "a-t-il"), "a-t-il");
    // Numbers stay.
    assert_eq!(normalise(Lang::Es, "Tengo 3 gatos."), "tengo 3 gatos");
    // A combining mark with no precomposed letter stays.
    assert_eq!(normalise(Lang::Es, "q\u{301}"), "q\u{301}");
    // Nothing but punctuation and spaces is the empty string.
    assert_eq!(normalise(Lang::Es, " ¿? ... "), "");
}

// Bullet 1 and Interfaces: the rules do not depend on the language, and a normal form is its
// own normal form.
#[test]
fn t01_normalise_is_the_same_in_every_language_and_idempotent() {
    for sample in [
        "  ¿Qué  TAL? ",
        "Aujourd\u{2019}hui, c\u{2019}est lundi !",
        "Die Straße.",
        "e\u{301}l",
        "Est-ce que\u{2026}",
        "I'm here!",
    ] {
        let once = normalise(Lang::Es, sample);
        assert_eq!(normalise(Lang::Fr, sample), once, "{sample}");
        assert_eq!(normalise(Lang::De, sample), once, "{sample}");
        assert_eq!(normalise(Lang::Es, &once), once, "{sample}");
    }
}

// Bullet 4 and Interfaces: F2 removes combining marks after NFD and recomposes; F1 maps the
// German umlauts and ß.
#[test]
fn t04_fold_marks_and_fold_german() {
    assert_eq!(fold_marks("días"), "dias");
    assert_eq!(fold_marks("Mädchen"), "Madchen");
    assert_eq!(fold_marks("cafe\u{301}"), "cafe");
    assert_eq!(fold_marks("français niño"), "francais nino");
    assert_eq!(fold_marks("straße"), "straße", "ß has no combining mark");
    assert_eq!(fold_german("das mädchen"), "das maedchen");
    assert_eq!(fold_german("die straße"), "die strasse");
    assert_eq!(fold_german("über öl"), "ueber oel");
}

// Bullet 6 and ARCHITECTURE §6: the optimal string alignment distance (Damerau 1964;
// Levenshtein 1966) over chars. A transposition costs 1, and no substring is edited twice, so
// "ca" to "abc" is 3 where the unrestricted Damerau–Levenshtein distance is 2.
#[test]
fn t06_osa_distance() {
    assert_eq!(osa_distance("leche", "leche"), 0);
    assert_eq!(osa_distance("leche", "lecje"), 1);
    assert_eq!(osa_distance("perro", "pero"), 1);
    assert_eq!(osa_distance("leche", "lehce"), 1);
    assert_eq!(osa_distance("comprar", "compar"), 1);
    assert_eq!(osa_distance("día", "dia"), 1, "chars, not bytes");
    assert_eq!(osa_distance("", "abc"), 3);
    assert_eq!(osa_distance("ca", "abc"), 3);
}

// ---------------------------------------------------------------------------------------------
// 2 to 8. Grading
// ---------------------------------------------------------------------------------------------

// Bullet 2: expected `Hola, ¿qué tal?`; `hola qué tal` is exact once normalised; `Hola que tal`
// is an accent slip, lenient by default: almost, Hard, naming the expected spelling `qué`.
#[test]
fn t02_hola_que_tal_exact_and_accent() {
    let forms = Forms::default();
    let ctx = plain_ctx(Lang::Es, &forms);
    let ex = to_target("e1", "x", "Hi, how are you?", &["Hola, ¿qué tal?"]);
    assert_grade(
        &grade(&ctx, &ex, &text("hola qué tal")),
        Verdict::Correct,
        &[],
        None,
        None,
        &[],
        Some(Rating::Good),
    );
    assert_grade(
        &grade(&ctx, &ex, &text("Hola que tal")),
        Verdict::Almost,
        &[Tag::Accent],
        Some("Almost: mind the accent: qué"),
        Some("Hola, ¿qué tal?"),
        &["qué"],
        Some(Rating::Hard),
    );
}

// Bullet 3: `Buenos dias` for `Buenos días.` under each accent setting: lenient almost (Hard),
// strict wrong (Again), ignore correct (Good) with a note.
#[test]
fn t03_buenos_dias_under_each_accent_setting() {
    let forms = Forms::default();
    let ex = to_target("e1", "x", "Good morning.", &["Buenos días."]);
    let answer = text("Buenos dias");
    let with = |leniency: AccentLeniency| GradeCtx {
        leniency,
        ..plain_ctx(Lang::Es, &forms)
    };
    assert_grade(
        &grade(&with(AccentLeniency::Lenient), &ex, &answer),
        Verdict::Almost,
        &[Tag::Accent],
        Some("Almost: mind the accent: días"),
        Some("Buenos días."),
        &["días"],
        Some(Rating::Hard),
    );
    assert_grade(
        &grade(&with(AccentLeniency::Strict), &ex, &answer),
        Verdict::Wrong,
        &[Tag::Accent],
        Some("Wrong: mind the accent: días"),
        Some("Buenos días."),
        &["días"],
        Some(Rating::Again),
    );
    assert_grade(
        &grade(&with(AccentLeniency::Ignore), &ex, &answer),
        Verdict::Correct,
        &[Tag::Accent],
        Some("Note the accent: días"),
        Some("Buenos días."),
        &["días"],
        Some(Rating::Good),
    );
    assert!(matches!(AccentLeniency::default(), AccentLeniency::Lenient));
}

// Bullet 4: German umlaut and ß alternatives are accent slips (F1 on both sides, or F2).
#[test]
fn t04_german_umlaut_and_eszett_alternatives() {
    let forms = Forms::default();
    let ctx = plain_ctx(Lang::De, &forms);
    let girl = to_target("e1", "x", "the girl", &["Das Mädchen"]);
    for answer in ["das Maedchen", "das Madchen"] {
        assert_grade(
            &grade(&ctx, &girl, &text(answer)),
            Verdict::Almost,
            &[Tag::Accent],
            Some("Almost: mind the accent: Mädchen"),
            Some("Das Mädchen"),
            &["Mädchen"],
            Some(Rating::Hard),
        );
    }
    let street = to_target("e2", "x", "the street", &["die Straße"]);
    assert_grade(
        &grade(&ctx, &street, &text("die Strasse")),
        Verdict::Almost,
        &[Tag::Accent],
        Some("Almost: mind the accent: Straße"),
        Some("die Straße"),
        &["Straße"],
        Some(Rating::Hard),
    );
    // F1 applies to both sides, so ß for an expected ss is a slip too.
    let written_ss = to_target("e3", "x", "the street", &["die Strasse"]);
    assert_grade(
        &grade(&ctx, &written_ss, &text("die Straße")),
        Verdict::Almost,
        &[Tag::Accent],
        Some("Almost: mind the accent: Strasse"),
        Some("die Strasse"),
        &["Strasse"],
        Some(Rating::Hard),
    );
    // F1 is German only: in a Spanish course ss for ß is two edits, neither a slip nor a typo.
    let es = plain_ctx(Lang::Es, &forms);
    let heard = listen("e4", "x", "die Straße", &["die Straße"]);
    assert_eq!(
        grade(&es, &heard, &text("die Strasse")).verdict,
        Verdict::Wrong
    );
    assert_eq!(
        grade(&ctx, &heard, &text("die Strasse")).verdict,
        Verdict::Almost
    );
}

// Bullet 5 and Design notes (`noun_info`): a definite article, a space and a noun; the gender
// is the forms table's, else the article's, else none.
#[test]
fn t05_noun_info_reads_the_article_and_the_gender() {
    let forms = course_forms();
    let none = Forms::default();
    let day = noun_info(Lang::Es, "el día", &forms).expect("el día is a noun with its article");
    assert_eq!(
        (day.article.as_str(), day.noun.as_str(), day.gender),
        ("el", "día", Gender::M)
    );
    // Agua takes el but is feminine: the forms table's gender comes first.
    let water = noun_info(Lang::Es, "el agua", &forms).expect("el agua");
    assert_eq!(
        (water.article.as_str(), water.noun.as_str(), water.gender),
        ("el", "agua", Gender::F)
    );
    // Without the table, the article decides.
    assert_eq!(
        noun_info(Lang::Es, "el agua", &none).map(|n| n.gender),
        Some(Gender::M)
    );
    assert_eq!(
        noun_info(Lang::Es, "la leche", &none).map(|n| n.gender),
        Some(Gender::F)
    );
    assert!(noun_info(Lang::Es, "querer", &forms).is_none());
    assert!(noun_info(Lang::Es, "Quiero un café.", &forms).is_none());
    // German der, die, das.
    assert_eq!(
        noun_info(Lang::De, "das Mädchen", &none).map(|n| n.gender),
        Some(Gender::N)
    );
    assert_eq!(
        noun_info(Lang::De, "der Tisch", &none).map(|n| n.gender),
        Some(Gender::M)
    );
    assert_eq!(
        noun_info(Lang::De, "die Straße", &none).map(|n| n.gender),
        Some(Gender::F)
    );
    // French l' joins the noun with no space and carries no gender of its own.
    let homme = Forms::from_rows(vec![row("homme", "homme", "noun", Some(Gender::M))]);
    let man = noun_info(Lang::Fr, "l'homme", &homme).expect("l'homme with the table's gender");
    assert_eq!(
        (man.article.as_str(), man.noun.as_str(), man.gender),
        ("l'", "homme", Gender::M)
    );
    assert!(noun_info(Lang::Fr, "l'homme", &none).is_none());
    assert_eq!(
        noun_info(Lang::Fr, "la maison", &none).map(|n| n.gender),
        Some(Gender::F)
    );
}

// Bullet 5: the article rule for a vocabulary noun. `día` alone is almost; another definite
// article is wrong and names the gender; `el agua` is feminine although it takes `el`.
#[test]
fn t05_article_rule_for_vocabulary_nouns() {
    let forms = course_forms();
    let day = to_target("e1", &id(GREETINGS, "l:día"), "day", &["el día"]);
    let ctx = GradeCtx {
        noun: noun_info(Lang::Es, "el día", &forms),
        ..plain_ctx(Lang::Es, &forms)
    };
    assert_grade(
        &grade(&ctx, &day, &text("día")),
        Verdict::Almost,
        &[Tag::Article],
        Some("Almost: remember the article: el día"),
        Some("el día"),
        &["el día"],
        Some(Rating::Hard),
    );
    assert_grade(
        &grade(&ctx, &day, &text("la día")),
        Verdict::Wrong,
        &[Tag::Article],
        Some("Día is masculine: el día."),
        Some("el día"),
        &["el día"],
        Some(Rating::Again),
    );
    assert_grade(
        &grade(&ctx, &day, &text("El día")),
        Verdict::Correct,
        &[],
        None,
        None,
        &[],
        Some(Rating::Good),
    );
    // Without ctx.noun the rule does not run: a bare noun is an unchecked free translation.
    assert_eq!(
        grade(&plain_ctx(Lang::Es, &forms), &day, &text("día")).verdict,
        Verdict::Unchecked
    );

    let water = to_target("e2", &id(CAFE, "l:agua"), "water", &["el agua"]);
    let ctx = GradeCtx {
        noun: noun_info(Lang::Es, "el agua", &forms),
        ..plain_ctx(Lang::Es, &forms)
    };
    assert_grade(
        &grade(&ctx, &water, &text("la agua")),
        Verdict::Wrong,
        &[Tag::Article],
        Some("Agua is feminine: el agua."),
        Some("el agua"),
        &["el agua"],
        Some(Rating::Again),
    );
    assert_grade(
        &grade(&ctx, &water, &text("agua")),
        Verdict::Almost,
        &[Tag::Article],
        Some("Almost: remember the article: el agua"),
        Some("el agua"),
        &["el agua"],
        Some(Rating::Hard),
    );

    // German neuter.
    let none = Forms::default();
    let girl = to_target("e3", "x", "the girl", &["das Mädchen"]);
    let ctx = GradeCtx {
        noun: noun_info(Lang::De, "das Mädchen", &none),
        ..plain_ctx(Lang::De, &none)
    };
    assert_grade(
        &grade(&ctx, &girl, &text("der Mädchen")),
        Verdict::Wrong,
        &[Tag::Article],
        Some("Mädchen is neuter: das Mädchen."),
        Some("das Mädchen"),
        &["das Mädchen"],
        Some(Rating::Again),
    );
}

// Bullet 6: one slip in a long word is a typo, naming the expected word; an accent slip in
// another word adds the accent tag; two edits, or a missing word, are not typos.
#[test]
fn t06_one_letter_slip_in_a_long_word_is_a_typo() {
    let forms = course_forms();
    let ctx = plain_ctx(Lang::Es, &forms);
    let ex = to_target(
        "e1",
        &id(CAFE, "e:2"),
        "I want a coffee with milk.",
        &["Quiero un café con leche."],
    );
    assert_grade(
        &grade(&ctx, &ex, &text("Quiero un café con lecje")),
        Verdict::Typo,
        &[Tag::Typo],
        Some("Typo: leche"),
        Some("Quiero un café con leche."),
        &["leche"],
        Some(Rating::Hard),
    );
    assert_eq!(
        grade(&ctx, &ex, &text("quiero un café con lehce")).verdict,
        Verdict::Typo,
        "a transposition is one edit"
    );
    assert_grade(
        &grade(&ctx, &ex, &text("quiero un cafe con lecje")),
        Verdict::Typo,
        &[Tag::Typo, Tag::Accent],
        Some("Typo: café, leche"),
        Some("Quiero un café con leche."),
        &["café", "leche"],
        Some(Rating::Hard),
    );
    assert_eq!(
        grade(&ctx, &ex, &text("Quiero un café con lxxhe")).verdict,
        Verdict::Unchecked
    );
    assert_eq!(
        grade(&ctx, &ex, &text("Quiero café con leche")).verdict,
        Verdict::Unchecked
    );
}

// Bullet 6: a slip that is itself a word of the forms table (`pero`, or German `Fisch` looked
// up with its first letter in upper case) is not a typo, nor is a slip in a word shorter than
// four letters.
#[test]
fn t06_a_known_word_or_a_short_word_is_not_a_typo() {
    let dog = listen("e1", "x", "el perro", &["el perro"]);
    let without_pero = Forms::from_rows(vec![row("perro", "perro", "noun", Some(Gender::M))]);
    assert_grade(
        &grade(&plain_ctx(Lang::Es, &without_pero), &dog, &text("el pero")),
        Verdict::Typo,
        &[Tag::Typo],
        Some("Typo: perro"),
        Some("el perro"),
        &["perro"],
        Some(Rating::Hard),
    );
    let with_pero = course_forms();
    assert_grade(
        &grade(&plain_ctx(Lang::Es, &with_pero), &dog, &text("el pero")),
        Verdict::Wrong,
        &[],
        None,
        Some("el perro"),
        &[],
        Some(Rating::Again),
    );

    let table = listen("e2", "x", "der Tisch", &["der Tisch"]);
    let tisch = row("Tisch", "Tisch", "noun", Some(Gender::M));
    let no_fisch = Forms::from_rows(vec![tisch.clone()]);
    let fisch = Forms::from_rows(vec![tisch, row("Fisch", "Fisch", "noun", Some(Gender::M))]);
    assert_grade(
        &grade(&plain_ctx(Lang::De, &no_fisch), &table, &text("der fisch")),
        Verdict::Typo,
        &[Tag::Typo],
        Some("Typo: Tisch"),
        Some("der Tisch"),
        &["Tisch"],
        Some(Rating::Hard),
    );
    assert_eq!(
        grade(&plain_ctx(Lang::De, &fisch), &table, &text("der fisch")).verdict,
        Verdict::Wrong
    );

    let none = Forms::default();
    let book = listen("e3", "x", "le livre", &["le livre"]);
    assert_eq!(
        grade(&plain_ctx(Lang::Fr, &none), &book, &text("la livre")).verdict,
        Verdict::Wrong,
        "le has two letters"
    );
    let sun = listen("e4", "x", "el sol", &["el sol"]);
    assert_eq!(
        grade(&plain_ctx(Lang::Es, &none), &sun, &text("el sal")).verdict,
        Verdict::Wrong,
        "sol has three letters"
    );
    let house = listen("e5", "x", "una casa", &["una casa"]);
    assert_grade(
        &grade(&plain_ctx(Lang::Es, &none), &house, &text("una cosa")),
        Verdict::Typo,
        &[Tag::Typo],
        Some("Typo: casa"),
        Some("una casa"),
        &["casa"],
        Some(Rating::Hard),
    );
}

// Bullet 7: up to max(1, n / 4) differing words may slip: 2 of 8 is a typo, 3 of 8 is not; 1 of
// 4 is, 2 of 4 is not.
#[test]
fn t07_up_to_a_quarter_of_the_words_may_slip() {
    let forms = course_forms();
    let ctx = plain_ctx(Lang::Es, &forms);
    let long = "Nosotros queremos comprar pescado fresco para nuestra familia.";
    let eight = listen("e1", "x", long, &[long]);
    assert_grade(
        &grade(
            &ctx,
            &eight,
            &text("Nosotros queremos comprar pescodo fresco para nuestra famila"),
        ),
        Verdict::Typo,
        &[Tag::Typo],
        Some("Typo: pescado, familia"),
        Some(long),
        &["pescado", "familia"],
        Some(Rating::Hard),
    );
    assert_grade(
        &grade(
            &ctx,
            &eight,
            &text("Nosotros queremos compar pescodo fresco para nuestra famila"),
        ),
        Verdict::Wrong,
        &[],
        None,
        Some(long),
        &[],
        Some(Rating::Again),
    );
    let four = listen(
        "e2",
        "x",
        "Quiero comprar pescado fresco",
        &["Quiero comprar pescado fresco"],
    );
    assert_eq!(
        grade(&ctx, &four, &text("Quiero compar pescado fresco")).verdict,
        Verdict::Typo
    );
    assert_eq!(
        grade(&ctx, &four, &text("Quiero compar pescodo fresco")).verdict,
        Verdict::Wrong
    );
}

// Bullet 8: an unmatched translate_to_target or translate_to_en is unchecked (no rating, nothing
// corrected); every other type is wrong, corrected to its first answer, rated Again. An answer
// that normalises to nothing is wrong with no message.
#[test]
fn t08_unmatched_free_translations_are_unchecked_and_the_rest_wrong() {
    let forms = course_forms();
    let ctx = plain_ctx(Lang::Es, &forms);
    let e3 = id(CAFE, "e:3");
    let answer = text("Tengo un perro grande y negro");
    for ex in [
        to_target("e1", &e3, "I want a coffee.", &["Quiero un café."]),
        to_en("e2", &e3, "Quiero un café.", &["I want a coffee."]),
    ] {
        assert_grade(
            &grade(&ctx, &ex, &answer),
            Verdict::Unchecked,
            &[],
            None,
            None,
            &[],
            None,
        );
    }
    for (ex, corrected) in [
        (
            listen("e3", &e3, "Quiero un café.", &["Quiero un café."]),
            "Quiero un café.",
        ),
        (
            speak("e4", &e3, "Quiero un café.", "I want a coffee."),
            "Quiero un café.",
        ),
        (
            fill_gap(
                "e5",
                &id(CAFE, "l:leche"),
                "Quiero un café con ___.",
                "leche",
                "Quiero un café con leche.",
            ),
            "leche",
        ),
    ] {
        assert_grade(
            &grade(&ctx, &ex, &answer),
            Verdict::Wrong,
            &[],
            None,
            Some(corrected),
            &[],
            Some(Rating::Again),
        );
    }
    let empty = grade(
        &ctx,
        &to_target("e6", &e3, "I want a coffee.", &["Quiero un café."]),
        &text(" ¿? "),
    );
    assert_eq!(empty.verdict, Verdict::Wrong);
    assert_eq!(empty.message, None);
}

// Design notes: `pick_meaning` is correct for its answer index or a text equal to that option;
// `match_pairs` for exactly the set {[i, i]}; anything else is wrong.
#[test]
fn pick_meaning_and_match_pairs_are_graded_by_choice() {
    let forms = Forms::default();
    let ctx = plain_ctx(Lang::Es, &forms);
    let ex = pick(
        "e1",
        &id(CAFE, "l:agua"),
        "el agua",
        &["milk", "Water", "coffee", "dog"],
        1,
    );
    let right = grade(&ctx, &ex, &Answer::Choice(1));
    assert_eq!(right.verdict, Verdict::Correct);
    assert_eq!(right.rating, Some(Rating::Good));
    assert_eq!(grade(&ctx, &ex, &text("water!")).verdict, Verdict::Correct);
    let wrong = grade(&ctx, &ex, &Answer::Choice(0));
    assert_eq!(wrong.verdict, Verdict::Wrong);
    assert_eq!(wrong.rating, Some(Rating::Again));
    assert_eq!(grade(&ctx, &ex, &text("milk")).verdict, Verdict::Wrong);

    let mp = pairs(
        "e2",
        &id(CAFE, "l:café"),
        &[
            ("el café", "coffee"),
            ("la leche", "milk"),
            ("el agua", "water"),
            ("el perro", "dog"),
        ],
    );
    assert_eq!(
        grade(
            &ctx,
            &mp,
            &Answer::Pairs(vec![[2, 2], [0, 0], [3, 3], [1, 1]])
        )
        .verdict,
        Verdict::Correct
    );
    assert_eq!(
        grade(
            &ctx,
            &mp,
            &Answer::Pairs(vec![[0, 1], [1, 0], [2, 2], [3, 3]])
        )
        .verdict,
        Verdict::Wrong
    );
    assert_eq!(
        grade(&ctx, &mp, &Answer::Pairs(vec![[0, 0], [1, 1], [2, 2]])).verdict,
        Verdict::Wrong
    );
    assert_eq!(grade(&ctx, &mp, &Answer::Choice(0)).verdict, Verdict::Wrong);
}

// Design notes, rules 1 to 5: the stored answers follow the exercise's own as candidates, and a
// cached judgement applies only after exact, accent and typo, keeping its accent, article and
// typo tags.
#[test]
fn stored_answers_and_judgements_extend_the_candidates() {
    let forms = course_forms();
    let ex = to_target(
        "e1",
        &id(CAFE, "e:3"),
        "I want a coffee.",
        &["Quiero un café."],
    );
    let stored = GradeCtx {
        stored: strings(&["Yo quiero un café.", "Un café, por favor."]),
        ..plain_ctx(Lang::Es, &forms)
    };
    assert_grade(
        &grade(&stored, &ex, &text("yo quiero un café")),
        Verdict::Correct,
        &[],
        None,
        None,
        &[],
        Some(Rating::Good),
    );
    assert_grade(
        &grade(&stored, &ex, &text("un cafe por favor")),
        Verdict::Almost,
        &[Tag::Accent],
        Some("Almost: mind the accent: café"),
        Some("Un café, por favor."),
        &["café"],
        Some(Rating::Hard),
    );

    let judged = |verdict: srs::Verdict, tags: &[&str]| GradeCtx {
        judgement: Some(StoredJudgement {
            verdict,
            corrected: "Quiero un café, por favor.".to_string(),
            explanation: "Ask with por favor.".to_string(),
            tags: strings(tags),
            provider: "fixture".to_string(),
        }),
        ..plain_ctx(Lang::Es, &forms)
    };
    assert_grade(
        &grade(
            &judged(srs::Verdict::Almost, &["typo", "grammar", "accent"]),
            &ex,
            &text("Dame un café"),
        ),
        Verdict::Almost,
        &[Tag::Typo, Tag::Accent],
        None,
        Some("Quiero un café, por favor."),
        &[],
        Some(Rating::Hard),
    );
    assert_grade(
        &grade(
            &judged(srs::Verdict::Wrong, &[]),
            &ex,
            &text("Dame un café"),
        ),
        Verdict::Wrong,
        &[],
        None,
        Some("Quiero un café, por favor."),
        &[],
        Some(Rating::Again),
    );
    assert_grade(
        &grade(
            &judged(srs::Verdict::Wrong, &[]),
            &ex,
            &text("Quiero un café"),
        ),
        Verdict::Correct,
        &[],
        None,
        None,
        &[],
        Some(Rating::Good),
    );
}

// ---------------------------------------------------------------------------------------------
// 9. Lookups, "I was right" and judgement keys
// ---------------------------------------------------------------------------------------------

// Bullet 9: judgement_key is the SHA-256 of lang, U+001F, the direction, U+001F, the NFC source,
// U+001F and the answer's normal form, in lower-case hex. The two digests were computed with
// `printf 'es\x1fto_target\x1fI want a coffee.\x1fyo quiero un caf\xc3\xa9' | sha256sum` and
// `printf 'de\x1fto_en\x1fDas M\xc3\xa4dchen trinkt Wasser.\x1fthe girl drinks water' |
// sha256sum`, and agree with Python's hashlib.sha256 over the same UTF-8 bytes.
#[test]
fn t09_judgement_key_is_the_sha256_of_its_fields() {
    assert_eq!(
        judgement_key(
            Lang::Es,
            Direction::ToTarget,
            "I want a coffee.",
            "yo quiero un café"
        ),
        "f599477e4ed011ca540a2a5e0881ce263a90b745e4b3305582baf53a9def3b07"
    );
    let girl = "39faab7ffbb0ae3df5e7da532162e0534fedbfe0594cc4dd8d45b4f5ce5e611d";
    assert_eq!(
        judgement_key(
            Lang::De,
            Direction::ToEn,
            "Das Mädchen trinkt Wasser.",
            "the girl drinks water"
        ),
        girl
    );
    assert_eq!(
        judgement_key(
            Lang::De,
            Direction::ToEn,
            "Das Ma\u{308}dchen trinkt Wasser.",
            "the girl drinks water"
        ),
        girl,
        "the source is taken in NFC"
    );
    assert_eq!(Direction::ToTarget.as_str(), "to_target");
    assert_eq!(Direction::ToEn.as_str(), "to_en");
}

// Bullet 9 and Design notes (lookups): stored answers for the item and direction when the
// prompt is the item's own; the judgement under the key of the prompt; the noun of a vocabulary
// item's translate_to_target, listen_type and speak.
#[test]
fn t09_grade_ctx_looks_up_answers_judgements_and_nouns() {
    let forms = course_forms();
    let mut store = course_store();
    let e3 = id(CAFE, "e:3");
    let answer = text("Quiero café");

    let own = to_target("e1", &e3, "I would like a coffee.", &["Quiero un café."]);
    {
        let ctx = ok(grade_ctx(
            &store,
            &forms,
            AccentLeniency::Strict,
            &own,
            &answer,
        ));
        assert_eq!(ctx.stored, strings(&["Quiero un café."]));
        assert_eq!(ctx.lang, Lang::Es);
        assert!(matches!(ctx.leniency, AccentLeniency::Strict));
        assert!(ctx.noun.is_none());
        assert!(ctx.judgement.is_none());
    }

    let other = to_target("e2", &e3, "Coffee for me.", &["Quiero un café."]);
    assert!(
        ok(grade_ctx(
            &store,
            &forms,
            AccentLeniency::Lenient,
            &other,
            &answer
        ))
        .stored
        .is_empty(),
        "a prompt that is not the item's own looks up no stored answers"
    );

    let back = to_en("e3", &e3, "Quiero un cafe\u{301}.", &["I want a coffee."]);
    let mut stored = ok(grade_ctx(
        &store,
        &forms,
        AccentLeniency::Lenient,
        &back,
        &text("I want coffee"),
    ))
    .stored;
    stored.sort();
    assert_eq!(
        stored,
        strings(&["I want a coffee.", "I would like a coffee."]),
        "the item's target, compared in NFC"
    );
    for ex in [
        listen("e4", &e3, "Quiero un café.", &["Quiero un café."]),
        speak("e5", &e3, "Quiero un café.", "I want a coffee."),
    ] {
        assert_eq!(
            ok(grade_ctx(
                &store,
                &forms,
                AccentLeniency::Lenient,
                &ex,
                &answer
            ))
            .stored,
            strings(&["Quiero un café."])
        );
    }
    let gap = fill_gap(
        "e6",
        &id(CAFE, "l:leche"),
        "Quiero un café con ___.",
        "leche",
        "Quiero un café con leche.",
    );
    assert!(
        ok(grade_ctx(
            &store,
            &forms,
            AccentLeniency::Lenient,
            &gap,
            &text("agua")
        ))
        .stored
        .is_empty()
    );

    let agua = id(CAFE, "l:agua");
    for ex in [
        to_target("e7", &agua, "water", &["el agua"]),
        listen("e8", &agua, "el agua", &["el agua"]),
        speak("e9", &agua, "el agua", "water"),
    ] {
        let noun = ok(grade_ctx(
            &store,
            &forms,
            AccentLeniency::Lenient,
            &ex,
            &text("agua"),
        ))
        .noun
        .expect("el agua is a noun");
        assert_eq!(
            (noun.article.as_str(), noun.noun.as_str(), noun.gender),
            ("el", "agua", Gender::F)
        );
    }
    for ex in [
        to_en("e10", &agua, "el agua", &["water"]),
        to_target(
            "e11",
            &id(CAFE, "e:4"),
            "The dog wants water.",
            &["El perro quiere agua."],
        ),
        to_target("e12", &id(CAFE, "l:querer"), "to want", &["querer"]),
    ] {
        assert!(
            ok(grade_ctx(
                &store,
                &forms,
                AccentLeniency::Lenient,
                &ex,
                &text("agua")
            ))
            .noun
            .is_none(),
            "{}",
            ex.id
        );
    }

    let key = judgement_key(
        Lang::Es,
        Direction::ToTarget,
        "Coffee for me.",
        "un café para mí",
    );
    let stored_judgement = StoredJudgement {
        verdict: srs::Verdict::Almost,
        corrected: "Un café para mí, por favor.".to_string(),
        explanation: "Add por favor.".to_string(),
        tags: Vec::new(),
        provider: "fixture".to_string(),
    };
    ok(store.put_judgement(&key, &stored_judgement, 5));
    let judgement = ok(grade_ctx(
        &store,
        &forms,
        AccentLeniency::Lenient,
        &other,
        &text("Un café, para mí."),
    ))
    .judgement
    .expect("the cached judgement of the answer's key");
    assert_eq!(judgement.verdict, srs::Verdict::Almost);
    assert_eq!(judgement.corrected, "Un café para mí, por favor.");
    assert_eq!(judgement.provider, "fixture");

    // fill_gap's source is its sentence, gap included.
    let gap_key = judgement_key(
        Lang::Es,
        Direction::ToTarget,
        "Quiero un café con ___.",
        "agua",
    );
    ok(store.put_judgement(&gap_key, &stored_judgement, 6));
    assert!(
        ok(grade_ctx(
            &store,
            &forms,
            AccentLeniency::Lenient,
            &gap,
            &text("Agua")
        ))
        .judgement
        .is_some()
    );
}

// Bullet 9: "I was right" on an unchecked translate_to_target whose prompt is the item's own:
// rating Good, a learner row in accepted_answers (the answer in NFC, trimmed), no judgement; the
// same answer is correct next time, offline.
#[test]
fn t09_i_was_right_on_the_items_own_prompt_adds_a_learner_answer() {
    let forms = course_forms();
    let settings = RunSettings::default();
    let mut store = course_store();
    let e3 = id(CAFE, "e:3");
    let lesson = lesson_of(vec![to_target(
        "e1",
        &e3,
        "I want a coffee.",
        &["Quiero un café."],
    )]);
    let at = clock(jan_at(5, 9));
    let mut run = ok(LessonRun::start_lesson(
        &mut store,
        &lesson,
        "session-1",
        &at,
    ));
    let unchecked = run_ok(run.submit(
        &mut store,
        &forms,
        &settings,
        &submission("e1", text("  Yo quiero un cafe\u{301}.  ")),
        &at,
    ));
    assert_eq!(unchecked.verdict, Verdict::Unchecked);
    assert_eq!(unchecked.rating, None);
    assert!(
        ok(store.review_log()).is_empty(),
        "an unchecked answer reviews nothing"
    );

    let right = run_ok(run.i_was_right(&mut store, &settings, "e1", &at));
    assert_eq!(right.verdict, Verdict::Correct);
    assert_eq!(right.rating, Some(Rating::Good));
    let learner: Vec<String> = ok(store.accepted_answers(&e3, Direction::ToTarget))
        .into_iter()
        .filter(|a| matches!(a.source, AnswerSource::Learner))
        .map(|a| a.display)
        .collect();
    assert_eq!(learner, strings(&["Yo quiero un café."]));
    let key = judgement_key(
        Lang::Es,
        Direction::ToTarget,
        "I want a coffee.",
        "yo quiero un café",
    );
    assert!(ok(store.judgement(&key)).is_none(), "no judgement row");
    let log = ok(store.review_log());
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].item_id, e3);
    assert_eq!(log[0].facet, Facet::Productive);
    assert_eq!(log[0].rating, Rating::Good);
    assert_eq!(log[0].session_id, "session-1");
    run_ok(run.finish(&mut store, &settings, &at));

    let later = clock(jan_at(6, 9));
    let mut next = ok(LessonRun::start_lesson(
        &mut store,
        &lesson,
        "session-2",
        &later,
    ));
    let again = run_ok(next.submit(
        &mut store,
        &forms,
        &settings,
        &submission("e1", text("Yo quiero un café.")),
        &later,
    ));
    assert_eq!(again.verdict, Verdict::Correct);
    assert_eq!(again.corrected, None);
}

// Bullet 9: with a prompt that is not the item's own, "I was right" stores a judgements row
// (verdict correct, the answer as corrected text, no explanation or tags, provider learner) and
// no accepted answer; the same answer is then correct.
#[test]
fn t09_i_was_right_on_another_prompt_stores_a_learner_judgement() {
    let forms = course_forms();
    let settings = RunSettings::default();
    let mut store = course_store();
    let e2 = id(CAFE, "e:2");
    let lesson = lesson_of(vec![to_target(
        "e1",
        &e2,
        "A white coffee for me.",
        &["Quiero un café con leche."],
    )]);
    let at = clock(jan_at(5, 9));
    let mut run = ok(LessonRun::start_lesson(
        &mut store,
        &lesson,
        "session-1",
        &at,
    ));
    let answer = "Un café con leche para mí.";
    let first = run_ok(run.submit(
        &mut store,
        &forms,
        &settings,
        &submission("e1", text(answer)),
        &at,
    ));
    assert_eq!(first.verdict, Verdict::Unchecked);
    let right = run_ok(run.i_was_right(&mut store, &settings, "e1", &at));
    assert_eq!(right.verdict, Verdict::Correct);
    assert_eq!(right.rating, Some(Rating::Good));

    let key = judgement_key(
        Lang::Es,
        Direction::ToTarget,
        "A white coffee for me.",
        "un café con leche para mí",
    );
    let judgement = ok(store.judgement(&key)).expect("a learner judgement");
    assert_eq!(judgement.verdict, srs::Verdict::Correct);
    assert_eq!(judgement.corrected, answer);
    assert_eq!(judgement.explanation, "");
    assert!(judgement.tags.is_empty());
    assert_eq!(judgement.provider, "learner");
    assert!(
        ok(store.accepted_answers(&e2, Direction::ToTarget))
            .iter()
            .all(|a| !matches!(a.source, AnswerSource::Learner)),
        "no learner answer for a prompt that is not the item's own"
    );
    run_ok(run.finish(&mut store, &settings, &at));

    let later = clock(jan_at(6, 9));
    let mut next = ok(LessonRun::start_lesson(
        &mut store,
        &lesson,
        "session-2",
        &later,
    ));
    let again = run_ok(next.submit(
        &mut store,
        &forms,
        &settings,
        &submission("e1", text(answer)),
        &later,
    ));
    assert_eq!(again.verdict, Verdict::Correct);
}

// Bullet 9: a judgements row with verdict almost for an answer's key grades that answer almost
// with its corrected text; accept_learner_answer on its own behaves as the runner's
// "I was right".
#[test]
fn t09_a_cached_almost_judgement_and_accept_learner_answer() {
    let forms = course_forms();
    let settings = RunSettings::default();
    let mut store = course_store();
    let e3 = id(CAFE, "e:3");
    let key = judgement_key(
        Lang::Es,
        Direction::ToEn,
        "Quiero un café.",
        "i am wanting a coffee",
    );
    ok(store.put_judgement(
        &key,
        &StoredJudgement {
            verdict: srs::Verdict::Almost,
            corrected: "I want a coffee.".to_string(),
            explanation: "Want is rarely used in the continuous form.".to_string(),
            tags: strings(&["grammar"]),
            provider: "fixture".to_string(),
        },
        1,
    ));
    let lesson = lesson_of(vec![to_en(
        "e1",
        &e3,
        "Quiero un café.",
        &["I want a coffee.", "I would like a coffee."],
    )]);
    let at = clock(jan_at(5, 9));
    let mut run = ok(LessonRun::start_lesson(
        &mut store,
        &lesson,
        "session-1",
        &at,
    ));
    let g = run_ok(run.submit(
        &mut store,
        &forms,
        &settings,
        &submission("e1", text("I am wanting a coffee.")),
        &at,
    ));
    assert_grade(
        &g,
        Verdict::Almost,
        &[],
        None,
        Some("I want a coffee."),
        &[],
        Some(Rating::Hard),
    );

    // accept_learner_answer directly: the item's own prompt in the to_en direction.
    let own = to_en("e2", &e3, "Quiero un café.", &["I want a coffee."]);
    let accepted = ok(accept_learner_answer(
        &mut store,
        &own,
        &text("I'd like a coffee"),
        jan_at(5, 10),
    ));
    assert_eq!(accepted.verdict, Verdict::Correct);
    assert_eq!(accepted.rating, Some(Rating::Good));
    assert!(
        ok(store.accepted_answers(&e3, Direction::ToEn))
            .iter()
            .any(|a| matches!(a.source, AnswerSource::Learner) && a.display == "I'd like a coffee")
    );
    let ctx = ok(grade_ctx(
        &store,
        &forms,
        AccentLeniency::Lenient,
        &own,
        &text("i'd like a coffee"),
    ));
    assert_eq!(
        grade(&ctx, &own, &text("i'd like a coffee")).verdict,
        Verdict::Correct
    );
}

// ---------------------------------------------------------------------------------------------
// Store additions
// ---------------------------------------------------------------------------------------------

// Interfaces (store additions): items read back as ItemRow; an unknown id is UnknownItem.
#[test]
fn store_item_rows() {
    let store = course_store();
    let agua = ok(store.item(&id(CAFE, "l:agua")));
    assert_eq!(agua.id, id(CAFE, "l:agua"));
    assert_eq!(agua.lang, Lang::Es);
    assert_eq!(agua.skill_id, CAFE);
    assert_eq!(agua.kind, ItemKind::Vocab);
    assert_eq!(agua.target, "el agua");
    assert_eq!(agua.en, strings(&["water"]));
    assert_eq!(agua.lemmas, strings(&["agua"]));
    assert!(!agua.retired);
    let dog = ok(store.item(&id(CAFE, "e:4")));
    assert_eq!(dog.kind, ItemKind::Phrase);
    assert_eq!(dog.target, "El perro quiere agua.");
    assert_eq!(dog.lemmas, strings(&["perro", "querer", "agua"]));
    match store.item("es.a1.u09.s09#l:nada") {
        Ok(_) => panic!("an unknown item is refused"),
        Err(e) => {
            assert_eq!(
                e,
                StoreError::UnknownItem("es.a1.u09.s09#l:nada".to_string())
            );
            assert_eq!(e.to_string(), "unknown item es.a1.u09.s09#l:nada");
        }
    }
}

// Interfaces (store additions): accepted answers are ordered content, judge, learner, then by
// created_at, then by norm (module 02's NFC lower case key); a display whose norm exists is not
// added; start_course keeps judge and learner rows.
#[test]
fn store_accepted_answers_order_and_uniqueness() {
    let mut store = course_store();
    let e3 = id(CAFE, "e:3");
    let content = ok(store.accepted_answers(&e3, Direction::ToTarget));
    assert_eq!(content.len(), 1);
    assert_eq!(content[0].display, "Quiero un café.");
    assert_eq!(content[0].norm, "quiero un café.");
    assert_eq!(source_name(&content[0].source), "content");
    let en: Vec<String> = ok(store.accepted_answers(&e3, Direction::ToEn))
        .into_iter()
        .map(|a| a.display)
        .collect();
    assert_eq!(en, strings(&["I want a coffee.", "I would like a coffee."]));

    assert!(ok(store.add_accepted_answer(
        &e3,
        Direction::ToTarget,
        "Yo quiero un café.",
        AnswerSource::Learner,
        20
    )));
    assert!(
        !ok(store.add_accepted_answer(
            &e3,
            Direction::ToTarget,
            "YO QUIERO UN CAFÉ.",
            AnswerSource::Judge,
            30
        )),
        "the same NFC lower case form exists"
    );
    assert!(ok(store.add_accepted_answer(
        &e3,
        Direction::ToTarget,
        "Quiero café.",
        AnswerSource::Judge,
        30
    )));
    assert!(ok(store.add_accepted_answer(
        &e3,
        Direction::ToTarget,
        "B, quiero un café.",
        AnswerSource::Learner,
        10
    )));
    assert!(ok(store.add_accepted_answer(
        &e3,
        Direction::ToTarget,
        "A, quiero un café.",
        AnswerSource::Learner,
        10
    )));
    let expected = [
        ("Quiero un café.", "content"),
        ("Quiero café.", "judge"),
        ("A, quiero un café.", "learner"),
        ("B, quiero un café.", "learner"),
        ("Yo quiero un café.", "learner"),
    ];
    let listed = |store: &Store| -> Vec<(String, &'static str)> {
        ok(store.accepted_answers(&e3, Direction::ToTarget))
            .iter()
            .map(|a| (a.display.clone(), source_name(&a.source)))
            .collect()
    };
    let expected: Vec<(String, &'static str)> = expected
        .iter()
        .map(|(display, source)| (display.to_string(), *source))
        .collect();
    assert_eq!(listed(&store), expected);
    assert_eq!(ok(store.accepted_answers(&e3, Direction::ToEn)).len(), 2);
    ok(store.start_course(&course(), &course_forms(), 50));
    assert_eq!(
        listed(&store),
        expected,
        "start_course keeps judge and learner rows"
    );
}

// Interfaces (store additions): judgements, the next lesson seq, and the empty daily and
// freeze tables of a new course.
#[test]
fn store_judgements_lesson_seq_daily_and_freezes() {
    let mut store = course_store();
    let key = judgement_key(Lang::Es, Direction::ToEn, "Hola.", "hello");
    assert!(ok(store.judgement(&key)).is_none());
    ok(store.put_judgement(
        &key,
        &StoredJudgement {
            verdict: srs::Verdict::Wrong,
            corrected: "Hello.".to_string(),
            explanation: "Hola is hello.".to_string(),
            tags: strings(&["typo"]),
            provider: "fixture".to_string(),
        },
        42,
    ));
    let back = ok(store.judgement(&key)).expect("stored");
    assert_eq!(back.verdict, srs::Verdict::Wrong);
    assert_eq!(back.corrected, "Hello.");
    assert_eq!(back.explanation, "Hola is hello.");
    assert_eq!(back.tags, strings(&["typo"]));
    assert_eq!(back.provider, "fixture");

    assert!(ok(store.daily()).is_empty());
    assert!(ok(store.freeze_events()).is_empty());
    assert_eq!(ok(store.next_lesson_seq(CAFE)), 1);
    let at = clock(jan_at(5, 9));
    let first = lesson_of(vec![agua_pick("e1")]);
    let _ = ok(LessonRun::start_lesson(&mut store, &first, "a", &at));
    assert_eq!(ok(store.next_lesson_seq(CAFE)), 2);
    let third = Lesson {
        seq: 3,
        ..lesson_of(vec![agua_pick("e1")])
    };
    let _ = ok(LessonRun::start_lesson(&mut store, &third, "b", &at));
    assert_eq!(
        ok(store.next_lesson_seq(CAFE)),
        4,
        "1 + the highest stored seq"
    );
    assert_eq!(ok(store.next_lesson_seq(GREETINGS)), 1);
}

// ---------------------------------------------------------------------------------------------
// Exercise types
// ---------------------------------------------------------------------------------------------

// Interfaces: each type's kind, direction, main texts and facet as the spec lists them.
#[test]
fn exercise_kind_direction_and_main_texts() {
    let item = id(CAFE, "e:3");
    let cases = [
        (
            pick(
                "a",
                &item,
                "el agua",
                &["water", "milk", "dog", "coffee"],
                0,
            ),
            "pick_meaning",
            None,
            &["el agua"][..],
        ),
        (
            to_target(
                "b",
                &item,
                "I want a coffee.",
                &["Quiero un café.", "Quiero café."],
            ),
            "translate_to_target",
            Some("to_target"),
            &["Quiero un café."][..],
        ),
        (
            to_en("c", &item, "Quiero un café.", &["I want a coffee."]),
            "translate_to_en",
            Some("to_en"),
            &["Quiero un café."][..],
        ),
        (
            listen("d", &item, "Quiero un café.", &["Quiero un café."]),
            "listen_type",
            Some("to_target"),
            &["Quiero un café."][..],
        ),
        (
            speak("e", &item, "Quiero un café.", "I want a coffee."),
            "speak",
            Some("to_target"),
            &["Quiero un café."][..],
        ),
        (
            fill_gap("f", &item, "Quiero un ___.", "café", "Quiero un café."),
            "fill_gap",
            Some("to_target"),
            &["Quiero un café."][..],
        ),
        (
            pairs(
                "g",
                &item,
                &[
                    ("el café", "coffee"),
                    ("la leche", "milk"),
                    ("el agua", "water"),
                    ("el perro", "dog"),
                ],
            ),
            "match_pairs",
            None,
            &["el café", "la leche", "el agua", "el perro"][..],
        ),
    ];
    for (ex, kind, direction, targets) in &cases {
        assert_eq!(ex.kind().as_str(), *kind);
        assert_eq!(ex.direction().map(|d| d.as_str()), *direction, "{kind}");
        assert_eq!(ex.targets(), *targets, "{kind}");
    }
}

// Interfaces: serde shapes of exercises, answers and the enumerations.
#[test]
fn exercises_answers_and_enumerations_serialise_as_specified() {
    let ex = to_target(
        "e1",
        &id(CAFE, "e:3"),
        "I want a coffee.",
        &["Quiero un café."],
    );
    let value = serde_json::to_value(&ex).unwrap();
    assert_eq!(
        value,
        json!({"id": "e1", "item_id": "es.a1.u01.s02#e:3", "facet": "productive",
               "origin": "template", "type": "translate_to_target",
               "prompt_en": "I want a coffee.", "target": "Quiero un café.",
               "accepted": ["Quiero un café."]}),
        "tiles is left out when there are none"
    );
    let with_tiles = exercise(
        "e2",
        &id(CAFE, "e:3"),
        Facet::Productive,
        Body::TranslateToTarget {
            prompt_en: "I want a coffee.".to_string(),
            target: "Quiero un café.".to_string(),
            accepted: strings(&["Quiero un café."]),
            tiles: Some(strings(&["café", "un", "quiero"])),
        },
    );
    let value = serde_json::to_value(&with_tiles).unwrap();
    assert_eq!(value["tiles"], json!(["café", "un", "quiero"]));
    let back: Exercise = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&back).unwrap(), value);
    let mp = pairs(
        "e3",
        "x",
        &[
            ("el café", "coffee"),
            ("la leche", "milk"),
            ("el agua", "water"),
            ("el perro", "dog"),
        ],
    );
    assert_eq!(
        serde_json::to_value(&mp).unwrap()["pairs"][0],
        json!(["el café", "coffee"])
    );

    assert_eq!(
        serde_json::to_value(text("hola")).unwrap(),
        json!({"text": "hola"})
    );
    assert_eq!(
        serde_json::to_value(Answer::Choice(2)).unwrap(),
        json!({"choice": 2})
    );
    assert_eq!(
        serde_json::to_value(Answer::Pairs(vec![[0, 1], [1, 0]])).unwrap(),
        json!({"pairs": [[0, 1], [1, 0]]})
    );
    let parsed: Answer = serde_json::from_value(json!({"pairs": [[0, 0]]})).unwrap();
    assert!(matches!(parsed, Answer::Pairs(p) if p == [[0, 0]]));

    for (verdict, name) in [
        (Verdict::Correct, "correct"),
        (Verdict::Almost, "almost"),
        (Verdict::Typo, "typo"),
        (Verdict::Wrong, "wrong"),
        (Verdict::Unchecked, "unchecked"),
    ] {
        assert_eq!(serde_json::to_value(verdict).unwrap(), json!(name));
    }
    assert_eq!(Verdict::Correct.srs(), Some(srs::Verdict::Correct));
    assert_eq!(Verdict::Almost.srs(), Some(srs::Verdict::Almost));
    assert_eq!(Verdict::Typo.srs(), Some(srs::Verdict::Almost));
    assert_eq!(Verdict::Wrong.srs(), Some(srs::Verdict::Wrong));
    assert_eq!(Verdict::Unchecked.srs(), None);
    for (tag, name) in [
        (Tag::Accent, "accent"),
        (Tag::Article, "article"),
        (Tag::Typo, "typo"),
    ] {
        assert_eq!(serde_json::to_value(tag).unwrap(), json!(name));
    }
    assert_eq!(
        serde_json::to_value(Direction::ToTarget).unwrap(),
        json!("to_target")
    );
    assert_eq!(
        serde_json::to_value(Direction::ToEn).unwrap(),
        json!("to_en")
    );
    for (leniency, name) in [
        (AccentLeniency::Lenient, "lenient"),
        (AccentLeniency::Strict, "strict"),
        (AccentLeniency::Ignore, "ignore"),
    ] {
        assert_eq!(serde_json::to_value(leniency).unwrap(), json!(name));
    }
    for (kind, name) in [
        (ExerciseType::PickMeaning, "pick_meaning"),
        (ExerciseType::TranslateToTarget, "translate_to_target"),
        (ExerciseType::TranslateToEn, "translate_to_en"),
        (ExerciseType::ListenType, "listen_type"),
        (ExerciseType::Speak, "speak"),
        (ExerciseType::FillGap, "fill_gap"),
        (ExerciseType::MatchPairs, "match_pairs"),
    ] {
        assert_eq!(kind.as_str(), name);
        assert_eq!(serde_json::to_value(kind).unwrap(), json!(name));
    }
    for (origin, name) in [
        (Origin::Template, "template"),
        (Origin::Ai, "ai"),
        (Origin::CoursePack, "course_pack"),
    ] {
        assert_eq!(serde_json::to_value(origin).unwrap(), json!(name));
    }
    assert_eq!(
        serde_json::to_value(SessionKind::Lesson).unwrap(),
        json!("lesson")
    );
    assert_eq!(
        serde_json::to_value(SessionKind::Review).unwrap(),
        json!("review")
    );
    assert_eq!(
        serde_json::to_value(FreezeKind::Earned).unwrap(),
        json!("earned")
    );
    assert_eq!(
        serde_json::to_value(FreezeKind::Used).unwrap(),
        json!("used")
    );
    for (source, name) in [
        (AnswerSource::Content, "content"),
        (AnswerSource::Judge, "judge"),
        (AnswerSource::Learner, "learner"),
    ] {
        assert_eq!(serde_json::to_value(source).unwrap(), json!(name));
    }
    let lesson = serde_json::to_value(lesson_of(vec![agua_pick("e1")])).unwrap();
    assert_eq!(lesson["schema"], json!("parley.lesson/1"));
    assert_eq!(lesson["lang"], json!("es"));
    assert_eq!(lesson["origin"], json!("template"));
    assert_eq!(lesson["exercises"][0]["type"], json!("pick_meaning"));
}

// ---------------------------------------------------------------------------------------------
// Structural checks L5 to L9
// ---------------------------------------------------------------------------------------------

fn token<'a>(tokens: &'a [Token], norm: &str) -> &'a Token {
    tokens
        .iter()
        .find(|t| t.norm == norm)
        .unwrap_or_else(|| panic!("no token {norm}"))
}

// Design notes (coverage): Allowed follows module 01's V12 rule over the skills up to the given
// one; numbers, number words and names are excluded from the count.
#[test]
fn allowed_and_coverage_follow_the_taught_forms() {
    let syllabus = course();
    let forms = course_forms();
    let greetings = Allowed::for_skill(&syllabus, &[], GREETINGS);
    let cafe = Allowed::for_skill(&syllabus, &[], CAFE);
    let tokens = tokenise(
        Lang::Es,
        "Hola, Ana: quiero un café, por favor, 3 gatos.",
        &forms,
    );
    for word in ["hola", "un"] {
        assert!(greetings.allows(token(&tokens, word)), "{word}");
        assert!(cafe.allows(token(&tokens, word)), "{word}");
    }
    for word in ["quiero", "café", "favor"] {
        assert!(!greetings.allows(token(&tokens, word)), "{word}");
        assert!(cafe.allows(token(&tokens, word)), "{word}");
    }
    assert!(!cafe.allows(token(&tokens, "gatos")));
    assert!(!cafe.is_excluded(token(&tokens, "gatos")));
    assert!(cafe.is_excluded(token(&tokens, "ana")), "a name");
    assert!(cafe.is_excluded(token(&tokens, "3")), "a number");

    assert_eq!(
        coverage(Lang::Es, "Quiero un café con leche.", &cafe, &forms),
        (5, 5)
    );
    assert_eq!(
        coverage(Lang::Es, "Quiero un café con leche.", &greetings, &forms),
        (2, 5)
    );
    assert_eq!(
        coverage(Lang::Es, "Hola, Ana, 3 gatos.", &greetings, &forms),
        (1, 2)
    );
    assert_eq!(coverage(Lang::Es, "Ana, 3.", &cafe, &forms), (0, 0));
    let with_numbers = Allowed::for_skill(&syllabus, &strings(&["tres"]), CAFE);
    assert_eq!(
        coverage(Lang::Es, "Quiero tres café", &with_numbers, &forms),
        (2, 2),
        "a number word is not counted"
    );
    assert_eq!(max_words(Cefr::A1), 8);
    assert_eq!(max_words(Cefr::A2), 12);
}

/// The checks of `ex` in the café skill with `max` words, the given focus and seen list.
fn check_in_cafe(
    ex: &Exercise,
    max: usize,
    focus: &[Lemma],
    seen: &[(ExerciseType, String)],
) -> Result<(), CheckFailure> {
    let syllabus = course();
    let forms = course_forms();
    let allowed = Allowed::for_skill(&syllabus, &[], CAFE);
    let ctx = CheckCtx {
        lang: Lang::Es,
        forms: &forms,
        allowed: &allowed,
        max_words: max,
        focus,
        seen,
    };
    check_exercise(ex, &ctx)
}

#[track_caller]
fn assert_rule(result: Result<(), CheckFailure>, rule: &str) {
    match result {
        Ok(()) => panic!("expected {rule}"),
        Err(failure) => assert_eq!(failure.rule(), rule),
    }
}

fn lemma_of(syllabus: &Syllabus, name: &str) -> Lemma {
    syllabus
        .skills()
        .flat_map(|(_, skill)| skill.lemmas.iter())
        .find(|lemma| lemma.lemma == name)
        .cloned()
        .unwrap_or_else(|| panic!("no lemma {name}"))
}

// Design notes, L5: at least 90% of the counted tokens covered; nothing counted passes; L5 is
// checked first.
#[test]
fn l5_coverage() {
    let x = id(CAFE, "l:agua");
    let options = ["water", "milk", "dog", "coffee"];
    let unknown = pick("e1", &x, "el gato negro", &options, 0);
    match check_in_cafe(&unknown, 8, &[], &[]) {
        Err(CheckFailure::Coverage { covered, counted }) => assert_eq!((covered, counted), (1, 3)),
        Err(other) => panic!("expected L5, got {}", other.rule()),
        Ok(()) => panic!("expected L5"),
    }
    // Nine of ten words covered is 90%: L5 passes, and only the length (L8) is left to fail.
    let nine = pick(
        "e2",
        &x,
        "hola hola hola hola hola hola hola hola hola gato",
        &options,
        0,
    );
    assert!(check_in_cafe(&nine, 12, &[], &[]).is_ok());
    assert_rule(check_in_cafe(&nine, 8, &[], &[]), "L8");
    let eight = pick(
        "e3",
        &x,
        "hola hola hola hola hola hola hola hola gato gato",
        &options,
        0,
    );
    assert_rule(check_in_cafe(&eight, 8, &[], &[]), "L5");
    let names_only = pick("e4", &x, "Ana, Luis, 3", &options, 0);
    assert!(check_in_cafe(&names_only, 8, &[], &[]).is_ok());
}

// Design notes, L6: each type's answers.
#[test]
fn l6_answers() {
    let x = id(CAFE, "e:3");
    let ok_cases = [
        to_target(
            "a",
            &x,
            "I want a coffee.",
            &["Quiero un café.", "Quiero café."],
        ),
        to_en("b", &x, "Quiero un café.", &["I want a coffee."]),
        listen("c", &x, "Quiero un café.", &["quiero un cafe\u{301}"]),
        fill_gap(
            "d",
            &x,
            "Quiero un café con ___.",
            "leche",
            "Quiero un café con leche.",
        ),
        pick("e", &x, "el agua", &["water", "milk", "dog", "coffee"], 3),
        pairs(
            "f",
            &x,
            &[
                ("el café", "coffee"),
                ("la leche", "milk"),
                ("el agua", "water"),
                ("el perro", "dog"),
                ("querer", "to want"),
            ],
        ),
    ];
    for ex in &ok_cases {
        assert!(check_in_cafe(ex, 8, &[], &[]).is_ok(), "{}", ex.id);
    }
    let four = [
        ("el café", "coffee"),
        ("la leche", "milk"),
        ("el agua", "water"),
        ("el perro", "dog"),
    ];
    let failing = [
        exercise(
            "a",
            &x,
            Facet::Productive,
            Body::TranslateToTarget {
                prompt_en: "I want a coffee.".to_string(),
                target: "Quiero un café.".to_string(),
                accepted: strings(&["Quiero café."]),
                tiles: None,
            },
        ),
        exercise(
            "b",
            &x,
            Facet::Productive,
            Body::TranslateToTarget {
                prompt_en: "I want a coffee.".to_string(),
                target: "Quiero un café.".to_string(),
                accepted: Vec::new(),
                tiles: None,
            },
        ),
        to_en("c", &x, "Quiero un café.", &[]),
        listen("d", &x, "Quiero un café.", &["Quiero café."]),
        fill_gap(
            "e",
            &x,
            "Quiero un ___ con ___.",
            "café",
            "Quiero un café con leche.",
        ),
        fill_gap(
            "f",
            &x,
            "Quiero un café con leche.",
            "leche",
            "Quiero un café con leche.",
        ),
        fill_gap(
            "g",
            &x,
            "Quiero un ___.",
            "café con leche",
            "Quiero un café con leche.",
        ),
        fill_gap(
            "h",
            &x,
            "Quiero un café con ___.",
            "agua",
            "Quiero un café con leche.",
        ),
        pick("i", &x, "el agua", &["water", "milk", "dog"], 0),
        pick("j", &x, "el agua", &["water", "Water.", "milk", "dog"], 0),
        pick("k", &x, "el agua", &["water", "milk", "dog", "coffee"], 4),
        pairs("l", &x, &four[..3]),
        pairs(
            "m",
            &x,
            &[
                ("el café", "coffee"),
                ("la leche", "milk"),
                ("el agua", "water"),
                ("el perro", "dog"),
                ("querer", "to want"),
                ("hola", "hello"),
            ],
        ),
        pairs(
            "n",
            &x,
            &[
                ("el café", "coffee"),
                ("El café", "milk"),
                ("el agua", "water"),
                ("el perro", "dog"),
            ],
        ),
        pairs(
            "o",
            &x,
            &[
                ("el café", "coffee"),
                ("la leche", "Coffee"),
                ("el agua", "water"),
                ("el perro", "dog"),
            ],
        ),
    ];
    for ex in &failing {
        match check_in_cafe(ex, 8, &[], &[]) {
            Err(CheckFailure::Answers(_)) => {}
            Err(other) => panic!("{}: expected L6, got {}", ex.id, other.rule()),
            Ok(()) => panic!("{}: expected L6", ex.id),
        }
    }
}

// Design notes, L7 to L9: no earlier exercise of the same type with the same first main text
// (match_pairs exempt); at most max_words words per main text; a token of a main text matches
// the focus. Each failure names its rule.
#[test]
fn l7_l8_l9_duplicates_length_and_focus() {
    let syllabus = course();
    let x = id(CAFE, "e:3");
    let ex = to_en("e1", &x, "Quiero un café.", &["I want a coffee."]);
    let seen_en = [(ExerciseType::TranslateToEn, "quiero un café".to_string())];
    assert!(matches!(
        check_in_cafe(&ex, 8, &[], &seen_en),
        Err(CheckFailure::Duplicate)
    ));
    assert_rule(check_in_cafe(&ex, 8, &[], &seen_en), "L7");
    let seen_other = [(
        ExerciseType::TranslateToTarget,
        "quiero un café".to_string(),
    )];
    assert!(check_in_cafe(&ex, 8, &[], &seen_other).is_ok());
    let mp = pairs(
        "e2",
        &x,
        &[
            ("el café", "coffee"),
            ("la leche", "milk"),
            ("el agua", "water"),
            ("el perro", "dog"),
        ],
    );
    let seen_pairs = [(ExerciseType::MatchPairs, "el café".to_string())];
    assert!(
        check_in_cafe(&mp, 8, &[], &seen_pairs).is_ok(),
        "match_pairs is exempt"
    );

    let long = to_en(
        "e3",
        &id(CAFE, "e:1"),
        "Hola, quiero un café y un agua, por favor.",
        &["Hello, I want a coffee and a water, please."],
    );
    match check_in_cafe(&long, 8, &[], &[]) {
        Err(CheckFailure::TooLong { words, max }) => assert_eq!((words, max), (9, 8)),
        Err(other) => panic!("expected L8, got {}", other.rule()),
        Ok(()) => panic!("expected L8"),
    }
    assert!(check_in_cafe(&long, 12, &[], &[]).is_ok());

    let leche = [lemma_of(&syllabus, "leche")];
    assert!(matches!(
        check_in_cafe(&ex, 8, &leche, &[]),
        Err(CheckFailure::Focus)
    ));
    assert_rule(check_in_cafe(&ex, 8, &leche, &[]), "L9");
    assert!(check_in_cafe(&ex, 8, &[lemma_of(&syllabus, "café")], &[]).is_ok());
    assert!(
        check_in_cafe(&ex, 8, &[lemma_of(&syllabus, "querer")], &[]).is_ok(),
        "quiero is a form of querer"
    );
    let rules = [
        CheckFailure::Coverage {
            covered: 1,
            counted: 2,
        },
        CheckFailure::Answers("x".to_string()),
        CheckFailure::Duplicate,
        CheckFailure::TooLong { words: 9, max: 8 },
        CheckFailure::Focus,
    ];
    let names: Vec<&str> = rules.iter().map(CheckFailure::rule).collect();
    assert_eq!(names, ["L5", "L6", "L7", "L8", "L9"]);
}

// ---------------------------------------------------------------------------------------------
// Templates (11, 12) and the word bank (14)
// ---------------------------------------------------------------------------------------------

fn cafe_ctx<'a>(
    syllabus: &'a Syllabus,
    forms: &'a Forms,
    items: &'a [Item],
    allowed: &'a Allowed,
    sentences: &'a [Example],
    plan_items: &'a [String],
) -> TemplateCtx<'a> {
    TemplateCtx {
        syllabus,
        forms,
        items,
        allowed,
        sentences,
        skill_id: CAFE,
        plan_items,
    }
}

// Bullet 11: the distractors of `el agua` never equal (in normal form) one of its accepted
// glosses or each other. The test course gains two vocabulary items whose first glosses are
// `Water` and a second `coffee`.
#[test]
fn t11_pick_meaning_never_offers_an_accepted_gloss_as_a_distractor() {
    let mut syllabus = course();
    let forms = course_forms();
    let cafe = syllabus.units[0]
        .skills
        .iter_mut()
        .find(|skill| skill.id == CAFE)
        .expect("the café skill");
    for (lemma, gender, en) in [
        ("agüita", Gender::F, ["Water", "a little water"]),
        ("cafecito", Gender::M, ["coffee", "a small coffee"]),
    ] {
        cafe.lemmas.push(Lemma {
            lemma: lemma.to_string(),
            pos: Pos::Noun,
            gender: Some(gender),
            en: strings(&en),
            rank: None,
            separable: None,
            article: None,
        });
    }
    let items = derive_items(&syllabus, &forms);
    let allowed = Allowed::for_skill(&syllabus, &[], CAFE);
    let ctx = cafe_ctx(&syllabus, &forms, &items, &allowed, &[], &[]);
    let agua = find_item(&items, &id(CAFE, "l:agua"));
    let same_skill: BTreeSet<&str> = ["coffee", "milk", "dog", "to want", "please"]
        .into_iter()
        .collect();
    for seed in 0..200 {
        let ex = template(
            agua,
            ExerciseType::PickMeaning,
            &ctx,
            &mut SplitMix64::new(seed),
        )
        .expect("el agua has enough distractors");
        assert_eq!(ex.item_id, agua.id);
        assert_eq!(ex.facet, Facet::Receptive);
        assert!(matches!(ex.origin, Origin::Template));
        let Body::PickMeaning {
            prompt_target,
            options,
            answer_index,
        } = &ex.body
        else {
            panic!("seed {seed}: a pick_meaning body");
        };
        assert_eq!(prompt_target, "el agua");
        assert_eq!(options.len(), 4, "seed {seed}");
        let answer = *answer_index as usize;
        assert_eq!(options[answer], "water", "seed {seed}");
        let norms: BTreeSet<String> = options.iter().map(|o| normalise(Lang::Es, o)).collect();
        assert_eq!(norms.len(), 4, "seed {seed}: distinct options {options:?}");
        for (i, option) in options.iter().enumerate() {
            if i != answer {
                assert_ne!(normalise(Lang::Es, option), "water", "seed {seed}");
                assert!(
                    same_skill.contains(option.as_str()),
                    "seed {seed}: {option} is a first gloss of the same skill"
                );
            }
        }
    }
}

// Bullet 12: fill_gap for `leche` blanks it in the one example that holds it; a pack sentence
// joins the choice, and its first form of the lemma is the gap.
#[test]
fn t12_fill_gap_for_leche() {
    let syllabus = course();
    let forms = course_forms();
    let items = derive_items(&syllabus, &forms);
    let allowed = Allowed::for_skill(&syllabus, &[], CAFE);
    let leche = find_item(&items, &id(CAFE, "l:leche"));
    let ctx = cafe_ctx(&syllabus, &forms, &items, &allowed, &[], &[]);
    for seed in 0..20 {
        let ex = template(
            leche,
            ExerciseType::FillGap,
            &ctx,
            &mut SplitMix64::new(seed),
        )
        .expect("a fill_gap for leche");
        assert_eq!(ex.facet, Facet::Productive);
        let Body::FillGap {
            sentence,
            answer,
            full_sentence,
            hint_en,
        } = &ex.body
        else {
            panic!("a fill_gap body");
        };
        assert_eq!(sentence, "Quiero un café con ___.");
        assert_eq!(answer, "leche");
        assert_eq!(full_sentence, "Quiero un café con leche.");
        assert_eq!(hint_en, "milk");
    }

    let pack = [
        Example {
            id: 1,
            target: "Leche con leche.".to_string(),
            en: strings(&["Milk with milk."]),
        },
        Example {
            id: 2,
            target: "Leche con chocolate.".to_string(),
            en: strings(&["Milk with chocolate."]),
        },
    ];
    let ctx = cafe_ctx(&syllabus, &forms, &items, &allowed, &pack, &[]);
    let mut seen = BTreeSet::new();
    for seed in 0..60 {
        let ex = template(
            leche,
            ExerciseType::FillGap,
            &ctx,
            &mut SplitMix64::new(seed),
        )
        .expect("a fill_gap for leche");
        let Body::FillGap {
            sentence,
            answer,
            full_sentence,
            ..
        } = &ex.body
        else {
            panic!("a fill_gap body");
        };
        match full_sentence.as_str() {
            "Quiero un café con leche." => assert_eq!(sentence, "Quiero un café con ___."),
            "Leche con leche." => {
                assert_eq!(sentence, "___ con leche.");
                assert_eq!(answer, "Leche");
            }
            other => panic!("seed {seed}: {other} has a word no skill teaches or no leche"),
        }
        seen.insert(full_sentence.clone());
    }
    assert_eq!(seen.len(), 2, "both allowed sentences are chosen at random");
}

/// The template exercise of `kind` for `item` from SplitMix64(seed).
fn templ(item: &Item, kind: ExerciseType, ctx: &TemplateCtx<'_>, seed: u64) -> Option<Exercise> {
    template(item, kind, ctx, &mut SplitMix64::new(seed))
}

// Design notes (template rules): every type from the item, deterministic by seed; fill_gap only
// for single-word vocabulary; match_pairs from the plan's next vocabulary items; pick_meaning
// needs three distractors.
#[test]
fn templates_build_each_type_from_the_item() {
    let syllabus = course();
    let forms = course_forms();
    let items = derive_items(&syllabus, &forms);
    let allowed = Allowed::for_skill(&syllabus, &[], CAFE);
    let ctx = cafe_ctx(&syllabus, &forms, &items, &allowed, &[], &[]);
    let e3 = find_item(&items, &id(CAFE, "e:3"));

    let ex = templ(e3, ExerciseType::TranslateToTarget, &ctx, 1).expect("translate_to_target");
    assert_eq!(ex.facet, Facet::Productive);
    let Body::TranslateToTarget {
        prompt_en,
        target,
        accepted,
        tiles,
    } = &ex.body
    else {
        panic!("a translate_to_target body");
    };
    assert_eq!(prompt_en, "I want a coffee.");
    assert_eq!(target, "Quiero un café.");
    assert_eq!(accepted, &strings(&["Quiero un café."]));
    let tiles = tiles.as_ref().expect("word-bank tiles");
    assert_eq!(tiles.len(), 6);
    for word in ["quiero", "un", "café"] {
        assert!(tiles.iter().any(|t| t == word), "{word} in {tiles:?}");
    }

    let ex = templ(e3, ExerciseType::TranslateToEn, &ctx, 1).expect("translate_to_en");
    assert_eq!(ex.facet, Facet::Receptive);
    assert_eq!(
        serde_json::to_value(&ex.body).unwrap(),
        json!({"type": "translate_to_en", "prompt_target": "Quiero un café.",
               "accepted": ["I want a coffee.", "I would like a coffee."]})
    );
    let ex = templ(e3, ExerciseType::ListenType, &ctx, 1).expect("listen_type");
    assert_eq!(ex.facet, Facet::Receptive);
    assert_eq!(
        serde_json::to_value(&ex.body).unwrap(),
        json!({"type": "listen_type", "text": "Quiero un café.", "accepted": ["Quiero un café."]})
    );
    let ex = templ(e3, ExerciseType::Speak, &ctx, 1).expect("speak");
    assert_eq!(ex.facet, Facet::Productive);
    assert_eq!(
        serde_json::to_value(&ex.body).unwrap(),
        json!({"type": "speak", "text": "Quiero un café.", "prompt_en": "I want a coffee."})
    );
    assert!(
        templ(e3, ExerciseType::FillGap, &ctx, 1).is_none(),
        "a phrase"
    );
    let please = find_item(&items, &id(CAFE, "l:por favor"));
    assert!(
        templ(please, ExerciseType::FillGap, &ctx, 1).is_none(),
        "a lemma of two words"
    );

    let cafe_item = find_item(&items, &id(CAFE, "l:café"));
    let plan_items = vec![
        id(CAFE, "l:café"),
        id(CAFE, "e:3"),
        id(CAFE, "l:café"),
        id(CAFE, "l:leche"),
        id(CAFE, "l:agua"),
        id(CAFE, "l:perro"),
        id(CAFE, "l:querer"),
    ];
    let planned = cafe_ctx(&syllabus, &forms, &items, &allowed, &[], &plan_items);
    let ex = templ(cafe_item, ExerciseType::MatchPairs, &planned, 4).expect("match_pairs");
    let Body::MatchPairs { pairs } = &ex.body else {
        panic!("a match_pairs body");
    };
    let got: BTreeSet<(String, String)> = pairs
        .iter()
        .map(|[target, en]| (target.clone(), en.clone()))
        .collect();
    let want: BTreeSet<(String, String)> = [
        ("el café", "coffee"),
        ("la leche", "milk"),
        ("el agua", "water"),
        ("el perro", "dog"),
    ]
    .iter()
    .map(|(t, e)| (t.to_string(), e.to_string()))
    .collect();
    assert_eq!(pairs.len(), 4);
    assert_eq!(got, want);
    let short_plan = vec![id(CAFE, "l:café"), id(CAFE, "l:leche"), id(CAFE, "e:3")];
    let short = cafe_ctx(&syllabus, &forms, &items, &allowed, &[], &short_plan);
    assert!(templ(cafe_item, ExerciseType::MatchPairs, &short, 4).is_none());

    for kind in [
        ExerciseType::PickMeaning,
        ExerciseType::TranslateToTarget,
        ExerciseType::FillGap,
    ] {
        let a = templ(cafe_item, kind, &ctx, 9).map(|ex| serde_json::to_value(ex).unwrap());
        let b = templ(cafe_item, kind, &ctx, 9).map(|ex| serde_json::to_value(ex).unwrap());
        assert_eq!(
            a,
            b,
            "{}: the same seed gives the same exercise",
            kind.as_str()
        );
    }

    let three = parse_syllabus(
        "three.yaml",
        r#"schema: parley.syllabus/1
lang: es
name: Three words
variety: test
version: 1
function_words: [el, la]
names: []
units:
  - id: es.a1.u01
    title: Sky and sea
    cefr: A1
    skills:
      - id: es.a1.u01.s01
        kind: vocab
        title: Sky and sea
        cefr: A1
        can_do: I can name the sun, the moon and the sea.
        requires: []
        lemmas:
          - {lemma: sol, pos: noun, gender: m, en: [sun], rank: null}
          - {lemma: luna, pos: noun, gender: f, en: [moon], rank: null}
          - {lemma: mar, pos: noun, gender: m, en: [sea], rank: null}
        examples:
          - {id: 1, target: "El sol.", en: ["The sun."]}
"#,
    )
    .expect("the three-word syllabus parses");
    let none = Forms::default();
    let three_items = derive_items(&three, &none);
    let three_allowed = Allowed::for_skill(&three, &[], "es.a1.u01.s01");
    let three_ctx = TemplateCtx {
        syllabus: &three,
        forms: &none,
        items: &three_items,
        allowed: &three_allowed,
        sentences: &[],
        skill_id: "es.a1.u01.s01",
        plan_items: &[],
    };
    let sun = find_item(&three_items, "es.a1.u01.s01#l:sol");
    assert!(
        templ(sun, ExerciseType::PickMeaning, &three_ctx, 1).is_none(),
        "two distractors are not enough"
    );
}

/// The allowed single-word forms of the café skill, worked out from the fixture by hand: the
/// function words, the single-word lemmas of both skills, and the forms table's forms of those
/// lemmas, in lower case.
fn cafe_word_pool() -> BTreeSet<String> {
    [
        "el", "la", "los", "las", "un", "una", "y", "o", "pero", "de", "a", "en", "con", "sin",
        "por", "para", "no", "sí", "yo", "tú", "me", "te", "se", "que", "qué", "hola", "bueno",
        "día", "noche", "gracias", "café", "leche", "agua", "perro", "querer", "buenas", "buenos",
        "días", "noches", "quiere", "quieres", "quiero",
    ]
    .iter()
    .map(|w| w.to_string())
    .collect()
}

// Bullet 14: the word bank for `Quiero un café.` is its 3 tiles plus 3 distractors from the
// allowed single-word forms; the same seed gives the same order; the tiles in the right order
// grade correct.
#[test]
fn t14_word_bank_for_quiero_un_cafe() {
    let syllabus = course();
    let forms = course_forms();
    let allowed = Allowed::for_skill(&syllabus, &[], CAFE);
    let pool = cafe_word_pool();
    let target = "Quiero un café.";
    let target_tiles = ["quiero", "un", "café"];
    let mut orders = BTreeSet::new();
    for seed in 0..50 {
        let tiles = word_bank(
            target,
            Lang::Es,
            &allowed,
            &forms,
            &mut SplitMix64::new(seed),
        );
        assert_eq!(
            tiles,
            word_bank(
                target,
                Lang::Es,
                &allowed,
                &forms,
                &mut SplitMix64::new(seed)
            ),
            "seed {seed}: the same seed gives the same tiles"
        );
        assert_eq!(tiles.len(), 6, "seed {seed}: {tiles:?}");
        let mut rest = tiles.clone();
        for word in target_tiles {
            let at = rest
                .iter()
                .position(|t| t == word)
                .unwrap_or_else(|| panic!("seed {seed}: {word} in {tiles:?}"));
            rest.remove(at);
        }
        let distinct: BTreeSet<&String> = rest.iter().collect();
        assert_eq!(distinct.len(), 3, "seed {seed}: distractors {rest:?}");
        for word in &rest {
            assert!(
                !target_tiles.contains(&word.as_str()),
                "seed {seed}: {word} is a target tile"
            );
            assert!(
                pool.contains(word),
                "seed {seed}: {word} is an allowed form"
            );
        }
        orders.insert(tiles.join(" "));
    }
    assert!(orders.len() > 1, "the tiles are shuffled");

    let joined = target_tiles.join(" ");
    assert_eq!(joined, normalise(Lang::Es, target));
    let ex = exercise(
        "e1",
        &id(CAFE, "e:3"),
        Facet::Productive,
        Body::TranslateToTarget {
            prompt_en: "I want a coffee.".to_string(),
            target: target.to_string(),
            accepted: strings(&[target]),
            tiles: Some(word_bank(
                target,
                Lang::Es,
                &allowed,
                &forms,
                &mut SplitMix64::new(3),
            )),
        },
    );
    assert_eq!(
        grade(&plain_ctx(Lang::Es, &forms), &ex, &text(&joined)).verdict,
        Verdict::Correct
    );
}

// ---------------------------------------------------------------------------------------------
// 13. The composer
// ---------------------------------------------------------------------------------------------

fn verdict_for(rating: Rating) -> srs::Verdict {
    match rating {
        Rating::Again => srs::Verdict::Wrong,
        Rating::Hard => srs::Verdict::Almost,
        Rating::Good | Rating::Easy => srs::Verdict::Correct,
    }
}

/// The plan of a composer fixture (bullet 13): the course started at 0, each reviewed item
/// introduced and its reviews applied in order at the default retention, then compose at
/// `now_ms`. `capabilities` overrides the fixture's.
fn compose_fixture(fixture: &Value, capabilities: Option<(bool, bool)>) -> Plan {
    let syllabus = syllabus_at(fixture["syllabus"].as_str().unwrap());
    let forms = forms_at(fixture["forms"].as_str().unwrap());
    let mut store = ok(Store::open_in_memory());
    ok(store.start_course(&syllabus, &forms, 0));
    for review in fixture["reviews"].as_array().unwrap() {
        let item = review["item_id"].as_str().unwrap();
        let facet: Facet = review["facet"].as_str().unwrap().parse().unwrap();
        let rating = Rating::try_from(review["rating"].as_i64().unwrap()).unwrap();
        ok(store.introduce(item, 0));
        let ctx = ReviewContext {
            exercise_type: "fixture".to_string(),
            verdict: verdict_for(rating),
            response_ms: None,
            session_id: "fixture".to_string(),
        };
        ok(store.review(
            item,
            facet,
            rating,
            review["at_ms"].as_i64().unwrap(),
            &ctx,
            &SrsParams::default(),
        ));
    }
    let (audio_out, mic) = capabilities.unwrap_or((
        fixture["audio_out"].as_bool().unwrap(),
        fixture["mic"].as_bool().unwrap(),
    ));
    let request = ComposeRequest {
        lang: syllabus.lang,
        skill_id: fixture["skill"].as_str().unwrap().to_string(),
        lesson_length: fixture["lesson_length"].as_u64().unwrap() as usize,
        new_items_per_lesson: fixture["new_items_per_lesson"].as_u64().unwrap() as usize,
        capabilities: Capabilities { audio_out, mic },
    };
    ok(compose(
        &request,
        &store,
        fixture["now_ms"].as_i64().unwrap(),
    ))
}

fn plan_json(plan: &Plan) -> Value {
    Value::Array(
        plan.entries
            .iter()
            .map(|e| {
                json!({"n": e.n, "item_id": e.item_id, "facet": e.facet.as_str(),
                       "kind": e.kind.as_str(), "focus": e.focus, "new": e.new})
            })
            .collect(),
    )
}

fn json_file(rel: &str) -> Value {
    serde_json::from_str(&read(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

// Bullet 13: composer-a (5 new items, no dues), composer-b (3 new, 5 due across both skills,
// cut at lesson_length 13) and composer-c (capabilities on) give the plans of their expected
// files, worked out by hand from the composer rules: every review is Good at time 0, so at 30
// days every reviewed card is due with equal retrievability and due date and the queue falls to
// item id in byte order, receptive first; introduced cards never reviewed have retrievability 0.
// `new` is true for every entry of an item the lesson introduces (its intro, its productive
// entry and a match_pairs entry for it), as the Design notes' decisions record.
#[test]
fn t13_composer_fixtures_give_the_expected_plans() {
    for name in ["composer-a", "composer-b", "composer-c"] {
        let fixture = json_file(&format!("{CASES}/{name}.json"));
        let plan = compose_fixture(&fixture, None);
        let expected = json_file(&format!("{CASES}/{name}.expected.json"));
        assert_eq!(plan_json(&plan), expected["entries"], "{name}");
        let length = fixture["lesson_length"].as_u64().unwrap() as usize;
        assert!(
            plan.entries.len() <= length,
            "{name}: at most lesson_length"
        );
        let numbers: Vec<u32> = plan.entries.iter().map(|e| e.n).collect();
        let counted: Vec<u32> = (1..=plan.entries.len() as u32).collect();
        assert_eq!(numbers, counted, "{name}: n counts from 1");
        let kinds: Vec<&str> = plan.entries.iter().map(|e| e.kind.as_str()).collect();
        if !fixture["audio_out"].as_bool().unwrap() {
            assert!(!kinds.contains(&"listen_type"), "{name}");
        }
        if !fixture["mic"].as_bool().unwrap() {
            assert!(!kinds.contains(&"speak"), "{name}");
        }
    }
}

// Bullet 13: the capability mask. composer-c without audio has no listen_type (those entries
// stay translate_to_en), without a microphone no speak (they stay translate_to_target), and
// nothing else changes.
#[test]
fn t13_capabilities_only_change_listen_and_speak() {
    let fixture = json_file(&format!("{CASES}/composer-c.json"));
    let expected = json_file(&format!("{CASES}/composer-c.expected.json"))["entries"].clone();
    let revert = |entries: &Value, listen: bool, speak: bool| -> Value {
        let mut entries = entries.clone();
        for entry in entries.as_array_mut().unwrap() {
            if !listen && entry["kind"] == "listen_type" {
                entry["kind"] = json!("translate_to_en");
            }
            if !speak && entry["kind"] == "speak" {
                entry["kind"] = json!("translate_to_target");
            }
        }
        entries
    };
    for (audio_out, mic) in [(false, false), (true, false), (false, true)] {
        let plan = compose_fixture(&fixture, Some((audio_out, mic)));
        assert_eq!(
            plan_json(&plan),
            revert(&expected, audio_out, mic),
            "audio_out {audio_out}, mic {mic}"
        );
    }
    let b = json_file(&format!("{CASES}/composer-b.json"));
    for length in 1..=16u64 {
        let mut shorter = b.clone();
        shorter["lesson_length"] = json!(length);
        let plan = compose_fixture(&shorter, None);
        assert!(
            plan.entries.len() <= length as usize,
            "lesson_length {length}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Template lessons (10) on the fixture and on the seed content
// ---------------------------------------------------------------------------------------------

/// The kinds an entry of `kind` may end up as: the kind, then its fallbacks in order.
fn fallbacks(kind: &str) -> &'static [&'static str] {
    match kind {
        "match_pairs" => &["match_pairs", "pick_meaning", "translate_to_en"],
        "pick_meaning" => &["pick_meaning", "translate_to_en"],
        "fill_gap" => &["fill_gap", "translate_to_target"],
        "listen_type" => &["listen_type", "translate_to_en"],
        "speak" => &["speak", "translate_to_target"],
        "translate_to_en" => &["translate_to_en"],
        "translate_to_target" => &["translate_to_target"],
        other => panic!("unknown kind {other}"),
    }
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

/// Every problem of a template lesson against its plan, as bullet 10 lists them: one exercise
/// per entry with id e{n} and the entry's item and facet, a kind from the entry's fallbacks,
/// and every exercise passing check_exercise with the item's lemmas as focus and the earlier
/// exercises as seen.
fn lesson_problems(
    syllabus: &Syllabus,
    forms: &Forms,
    numbers: &[String],
    items: &[Item],
    plan: &Plan,
    lesson: &Lesson,
) -> Vec<String> {
    let skill = lesson.skill_id.as_str();
    let mut problems = Vec::new();
    if lesson.exercises.len() != plan.entries.len() {
        problems.push(format!(
            "{skill}: {} exercises for {} plan entries",
            lesson.exercises.len(),
            plan.entries.len()
        ));
    }
    let cefr = syllabus
        .skills()
        .find(|(_, s)| s.id == skill)
        .map(|(_, s)| s.cefr)
        .unwrap_or_else(|| panic!("no skill {skill}"));
    let allowed = Allowed::for_skill(syllabus, numbers, skill);
    let mut seen: Vec<(ExerciseType, String)> = Vec::new();
    for (ex, entry) in lesson.exercises.iter().zip(&plan.entries) {
        if ex.id != format!("e{}", entry.n)
            || ex.item_id != entry.item_id
            || ex.facet != entry.facet
        {
            problems.push(format!(
                "{skill} {}: does not match entry {} ({})",
                ex.id, entry.n, entry.item_id
            ));
        }
        if !fallbacks(entry.kind.as_str()).contains(&ex.kind().as_str()) {
            problems.push(format!(
                "{skill} {}: {} for a {} entry",
                ex.id,
                ex.kind().as_str(),
                entry.kind.as_str()
            ));
        }
        let focus = items
            .iter()
            .find(|item| item.id == ex.item_id)
            .map(|item| focus_of(syllabus, &item.lemmas))
            .unwrap_or_default();
        let ctx = CheckCtx {
            lang: lesson.lang,
            forms,
            allowed: &allowed,
            max_words: max_words(cefr),
            focus: &focus,
            seen: &seen,
        };
        if let Err(failure) = check_exercise(ex, &ctx) {
            problems.push(format!(
                "{skill} {}: {} fails {}",
                ex.id,
                ex.kind().as_str(),
                failure.rule()
            ));
        }
        let first = ex.targets().first().copied().unwrap_or("").to_string();
        seen.push((ex.kind(), normalise(lesson.lang, &first)));
    }
    problems
}

fn lesson_validator() -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(&read("schemas/lesson.schema.json"))
        .expect("schemas/lesson.schema.json is JSON");
    jsonschema::validator_for(&schema).expect("schemas/lesson.schema.json is a JSON Schema")
}

fn schema_problems(validator: &jsonschema::Validator, lesson: &Lesson) -> Vec<String> {
    let value = serde_json::to_value(lesson).expect("a lesson serialises");
    validator
        .iter_errors(&value)
        .map(|e| format!("{}: schema: {e}", lesson.skill_id))
        .collect()
}

// Design notes (template lesson): on the fixture, composer-a's plan gives one exercise per
// entry, ids e{n}, each passing the checks; the same seed gives the same lesson.
#[test]
fn template_lesson_on_the_fixture_course() {
    let syllabus = course();
    let forms = course_forms();
    let items = derive_items(&syllabus, &forms);
    let allowed = Allowed::for_skill(&syllabus, &[], CAFE);
    let plan = compose_fixture(&json_file(&format!("{CASES}/composer-a.json")), None);
    let plan_items: Vec<String> = plan.entries.iter().map(|e| e.item_id.clone()).collect();
    let ctx = cafe_ctx(&syllabus, &forms, &items, &allowed, &[], &plan_items);
    let validator = lesson_validator();
    for seed in 0..20 {
        let lesson = template_lesson(&plan, &ctx, 7, &mut SplitMix64::new(seed));
        assert_eq!(lesson.schema, "parley.lesson/1");
        assert_eq!(lesson.lang, Lang::Es);
        assert_eq!(lesson.skill_id, CAFE);
        assert_eq!(lesson.seq, 7);
        assert!(matches!(lesson.origin, Origin::Template));
        let problems = lesson_problems(&syllabus, &forms, &[], &items, &plan, &lesson);
        assert!(problems.is_empty(), "seed {seed}: {problems:#?}");
        let schema = schema_problems(&validator, &lesson);
        assert!(schema.is_empty(), "seed {seed}: {schema:#?}");
        let again = template_lesson(&plan, &ctx, 7, &mut SplitMix64::new(seed));
        assert_eq!(
            serde_json::to_value(&lesson).unwrap(),
            serde_json::to_value(&again).unwrap(),
            "seed {seed}"
        );
    }
}

/// One language of the seed content, as `parley lesson template` reads it from content/<l>/.
struct SeedCourse {
    lang: Lang,
    syllabus: Syllabus,
    forms: Forms,
    numbers: Vec<String>,
    items: Vec<Item>,
}

fn seed_course(lang: Lang) -> SeedCourse {
    let dir = format!("content/{}", lang.as_str());
    let syllabus = syllabus_at(&format!("{dir}/syllabus.yaml"));
    let forms = forms_at(&format!("{dir}/forms.tsv"));
    let numbers = load_numbers(&read(&format!("{dir}/numbers.yaml")))
        .unwrap_or_else(|e| panic!("{dir}/numbers.yaml: {e}"));
    let items = derive_items(&syllabus, &forms);
    SeedCourse {
        lang,
        syllabus,
        forms,
        numbers,
        items,
    }
}

/// The plan `parley lesson template --seq <seq>` composes: an in-memory store started at time
/// 0, the skill's first (seq − 1) × 5 items introduced at time 0, compose at time 0 with 12
/// exercises, 5 new items and no capabilities.
fn template_plan(course: &SeedCourse, skill: &str, seq: u32) -> Plan {
    let mut store = ok(Store::open_in_memory());
    ok(store.start_course(&course.syllabus, &course.forms, 0));
    let introduced = ((seq - 1) * 5) as usize;
    for item in ok(store.item_ids(skill)).iter().take(introduced) {
        ok(store.introduce(item, 0));
    }
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
    ok(compose(&request, &store, 0))
}

/// The lesson `parley lesson template` builds from that plan: Allowed::for_skill with the
/// language's number words, no pack sentences, the plan's item ids, SplitMix64(seed).
fn seed_lesson(course: &SeedCourse, skill: &str, plan: &Plan, seq: u32, seed: u64) -> Lesson {
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
    template_lesson(plan, &ctx, seq, &mut SplitMix64::new(seed))
}

// Bullet 10: for every skill of the seed content (es, fr, de) and seeds 1 to 100, the template
// lesson of seq 1 built as `parley lesson template` builds it has one exercise per plan entry,
// every exercise passes check_exercise, the lesson validates against schemas/lesson.schema.json,
// and the same seed gives the same lesson.
#[test]
fn t10_template_lessons_over_the_seed_content() {
    let validator = lesson_validator();
    let mut problems = Vec::new();
    let mut lessons = 0;
    for lang in Lang::ALL {
        let course = seed_course(lang);
        let skills: Vec<String> = course
            .syllabus
            .skills()
            .map(|(_, skill)| skill.id.clone())
            .collect();
        for skill in &skills {
            let plan = template_plan(&course, skill, 1);
            assert!(!plan.entries.is_empty(), "{skill}: an empty plan");
            assert!(plan.entries.len() <= 12, "{skill}: more than 12 entries");
            for seed in 1..=100u64 {
                let lesson = seed_lesson(&course, skill, &plan, 1, seed);
                let again = seed_lesson(&course, skill, &plan, 1, seed);
                if serde_json::to_value(&lesson).unwrap() != serde_json::to_value(&again).unwrap() {
                    problems.push(format!("{skill} seed {seed}: not deterministic"));
                }
                for problem in lesson_problems(
                    &course.syllabus,
                    &course.forms,
                    &course.numbers,
                    &course.items,
                    &plan,
                    &lesson,
                )
                .into_iter()
                .chain(schema_problems(&validator, &lesson))
                {
                    problems.push(format!("seed {seed}: {problem}"));
                }
                lessons += 1;
            }
        }
    }
    assert!(lessons >= 1_000, "{lessons} lessons built");
    let shown: Vec<&String> = problems.iter().take(30).collect();
    assert!(
        problems.is_empty(),
        "{} problem(s), the first: {shown:#?}",
        problems.len()
    );
}

// ---------------------------------------------------------------------------------------------
// 15. XP
// ---------------------------------------------------------------------------------------------

fn session(kind: SessionKind, counts: &[(Verdict, usize)], finished: bool) -> SessionResult {
    let mut verdicts = Vec::new();
    for (verdict, count) in counts {
        verdicts.extend(std::iter::repeat_n(*verdict, *count));
    }
    SessionResult {
        kind,
        verdicts,
        finished,
    }
}

// Bullet 15 and Design notes (XP): 1 per correct, typo or almost answer; 10 for finishing a
// lesson, 5 for a review session; +5 for a finished lesson with no wrong answer.
#[test]
fn t15_xp_per_answer_and_for_finishing() {
    use SessionKind::{Lesson as L, Review as R};
    use Verdict::{Almost, Correct, Typo, Unchecked, Wrong};
    assert_eq!(xp_for(&session(L, &[(Correct, 10), (Wrong, 2)], true)), 20);
    assert_eq!(xp_for(&session(L, &[(Correct, 12)], true)), 27);
    assert_eq!(
        xp_for(&session(L, &[(Correct, 10), (Typo, 1), (Almost, 1)], true)),
        27,
        "typo and almost earn 1 and are not wrong"
    );
    assert_eq!(
        xp_for(&session(L, &[(Correct, 11), (Unchecked, 1)], true)),
        26,
        "unchecked earns nothing and is not wrong"
    );
    assert_eq!(xp_for(&session(L, &[(Correct, 5)], false)), 5);
    assert_eq!(xp_for(&session(R, &[(Correct, 8), (Wrong, 1)], true)), 13);
    assert_eq!(xp_for(&session(R, &[(Correct, 9)], true)), 14);
    assert_eq!(xp_for(&session(R, &[(Correct, 3)], false)), 3);
}

// ---------------------------------------------------------------------------------------------
// 16 and 17. Streaks, freezes and weekly records
// ---------------------------------------------------------------------------------------------

fn active(days: impl IntoIterator<Item = u32>) -> Vec<DayRow> {
    days.into_iter()
        .map(|day| DayRow {
            local_date: jan(day),
            xp: 10,
            sessions: 1,
            goal_xp: 20,
            frozen: false,
        })
        .collect()
}

fn state_events(state: &StreakState) -> Vec<(LocalDate, &'static str, u32)> {
    state
        .events
        .iter()
        .map(|e| (e.local_date, kind_name(&e.kind), e.streak_len))
        .collect()
}

// Bullet 16: seven active days earn a freeze; a missed day uses it; at most two are banked;
// two missed days with one freeze reset the streak.
#[test]
fn t16_streaks_and_freezes() {
    let week = streak(&active(1..=7), jan(7));
    assert_eq!((week.length, week.freezes, week.active_today), (7, 1, true));
    assert!(week.frozen.is_empty());
    assert_eq!(state_events(&week), [(jan(7), "earned", 7)]);

    let frozen = streak(&active((1..=7).chain([9])), jan(9));
    assert_eq!((frozen.length, frozen.freezes), (8, 0));
    assert_eq!(frozen.frozen, [jan(8)]);
    assert_eq!(
        state_events(&frozen),
        [(jan(7), "earned", 7), (jan(8), "used", 7)]
    );

    let three_weeks = streak(&active(1..=21), jan(21));
    assert_eq!((three_weeks.length, three_weeks.freezes), (21, 2));
    assert_eq!(
        state_events(&three_weeks),
        [(jan(7), "earned", 7), (jan(14), "earned", 14)],
        "none at 21: two are banked"
    );
    let four_weeks = streak(&active(1..=28), jan(28));
    assert_eq!((four_weeks.length, four_weeks.freezes), (28, 2));
    assert_eq!(state_events(&four_weeks).len(), 2);

    let reset = streak(&active((1..=7).chain([10])), jan(10));
    assert_eq!((reset.length, reset.freezes), (1, 0));
    assert_eq!(reset.frozen, [jan(8)]);
    assert_eq!(
        state_events(&reset),
        [(jan(7), "earned", 7), (jan(8), "used", 7)]
    );

    // Two banked freezes cover two missed days.
    let covered = streak(&active((1..=14).chain([17])), jan(17));
    assert_eq!((covered.length, covered.freezes), (15, 0));
    assert_eq!(covered.frozen, [jan(15), jan(16)]);
    assert_eq!(
        state_events(&covered),
        [
            (jan(7), "earned", 7),
            (jan(14), "earned", 14),
            (jan(15), "used", 14),
            (jan(16), "used", 14)
        ]
    );

    // A freeze used, then earned again at length 14.
    let again = streak(&active((1..=7).chain(9..=15)), jan(15));
    assert_eq!((again.length, again.freezes), (14, 1));
    assert_eq!(
        state_events(&again),
        [
            (jan(7), "earned", 7),
            (jan(8), "used", 7),
            (jan(15), "earned", 14)
        ]
    );
}

// Bullet 16 and Design notes: today, not yet active, keeps yesterday's streak; a frozen row with
// no session is not an active day, so recomputing from the written rows gives the same state.
#[test]
fn t16_today_and_frozen_rows() {
    let waiting = streak(&active(1..=3), jan(4));
    assert_eq!(
        (waiting.length, waiting.freezes, waiting.active_today),
        (3, 0, false)
    );
    let lapsed = streak(&active(1..=3), jan(5));
    assert_eq!((lapsed.length, lapsed.active_today), (0, false));

    let mut written = active((1..=7).chain([9]));
    written.insert(
        7,
        DayRow {
            local_date: jan(8),
            xp: 0,
            sessions: 0,
            goal_xp: 20,
            frozen: true,
        },
    );
    let recomputed = streak(&written, jan(9));
    assert_eq!((recomputed.length, recomputed.freezes), (8, 0));
    assert_eq!(recomputed.frozen, [jan(8)]);
    assert_eq!(
        state_events(&recomputed),
        [(jan(7), "earned", 7), (jan(8), "used", 7)]
    );

    let nothing = streak(&[], jan(1));
    assert_eq!(
        (nothing.length, nothing.freezes, nothing.active_today),
        (0, 0, false)
    );
    assert!(nothing.frozen.is_empty() && nothing.events.is_empty());
}

fn week_rows(entries: &[(LocalDate, u32)]) -> Vec<DayRow> {
    entries
        .iter()
        .map(|&(local_date, xp)| DayRow {
            local_date,
            xp,
            sessions: 1,
            goal_xp: 20,
            frozen: false,
        })
        .collect()
}

// Bullet 17 and Design notes: weeks of 100 and 80 XP, then a third week reaching 101 is a record
// exactly once; ISO weeks start on Monday, across the turn of the year too.
#[test]
fn t17_weekly_record() {
    assert_eq!(WEEKLY_RECORD, "New weekly record!");
    assert!(!weekly_record(&[], jan(5), 100), "no earlier week");
    let two = week_rows(&[(jan(5), 100)]);
    assert!(!weekly_record(&two, jan(12), 80));
    let mut rows = week_rows(&[(jan(5), 100), (jan(12), 80)]);
    let mut records = Vec::new();
    for (day, xp) in [(19, 50), (20, 50), (21, 1), (22, 10), (22, 5)] {
        records.push(weekly_record(&rows, jan(day), xp));
        match rows.iter_mut().find(|r| r.local_date == jan(day)) {
            Some(row) => {
                row.xp += xp;
                row.sessions += 1;
            }
            None => rows.extend(week_rows(&[(jan(day), xp)])),
        }
    }
    assert_eq!(records, [false, false, true, false, false]);

    assert!(
        !weekly_record(&week_rows(&[(jan(5), 100), (jan(12), 60)]), jan(13), 40),
        "equal to the best week is not above it"
    );
    assert!(
        !weekly_record(&week_rows(&[(jan(5), 0)]), jan(12), 5),
        "the best earlier week must be above 0"
    );
    assert!(
        !weekly_record(&two, jan(11), 150),
        "Sunday 11 January is in the week of Monday 5 January"
    );
    assert!(weekly_record(&two, jan(12), 150));
    assert!(
        !weekly_record(&week_rows(&[(ymd(2025, 12, 29), 100)]), jan(1), 150),
        "29 December 2025 and 1 January 2026 are both in 2026-W01"
    );
    assert!(weekly_record(
        &week_rows(&[(ymd(2025, 12, 28), 100)]),
        jan(1),
        150
    ));
}

// ---------------------------------------------------------------------------------------------
// The lesson runner (18, 19)
// ---------------------------------------------------------------------------------------------

fn query<T: rusqlite::types::FromSql>(conn: &Connection, sql: &str) -> T {
    conn.query_row(sql, [], |row| row.get(0))
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

/// The first row of `sql` as a JSON array of its `columns` values.
fn row_values(conn: &Connection, sql: &str, columns: usize) -> Value {
    use rusqlite::types::Value as Sql;
    conn.query_row(sql, [], |row| {
        let mut values = Vec::new();
        for i in 0..columns {
            values.push(match row.get::<_, Sql>(i)? {
                Sql::Null => Value::Null,
                Sql::Integer(n) => json!(n),
                Sql::Real(x) => json!(x),
                Sql::Text(text) => json!(text),
                Sql::Blob(bytes) => json!(bytes),
            });
        }
        Ok(Value::Array(values))
    })
    .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

// Design notes (runner): start_lesson stores the lesson as `{skill_id}/{seq}` and a lesson
// session on the local date of the start; finish completes the session and credits the day;
// start_review writes a review session only; an existing (skill, seq) lesson row is kept.
#[test]
fn runner_writes_sessions_lessons_and_daily_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parley.sqlite3");
    let mut store = ok(Store::open(&path));
    ok(store.start_course(&course(), &course_forms(), 0));
    let forms = course_forms();
    let settings = RunSettings::default();
    // 09:00 UTC is 10:00 at UTC+1 on Monday 5 January.
    let start = FixedClock {
        ms: jan_at(5, 9),
        offset: 60,
    };
    let lesson = lesson_of(vec![
        agua_pick("e1"),
        to_target(
            "e2",
            &id(CAFE, "e:3"),
            "I want a coffee.",
            &["Quiero un café."],
        ),
        fill_gap(
            "e3",
            &id(CAFE, "l:leche"),
            "Quiero un café con ___.",
            "leche",
            "Quiero un café con leche.",
        ),
    ]);
    let mut run = ok(LessonRun::start_lesson(
        &mut store, &lesson, "lesson-1", &start,
    ));
    assert_eq!(run.exercises().len(), 3);
    {
        let conn = Connection::open(&path).unwrap();
        assert_eq!(
            row_values(
                &conn,
                "SELECT lang, kind, skill_id, started_at, ended_at, local_date, xp FROM sessions WHERE id = 'lesson-1'",
                7
            ),
            json!(["es", "lesson", CAFE, start.ms, null, "2026-01-05", 0])
        );
        let stored = row_values(
            &conn,
            "SELECT id, skill_id, seq, origin, content FROM lessons",
            5,
        );
        assert_eq!(
            json!([stored[0], stored[1], stored[2], stored[3]]),
            json!(["es.a1.u01.s02/1", CAFE, 1, "template"])
        );
        let content: Value = serde_json::from_str(stored[4].as_str().unwrap()).unwrap();
        assert_eq!(content["exercises"].as_array().unwrap().len(), 3);
    }
    let g = run_ok(run.submit(
        &mut store,
        &forms,
        &settings,
        &submission("e1", Answer::Choice(0)),
        &start,
    ));
    assert_eq!(g.verdict, Verdict::Correct);
    let g = run_ok(run.submit(
        &mut store,
        &forms,
        &settings,
        &submission("e2", text("Quiero un café.")),
        &start,
    ));
    assert_eq!(g.verdict, Verdict::Correct);
    let g = run_ok(run.submit(
        &mut store,
        &forms,
        &settings,
        &submission("e3", text("agua")),
        &start,
    ));
    assert_eq!(g.verdict, Verdict::Wrong);
    let end = FixedClock {
        ms: jan_at(5, 9) + 10 * MINUTE,
        offset: 60,
    };
    let summary = run_ok(run.finish(&mut store, &settings, &end));
    assert_eq!(
        summary.xp, 12,
        "2 right answers and 10 for finishing; a wrong answer, no bonus"
    );
    assert_eq!((summary.correct, summary.total), (2, 3));
    assert_eq!((summary.streak, summary.freezes), (1, 0));
    assert!(!summary.record_week);
    assert_eq!(summary.message, None);
    assert_eq!((summary.daily_xp, summary.goal_xp), (12, 20));
    {
        let conn = Connection::open(&path).unwrap();
        assert_eq!(
            row_values(
                &conn,
                "SELECT ended_at, xp, exercises, correct FROM sessions WHERE id = 'lesson-1'",
                4
            ),
            json!([end.ms, 12, 3, 2])
        );
        assert_eq!(
            row_values(
                &conn,
                "SELECT local_date, xp, sessions, goal_xp, frozen FROM daily",
                5
            ),
            json!(["2026-01-05", 12, 1, 20, 0])
        );
    }

    let mut review = ok(LessonRun::start_review(
        &mut store,
        Lang::Es,
        vec![agua_pick("r1")],
        "review-1",
        &end,
    ));
    run_ok(review.submit(
        &mut store,
        &forms,
        &settings,
        &submission("r1", Answer::Choice(0)),
        &end,
    ));
    let summary = run_ok(review.finish(&mut store, &settings, &end));
    assert_eq!(summary.xp, 6, "1 for the answer and 5 for a review session");
    assert_eq!(summary.daily_xp, 18);
    let daily = ok(store.daily());
    assert_eq!(daily.len(), 1);
    assert_eq!(
        (
            daily[0].local_date,
            daily[0].xp,
            daily[0].sessions,
            daily[0].goal_xp,
            daily[0].frozen
        ),
        (jan(5), 18, 2, 20, false)
    );

    let replacement = lesson_of(vec![agua_pick("e1")]);
    let _ = ok(LessonRun::start_lesson(
        &mut store,
        &replacement,
        "lesson-2",
        &end,
    ));
    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        query::<String>(&conn, "SELECT kind FROM sessions WHERE id = 'review-1'"),
        "review"
    );
    assert_eq!(query::<i64>(&conn, "SELECT COUNT(*) FROM lessons"), 1);
    let kept: String = query(&conn, "SELECT content FROM lessons");
    let kept: Value = serde_json::from_str(&kept).unwrap();
    assert_eq!(
        kept["exercises"].as_array().unwrap().len(),
        3,
        "the existing (skill, seq) row is kept"
    );
}

// Design notes (runner): each graded answer introduces the item and reviews (item, facet) with
// rating_for over the verdict's srs value, the response time and the hint; an unchecked answer
// reviews nothing.
#[test]
fn runner_reviews_each_graded_answer_with_its_rating() {
    let forms = course_forms();
    let settings = RunSettings::default();
    let mut store = course_store();
    let lesson = lesson_of(vec![
        agua_pick("e1"),
        pick(
            "e2",
            &id(CAFE, "l:leche"),
            "la leche",
            &["milk", "water", "coffee", "dog"],
            0,
        ),
        to_target(
            "e3",
            &id(CAFE, "e:3"),
            "I want a coffee.",
            &["Quiero un café."],
        ),
        listen(
            "e4",
            &id(CAFE, "e:2"),
            "Quiero un café con leche.",
            &["Quiero un café con leche."],
        ),
        fill_gap(
            "e5",
            &id(CAFE, "l:perro"),
            "El ___ quiere agua.",
            "perro",
            "El perro quiere agua.",
        ),
        to_en(
            "e6",
            &id(CAFE, "e:5"),
            "Un agua, por favor.",
            &["A water, please."],
        ),
    ]);
    let at = clock(jan_at(5, 9));
    let mut run = ok(LessonRun::start_lesson(
        &mut store,
        &lesson,
        "session-1",
        &at,
    ));
    let answers = [
        Submission {
            exercise_id: "e1".to_string(),
            answer: Answer::Choice(0),
            response_ms: Some(1_000),
            used_hint: false,
        },
        Submission {
            exercise_id: "e2".to_string(),
            answer: Answer::Choice(0),
            response_ms: Some(1_000),
            used_hint: true,
        },
        submission("e3", text("Quiero un cafe")),
        submission("e4", text("Quiero un café con lecje")),
        submission("e5", text("gato")),
        submission("e6", text("Some water please now")),
    ];
    let verdicts: Vec<Verdict> = answers
        .iter()
        .map(|a| run_ok(run.submit(&mut store, &forms, &settings, a, &at)).verdict)
        .collect();
    assert_eq!(
        verdicts,
        [
            Verdict::Correct,
            Verdict::Correct,
            Verdict::Almost,
            Verdict::Typo,
            Verdict::Wrong,
            Verdict::Unchecked
        ]
    );
    let log = ok(store.review_log());
    let logged: Vec<(String, Rating, srs::Verdict)> = log
        .iter()
        .map(|e| (e.item_id.clone(), e.rating, e.verdict))
        .collect();
    assert_eq!(
        logged,
        [
            (id(CAFE, "l:agua"), Rating::Easy, srs::Verdict::Correct),
            (id(CAFE, "l:leche"), Rating::Hard, srs::Verdict::Correct),
            (id(CAFE, "e:3"), Rating::Hard, srs::Verdict::Almost),
            (id(CAFE, "e:2"), Rating::Hard, srs::Verdict::Almost),
            (id(CAFE, "l:perro"), Rating::Again, srs::Verdict::Wrong),
        ]
    );
    assert!(
        ok(store.card(&id(CAFE, "e:5"), Facet::Receptive)).is_some(),
        "the item of an unchecked answer is introduced"
    );
    let summary = run_ok(run.finish(&mut store, &settings, &at));
    assert_eq!(
        summary.xp, 14,
        "four answers earn 1 each, 10 for finishing, no bonus"
    );
}

// Bullet 19: the three run messages, and when each applies.
#[test]
fn t19_run_messages() {
    assert_eq!(RunError::EmptyAnswer.to_string(), "Type an answer first.");
    assert_eq!(
        RunError::AlreadyChecked.to_string(),
        "This exercise was already checked."
    );
    assert_eq!(
        RunError::NotAvailable.to_string(),
        "This exercise is no longer available."
    );
    assert_eq!(
        RunError::Store(StoreError::UnknownItem("x".to_string())).to_string(),
        "unknown item x"
    );

    let forms = course_forms();
    let settings = RunSettings::default();
    let mut store = course_store();
    let e3 = id(CAFE, "e:3");
    let lesson = lesson_of(vec![
        to_target("e1", &e3, "I want a coffee.", &["Quiero un café."]),
        pairs(
            "e2",
            &id(CAFE, "l:café"),
            &[
                ("el café", "coffee"),
                ("la leche", "milk"),
                ("el agua", "water"),
                ("el perro", "dog"),
            ],
        ),
        to_target("e3", &e3, "I want a coffee.", &["Quiero un café."]),
    ]);
    let at = clock(jan_at(5, 9));
    let mut run = ok(LessonRun::start_lesson(
        &mut store,
        &lesson,
        "session-1",
        &at,
    ));
    let submit = |run: &mut LessonRun, store: &mut Store, id_text: &str, answer: Answer| {
        run.submit(store, &forms, &settings, &submission(id_text, answer), &at)
    };

    let unknown = run_err(submit(&mut run, &mut store, "e9", text("hola")));
    assert!(matches!(unknown, RunError::NotAvailable));
    assert_eq!(unknown.to_string(), "This exercise is no longer available.");
    for empty in ["", "  ¿?  "] {
        let e = run_err(submit(&mut run, &mut store, "e1", text(empty)));
        assert!(matches!(e, RunError::EmptyAnswer), "{empty:?}");
        assert_eq!(e.to_string(), "Type an answer first.");
    }
    let e = run_err(submit(
        &mut run,
        &mut store,
        "e2",
        Answer::Pairs(Vec::new()),
    ));
    assert!(matches!(e, RunError::EmptyAnswer));
    assert!(ok(store.review_log()).is_empty(), "nothing is recorded");
    assert!(ok(store.card(&e3, Facet::Productive)).is_none());

    let e = run_err(run.i_was_right(&mut store, &settings, "e1", &at));
    assert!(matches!(e, RunError::EmptyAnswer), "not submitted yet");
    let e = run_err(run.i_was_right(&mut store, &settings, "e9", &at));
    assert!(matches!(e, RunError::NotAvailable));

    let g = run_ok(submit(&mut run, &mut store, "e1", text("Quiero un café.")));
    assert_eq!(g.verdict, Verdict::Correct);
    let again = run_err(submit(&mut run, &mut store, "e1", text("Quiero un café.")));
    assert!(matches!(again, RunError::AlreadyChecked));
    assert_eq!(again.to_string(), "This exercise was already checked.");
    let e = run_err(run.i_was_right(&mut store, &settings, "e1", &at));
    assert!(
        matches!(e, RunError::AlreadyChecked),
        "I was right on a correct answer"
    );

    let g = run_ok(submit(
        &mut run,
        &mut store,
        "e3",
        text("Me gustaría un café"),
    ));
    assert_eq!(g.verdict, Verdict::Unchecked);
    run_ok(run.i_was_right(&mut store, &settings, "e3", &at));
    let e = run_err(run.i_was_right(&mut store, &settings, "e3", &at));
    assert!(
        matches!(e, RunError::AlreadyChecked),
        "a second I was right"
    );

    // An exercise whose item the store does not know fails with the store's message.
    let stray = lesson_of(vec![
        agua_pick("e1"),
        pick(
            "e2",
            "es.a1.u09.s09#l:nada",
            "la nada",
            &["nothing", "water", "milk", "dog"],
            0,
        ),
    ]);
    let mut run = ok(LessonRun::start_lesson(
        &mut store,
        &stray,
        "session-2",
        &at,
    ));
    let e = run_err(submit(&mut run, &mut store, "e2", Answer::Choice(0)));
    assert!(matches!(e, RunError::Store(_)));
    assert_eq!(e.to_string(), "unknown item es.a1.u09.s09#l:nada");
}

// Bullet 18: 100 lessons started and finished on one local date all succeed (invariant I8: no
// lesson limit); the day's row has 100 sessions; no freeze appears for a one-day streak.
#[test]
fn t18_a_hundred_lessons_on_one_local_date() {
    let forms = course_forms();
    let mut store = course_store();
    for i in 0..100i64 {
        // From 08:00 to 16:15 UTC on 1 January, five minutes apart.
        let summary = one_lesson_at(
            &mut store,
            &forms,
            &format!("session-{i:03}"),
            jan_at(1, 8) + i * 5 * MINUTE,
        );
        assert_eq!(
            summary.xp, 16,
            "1 for the answer, 10 for finishing, 5 for no wrong answer"
        );
        assert_eq!(summary.daily_xp, 16 * (i as u32 + 1));
        assert_eq!((summary.streak, summary.freezes), (1, 0));
        assert!(!summary.record_week);
    }
    let daily = ok(store.daily());
    assert_eq!(daily.len(), 1);
    assert_eq!(
        (
            daily[0].local_date,
            daily[0].xp,
            daily[0].sessions,
            daily[0].goal_xp,
            daily[0].frozen
        ),
        (jan(1), 1_600, 100, 20, false)
    );
    assert!(ok(store.freeze_events()).is_empty());
}

fn stored_events(store: &Store) -> Vec<(LocalDate, &'static str, u32)> {
    ok(store.freeze_events())
        .iter()
        .map(|e| (e.local_date, kind_name(&e.kind), e.streak_len))
        .collect()
}

// Bullet 18 (invariant I7): freezes reach freeze_events only as the streak earns them, at the
// finish of the seventh day; a missed day is written frozen when a later lesson uses the freeze;
// a second lesson that day appends nothing. The runner also decides the weekly record before
// crediting the day (bullet 17).
#[test]
fn t18_freezes_are_written_only_as_the_streak_earns_them() {
    let forms = course_forms();
    let settings = RunSettings::default();
    let mut store = course_store();
    for day in 1..=7u32 {
        let at = clock(jan_at(day, 18));
        let lesson = lesson_of(vec![agua_pick("e1")]);
        let mut run = ok(LessonRun::start_lesson(
            &mut store,
            &lesson,
            &format!("day-{day}"),
            &at,
        ));
        run_ok(run.submit(
            &mut store,
            &forms,
            &settings,
            &submission("e1", Answer::Choice(0)),
            &at,
        ));
        assert!(
            stored_events(&store).is_empty(),
            "day {day}: nothing before the finish"
        );
        let summary = run_ok(run.finish(&mut store, &settings, &at));
        assert_eq!(summary.streak, day);
        if day < 7 {
            assert_eq!(summary.freezes, 0);
            assert!(stored_events(&store).is_empty(), "day {day}");
        } else {
            assert_eq!(summary.freezes, 1);
            assert_eq!(stored_events(&store), [(jan(7), "earned", 7)]);
        }
        assert!(!summary.record_week, "day {day}");
    }
    // 1 to 4 January are 2026-W01 (64 XP); 5 to 7 January start W02 with 48.
    let ninth = one_lesson_at(&mut store, &forms, "day-9-a", jan_at(9, 18));
    assert_eq!((ninth.streak, ninth.freezes), (8, 0));
    assert!(!ninth.record_week, "64 is not above 64");
    assert_eq!(ninth.message, None);
    assert_eq!(
        stored_events(&store),
        [(jan(7), "earned", 7), (jan(8), "used", 7)]
    );
    let daily = ok(store.daily());
    let eighth = daily
        .iter()
        .find(|row| row.local_date == jan(8))
        .expect("a row for the frozen day");
    assert_eq!((eighth.xp, eighth.sessions, eighth.frozen), (0, 0, true));
    assert_eq!(daily.len(), 9);

    let record = one_lesson_at(&mut store, &forms, "day-9-b", jan_at(9, 19));
    assert!(record.record_week, "80 is the first week above 64");
    assert_eq!(record.message.as_deref(), Some(WEEKLY_RECORD));
    assert_eq!(stored_events(&store).len(), 2, "nothing appended twice");
    let after = one_lesson_at(&mut store, &forms, "day-9-c", jan_at(9, 20));
    assert!(!after.record_week, "once per week");
    assert_eq!(after.message, None);
}

// ---------------------------------------------------------------------------------------------
// 21. Migration 0002
// ---------------------------------------------------------------------------------------------

/// (name, declared type, NOT NULL, default, position in the primary key) of PRAGMA table_info.
type Column = (&'static str, &'static str, bool, Option<&'static str>, i64);
/// The same, as read from a file.
type ColumnInfo = (String, String, bool, Option<String>, i64);

/// The five tables of 0002_sessions, column by column from docs/ARCHITECTURE.md §4. A TEXT or
/// INTEGER PRIMARY KEY column without NOT NULL reports notnull 0, as SQLite does.
fn session_tables() -> Vec<(&'static str, Vec<Column>)> {
    vec![
        (
            "sessions",
            vec![
                ("id", "TEXT", false, None, 1),
                ("lang", "TEXT", true, None, 0),
                ("kind", "TEXT", true, None, 0),
                ("skill_id", "TEXT", false, None, 0),
                ("started_at", "INTEGER", true, None, 0),
                ("ended_at", "INTEGER", false, None, 0),
                ("local_date", "TEXT", true, None, 0),
                ("xp", "INTEGER", true, Some("0"), 0),
                ("exercises", "INTEGER", true, Some("0"), 0),
                ("correct", "INTEGER", true, Some("0"), 0),
                ("cost_micros", "INTEGER", true, Some("0"), 0),
            ],
        ),
        (
            "lessons",
            vec![
                ("id", "TEXT", false, None, 1),
                ("lang", "TEXT", true, None, 0),
                ("skill_id", "TEXT", true, None, 0),
                ("seq", "INTEGER", true, None, 0),
                ("origin", "TEXT", true, None, 0),
                ("prompt_id", "TEXT", false, None, 0),
                ("content", "TEXT", true, None, 0),
                ("created_at", "INTEGER", true, None, 0),
            ],
        ),
        (
            "judgements",
            vec![
                ("key", "TEXT", false, None, 1),
                ("verdict", "TEXT", true, None, 0),
                ("corrected", "TEXT", true, None, 0),
                ("explanation", "TEXT", true, None, 0),
                ("tags", "TEXT", true, None, 0),
                ("provider", "TEXT", true, None, 0),
                ("created_at", "INTEGER", true, None, 0),
            ],
        ),
        (
            "daily",
            vec![
                ("local_date", "TEXT", false, None, 1),
                ("xp", "INTEGER", true, None, 0),
                ("sessions", "INTEGER", true, None, 0),
                ("goal_xp", "INTEGER", true, None, 0),
                ("frozen", "INTEGER", true, Some("0"), 0),
            ],
        ),
        (
            "freeze_events",
            vec![
                ("id", "INTEGER", false, None, 1),
                ("local_date", "TEXT", true, None, 0),
                ("kind", "TEXT", true, None, 0),
                ("streak_len", "INTEGER", true, None, 0),
            ],
        ),
    ]
}

fn table_info(conn: &Connection, table: &str) -> Vec<ColumnInfo> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap();
    stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)? != 0,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, i64>(5)?,
        ))
    })
    .unwrap()
    .map(|r| r.unwrap())
    .collect()
}

fn user_tables(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap();
    stmt.query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

/// SQL as written, with `--` comments removed, whitespace collapsed and no space next to
/// parentheses or commas, so §4's layout and the stored text compare by content.
fn normalised_sql(sql: &str) -> String {
    let without_comments: Vec<&str> = sql
        .lines()
        .map(|line| line.split("--").next().unwrap_or(""))
        .collect();
    without_comments
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" (", "(")
        .replace("( ", "(")
        .replace(" )", ")")
        .replace(" ,", ",")
        .replace(", ", ",")
        .trim_end_matches(';')
        .to_string()
}

/// The CREATE statements of docs/ARCHITECTURE.md §4, keyed by the object they create.
fn architecture_statements() -> Vec<(String, String)> {
    let doc = read("docs/ARCHITECTURE.md");
    let start = doc.find("```sql").expect("§4 has an sql block") + "```sql".len();
    let block = &doc[start..];
    let block = &block[..block.find("```").expect("the sql block ends")];
    let without_comments: Vec<&str> = block
        .lines()
        .map(|line| line.split("--").next().unwrap_or(""))
        .collect();
    without_comments
        .join("\n")
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|statement| {
            let words: Vec<&str> = statement.split_whitespace().collect();
            let name = if words[1] == "UNIQUE" {
                words[3]
            } else {
                words[2]
            };
            (name.to_string(), normalised_sql(statement))
        })
        .collect()
}

// Bullet 21: migration 0002 on a version-1 file (made from 0001_init.sql as module 02 wrote it)
// creates sessions, lessons, judgements, daily and freeze_events with the columns of
// ARCHITECTURE §4, their two indexes and §4's SQL verbatim, keeps the existing rows, and sets
// user_version 2. A new file opens at version 2 with the twelve tables.
#[test]
fn t21_migration_0002_on_a_version_1_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parley.sqlite3");
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(&read(
            "crates/parley-core/src/store/migrations/0001_init.sql",
        ))
        .unwrap();
        conn.execute("INSERT INTO meta (key, value) VALUES ('kept', 'yes')", [])
            .unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
    }
    let store = ok(Store::open(&path));
    assert_eq!(ok(store.user_version()), 2);
    drop(store);

    let conn = Connection::open(&path).unwrap();
    assert_eq!(query::<i64>(&conn, "PRAGMA user_version"), 2);
    assert_eq!(
        query::<String>(&conn, "SELECT value FROM meta WHERE key = 'kept'"),
        "yes"
    );
    let twelve = [
        "accepted_answers",
        "cards",
        "courses",
        "daily",
        "freeze_events",
        "items",
        "judgements",
        "lessons",
        "meta",
        "review_log",
        "sessions",
        "skills",
    ];
    assert_eq!(user_tables(&conn), twelve);
    for (table, columns) in session_tables() {
        let expected: Vec<ColumnInfo> = columns
            .iter()
            .map(|&(name, kind, not_null, default, pk)| {
                (
                    name.to_string(),
                    kind.to_string(),
                    not_null,
                    default.map(str::to_string),
                    pk,
                )
            })
            .collect();
        assert_eq!(
            table_info(&conn, table),
            expected,
            "PRAGMA table_info({table})"
        );
    }
    let indexes: [(&str, &str, &[&str], bool); 2] = [
        ("sessions_date", "sessions", &["local_date"], false),
        ("lessons_skill_seq", "lessons", &["skill_id", "seq"], true),
    ];
    for (index, table, columns, unique) in indexes {
        let mut stmt = conn
            .prepare(&format!("PRAGMA index_info({index})"))
            .unwrap();
        let actual: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(2))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(actual, columns, "index {index}");
        let mut stmt = conn
            .prepare(&format!("PRAGMA index_list({table})"))
            .unwrap();
        let listed: Vec<(String, bool)> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, i64>(2)? != 0))
            })
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(
            listed.contains(&(index.to_string(), unique)),
            "{table}: {listed:?}"
        );
    }
    let statements = architecture_statements();
    for object in [
        "sessions",
        "sessions_date",
        "lessons",
        "lessons_skill_seq",
        "judgements",
        "daily",
        "freeze_events",
    ] {
        let expected = &statements
            .iter()
            .find(|(name, _)| name == object)
            .unwrap_or_else(|| panic!("§4 creates {object}"))
            .1;
        let stored: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = ?1",
                [object],
                |row| row.get(0),
            )
            .unwrap_or_else(|e| panic!("{object} is in the file: {e}"));
        assert_eq!(
            &normalised_sql(&stored),
            expected,
            "the stored SQL of {object}"
        );
    }
    drop(conn);

    let fresh = dir.path().join("new.sqlite3");
    let store = ok(Store::open(&fresh));
    assert_eq!(ok(store.user_version()), 2);
    drop(store);
    assert_eq!(user_tables(&Connection::open(&fresh).unwrap()), twelve);
    assert_eq!(ok(ok(Store::open_in_memory()).user_version()), 2);
}

// ---------------------------------------------------------------------------------------------
// 22. Dates and the random helpers
// ---------------------------------------------------------------------------------------------

// Bullet 22: ISO 8601 weeks (week 1 holds the year's first Thursday; weeks start on Monday),
// checked against Python's datetime.date.isocalendar().
#[test]
fn t22_iso_weeks() {
    assert_eq!(ymd(2026, 1, 1).iso_week(), (2026, 1));
    assert_eq!(ymd(2027, 1, 1).iso_week(), (2026, 53));
    assert_eq!(ymd(2024, 12, 30).iso_week(), (2025, 1));
    assert_eq!(ymd(2025, 12, 29).iso_week(), (2026, 1));
    assert_eq!(ymd(2026, 1, 4).iso_week(), (2026, 1));
    assert_eq!(ymd(2026, 1, 5).iso_week(), (2026, 2));
    assert_eq!(ymd(2026, 12, 31).iso_week(), (2026, 53));
    assert_eq!(ymd(2021, 1, 3).iso_week(), (2020, 53));
    assert_eq!(ymd(2020, 12, 31).iso_week(), (2020, 53));
    assert_eq!(ymd(2015, 12, 28).iso_week(), (2015, 53));
}

// Bullet 22: parse accepts exactly YYYY-MM-DD of a real date; days and from_days are inverse
// (days since 1970-01-01 from Python's date arithmetic).
#[test]
fn t22_parse_days_and_from_days() {
    assert_eq!(LocalDate::parse("2026-02-30"), None);
    for refused in [
        "2026-2-01",
        "2026-01-1",
        "26-01-01",
        "2026/01/01",
        " 2026-01-01",
        "2026-01-01 ",
        "2026-13-01",
        "2026-00-10",
        "2026-01-00",
        "2026-04-31",
        "2023-02-29",
        "1900-02-29",
        "",
        "yyyy-mm-dd",
    ] {
        assert_eq!(LocalDate::parse(refused), None, "{refused:?}");
    }
    for accepted in ["2024-02-29", "2000-02-29", "2026-12-31", "1970-01-01"] {
        let date = LocalDate::parse(accepted).unwrap_or_else(|| panic!("{accepted}"));
        assert_eq!(date.to_string(), accepted);
    }
    assert_eq!(LocalDate::parse("2026-01-05"), Some(jan(5)));
    assert_eq!(ymd(1970, 1, 1).days(), 0);
    assert_eq!(ymd(1969, 12, 31).days(), -1);
    assert_eq!(ymd(2026, 1, 1).days(), 20_454);
    assert_eq!(ymd(2027, 1, 1).days(), 20_819);
    assert_eq!(ymd(2024, 12, 30).days(), 20_087);
    assert_eq!(LocalDate::from_days(20_454), ymd(2026, 1, 1));
    assert_eq!(LocalDate::from_days(-1), ymd(1969, 12, 31));
    for d in [
        ymd(2026, 1, 1),
        ymd(2027, 1, 1),
        ymd(2024, 2, 29),
        ymd(2000, 3, 1),
        ymd(1999, 12, 31),
    ] {
        assert_eq!(LocalDate::from_days(d.days()), d);
    }
    let mut n = -800_000i64;
    while n <= 800_000 {
        assert_eq!(LocalDate::from_days(n).days(), n);
        n += 997;
    }
    assert_eq!(serde_json::to_value(jan(5)).unwrap(), json!("2026-01-05"));
}

// Interfaces (env additions): local_date_at shifts by the offset before flooring, as
// local_date does with a clock.
#[test]
fn t22_local_date_at_matches_local_date() {
    for (ms, offset, expected) in [
        (JAN_1 - 30 * MINUTE, 60, jan(1)),
        (JAN_1 - 30 * MINUTE, 0, ymd(2025, 12, 31)),
        (JAN_1 + 30 * MINUTE, -60, ymd(2025, 12, 31)),
        (jan_at(1, 23) + 10 * MINUTE, 60, jan(2)),
        (jan_at(1, 22) + 30 * MINUTE, 60, jan(1)),
    ] {
        assert_eq!(local_date_at(ms, offset), expected, "{ms} at {offset}");
        assert_eq!(local_date(&FixedClock { ms, offset }), expected);
    }
}

// Interfaces (env additions): below is next_u64() % n and shuffle is Fisher–Yates from the end.
// Expected values from a Python transcription of SplitMix64 (seed 0 first gives
// 0xE220A8397B1DCDAF, the reference C output) and of the stated shuffle.
#[test]
fn t22_below_and_shuffle() {
    let mut rng = SplitMix64::new(0);
    let draws: Vec<usize> = (0..5).map(|_| below(&mut rng, 10)).collect();
    assert_eq!(draws, [5, 0, 9, 4, 7]);
    let mut rng = SplitMix64::new(0);
    let draws: Vec<usize> = (0..5).map(|_| below(&mut rng, 7)).collect();
    assert_eq!(draws, [2, 1, 2, 4, 2]);
    let mut rng = SplitMix64::new(3);
    assert!((0..20).all(|_| below(&mut rng, 1) == 0));

    let mut ten: Vec<u32> = (0..10).collect();
    shuffle(&mut ten, &mut SplitMix64::new(7));
    assert_eq!(ten, [8, 1, 5, 9, 0, 4, 3, 2, 6, 7]);
    let mut letters = ["a", "b", "c", "d", "e"];
    shuffle(&mut letters, &mut SplitMix64::new(0));
    assert_eq!(letters, ["c", "d", "b", "e", "a"]);
    let mut six: Vec<u32> = (0..6).collect();
    shuffle(&mut six, &mut SplitMix64::new(42));
    assert_eq!(six, [4, 3, 0, 2, 5, 1]);

    // One element: no draw is made.
    let mut rng = SplitMix64::new(0);
    let mut one = [9];
    shuffle(&mut one, &mut rng);
    assert_eq!(one, [9]);
    assert_eq!(rng.next_u64(), 0xE220_A839_7B1D_CDAF);
    let mut none: [u8; 0] = [];
    shuffle(&mut none, &mut rng);
}
