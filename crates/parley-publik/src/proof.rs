//! `parley publik proof` (docs/PUBLIK-CONTRACT.md §11; `scripts/publik-proof`): one real P0 call,
//! for the maintainer only. Without `PUBLIK_PROOF=1` it sends nothing and exits 1. With it, it
//! resolves publik API (provisioning if it must: the person who runs it is the consent), makes one
//! connection check, and prints the reply, the charge line and every `x-publik-*` header, then the
//! ids `GET /models` lists, so the header names, the tier behaviour and the audio model ids can be
//! confirmed. It never prints the key.

use std::collections::BTreeMap;
use std::io::Write;
use std::time::Duration;

use parley_core::ai::{ChatRequest, format_dollars, ping};
use parley_core::env::HttpRequest;

use crate::errors::{body_json, error_message, reason, redact};
use crate::http::{self, header};
use crate::publik::{Provider, PublikEnv, provision_in, resolve_provider_in, top_up_url_from_402};

/// What the proof says without `PUBLIK_PROOF=1`.
pub const REFUSAL: &str = "This makes one real publik API call. Run it with PUBLIK_PROOF=1.";

/// Runs the proof with the process environment.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let proof = std::env::var("PUBLIK_PROOF").ok();
    if proof.as_deref() != Some("1") {
        let _ = writeln!(err, "{REFUSAL}");
        return 1;
    }
    run_in(
        &PublikEnv::from_process(),
        proof.as_deref(),
        parley_core::api::VERSION,
        args,
        out,
        err,
    )
}

