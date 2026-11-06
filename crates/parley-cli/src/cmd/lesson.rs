//! `parley lesson template`: the template lesson of a skill, built as the app builds one offline
//! (docs/modules/03-exercise-engine-and-grading.md, Interfaces).

use std::io::Write;
use std::path::Path;

use parley_core::content::{Lang, Report, derive_items, load_syllabus};
use parley_core::env::SplitMix64;
use parley_core::exercise::{
    Allowed, Capabilities, ComposeRequest, Lesson, TemplateCtx, compose, template_lesson,
};
use parley_core::store::Store;

use super::{
    failure, join, load_siblings, missing, not_found, parse, parse_lang, print_json, unknown, usage,
};

/// The settings the command composes with: the app's defaults.
const LESSON_LENGTH: usize = 12;
const NEW_ITEMS_PER_LESSON: usize = 5;

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match args.first().map(String::as_str) {
        Some("template") => template_command(&args[1..], out, err),
        Some(other) => unknown(err, other),
        None => missing(err, "lesson template"),
    }
}

/// Why the lesson could not be built.
enum Failed {
    /// The syllabus or a file beside it does not load: the report is printed.
    Load(Report),
    /// Any other failure: one line on standard error.
    Message(String),
}

/// The template lesson of `skill_id` for a learner who has done lessons 1 to `seq` − 1: a
/// course started in memory at time 0 from `dir/lang/`, the skill's first (seq − 1) × 5 items
/// introduced at time 0 with no reviews, composed at time 0 with the default lesson length and
/// new-item count and no audio, then built from the template with `SplitMix64(seed)`.
fn build_lesson(
    dir: &Path,
    lang: Lang,
    skill_id: &str,
    seq: u32,
    seed: u64,
) -> Result<Lesson, Failed> {
    let path = dir.join(lang.as_str()).join("syllabus.yaml");
    let load_failed = |errors: Vec<String>| {
        Failed::Load(Report {
            errors,
            warnings: Vec::new(),
        })
    };
    let syllabus = load_syllabus(&path)
        .map_err(|errors| load_failed(errors.into_iter().map(|e| e.0).collect()))?;
    let siblings = load_siblings(&path, None).map_err(|e| load_failed(vec![e.0]))?;
    if !syllabus.skills().any(|(_, skill)| skill.id == skill_id) {
        return Err(Failed::Message(format!("unknown skill {skill_id}")));
    }
    let store_failed = |e: parley_core::store::StoreError| Failed::Message(e.to_string());
    let mut store = Store::open_in_memory().map_err(store_failed)?;
    store
        .start_course(&syllabus, &siblings.forms, 0)
        .map_err(store_failed)?;
    let done = (seq as usize - 1).saturating_mul(NEW_ITEMS_PER_LESSON);
    for id in store
        .item_ids(skill_id)
        .map_err(store_failed)?
        .iter()
        .take(done)
    {
        store.introduce(id, 0).map_err(store_failed)?;
    }
    let request = ComposeRequest {
        lang,
        skill_id: skill_id.to_string(),
        lesson_length: LESSON_LENGTH,
        new_items_per_lesson: NEW_ITEMS_PER_LESSON,
        capabilities: Capabilities::default(),
    };
    let plan = compose(&request, &store, 0).map_err(store_failed)?;
    let items = derive_items(&syllabus, &siblings.forms);
    let allowed = Allowed::for_skill(&syllabus, &siblings.numbers, skill_id);
    let plan_items: Vec<String> = plan.entries.iter().map(|e| e.item_id.clone()).collect();
    let ctx = TemplateCtx {
        syllabus: &syllabus,
        forms: &siblings.forms,
        items: &items,
        allowed: &allowed,
        sentences: &[],
        skill_id,
        plan_items: &plan_items,
    };
    Ok(template_lesson(
        &plan,
        &ctx,
        seq,
        &mut SplitMix64::new(seed),
    ))
}

