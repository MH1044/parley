//! `parley speak score`: the score of a speaking check, offline, for a sentence and a transcript
//! (docs/modules/10-speaking-and-pronunciation.md, CLI).

use std::io::Write;
use std::path::Path;

use parley_core::content::{Lang, Numbers, load_number_words};
use parley_core::speech::{WordResult, score, speak_rating};
use serde::{Deserialize, Serialize};

use super::{failure, missing, not_found, parse, print_json, shown, unknown, usage};

/// The default pass mark (`speaking.pass_threshold`'s default).
const PASS_MARK: f64 = 0.8;

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match args.first().map(String::as_str) {
        Some("score") => score_command(&args[1..], out, err),
        Some(other) => unknown(err, other),
        None => missing(err, "speak score"),
    }
}

/// The input file: a sentence, a transcript, and optionally the pass mark.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScoreInput {
    lang: Lang,
    expected: String,
    heard: String,
    #[serde(default)]
    pass_mark: Option<f64>,
}

#[derive(Serialize)]
struct ScoreOutput {
    words: Vec<WordResult>,
    extra: Vec<String>,
    score: f64,
    pass: bool,
    rating: u32,
}

/// The number words of `content/<lang>/numbers.yaml`; none when the file is not there (digits
/// then stay as written).
fn numbers_for(lang: Lang, err: &mut dyn Write) -> Result<Numbers, u8> {
    let path = format!("content/{}/numbers.yaml", lang.as_str());
    if !Path::new(&path).is_file() {
        return Ok(Numbers::default());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| failure(err, &format!("{path}: {e}")))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    load_number_words(text).map_err(|e| failure(err, &e.0))
}

/// `parley speak score <input.json> [--json]`: {"words", "extra", "score", "pass", "rating"}
/// (`rating` 1 to 4 from `speak_rating`). Exit 1 on a file that is not such an input.
fn score_command(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse(args, &[], &["--json"]) {
        Ok(parsed) => parsed,
        Err(message) => return usage(err, &message),
    };
    if let Some(extra) = parsed.positional.get(1) {
        return unknown(err, extra);
    }
    let Some(given) = parsed.positional.first() else {
        return missing(err, "<input.json>");
    };
    let file = shown(given);
    let path = Path::new(given);
    if !path.is_file() {
        return not_found(err, &file);
    }
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => return failure(err, &format!("{file}: {e}")),
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let input: ScoreInput = match serde_json::from_str(text) {
        Ok(input) => input,
        Err(e) => return failure(err, &format!("{file}: {e}")),
    };
    let pass_mark = input.pass_mark.unwrap_or(PASS_MARK);
    if !(0.0..=1.0).contains(&pass_mark) {
        return failure(err, &format!("{file}: pass_mark must be from 0 to 1"));
    }
    let numbers = match numbers_for(input.lang, err) {
        Ok(numbers) => numbers,
        Err(code) => return code,
    };
    let scored = score(
        input.lang,
        &input.expected,
        &input.heard,
        &numbers,
        pass_mark,
    );
    let rating = speak_rating(scored.score, scored.pass).as_u32();
    print_json(
        out,
        &ScoreOutput {
            words: scored.words,
            extra: scored.extra,
            score: scored.score,
            pass: scored.pass,
            rating,
        },
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

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

    #[test]
    fn usage_errors_have_their_lines() {
        assert_eq!(
            run_capture(&[]).2,
            "error: speak score is required. Run parley --help.\n"
        );
        assert_eq!(
            run_capture(&["score"]).2,
            "error: <input.json> is required. Run parley --help.\n"
        );
        assert_eq!(
            run_capture(&["score", "no/such.json"]).2,
            "error: no/such.json not found\n"
        );
        assert_eq!(
            run_capture(&["say"]).2,
            "error: unknown argument say. Run parley --help.\n"
        );
    }

    #[test]
    fn an_input_is_scored_and_anything_else_exits_1() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.json");
        std::fs::write(
            &input,
            json!({"lang": "es", "expected": "yo soy estudiante", "heard": "yo es estudiante"})
                .to_string(),
        )
        .unwrap();
        let (code, out, _) = run_capture(&["score", input.to_str().unwrap(), "--json"]);
        assert_eq!(code, 0);
        let value: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["pass"], false);
        assert_eq!(value["rating"], 2);
        assert_eq!(
            value["words"][1],
            json!({"expected": "soy", "heard": "es", "status": "substituted"})
        );
        assert_eq!(value["extra"], json!([]));
        for bad in [
            json!({"lang": "it", "expected": "a", "heard": "a"}),
            json!({"lang": "es", "expected": "a"}),
            json!({"lang": "es", "expected": "a", "heard": "a", "pass_mark": 1.5}),
            json!({"lang": "es", "expected": "a", "heard": "a", "colour": 1}),
        ] {
            std::fs::write(&input, bad.to_string()).unwrap();
            let (code, out, _) = run_capture(&["score", input.to_str().unwrap()]);
            assert_eq!((code, out.as_str()), (1, ""), "{bad}");
        }
    }
}
