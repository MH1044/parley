//! `parley grade`: grade one typed answer offline, with nothing stored and no judgement
//! (docs/modules/03-exercise-engine-and-grading.md, Interfaces).

use std::io::Write;
use std::path::Path;

use parley_core::content::{Forms, Lang};
use parley_core::exercise::{Body, Exercise, ExerciseType, Origin};
use parley_core::grading::{AccentLeniency, Answer, GradeCtx, grade, normalise, noun_info};

use super::{
    failure, load_forms, missing, not_found, parse, parse_lang, print_json, shown, unknown, usage,
};

/// The types `--type` takes: those answered by typing.
fn parse_type(text: &str) -> Option<ExerciseType> {
    match text {
        "translate_to_target" => Some(ExerciseType::TranslateToTarget),
        "translate_to_en" => Some(ExerciseType::TranslateToEn),
        "listen_type" => Some(ExerciseType::ListenType),
        "speak" => Some(ExerciseType::Speak),
        "fill_gap" => Some(ExerciseType::FillGap),
        _ => None,
    }
}

fn parse_accent(text: &str) -> Option<AccentLeniency> {
    match text {
        "lenient" => Some(AccentLeniency::Lenient),
        "strict" => Some(AccentLeniency::Strict),
        "ignore" => Some(AccentLeniency::Ignore),
        _ => None,
    }
}

/// An exercise of `kind` whose own answers are the expected ones (for `speak` and `fill_gap`,
/// which hold one answer, the first), and the expected answers it cannot hold, which are tried
/// after it in order.
fn exercise_for(kind: ExerciseType, expected: &[String]) -> (Exercise, Vec<String>) {
    let first = expected[0].clone();
    let all = expected.to_vec();
    let rest = expected[1..].to_vec();
    let (body, extra) = match kind {
        ExerciseType::TranslateToTarget => (
            Body::TranslateToTarget {
                prompt_en: String::new(),
                target: first,
                accepted: all,
                tiles: None,
            },
            Vec::new(),
        ),
        ExerciseType::TranslateToEn => (
            Body::TranslateToEn {
                prompt_target: String::new(),
                accepted: all,
            },
            Vec::new(),
        ),
        ExerciseType::ListenType => (
            Body::ListenType {
                text: first,
                accepted: all,
            },
            Vec::new(),
        ),
        ExerciseType::Speak => (
            Body::Speak {
                text: first,
                prompt_en: String::new(),
            },
            rest,
        ),
        _ => (
            Body::FillGap {
                sentence: "___".to_string(),
                answer: first.clone(),
                full_sentence: first,
                hint_en: String::new(),
            },
            rest,
        ),
    };
    let exercise = Exercise {
        id: "e1".to_string(),
        item_id: String::new(),
        facet: kind.facet(),
        origin: Origin::Template,
        body,
    };
    (exercise, extra)
}