fn template_command(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse(
        args,
        &["--lang", "--skill", "--seq", "--seed", "--content"],
        &["--json"],
    ) {
        Ok(parsed) => parsed,
        Err(message) => return usage(err, &message),
    };
    if let Some(extra) = parsed.positional.first() {
        return unknown(err, extra);
    }
    let Some(lang) = parsed.value("--lang") else {
        return missing(err, "--lang");
    };
    let Some(skill) = parsed.value("--skill") else {
        return missing(err, "--skill");
    };
    let lang = match parse_lang(err, lang) {
        Ok(lang) => lang,
        Err(code) => return code,
    };
    let seq = match parsed.value("--seq") {
        Some(text) => match text.parse::<u32>() {
            Ok(seq) if seq >= 1 => seq,
            _ => {
                return usage(
                    err,
                    "--seq must be a whole number from 1. Run parley --help.",
                );
            }
        },
        None => 1,
    };
    let seed = match parsed.value("--seed") {
        Some(text) => match text.parse::<u64>() {
            Ok(seed) => seed,
            Err(_) => return usage(err, "--seed must be a whole number. Run parley --help."),
        },
        None => 0,
    };
    let content = parsed.value("--content").unwrap_or("content");
    let syllabus = Path::new(content).join(lang.as_str()).join("syllabus.yaml");
    if !syllabus.is_file() {
        return not_found(
            err,
            &join(content, &format!("{}/syllabus.yaml", lang.as_str())),
        );
    }
    match build_lesson(Path::new(content), lang, skill, seq, seed) {
        Ok(lesson) => {
            print_json(out, &lesson);
            0
        }
        Err(Failed::Load(report)) => {
            print_json(out, &report);
            1
        }
        Err(Failed::Message(message)) => failure(err, &message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parley_core::content::{Forms, load_syllabus};
    use parley_core::exercise::{CheckCtx, check_exercise, max_words};

    fn run_capture(args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(&args, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    fn usage_line(args: &[&str]) -> String {
        let (code, out, err) = run_capture(args);
        assert_eq!((code, out.as_str()), (2, ""), "{args:?}");
        err
    }

    /// The repository's `content` folder.
    fn content() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn build(dir: &Path, lang: Lang, skill_id: &str, seq: u32, seed: u64) -> Lesson {
        match build_lesson(dir, lang, skill_id, seq, seed) {
            Ok(lesson) => lesson,
            Err(Failed::Load(report)) => panic!("{:?}", report.errors),
            Err(Failed::Message(message)) => panic!("{message}"),
        }
    }

    #[test]
    fn every_usage_error_has_its_line() {
        assert_eq!(
            usage_line(&["template", "--skill", "x"]),
            "error: --lang is required. Run parley --help.\n"
        );
        assert_eq!(
            usage_line(&["template", "--lang", "es"]),
            "error: --skill is required. Run parley --help.\n"
        );
        for seq in ["0", "-1", "1.5", "x"] {
            assert_eq!(
                usage_line(&["template", "--lang", "es", "--skill", "x", "--seq", seq]),
                "error: --seq must be a whole number from 1. Run parley --help.\n"
            );
        }
        assert_eq!(
            usage_line(&["template", "--lang", "es", "--skill", "x", "--seed", "-3"]),
            "error: --seed must be a whole number. Run parley --help.\n"
        );
        assert_eq!(
            usage_line(&[
                "template",
                "--lang",
                "es",
                "--skill",
                "x",
                "--content",
                "no/such"
            ]),
            "error: no/such/es/syllabus.yaml not found\n"
        );
        assert_eq!(
            usage_line(&[]),
            "error: lesson template is required. Run parley --help.\n"
        );
    }

    #[test]
    fn an_unknown_skill_and_a_broken_syllabus_exit_1() {
        let dir = content().to_string_lossy().into_owned();
        let (code, out, err) = run_capture(&[
            "template",
            "--lang",
            "es",
            "--skill",
            "es.a1.u99.s01",
            "--content",
            &dir,
        ]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert_eq!(err, "error: unknown skill es.a1.u99.s01\n");
        let broken = tempfile::tempdir().unwrap();
        std::fs::create_dir(broken.path().join("es")).unwrap();
        std::fs::write(broken.path().join("es/syllabus.yaml"), "units: [\n").unwrap();
        let broken = broken.path().to_string_lossy().into_owned();
        let (code, out, err) = run_capture(&[
            "template",
            "--lang",
            "es",
            "--skill",
            "x",
            "--content",
            &broken,
        ]);
        assert_eq!((code, err.as_str()), (1, ""));
        let report: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(report["ok"], false);
    }

    /// Every skill of the seed content, a few seeds and lessons: one exercise per plan entry,
    /// every exercise passes the checks, and a seed gives the same lesson. The seed content is the
    /// course this command was written against, frozen in tests/fixtures/seed-course; every
    /// skill of the full course is covered by the module 05 tests and gate G3.
    #[test]
    fn seed_content_template_lessons_pass_the_checks() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/seed-course");
        for lang in Lang::ALL {
            let path = dir.join(lang.as_str()).join("syllabus.yaml");
            let syllabus = load_syllabus(&path).unwrap();
            let siblings = load_siblings(&path, None).unwrap();
            for (_, skill) in syllabus.skills() {
                let allowed = Allowed::for_skill(&syllabus, &siblings.numbers, &skill.id);
                for seq in 1..=2 {
                    for seed in [1, 2] {
                        let lesson = build(&dir, lang, &skill.id, seq, seed);
                        let again = build(&dir, lang, &skill.id, seq, seed);
                        assert_eq!(lesson, again);
                        let introduced = (seq as usize - 1) * 5;
                        let items = skill.lemmas.len() + skill.examples.len();
                        if introduced < items {
                            assert!(!lesson.exercises.is_empty(), "{} {seq}", skill.id);
                        }
                        let ids: Vec<&str> =
                            lesson.exercises.iter().map(|e| e.id.as_str()).collect();
                        let numbered: Vec<String> =
                            (1..=ids.len()).map(|n| format!("e{n}")).collect();
                        assert_eq!(
                            ids, numbered,
                            "no plan entry dropped: {} {seq} {seed}",
                            skill.id
                        );
                        check_lesson(
                            &lesson,
                            &syllabus,
                            &siblings.forms,
                            &allowed,
                            max_words(skill.cefr),
                        );
                    }
                }
            }
        }
    }

    fn check_lesson(
        lesson: &Lesson,
        syllabus: &parley_core::content::Syllabus,
        forms: &Forms,
        allowed: &Allowed,
        limit: usize,
    ) {
        let items = derive_items(syllabus, forms);
        let mut seen = Vec::new();
        for ex in &lesson.exercises {
            let item = items.iter().find(|i| i.id == ex.item_id).unwrap();
            let focus: Vec<_> = item
                .lemmas
                .iter()
                .filter_map(|l| {
                    syllabus
                        .skills()
                        .flat_map(|(_, s)| s.lemmas.iter())
                        .find(|lemma| lemma.lemma == *l)
                        .cloned()
                })
                .collect();
            let ctx = CheckCtx {
                lang: lesson.lang,
                forms,
                allowed,
                max_words: limit,
                focus: &focus,
                seen: &seen,
            };
            assert_eq!(
                check_exercise(ex, &ctx),
                Ok(()),
                "{} {}",
                lesson.skill_id,
                ex.id
            );
            seen.push((ex.kind(), ex.targets()[0].to_string()));
        }
    }
}
