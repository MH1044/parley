//! Acceptance tests of module 06 in the core (docs/modules/06-publik-and-providers.md): the
//! `ai::call` pipeline driven by a scripted `AiClient` (tests 11, 14 and 28), the prompt files and
//! their renderer (test 20), Check connection and the dollar format (test 24), the message table of
//! `AiError`, the copy rule (test 17) and SECURITY.md (test 27).
//!
//! Expected values come from the spec, docs/PUBLIK-INTEGRATION.md (§6.1 for the keys and the
//! renderer, §6.2 for P0, §8 for the rounding rule) and docs/PUBLIK-CONTRACT.md; the cache and
//! idempotency keys are computed here from their definition with SHA-256. No test reaches the
//! network: the scripted client answers from a queue and records every request.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use parley_core::ai::{
    self, AiClient, AiError, AiStatus, AudioReply, Cache, Called, ChatMessage, ChatReply,
    ChatRequest, ConnectionCheck, MAX_REQUEST_BYTES, Prompt, PromptError, ProviderKind,
    RenderError, RuleFailure, SpeechRequest, TranscribeRequest, TranscriptReply, cache_key,
    cache_key_json, canonical_json, format_dollars, idempotency_key, image_part, parse_reply,
};
use parley_core::env::{Clock, SplitMix64};
use parley_core::error::AppError;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const ECHO_HOLA: &str = r#"{"echo": "hola"}"#;
const P0_OK: &str = r#"{"ok": true}"#;
const INVALID: &str = "The AI answer could not be used.";

// ---------------------------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------------------------

/// A clock fixed at 2026-11-06T00:00:00Z.
struct FixedClock;

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        1_762_387_200_000
    }

    fn utc_offset_minutes(&self) -> i32 {
        0
    }
}

/// An `AiClient` that answers chat requests from a queue and records every request it is sent.
struct Scripted {
    kind: ProviderKind,
    /// The chat model of every tier; none means the tier itself, as on publik API.
    model: Option<String>,
    replies: Mutex<VecDeque<Result<ChatReply, AiError>>>,
    sent: Mutex<Vec<ChatRequest>>,
}

impl Scripted {
    fn new(kind: ProviderKind, model: Option<&str>) -> Scripted {
        Scripted {
            kind,
            model: model.map(str::to_string),
            replies: Mutex::new(VecDeque::new()),
            sent: Mutex::new(Vec::new()),
        }
    }

    /// Queues an answer saying `content`, with the charge a publik API header would carry.
    fn reply(&self, content: &str, charge_micros: Option<i64>) -> &Scripted {
        self.replies
            .lock()
            .expect("the reply queue")
            .push_back(Ok(ChatReply {
                content: content.to_string(),
                charge_micros,
                request_id: Some("req_test".into()),
            }));
        self
    }

    /// Queues a failure, as a transport error or an HTTP error would give.
    fn fail(&self, error: AiError) -> &Scripted {
        self.replies
            .lock()
            .expect("the reply queue")
            .push_back(Err(error));
        self
    }

    fn sent(&self) -> Vec<ChatRequest> {
        self.sent.lock().expect("the request log").clone()
    }

    /// The idempotency key of every request, in order.
    fn keys(&self) -> Vec<Option<String>> {
        self.sent()
            .into_iter()
            .map(|request| request.idempotency_key)
            .collect()
    }
}

impl AiClient for Scripted {
    fn chat(&self, req: ChatRequest) -> Result<ChatReply, AiError> {
        self.sent.lock().expect("the request log").push(req);
        self.replies
            .lock()
            .expect("the reply queue")
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
        AiStatus::default()
    }

    fn chat_model(&self, tier: &str) -> String {
        self.model.clone().unwrap_or_else(|| tier.to_string())
    }
}

/// The answer shape of the test_echo prompts.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct Echo {
    echo: String,
}

fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("a temporary directory")
}

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

fn read_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()))
}

fn prompts_dir() -> PathBuf {
    repo_root().join("tests").join("fixtures").join("prompts")
}

/// The test-only prompt `tests/fixtures/prompts/test_echo.v<version>.md` with its schema.
fn echo_prompt(version: u32) -> Prompt {
    let text = read_text(&prompts_dir().join(format!("test_echo.v{version}.md")));
    let schema = read_text(&prompts_dir().join(format!("test_echo.v{version}.schema.json")));
    Prompt::parse(&text, &schema).unwrap_or_else(|e| panic!("test_echo.v{version} parses: {e:?}"))
}

fn echo_vars(text: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("level".to_string(), "A1".to_string()),
        ("text_json".to_string(), text.to_string()),
    ])
}

fn accept(_: &Echo) -> Result<(), RuleFailure> {
    Ok(())
}