/// The forms table: `--forms`, else `content/<lang>/forms.tsv`, else an empty table.
fn forms_for(lang: Lang, given: Option<&str>, err: &mut dyn Write) -> Result<Forms, u8> {
    let (path, explicit) = match given {
        Some(path) => (path.to_string(), true),
        None => (format!("content/{}/forms.tsv", lang.as_str()), false),
    };
    if !Path::new(&path).is_file() {
        return if explicit {
            Err(not_found(err, &shown(&path)))
        } else {
            Ok(Forms::default())
        };
    }
    load_forms(Path::new(&path)).map_err(|e| failure(err, &e.0))
}

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse(
        args,
        &[
            "--lang",
            "--type",
            "--expected",
            "--answer",
            "--accent",
            "--forms",
        ],
        &["--noun", "--json"],
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
    let Some(kind) = parsed.value("--type") else {
        return missing(err, "--type");
    };
    let expected: Vec<String> = parsed
        .values_of("--expected")
        .into_iter()
        .map(str::to_string)
        .collect();
    if expected.is_empty() {
        return missing(err, "--expected");
    }
    let Some(answer) = parsed.value("--answer") else {
        return missing(err, "--answer");
    };
    let lang = match parse_lang(err, lang) {
        Ok(lang) => lang,
        Err(code) => return code,
    };
    let Some(kind) = parse_type(kind) else {
        return usage(
            err,
            "--type must be translate_to_target, translate_to_en, listen_type, speak or fill_gap. Run parley --help.",
        );
    };
    let leniency = match parsed.value("--accent") {
        Some(text) => match parse_accent(text) {
            Some(leniency) => leniency,
            None => {
                return usage(
                    err,
                    "--accent must be lenient, strict or ignore. Run parley --help.",
                );
            }
        },
        None => AccentLeniency::Lenient,
    };
    let forms = match forms_for(lang, parsed.value("--forms"), err) {
        Ok(forms) => forms,
        Err(code) => return code,
    };
    let noun = if parsed.has("--noun") {
        match noun_info(lang, &expected[0], &forms) {
            Some(noun) => Some(noun),
            None => {
                return usage(
                    err,
                    "--noun needs an expected answer that starts with an article. Run parley --help.",
                );
            }
        }
    } else {
        None
    };
    if normalise(lang, answer).is_empty() {
        return failure(err, "Type an answer first.");
    }
    let (exercise, stored) = exercise_for(kind, &expected);
    let ctx = GradeCtx {
        lang,
        leniency,
        forms: &forms,
        noun,
        stored,
        judgement: None,
    };
    print_json(
        out,
        &grade(&ctx, &exercise, &Answer::Text(answer.to_string())),
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn graded(args: &[&str]) -> serde_json::Value {
        let (code, out, err) = run_capture(args);
        assert_eq!((code, err.as_str()), (0, ""), "{args:?}");
        serde_json::from_str(&out).unwrap()
    }

    fn usage_line(args: &[&str]) -> String {
        let (code, out, err) = run_capture(args);
        assert_eq!((code, out.as_str()), (2, ""), "{args:?}");
        err
    }

    const EMPTY: &str = "no/such/forms.tsv";

    #[test]
    fn an_accent_slip_is_almost() {
        let dir = tempfile::tempdir().unwrap();
        let forms = dir.path().join("forms.tsv");
        std::fs::write(&forms, "# source: test\n").unwrap();
        let forms = forms.to_string_lossy().into_owned();
        let value = graded(&[
            "--lang",
            "es",
            "--type",
            "listen_type",
            "--expected",
            "Buenos días.",
            "--answer",
            "Buenos dias",
            "--forms",
            &forms,
            "--json",
        ]);
        assert_eq!(
            value,
            serde_json::json!({"verdict": "almost", "tags": ["accent"],
                "message": "Almost: mind the accent: días", "corrected": "Buenos días.",
                "words": ["días"], "rating": 2})
        );
        let value = graded(&[
            "--lang",
            "es",
            "--type",
            "translate_to_target",
            "--expected",
            "Hola.",
            "--expected",
            "Buenas.",
            "--answer",
            "buenas",
            "--forms",
            &forms,
        ]);
        assert_eq!(value["verdict"], "correct");
        assert_eq!(value["rating"], 3);
        let value = graded(&[
            "--lang",
            "es",
            "--type",
            "speak",
            "--expected",
            "Hola.",
            "--expected",
            "Buenas.",
            "--answer",
            "buenas",
            "--forms",
            &forms,
        ]);
        assert_eq!(
            value["verdict"], "correct",
            "later expected answers count for speak"
        );
        let value = graded(&[
            "--lang",
            "es",
            "--type",
            "translate_to_en",
            "--expected",
            "Hello.",
            "--answer",
            "Hi there",
            "--forms",
            &forms,
        ]);
        assert_eq!(value["verdict"], "unchecked");
        assert_eq!(value["rating"], serde_json::Value::Null);
        let value = graded(&[
            "--lang",
            "es",
            "--type",
            "translate_to_target",
            "--expected",
            "el día",
            "--answer",
            "la día",
            "--noun",
            "--forms",
            &forms,
        ]);
        assert_eq!(value["message"], "Día is masculine: el día.");
    }

    #[test]
    fn every_usage_error_has_its_line() {
        let base = [
            "--lang",
            "es",
            "--type",
            "speak",
            "--expected",
            "Hola",
            "--answer",
            "hola",
        ];
        let without = |flag: &str| -> Vec<&str> {
            let at = base.iter().position(|a| *a == flag).unwrap();
            base.iter()
                .enumerate()
                .filter(|(i, _)| *i != at && *i != at + 1)
                .map(|(_, a)| *a)
                .collect()
        };
        for flag in ["--lang", "--type", "--expected", "--answer"] {
            assert_eq!(
                usage_line(&without(flag)),
                format!("error: {flag} is required. Run parley --help.\n")
            );
        }
        let with = |extra: &[&str]| -> Vec<String> {
            base.iter().chain(extra).map(|s| s.to_string()).collect()
        };
        let mut bad_lang: Vec<&str> = base.to_vec();
        bad_lang[1] = "en";
        assert_eq!(
            usage_line(&bad_lang),
            "error: --lang must be es, fr or de. Run parley --help.\n"
        );
        let mut bad_type: Vec<&str> = base.to_vec();
        bad_type[3] = "pick_meaning";
        assert_eq!(
            usage_line(&bad_type),
            "error: --type must be translate_to_target, translate_to_en, listen_type, speak or fill_gap. Run parley --help.\n"
        );
        let args = with(&["--accent", "loose"]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        assert_eq!(
            usage_line(&args),
            "error: --accent must be lenient, strict or ignore. Run parley --help.\n"
        );
        let args = with(&["--forms", EMPTY]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        assert_eq!(usage_line(&args), format!("error: {EMPTY} not found\n"));
        let args = with(&["--noun"]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        assert_eq!(
            usage_line(&args),
            "error: --noun needs an expected answer that starts with an article. Run parley --help.\n"
        );
        let args = with(&["extra"]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        assert_eq!(
            usage_line(&args),
            "error: unknown argument extra. Run parley --help.\n"
        );
    }

    #[test]
    fn an_answer_with_no_words_is_refused() {
        let (code, out, err) = run_capture(&[
            "--lang",
            "es",
            "--type",
            "speak",
            "--expected",
            "Hola",
            "--answer",
            " ¿? ",
        ]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert_eq!(err, "error: Type an answer first.\n");
    }
}