/// The proof against `env`; `proof` is the value of `PUBLIK_PROOF`.
pub fn run_in(
    env: &PublikEnv,
    proof: Option<&str>,
    version: &str,
    args: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    if proof != Some("1") {
        let _ = writeln!(err, "{REFUSAL}");
        return 1;
    }
    if let Some(arg) = args.first() {
        let _ = writeln!(err, "error: unknown argument {arg}. Run parley --help.");
        return 2;
    }
    let (base_url, key) = match resolve_provider_in(env, None) {
        Provider::Publik {
            base_url, api_key, ..
        } => (base_url, api_key),
        _ => match provision_in(env, version, None) {
            Ok(credential) => (
                credential["base_url"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                credential["key"].as_str().unwrap_or_default().to_string(),
            ),
            Err(message) => {
                let _ = writeln!(err, "error: {message}");
                return 1;
            }
        },
    };
    let base_url = base_url.trim_end_matches('/').to_string();
    let prompt = ping();
    let messages = match prompt.render(&BTreeMap::new()) {
        Ok(messages) => messages,
        Err(e) => {
            let _ = writeln!(err, "error: {e}");
            return 1;
        }
    };
    let idempotency_key = uuid::Uuid::new_v4().simple().to_string();
    let chat = ChatRequest {
        model: prompt.tier.clone(),
        messages,
        max_tokens: prompt.max_tokens,
        temperature: prompt.temperature,
        timeout_s: prompt.timeout_s,
        idempotency_key: Some(idempotency_key.clone()),
    };
    let auth = ("authorization".to_string(), format!("Bearer {key}"));
    let request = HttpRequest {
        method: "POST".into(),
        url: format!("{base_url}/chat/completions"),
        headers: vec![
            auth.clone(),
            ("content-type".into(), "application/json".into()),
            ("x-publik-idempotency-key".into(), idempotency_key),
        ],
        body: chat.body_bytes(false),
    };
    let response = match http::send(&request, Duration::from_secs(chat.timeout_s)) {
        Ok(response) => response,
        Err(e) => {
            let _ = writeln!(err, "error: publik API did not answer: {e}");
            return 1;
        }
    };
    let body = body_json(&response.body);
    let _ = writeln!(out, "chat: {} {}", response.status, reason(response.status));
    if response.status == 200 {
        let content = body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default();
        let _ = writeln!(out, "reply: {}", redact(content, Some(&key)));
    } else if response.status == 402 {
        let (message, link) = top_up_url_from_402(&body);
        let _ = writeln!(out, "error: {}", redact(&message, Some(&key)));
        let _ = writeln!(
            out,
            "link: {}",
            redact(&link.unwrap_or_default(), Some(&key))
        );
    } else {
        let message = error_message(&body).unwrap_or_else(|| reason(response.status));
        let _ = writeln!(out, "error: {}", redact(&message, Some(&key)));
    }
    match header(&response.headers, "x-publik-charge-micros").and_then(|v| v.trim().parse().ok()) {
        Some(charge) => {
            let _ = writeln!(out, "charge: {} ({charge} micros)", format_dollars(charge));
        }
        None => {
            let _ = writeln!(out, "charge: none");
        }
    }
    let mut names: Vec<&(String, String)> = response
        .headers
        .iter()
        .filter(|(name, _)| name.starts_with("x-publik-"))
        .collect();
    names.sort();
    for (name, value) in names {
        let _ = writeln!(out, "{name}: {}", redact(value, Some(&key)));
    }
    let models = http::send(
        &HttpRequest {
            method: "GET".into(),
            url: format!("{base_url}/models"),
            headers: vec![auth],
            body: Vec::new(),
        },
        Duration::from_secs(15),
    );
    match models {
        Ok(listed) if listed.status == 200 => {
            let ids: Vec<String> = body_json(&listed.body)["data"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| m["id"].as_str().map(str::to_string))
                .collect();
            let _ = writeln!(out, "models: {}", ids.join(", "));
        }
        Ok(listed) => {
            let _ = writeln!(out, "models: {} {}", listed.status, reason(listed.status));
        }
        Err(e) => {
            let _ = writeln!(out, "models: no answer ({e})");
        }
    }
    if response.status == 200 { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::test_server::{Reply, serve};

    fn env(base_url: &str, dir: &std::path::Path) -> PublikEnv {
        PublikEnv {
            api_key: Some("pk_live_a".into()),
            base_url: Some(base_url.into()),
            app_token: None,
            credential_path: dir.join("parley.json"),
        }
    }

    fn capture(env: &PublikEnv, proof: Option<&str>) -> (u8, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_in(env, proof, "0.1.0", &[], &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn without_the_variable_it_sends_nothing() {
        let server = serve(vec![Reply::json(200, "{}")]);
        let dir = tempfile::tempdir().unwrap();
        for proof in [None, Some("0"), Some("yes")] {
            let (code, out, err) = capture(&env(&server.url("/api/v1"), dir.path()), proof);
            assert_eq!(code, 1);
            assert!(out.is_empty());
            assert_eq!(
                err,
                "This makes one real publik API call. Run it with PUBLIK_PROOF=1.\n"
            );
        }
        assert!(server.requests().is_empty());
    }

    #[test]
    fn with_it_one_check_prints_the_reply_charge_headers_and_models_but_never_the_key() {
        let server = serve(vec![
            Reply::json(
                200,
                r#"{"choices":[{"message":{"content":"{\"ok\": true}"}}]}"#,
            )
            .header("x-publik-charge-micros", "17")
            .header("x-publik-balance", "49983"),
            Reply::json(
                200,
                r#"{"object":"list","data":[{"id":"publik-fast"},{"id":"publik-balanced"}]}"#,
            ),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let (code, out, err) = capture(&env(&server.url("/api/v1"), dir.path()), Some("1"));
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("reply: {\"ok\": true}\n"));
        assert!(out.contains("charge: $0.000017 (17 micros)\n"));
        assert!(out.contains("x-publik-balance: 49983\n"));
        assert!(out.contains("models: publik-fast, publik-balanced\n"));
        assert!(!out.contains("pk_live_a") && !err.contains("pk_live_a"));
        let seen = server.requests();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].path, "/api/v1/chat/completions");
        assert_eq!(seen[1].path, "/api/v1/models");
    }
}