/// One call of a test_echo prompt with the learner text `text`, keyed by `{"text": text}`.
fn echo(
    client: &dyn AiClient,
    cache: &Cache,
    prompt: &Prompt,
    text: &str,
) -> Result<Called<Echo>, AiError> {
    ai::call::<Echo>(
        client,
        cache,
        prompt,
        &echo_vars(text),
        &json!({"text": text}),
        &accept,
    )
}

fn open_cache(path: &Path) -> Cache {
    Cache::open(path, Box::new(FixedClock)).expect("the AI cache opens")
}

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn first_32(text: &str) -> String {
    sha256_hex(text)[..32].to_string()
}

fn is_lower_hex_32(text: &str) -> bool {
    text.len() == 32
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// Sorted keys, no insignificant whitespace (the inputs here are ASCII, so already NFC).
fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let fields: Vec<String> = keys
                .iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        Value::String((*k).clone()),
                        canonical(&map[k.as_str()])
                    )
                })
                .collect();
            format!("{{{}}}", fields.join(","))
        }
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", items.join(","))
        }
        other => other.to_string(),
    }
}

/// The object whose canonical JSON is the cache key's JSON (docs/PUBLIK-INTEGRATION.md §6.1).
fn key_object(feature: &str, version: u32, provider: &str, model: &str, input: &Value) -> Value {
    json!({
        "feature": feature,
        "v": version,
        "provider": provider,
        "model": model,
        "input": input,
    })
}

/// The idempotency key of attempt `n` of a cache key object.
fn attempt_key(object: &Value, n: u32) -> String {
    if n == 1 {
        return first_32(&canonical(object));
    }
    let mut with_attempt = object.clone();
    with_attempt["attempt"] = json!(n);
    first_32(&canonical(&with_attempt))
}

