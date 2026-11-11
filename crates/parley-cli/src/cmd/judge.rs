//! `parley judge render`: the request body the answer judge sends for a judge request
//! (docs/modules/08-judging-and-explanations.md, CLI).

use std::io::Write;
use std::path::Path;

use parley_core::ai::{ChatRequest, JudgeRequest, judge_prompt_file};

use super::{failure, missing, not_found, parse, print_json, shown, unknown, usage};

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match args.first().map(String::as_str) {
        Some("render") => render_command(&args[1..], out, err),
        Some(other) => unknown(err, other),
        None => missing(err, "judge render"),
    }
}

/// `parley judge render <request.json>`: the P3 request body for publik API (model publik-fast)
/// of the `JudgeRequest` in the file: `judge.v1` rendered with the request's values,
/// `ChatRequest::body(false)`.
fn render_command(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse(args, &[], &["--json"]) {
        Ok(parsed) => parsed,
        Err(message) => return usage(err, &message),
    };
    if let Some(extra) = parsed.positional.get(1) {
        return unknown(err, extra);
    }
    let Some(given) = parsed.positional.first() else {
        return missing(err, "<request.json>");
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
    let request: JudgeRequest = match serde_json::from_str(text) {
        Ok(request) => request,
        Err(e) => return failure(err, &format!("{file}: {e}")),
    };
    let feature = judge_prompt_file();
    let messages = match feature.render(&request.vars()) {
        Ok(messages) => messages,
        Err(e) => return failure(err, &format!("{file}: {e}")),
    };
    let chat = ChatRequest {
        model: feature.tier.clone(),
        messages,
        max_tokens: feature.max_tokens,
        temperature: feature.temperature,
        timeout_s: feature.timeout_s,
        idempotency_key: None,
    };
    print_json(out, &chat.body(false));
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

    fn usage_line(args: &[&str]) -> String {
        let (code, out, err) = run_capture(args);
        assert_eq!((code, out.as_str()), (2, ""), "{args:?}");
        err
    }

    #[test]
    fn every_usage_error_has_its_line() {
        assert_eq!(
            usage_line(&[]),
            "error: judge render is required. Run parley --help.\n"
        );
        assert_eq!(
            usage_line(&["render"]),
            "error: <request.json> is required. Run parley --help.\n"
        );
        assert_eq!(
            usage_line(&["render", "no/such.json"]),
            "error: no/such.json not found\n"
        );
        assert_eq!(
            usage_line(&["score"]),
            "error: unknown argument score. Run parley --help.\n"
        );
    }

    #[test]
    fn the_example_request_renders_the_p3_body_and_other_files_exit_1() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("request.json");
        std::fs::write(
            &input,
            json!({
                "lang": "es", "direction": "to_target", "cefr": "A1",
                "source": "I would like a coffee with milk.",
                "accepted": ["Quiero un café con leche.", "Quisiera un café con leche."],
                "answer": "Me gustaría una café con leche"
            })
            .to_string(),
        )
        .unwrap();
        let path = input.to_string_lossy().into_owned();
        let (code, out, err) = run_capture(&["render", &path, "--json"]);
        assert_eq!((code, err.as_str()), (0, ""));
        let body: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            (&body["model"], &body["max_tokens"], &body["temperature"]),
            (&json!("publik-fast"), &json!(200), &json!(0))
        );
        assert_eq!(
            body["messages"][1]["content"],
            json!(
                "LEVEL: A1\nSOURCE (English): \"I would like a coffee with milk.\"\nACCEPTED: [\"Quiero un café con leche.\", \"Quisiera un café con leche.\"]\nANSWER: \"Me gustaría una café con leche\""
            )
        );

        std::fs::write(&input, r#"{"lang": "it"}"#).unwrap();
        let (code, out, err) = run_capture(&["render", &path]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(
            err.starts_with(&format!("error: {}: ", shown(&path))),
            "{err}"
        );
    }
}