/// The request body of docs/PUBLIK-INTEGRATION.md §6.2, word for word.
fn p0_body() -> Value {
    serde_json::from_str(r#"{"model":"publik-fast","max_tokens":10,"temperature":0,"messages":[{"role":"system","content":"You are a connectivity check. Reply with exactly this JSON and nothing else: {\"ok\": true}"},{"role":"user","content":"ping"}]}"#)
        .expect("the §6.2 body is JSON")
}

fn message(role: &str, content: &str) -> ChatMessage {
    ChatMessage {
        role: role.to_string(),
        content: json!(content),
    }
}

// ---------------------------------------------------------------------------------------------
// The error table, the provider kinds and the status shape.
// ---------------------------------------------------------------------------------------------

#[test]
fn ai_error_messages_codes_and_actions_follow_the_spec_table() {
    let local_down = "The local model server at http://localhost:11434/v1 is not running. Start it, or choose another provider.";
    let unreachable = "publik API is unreachable right now. Use your own key for now.";
    let cases: [(AiError, &str, &str, Option<Value>); 18] = [
        (
            AiError::NeedsConsent,
            "needs_consent",
            "AI features are not set up yet.",
            None,
        ),
        (
            AiError::NoToken,
            "no_provider",
            "This build has no publik API app token. Use your own key for now.",
            None,
        ),
        (
            AiError::OwnKeyMissing,
            "no_provider",
            "Add your own key in Settings first.",
            None,
        ),
        (
            AiError::NeedsCredit {
                message: "Your publik balance is $0.00.".into(),
                top_up_url: "https://publikhq.com/wallet/add-credit".into(),
            },
            "needs_credit",
            "Your publik balance is $0.00.",
            Some(json!({"top_up_url": "https://publikhq.com/wallet/add-credit"})),
        ),
        (
            AiError::Disconnected,
            "disconnected",
            "publik API is disconnected.",
            None,
        ),
        (
            AiError::RateLimited {
                message: "Slow down, please.".into(),
            },
            "rate_limited",
            "Slow down, please.",
            None,
        ),
        (
            AiError::Busy,
            "busy",
            "publik API is busy; try again in a moment",
            None,
        ),
        (
            AiError::Timeout {
                provider: ProviderKind::Publik,
            },
            "busy",
            "publik API did not answer in time.",
            None,
        ),
        (
            AiError::Timeout {
                provider: ProviderKind::OwnKey,
            },
            "busy",
            "Your provider did not answer in time.",
            None,
        ),
        (
            AiError::Timeout {
                provider: ProviderKind::Local,
            },
            "busy",
            "The local model did not answer in time.",
            None,
        ),
        (
            AiError::Offline,
            "offline",
            "You're offline. AI features need a connection or a local model.",
            None,
        ),
        (AiError::Invalid, "ai_invalid", INVALID, None),
        (
            AiError::Provider {
                provider: ProviderKind::Publik,
                code: 500,
                message: "upstream failed".into(),
            },
            "provider_error",
            "publik API 500: upstream failed",
            None,
        ),
        (
            AiError::Provider {
                provider: ProviderKind::OwnKey,
                code: 401,
                message: "Incorrect key".into(),
            },
            "provider_error",
            "Your provider returned 401: Incorrect key",
            None,
        ),
        (
            AiError::Provider {
                provider: ProviderKind::Local,
                code: 404,
                message: "model not found".into(),
            },
            "provider_error",
            "The local model returned 404: model not found",
            None,
        ),
        (
            AiError::LocalDown {
                base_url: "http://localhost:11434/v1".into(),
            },
            "provider_error",
            local_down,
            None,
        ),
        (
            AiError::Setup {
                message: unreachable.into(),
            },
            "provider_error",
            unreachable,
            None,
        ),
        (
            AiError::TooLarge,
            "file_too_large",
            "This request is larger than 4 MB, so it was not sent.",
            None,
        ),
    ];
    for (error, code, text, action) in cases {
        assert_eq!(error.message(), text, "{error:?}");
        let shown = AppError::from(error.clone());
        assert_eq!(shown.code.as_str(), code, "{error:?}");
        assert_eq!(shown.message, text, "{error:?}");
        assert_eq!(shown.action, action, "{error:?}");
    }
}

#[test]
fn provider_kinds_serialise_in_snake_case() {
    for (kind, text) in [
        (ProviderKind::Publik, "publik"),
        (ProviderKind::OwnKey, "own_key"),
        (ProviderKind::Local, "local"),
    ] {
        assert_eq!(kind.as_str(), text);
        assert_eq!(serde_json::to_value(kind).expect("serialises"), json!(text));
    }
}

#[test]
fn ai_status_has_the_spec_fields_and_no_key_field() {
    let value = serde_json::to_value(AiStatus::default()).expect("AiStatus serialises");
    let fields: BTreeSet<&str> = value
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        fields,
        BTreeSet::from([
            "provider",
            "needs_consent",
            "connected",
            "claim_state",
            "balance_micros",
            "balance_line",
            "cost_sentence",
            "claim_url",
            "add_credit_url",
            "top_up_url",
            "needs_credit",
            "last_charge_micros",
            "last_request_id",
            "starter_remaining_micros",
            "week_used_micros",
            "week_budget_micros",
            "week_resets_at",
            "own_key_stored",
            "local_model",
        ])
    );
    assert_eq!(value["claim_state"], Value::Null, "an empty Option is null");
    let back: AiStatus = serde_json::from_value(value).expect("AiStatus deserialises");
    assert_eq!(back, AiStatus::default());
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 4 (core half): the cache and idempotency keys.
// ---------------------------------------------------------------------------------------------

#[test]
fn t04_cache_and_idempotency_keys_follow_their_definition() {
    // The helper is SHA-256: the "abc" vector of FIPS 180-2.
    assert_eq!(
        sha256_hex("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        canonical_json(&json!({"b": "cafe\u{301}", "a": [1, {"d": true, "c": null}]})),
        "{\"a\":[1,{\"c\":null,\"d\":true}],\"b\":\"caf\u{e9}\"}",
        "sorted keys, no whitespace, NFC strings"
    );

    let input = json!({"text": "hola", "lang": "es"});
    let key_json = cache_key_json("judge", 1, ProviderKind::OwnKey, "gpt-own-mini", &input);
    assert_eq!(
        key_json,
        r#"{"feature":"judge","input":{"lang":"es","text":"hola"},"model":"gpt-own-mini","provider":"own_key","v":1}"#
    );
    let key = cache_key(&key_json);
    assert_eq!(key, sha256_hex(&key_json));
    assert_eq!(key.len(), 64);

    let attempt_2 = r#"{"attempt":2,"feature":"judge","input":{"lang":"es","text":"hola"},"model":"gpt-own-mini","provider":"own_key","v":1}"#;
    let attempt_3 = r#"{"attempt":3,"feature":"judge","input":{"lang":"es","text":"hola"},"model":"gpt-own-mini","provider":"own_key","v":1}"#;
    assert_eq!(idempotency_key(&key_json, 1), first_32(&key_json));
    assert_eq!(idempotency_key(&key_json, 2), first_32(attempt_2));
    assert_eq!(idempotency_key(&key_json, 3), first_32(attempt_3));
    for n in 1..=3 {
        assert!(is_lower_hex_32(&idempotency_key(&key_json, n)));
    }
    let object = key_object("judge", 1, "own_key", "gpt-own-mini", &input);
    assert_eq!(canonical(&object), key_json);
    assert_eq!(attempt_key(&object, 2), first_32(attempt_2));
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 11: the cache, with a scripted client.
// ---------------------------------------------------------------------------------------------

#[test]
fn t11_caches_are_per_provider_identical_input_never_spends_twice_and_a_version_bump_misses() {
    let dir = tempdir();
    let cache = open_cache(&dir.path().join("ai.sqlite3"));
    let publik = Scripted::new(ProviderKind::Publik, None);
    publik
        .reply(ECHO_HOLA, Some(17))
        .reply(ECHO_HOLA, Some(17))
        .reply(r#"{"echo": "adiós"}"#, Some(17));
    let own = Scripted::new(ProviderKind::OwnKey, Some("gpt-own-mini"));
    own.reply(ECHO_HOLA, None);
    let local = Scripted::new(ProviderKind::Local, Some("llama3.1:8b"));
    local.reply(ECHO_HOLA, None);
    let (v1, v2) = (echo_prompt(1), echo_prompt(2));
    let hola = Echo {
        echo: "hola".into(),
    };

    let first = echo(&publik, &cache, &v1, "hola").expect("answered");
    assert_eq!(
        first,
        Called {
            value: hola.clone(),
            cached: false,
            charge_micros: Some(17),
        }
    );
    let second = echo(&publik, &cache, &v1, "hola").expect("answered");
    assert!(second.cached, "identical input is served from the cache");
    assert_eq!(second.value, hola);
    assert_eq!(publik.sent().len(), 1, "identical input never spends twice");

    assert!(!echo(&own, &cache, &v1, "hola").expect("answered").cached);
    assert!(!echo(&local, &cache, &v1, "hola").expect("answered").cached);
    assert_eq!(cache.len(), 3, "one entry per provider");

    let bumped = echo(&publik, &cache, &v2, "hola").expect("answered");
    assert!(!bumped.cached, "a prompt version bump misses");
    assert_eq!(publik.sent().len(), 2);

    let other = echo(&publik, &cache, &v1, "adiós").expect("answered");
    assert!(!other.cached, "other input misses");
    assert_eq!(cache.len(), 5);

    for (version, kind, model) in [
        (1, ProviderKind::Publik, "publik-fast"),
        (1, ProviderKind::OwnKey, "gpt-own-mini"),
        (1, ProviderKind::Local, "llama3.1:8b"),
        (2, ProviderKind::Publik, "publik-fast"),
    ] {
        let expected = canonical(&key_object(
            "test_echo",
            version,
            kind.as_str(),
            model,
            &json!({"text": "hola"}),
        ));
        let key_json = cache_key_json("test_echo", version, kind, model, &json!({"text": "hola"}));
        assert_eq!(key_json, expected);
        let stored = cache
            .get(&sha256_hex(&expected))
            .unwrap_or_else(|| panic!("no entry for {} v{version}", kind.as_str()));
        let stored: Value = serde_json::from_str(&stored).expect("the stored answer is JSON");
        assert_eq!(stored, json!({"echo": "hola"}));
    }
    // The request's model is the client's model for the prompt's tier.
    assert_eq!(publik.sent()[0].model, "publik-fast");
    assert_eq!(own.sent()[0].model, "gpt-own-mini");
    assert_eq!(local.sent()[0].model, "llama3.1:8b");
}

#[test]
fn t11_a_session_cache_serves_identical_input_for_the_session_only() {
    let client = Scripted::new(ProviderKind::Publik, None);
    client.reply(ECHO_HOLA, Some(17)).reply(ECHO_HOLA, Some(17));
    let prompt = echo_prompt(1);
    let session = Cache::in_memory(Box::new(FixedClock));
    assert!(
        !echo(&client, &session, &prompt, "hola")
            .expect("answered")
            .cached
    );
    assert!(
        echo(&client, &session, &prompt, "hola")
            .expect("answered")
            .cached
    );
    assert_eq!(client.sent().len(), 1);
    assert_eq!(session.len(), 1);
    let next_session = Cache::in_memory(Box::new(FixedClock));
    assert!(
        !echo(&client, &next_session, &prompt, "hola")
            .expect("answered")
            .cached
    );
    assert_eq!(client.sent().len(), 2);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 14: an invalid answer is discarded.
// ---------------------------------------------------------------------------------------------

#[test]
fn t14_an_invalid_answer_is_discarded_with_the_message_and_never_cached() {
    let dir = tempdir();
    let cache = open_cache(&dir.path().join("ai.sqlite3"));
    let prompt = echo_prompt(1);
    let client = Scripted::new(ProviderKind::Publik, None);
    let invalid = [
        r#"{"echo": 5}"#,
        "There is no JSON in this answer.",
        r#"{"echo": ""}"#,
        r#"{"echo": "hola", "more": 1}"#,
        r#"{"echo": "hola""#,
    ];
    for content in invalid {
        client.reply(content, Some(17));
    }
    for content in invalid {
        let error = echo(&client, &cache, &prompt, "hola").expect_err(content);
        assert_eq!(error, AiError::Invalid, "{content}");
        assert_eq!(error.message(), INVALID);
        assert_eq!(AppError::from(error).code.as_str(), "ai_invalid");
    }
    assert_eq!(cache.len(), 0, "nothing invalid is cached");
    let key = cache_key(&cache_key_json(
        "test_echo",
        1,
        ProviderKind::Publik,
        "publik-fast",
        &json!({"text": "hola"}),
    ));
    assert_eq!(cache.attempts(&key), invalid.len() as u32);

    // A schema-valid answer that fails the feature's rules is discarded too.
    client.reply(ECHO_HOLA, Some(17));
    let reject = |answer: &Echo| -> Result<(), RuleFailure> {
        if answer.echo == "hola" {
            Err(RuleFailure {
                rule: "E1".into(),
                detail: "a rule of the test".into(),
            })
        } else {
            Ok(())
        }
    };
    let error = ai::call::<Echo>(
        &client,
        &cache,
        &prompt,
        &echo_vars("hola"),
        &json!({"text": "hola"}),
        &reject,
    )
    .expect_err("the rule fails");
    assert_eq!(error, AiError::Invalid);
    assert_eq!(cache.len(), 0);

    // Text around the one JSON object is tolerated: from the first { to the last }.
    client.reply(
        "Here it is: {\"echo\": \"hola\"} I hope that helps.",
        Some(17),
    );
    let called = echo(&client, &cache, &prompt, "hola").expect("the JSON inside is used");
    assert_eq!(called.value.echo, "hola");
    assert_eq!(cache.len(), 1);
}

#[test]
fn t14_parse_reply_takes_the_first_brace_to_the_last_and_nothing_else() {
    assert_eq!(parse_reply(P0_OK), Some(json!({"ok": true})));
    assert_eq!(
        parse_reply("```json\n{\"ok\": true}\n```"),
        Some(json!({"ok": true}))
    );
    assert_eq!(
        parse_reply("Sure. {\"a\": {\"b\": 1}} Done."),
        Some(json!({"a": {"b": 1}}))
    );
    assert_eq!(parse_reply("no JSON at all"), None);
    assert_eq!(parse_reply(r#"{"a": 1} and {"b": 2}"#), None);
    assert_eq!(parse_reply(r#"{"ok": tru"#), None);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 15 (core half): the limit and the image part.
// ---------------------------------------------------------------------------------------------

#[test]
fn t15_the_request_limit_and_the_image_content_part() {
    assert_eq!(MAX_REQUEST_BYTES, 4_000_000);
    assert_eq!(
        image_part(&[0xFF, 0xD8, 0xFF, 0xE0]),
        json!({"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,/9j/4A=="}})
    );
    // 3,000,000 bytes are 4,000,000 bytes of base64, so such a photo alone is over the limit.
    let jpeg = vec![0_u8; 3_000_000];
    let part = image_part(&jpeg);
    let url = part["image_url"]["url"].as_str().expect("a data URL");
    assert_eq!(url.len(), "data:image/jpeg;base64,".len() + 4_000_000);
    assert!(url.ends_with("AAAA"));
}

#[test]
fn chat_request_bodies_carry_the_four_fields_and_a_whole_temperature_as_an_integer() {
    let request = ChatRequest {
        model: "publik-balanced".into(),
        messages: vec![message("user", "hola")],
        max_tokens: 2000,
        temperature: 0.7,
        timeout_s: 45,
        idempotency_key: Some("0123456789abcdef0123456789abcdef".into()),
    };
    assert_eq!(
        request.body(false),
        json!({
            "model": "publik-balanced",
            "max_tokens": 2000,
            "temperature": 0.7,
            "messages": [{"role": "user", "content": "hola"}],
        })
    );
    assert_eq!(request.body(true)["stream"], true);
    assert!(request.body(false).get("stream").is_none());
    let whole = ChatRequest {
        temperature: 1.0,
        ..request.clone()
    };
    assert_eq!(whole.body(false)["temperature"], json!(1));
    assert!(whole.body(false)["temperature"].is_u64() || whole.body(false)["temperature"].is_i64());
    assert!(
        !serde_json::to_string(&request.body(true))
            .expect("serialises")
            .contains("0123456789abcdef"),
        "the idempotency key is a header, never the body"
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 17: the copy rule.
// ---------------------------------------------------------------------------------------------

/// Every file under `dir` (relative to `root`, forward slashes), walked when git is not there.
fn walk(root: &Path, dir: &Path, files: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, files);
        } else if let Ok(rel) = path.strip_prefix(root) {
            files.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// The files the copy rule reads: app/src/strings/en.json, README.md, content/ and docs/ except
/// docs/PUBLIK-CONTRACT.md, which quotes the phrases. Git decides which files exist for the
/// repository; without git the folders are walked.
fn copy_rule_files(root: &Path) -> Vec<String> {
    let listed = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            "app/src/strings/en.json",
            "README.md",
            "content",
            "docs",
        ])
        .current_dir(root)
        .output();
    let mut files: Vec<String> = match listed {
        Ok(out) if out.status.success() && !out.stdout.is_empty() => out
            .stdout
            .split(|b| *b == 0)
            .filter(|name| !name.is_empty())
            .map(|name| String::from_utf8_lossy(name).into_owned())
            .collect(),
        _ => {
            let mut files = vec![
                "app/src/strings/en.json".to_string(),
                "README.md".to_string(),
            ];
            walk(root, &root.join("content"), &mut files);
            walk(root, &root.join("docs"), &mut files);
            files
        }
    };
    files.retain(|file| file != "docs/PUBLIK-CONTRACT.md" && root.join(file).is_file());
    files.sort();
    files.dedup();
    files
}

#[test]
fn t17_the_copy_rule_talks_in_dollars() {
    // Built from parts, so this file does not quote the phrases it looks for.
    let banned: Vec<String> = [
        concat!("publik", " credits"),
        concat!("in", " credits"),
        concat!("Gemini", " credit"),
        concat!("ChatGPT", " credit"),
        concat!("OpenAI", " API"),
    ]
    .iter()
    .map(|phrase| phrase.to_lowercase())
    .collect();
    let root = repo_root();
    let files = copy_rule_files(&root);
    assert!(files.iter().any(|f| f == "app/src/strings/en.json"));
    assert!(files.iter().any(|f| f == "README.md"));
    assert!(files.iter().any(|f| f == "docs/PUBLIK-INTEGRATION.md"));
    assert!(files.iter().any(|f| f.starts_with("content/")));
    assert!(!files.iter().any(|f| f == "docs/PUBLIK-CONTRACT.md"));

    let mut found = Vec::new();
    for rel in &files {
        let bytes = fs::read(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let text = String::from_utf8_lossy(&bytes).to_lowercase();
        for phrase in &banned {
            if text.contains(phrase.as_str()) {
                found.push(format!("{rel}: \"{phrase}\""));
            }
        }
    }
    assert!(
        found.is_empty(),
        "talk in dollars (docs/PUBLIK-CONTRACT.md §9): {found:?}"
    );

    // The scan does catch a phrase, in any case.
    let planted = format!("Top up your {} today.", concat!("Publik", " Credits")).to_lowercase();
    assert!(
        banned
            .iter()
            .any(|phrase| planted.contains(phrase.as_str()))
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 20: prompt files and the renderer.
// ---------------------------------------------------------------------------------------------

#[test]
fn t20_the_test_prompts_parse_with_their_front_matter() {
    let v1 = echo_prompt(1);
    assert_eq!(v1.id, "test_echo");
    assert_eq!(v1.version, 1);
    assert_eq!(v1.tier, "publik-fast");
    assert_eq!(v1.max_tokens, 50);
    assert_eq!(v1.timeout_s, 1);
    assert_eq!(v1.temperature, 0.0, "temperature defaults to 0");
    assert_eq!(v1.schema, "tests/fixtures/prompts/test_echo.v1.schema.json");
    assert_eq!(
        v1.system,
        "You repeat text for a test. The level is {level}.\nReply with one JSON object and nothing else: {\"echo\": TEXT, unchanged}"
    );
    assert_eq!(v1.user, "TEXT: {text_json}");
    assert_eq!(echo_prompt(2).version, 2);

    assert!(v1.schema_valid(&json!({"echo": "hola"})));
    assert!(!v1.schema_valid(&json!({"echo": 5})));
    assert!(!v1.schema_valid(&json!({"echo": "hola", "more": 1})));
    assert!(!v1.schema_valid(&json!({})));

    let text = read_text(&prompts_dir().join("test_echo.v1.md"));
    let schema = read_text(&prompts_dir().join("test_echo.v1.schema.json"));
    assert!(matches!(
        Prompt::parse("## system\nhello\n## user\nworld\n", &schema),
        Err(PromptError::FrontMatter(_))
    ));
    assert!(matches!(
        Prompt::parse(&text.replace("tier: publik-fast", ""), &schema),
        Err(PromptError::FrontMatter(_))
    ));
    assert!(matches!(
        Prompt::parse(&text, "{not json"),
        Err(PromptError::Schema(_))
    ));
}

#[test]
fn t20_the_renderer_fails_on_a_missing_or_unused_placeholder() {
    let prompt = echo_prompt(1);
    let missing = BTreeMap::from([("level".to_string(), "A1".to_string())]);
    assert_eq!(
        prompt.render(&missing),
        Err(RenderError::Missing("text_json".into()))
    );
    let mut unused = echo_vars("hola");
    unused.insert("mood".into(), "happy".into());
    assert_eq!(
        prompt.render(&unused),
        Err(RenderError::Unused("mood".into()))
    );
}

#[test]
fn t20_learner_text_is_inserted_as_a_json_string_literal() {
    let prompt = echo_prompt(1);
    let learner = "He said \"hi\"\n{level} \\ café";
    let messages = prompt
        .render(&echo_vars(learner))
        .expect("every placeholder has a value");
    assert_eq!(
        messages,
        [
            message(
                "system",
                "You repeat text for a test. The level is A1.\nReply with one JSON object and nothing else: {\"echo\": TEXT, unchanged}"
            ),
            message("user", r#"TEXT: "He said \"hi\"\n{level} \\ café""#),
        ],
        "a _json value is a JSON string literal, other braces are text, and a value is never re-read"
    );
}

#[test]
fn t20_ping_is_the_p0_prompt_of_the_integration_doc() {
    let ping = ai::ping();
    assert_eq!(ping.id, "ping");
    assert_eq!(ping.version, 1);
    assert_eq!(ping.tier, "publik-fast");
    assert_eq!(ping.max_tokens, 10);
    assert_eq!(ping.timeout_s, 15);
    assert_eq!(ping.temperature, 0.0);
    assert_eq!(ping.schema, "schemas/ai/ping.v1.schema.json");
    assert_eq!(
        ping.render(&BTreeMap::new())
            .expect("ping has no placeholders"),
        [
            message(
                "system",
                "You are a connectivity check. Reply with exactly this JSON and nothing else: {\"ok\": true}"
            ),
            message("user", "ping"),
        ]
    );
    assert!(ping.schema_valid(&json!({"ok": true})));
    assert!(!ping.schema_valid(&json!({"ok": false})));
    assert!(!ping.schema_valid(&json!({"ok": true, "more": 1})));

    let root = repo_root();
    assert!(root.join("prompts").join("ping.v1.md").is_file());
    let schema: Value = serde_json::from_str(&read_text(
        &root.join("schemas").join("ai").join("ping.v1.schema.json"),
    ))
    .expect("the ping schema is JSON");
    let expected = json!({"type":"object","required":["ok"],"properties":{"ok":{"const":true}},"additionalProperties":false});
    for field in ["type", "required", "properties", "additionalProperties"] {
        assert_eq!(
            schema[field], expected[field],
            "ping.v1.schema.json {field}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 24: Check connection and the dollar format.
// ---------------------------------------------------------------------------------------------

#[test]
fn t24_check_connection_sends_the_p0_body_and_reports_the_measured_charge() {
    let publik = Scripted::new(ProviderKind::Publik, None);
    publik.reply(P0_OK, Some(17));
    let check = ai::check_connection(&publik, &mut SplitMix64::new(1)).expect("connected");
    assert_eq!(
        check,
        ConnectionCheck {
            ok: true,
            charge_micros: Some(17),
            message: "Connected. This check cost $0.000017.".into(),
        }
    );
    let sent = publik.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].body(false),
        p0_body(),
        "P0's body is §6.2's exactly"
    );
    assert_eq!(sent[0].max_tokens, 10);
    assert_eq!(sent[0].timeout_s, 15);
    assert!(is_lower_hex_32(
        sent[0].idempotency_key.as_deref().unwrap_or_default()
    ));

    let own = Scripted::new(ProviderKind::OwnKey, Some("gpt-own-mini"));
    own.reply(P0_OK, None);
    let check = ai::check_connection(&own, &mut SplitMix64::new(1)).expect("connected");
    assert_eq!(
        check,
        ConnectionCheck {
            ok: true,
            charge_micros: None,
            message: "Connected.".into(),
        }
    );
    assert_eq!(own.sent()[0].model, "gpt-own-mini");

    let wrong = Scripted::new(ProviderKind::Publik, None);
    wrong
        .reply(r#"{"ok": false}"#, Some(17))
        .fail(AiError::Busy);
    assert_eq!(
        ai::check_connection(&wrong, &mut SplitMix64::new(1)),
        Err(AiError::Invalid)
    );
    assert_eq!(
        ai::check_connection(&wrong, &mut SplitMix64::new(1)),
        Err(AiError::Busy)
    );
}

#[test]
fn t24_format_dollars_two_decimals_from_ten_cents_otherwise_two_significant_figures() {
    for (micros, text) in [
        (3_120_000, "$3.12"),
        (100_000, "$0.10"),
        (49_000, "$0.049"),
        (5_800, "$0.0058"),
        (17, "$0.000017"),
        (0, "$0.00"),
        (1_000_000, "$1.00"),
        (20_000_000, "$20.00"),
        (15_000, "$0.015"),
        (3_817, "$0.0038"),
        (125, "$0.00013"),
        (5_849, "$0.0058"),
        (5_850, "$0.0059"),
        (3_125_000, "$3.13"),
        (105_000, "$0.11"),
        (104_999, "$0.10"),
        (19, "$0.000019"),
        (87, "$0.000087"),
    ] {
        assert_eq!(format_dollars(micros), text, "{micros} micros");
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 27: SECURITY.md.
// ---------------------------------------------------------------------------------------------

#[test]
fn t27_security_md_names_device_name_and_every_endpoint_of_architecture_8() {
    let root = repo_root();
    let security = read_text(&root.join("SECURITY.md"));
    assert!(
        security.contains("device_name"),
        "SECURITY.md names the device_name field"
    );
    let architecture = read_text(&root.join("docs").join("ARCHITECTURE.md"));
    let start = architecture
        .find("## 8.")
        .expect("ARCHITECTURE.md has section 8");
    let end = architecture[start..]
        .find("\n## 9.")
        .map_or(architecture.len(), |at| start + at);
    let section = &architecture[start..end];
    for endpoint in [
        "/installs",
        "/installs/revoke",
        "/wallet",
        "/chat/completions",
        "/audio/transcriptions",
        "/audio/speech",
        "/models",
        "/api/tags",
    ] {
        assert!(
            section.contains(endpoint),
            "ARCHITECTURE §8 lists {endpoint}"
        );
        assert!(security.contains(endpoint), "SECURITY.md names {endpoint}");
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 28: idempotency.
// ---------------------------------------------------------------------------------------------

#[test]
fn t28_a_try_again_after_an_invalid_answer_sends_a_new_key_and_the_count_survives_a_restart() {
    let dir = tempdir();
    let path = dir.path().join("ai.sqlite3");
    let prompt = echo_prompt(1);
    let object = key_object(
        "test_echo",
        1,
        "publik",
        "publik-fast",
        &json!({"text": "hola"}),
    );
    let key = sha256_hex(&canonical(&object));
    let client = Scripted::new(ProviderKind::Publik, None);
    client
        .reply(r#"{"echo": 5}"#, Some(17))
        .reply(r#"{"echo": 5}"#, Some(17));
    {
        let cache = open_cache(&path);
        assert_eq!(cache.attempts(&key), 0);
        assert_eq!(
            echo(&client, &cache, &prompt, "hola").err(),
            Some(AiError::Invalid)
        );
        assert_eq!(cache.attempts(&key), 1);
        assert_eq!(
            echo(&client, &cache, &prompt, "hola").err(),
            Some(AiError::Invalid)
        );
        assert_eq!(cache.attempts(&key), 2);
    }

    // The app restarts: the count is in cache/ai.sqlite3.
    let cache = open_cache(&path);
    assert_eq!(cache.attempts(&key), 2, "the count survives a restart");
    client
        .fail(AiError::Timeout {
            provider: ProviderKind::Publik,
        })
        .reply(ECHO_HOLA, Some(17));
    assert_eq!(
        echo(&client, &cache, &prompt, "hola").err(),
        Some(AiError::Timeout {
            provider: ProviderKind::Publik
        })
    );
    assert_eq!(cache.attempts(&key), 2, "a timeout changes no count");
    let called = echo(&client, &cache, &prompt, "hola").expect("the valid answer");
    assert!(!called.cached);

    assert_eq!(
        client.keys(),
        [
            Some(attempt_key(&object, 1)),
            Some(attempt_key(&object, 2)),
            Some(attempt_key(&object, 3)),
            Some(attempt_key(&object, 3)),
        ],
        "attempt 1, then 2, then 3 after the restart, and 3 again after the timeout"
    );

    // The valid answer is stored under the attempt-free key, so it never spends again.
    let stored = cache.get(&key).expect("cached under the plain key");
    let stored: Value = serde_json::from_str(&stored).expect("the stored answer is JSON");
    assert_eq!(stored, json!({"echo": "hola"}));
    assert!(
        echo(&client, &cache, &prompt, "hola")
            .expect("cached")
            .cached
    );
    assert_eq!(client.sent().len(), 4);
}

#[test]
fn t28_two_p0_clicks_send_two_different_random_keys() {
    let client = Scripted::new(ProviderKind::Publik, None);
    client
        .reply(P0_OK, Some(17))
        .reply(P0_OK, Some(17))
        .reply(P0_OK, Some(17));
    let mut rng = SplitMix64::new(7);
    ai::check_connection(&client, &mut rng).expect("connected");
    ai::check_connection(&client, &mut rng).expect("connected");
    let keys: Vec<String> = client
        .keys()
        .into_iter()
        .map(|key| key.expect("P0 sends an idempotency key"))
        .collect();
    assert!(keys.iter().all(|key| is_lower_hex_32(key)), "{keys:?}");
    assert_ne!(keys[0], keys[1], "a new key on every click");

    // The key comes from the injected randomness: the same seed gives the same key.
    ai::check_connection(&client, &mut SplitMix64::new(7)).expect("connected");
    assert_eq!(client.keys()[2].as_deref(), Some(keys[0].as_str()));
}
