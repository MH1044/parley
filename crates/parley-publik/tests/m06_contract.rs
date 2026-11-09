//! Acceptance tests of module 06, publik API and providers (docs/modules/06-publik-and-providers.md):
//! the contract tests of docs/PUBLIK-CONTRACT.md §11 with Parley's adaptations
//! (docs/PUBLIK-INTEGRATION.md §2, §3, §6 and §9).
//!
//! All HTTP goes to the fake server of `fake.rs` on 127.0.0.1; no test reaches publikhq.com. The
//! service is built with a temporary data directory and credential path, a keychain fake, seeded
//! randomness and no retry back-off, and its environment is passed as a `PublikEnv`, never read
//! from the process. Expected values come from the spec, the contract, the integration doc and the
//! reference module `publik.rs.md`; the idempotency and cache keys are computed here from their
//! definition with SHA-256. Each test names the acceptance test it covers.

mod fake;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use fake::{Fake, Reply, closed_url, repo_root};
use parley_core::ai::{
    self, AiClient, AiError, AiStatus, Cache, Called, ChatMessage, ChatRequest, ConnectionCheck,
    LocalModel, MAX_REQUEST_BYTES, Prompt, ProviderKind, RuleFailure, SpeechRequest,
    TranscribeRequest, format_dollars, image_part,
};
use parley_core::env::{Clock, SplitMix64};
use parley_core::error::AppError;
use parley_core::settings::{AiSettings, LocalSettings, OwnKeySettings, Settings};
use parley_publik::keychain::{Keychain, MemoryKeychain};
use parley_publik::publik::{self, DISCLOSURE, Provider, PublikEnv};
use parley_publik::service::{AiService, AiServiceConfig, OwnKeyInput};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const KEY: &str = "pk_live_a";
const OWN_KEY: &str = "sk-own-a";
const TOKEN: &str = "pat_parley_test";
const OWN_MODEL: &str = "gpt-own-mini";
const LOCAL_LARGE: &str = "llama3.1:8b";
const LOCAL_SMALL: &str = "llama3.2:3b";
const INSTALL_ID: &str = "5f0c8a52-6e0b-4c55-9a3e-2a3f1d9b7c11";
const CHAT: &str = "/chat/completions";
const OWN_CHAT: &str = "/v1/chat/completions";

// The fields of tests/fixtures/http/201-installs.json and wallet.json.
const CLAIM_URL: &str = "https://publikhq.com/claim/5f0c8a52";
const ADD_CREDIT_URL: &str = "https://publikhq.com/wallet/add-credit";
const WALLET_TOP_UP_URL: &str = "https://publikhq.com/claim/5f0c8a52?from=wallet";
const REFRESHED_TOP_UP_URL: &str = "https://publikhq.com/wallet/top-up";
const COST_SENTENCE: &str =
    "A typical use costs about $0.01 on publik-balanced and $0.001 on publik-fast.";

// The bodies of tests/fixtures/http/402-anonymous.json, 402-claimed.json and 429.json.
const ANONYMOUS_402_MESSAGE: &str =
    "This computer has $0.00 of publik balance. Link it to a publik account for $0.05 of free use.";
const ANONYMOUS_402_URL: &str = "https://publikhq.com/claim/5f0c8a52?from=402";
const CLAIMED_402_MESSAGE: &str =
    "Your publik balance is $0.00. Add a plan or pack to keep using AI features.";
const CLAIMED_402_URL: &str = "https://publikhq.com/wallet/add-credit?from=402";
const RATE_MESSAGE: &str = "Too many requests from this computer. Wait a minute, then try again.";

// The spec's messages.
const NO_TOKEN: &str = "This build has no publik API app token. Use your own key for now.";
const INVALID: &str = "The AI answer could not be used.";
const BUSY: &str = "publik API is busy; try again in a moment";
const DISCONNECTED: &str = "publik API is disconnected.";
const NOT_LOCAL: &str = "The local model server must be on this computer: use a localhost address.";
const BASE_URL_MESSAGE: &str =
    "Enter the base URL of an OpenAI-compatible API, starting with https:// or http://.";
const NOT_AVAILABLE: &str = "This link is not available yet.";
const FRESH_LINE: &str = "$0.00 · link this computer for $0.05 of free use";

/// `DISCLOSURE` of the reference module `publik.rs.md`, word for word.
const REFERENCE_DISCLOSURE: &str = "This app can use publik API for its AI features: no key to paste, priced per use in dollars from a publik balance that starts at $0.00. Linking this computer to a publik account gives $0.05 of free use, once. Your prompts go through publik's servers to the model; you can switch to your own key at any time.";

/// The data-path sentence of docs/PUBLIK-INTEGRATION.md §4.
const DATA_PATH: &str = "Parley sends only what the AI feature you use needs through publik's servers: the lesson topic and the words you know, the exercise and your answer, your conversation turn, the sentence you just recorded, the photo you picked, or the audio of a file you chose. Your progress, reviews and other recordings stay on this computer.";

const ECHO_HOLA: &str = r#"{"echo": "hola"}"#;
const P0_OK: &str = r#"{"ok": true}"#;

/// A test prompt of tier publik-smart (test 13), with the placeholders of test_echo.
const SMART_PROMPT: &str = r#"---
id: test_smart
version: 1
tier: publik-smart
max_tokens: 50
timeout_s: 1
schema: tests/fixtures/prompts/test_echo.v1.schema.json
---
## system
You repeat text for a test. The level is {level}.
Reply with one JSON object and nothing else: {"echo": TEXT, unchanged}
## user
TEXT: {text_json}
"#;

const TRANSCRIBE_MODEL: &str = "gpt-4o-mini-transcribe";
const SPEECH_MODEL: &str = "gpt-4o-mini-tts";
const AUDIO_KEY: &str = "0123456789abcdef0123456789abcdef";

macro_rules! json_of {
    ($value:expr) => {
        serde_json::to_string(&$value).expect("serialises")
    };
}

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

fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("a temporary directory")
}

fn read_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()))
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&read_text(path))
        .unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
}

fn write_json(path: &Path, value: &Value) {
    fs::create_dir_all(path.parent().expect("a parent folder")).expect("the folder is created");
    fs::write(path, serde_json::to_string_pretty(value).expect("JSON"))
        .expect("the file is written");
}

/// Where a test keeps the Publik credential: `<dir>/publik/apps/parley.json`.
fn credential_path(dir: &Path) -> PathBuf {
    dir.join("publik").join("apps").join("parley.json")
}

/// A `PublikEnv` with the test app token.
fn penv(api_key: Option<&str>, base_url: Option<&str>, credential_path: &Path) -> PublikEnv {
    PublikEnv {
        api_key: api_key.map(str::to_string),
        base_url: base_url.map(str::to_string),
        app_token: Some(TOKEN.to_string()),
        credential_path: credential_path.to_path_buf(),
    }
}

/// A credential file as `provision()` writes it from a 201 (the contract's fields).
fn credential(base_url: &str, claim_state: &str, balance_micros: i64) -> Value {
    json!({
        "key": KEY,
        "base_url": base_url,
        "install_id": INSTALL_ID,
        "claim_url": CLAIM_URL,
        "claim_state": claim_state,
        "add_credit_url": ADD_CREDIT_URL,
        "cost_sentence": COST_SENTENCE,
        "balance_micros": balance_micros,
    })
}

/// A `POST /installs` replay body (status 200) with this key (null or a string).
fn replay_body(base_url: &str, key: Value) -> Value {
    json!({
        "key": key,
        "key_id": "key_a",
        "base_url": base_url,
        "install_id": INSTALL_ID,
        "claim_url": CLAIM_URL,
        "claim_state": "anonymous",
        "balance_micros": 0,
        "wallet": {
            "balance_micros": 0,
            "add_credit_url": ADD_CREDIT_URL,
            "top_up_url": WALLET_TOP_UP_URL,
            "claim_state": "anonymous",
        },
        "disclosure": {"version": 1, "cost": COST_SENTENCE, "data_path": "Your prompts go through publik's servers."},
    })
}

fn publik_settings() -> AiSettings {
    AiSettings {
        provider: "publik".into(),
        ..AiSettings::default()
    }
}

fn own_settings(base_url: &str) -> AiSettings {
    AiSettings {
        provider: "own_key".into(),
        own_key: OwnKeySettings {
            base_url: Some(base_url.to_string()),
            chat_model: Some(OWN_MODEL.to_string()),
            ..OwnKeySettings::default()
        },
        ..AiSettings::default()
    }
}

fn local_settings(base_url: &str, chat_model: Option<&str>) -> AiSettings {
    AiSettings {
        provider: "local".into(),
        local: LocalSettings {
            base_url: base_url.to_string(),
            chat_model: chat_model.map(str::to_string),
        },
        ..AiSettings::default()
    }
}

fn own_input(base_url: &str) -> OwnKeyInput {
    OwnKeyInput {
        base_url: base_url.to_string(),
        key: OWN_KEY.to_string(),
        chat_model: OWN_MODEL.to_string(),
        transcribe_model: Some("whisper-own".into()),
        speech_model: Some("tts-own".into()),
        speech_voice: Some("alloy".into()),
    }
}

fn new_service(dir: &Path, publik: PublikEnv, keychain: &MemoryKeychain) -> AiService {
    new_service_with(dir, publik, Box::new(keychain.clone()))
}

fn new_service_with(dir: &Path, publik: PublikEnv, keychain: Box<dyn Keychain>) -> AiService {
    AiService::new(AiServiceConfig {
        data_dir: dir.to_path_buf(),
        publik,
        keychain,
        rng: Box::new(SplitMix64::new(6)),
        backoff: [Duration::ZERO; 2],
        app_version: "0.1.0".into(),
    })
}

/// A keychain double: a memory slot that can refuse to store or to remove the key, and that counts
/// how often the key is read.
#[derive(Clone, Default)]
struct DoubleKeychain {
    slot: MemoryKeychain,
    refuse_set: bool,
    refuse_delete: bool,
    reads: Arc<AtomicUsize>,
}

impl DoubleKeychain {
    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl Keychain for DoubleKeychain {
    fn get(&self) -> Result<Option<String>, String> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.slot.get()
    }

    fn set(&self, secret: &str) -> Result<(), String> {
        if self.refuse_set {
            return Err("the keychain is locked".into());
        }
        self.slot.set(secret)
    }

    fn delete(&self) -> Result<(), String> {
        if self.refuse_delete {
            return Err("the keychain is locked".into());
        }
        self.slot.delete()
    }
}

/// A service with its own data directory, credential path and keychain fake. Its
/// `PUBLIK_API_BASE_URL` is a fake's address, so any provisioning reaches the fake.
struct Rig {
    dir: tempfile::TempDir,
    keychain: MemoryKeychain,
    service: AiService,
    base_url: String,
}

impl Rig {
    fn new(base_url: &str) -> Rig {
        let dir = tempdir();
        let keychain = MemoryKeychain::default();
        let env = penv(None, Some(base_url), &credential_path(dir.path()));
        let service = new_service(dir.path(), env, &keychain);
        Rig {
            dir,
            keychain,
            service,
            base_url: base_url.to_string(),
        }
    }

    /// A service started with a credential file whose key is `pk_live_a` and whose base URL is
    /// `base_url`.
    fn with_credential(base_url: &str, claim_state: &str, balance_micros: i64) -> Rig {
        let mut rig = Rig::new(base_url);
        rig.write_credential(&credential(base_url, claim_state, balance_micros));
        rig.restart();
        rig
    }

    /// Stores `sk-own-a` in the keychain fake and starts the service again.
    fn store_own_key(&mut self) {
        self.keychain
            .set(OWN_KEY)
            .expect("the keychain fake stores");
        self.restart();
    }

    /// A new service over the same data directory, credential and keychain: the app restarted.
    fn restart(&mut self) {
        let env = penv(
            None,
            Some(&self.base_url),
            &credential_path(self.dir.path()),
        );
        self.service = new_service(self.dir.path(), env, &self.keychain);
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn credential_path(&self) -> PathBuf {
        credential_path(self.path())
    }

    fn write_credential(&self, value: &Value) {
        write_json(&self.credential_path(), value);
    }

    fn credential(&self) -> Value {
        read_json(&self.credential_path())
    }

    fn cache(&self) -> Cache {
        Cache::open(&self.path().join("ai.sqlite3"), Box::new(FixedClock))
            .expect("the AI cache opens")
    }

    fn client(&mut self, ai: &AiSettings) -> Arc<dyn AiClient> {
        self.service
            .client(ai)
            .unwrap_or_else(|e| panic!("a client for {}: {e:?}", ai.provider))
    }
}

/// The test-only prompt `tests/fixtures/prompts/test_echo.v<version>.md` with its schema.
fn echo_prompt(version: u32) -> Prompt {
    let dir = repo_root().join("tests").join("fixtures").join("prompts");
    let text = read_text(&dir.join(format!("test_echo.v{version}.md")));
    let schema = read_text(&dir.join(format!("test_echo.v{version}.schema.json")));
    Prompt::parse(&text, &schema).unwrap_or_else(|e| panic!("test_echo.v{version} parses: {e:?}"))
}

fn smart_prompt() -> Prompt {
    let schema = read_text(
        &repo_root()
            .join("tests")
            .join("fixtures")
            .join("prompts")
            .join("test_echo.v1.schema.json"),
    );
    Prompt::parse(SMART_PROMPT, &schema).expect("the publik-smart test prompt parses")
}

fn echo_vars(text: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("level".to_string(), "A1".to_string()),
        ("text_json".to_string(), text.to_string()),
    ])
}

fn no_rules(_: &Value) -> Result<(), RuleFailure> {
    Ok(())
}

/// One call of a test_echo prompt with the learner text `text`, keyed by `{"text": text}`.
fn echo(
    client: &dyn AiClient,
    cache: &Cache,
    prompt: &Prompt,
    text: &str,
) -> Result<Called<Value>, AiError> {
    ai::call::<Value>(
        client,
        cache,
        prompt,
        &echo_vars(text),
        &json!({"text": text}),
        &no_rules,
    )
}

/// A chat request of one user message.
fn chat_request(model: &str, timeout_s: u64) -> ChatRequest {
    ChatRequest {
        model: model.to_string(),
        messages: vec![ChatMessage {
            role: "user".into(),
            content: json!("ping"),
        }],
        max_tokens: 10,
        temperature: 0.0,
        timeout_s,
        idempotency_key: Some(AUDIO_KEY.to_string()),
    }
}

/// A chat request whose body, as `ChatRequest::body(false)` serialises it, is exactly `bytes`.
fn sized_request(bytes: usize) -> ChatRequest {
    let mut request = chat_request("publik-fast", 5);
    request.messages[0].content = json!("");
    let empty = serde_json::to_vec(&request.body(false))
        .expect("the body serialises")
        .len();
    request.messages[0].content = json!("x".repeat(bytes - empty));
    let size = serde_json::to_vec(&request.body(false))
        .expect("the body serialises")
        .len();
    assert_eq!(size, bytes, "the sized request body");
    request
}

/// A 16 kHz mono 16-bit WAV of 16 silent samples.
fn small_wav() -> Vec<u8> {
    let data_len: u32 = 32;
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&16_000u32.to_le_bytes());
    wav.extend_from_slice(&32_000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.resize(wav.len() + data_len as usize, 0);
    wav
}

fn transcribe_request() -> TranscribeRequest {
    TranscribeRequest {
        model: TRANSCRIBE_MODEL.into(),
        audio: small_wav(),
        file_name: "take.wav".into(),
        mime: "audio/wav".into(),
        language: "es".into(),
        response_format: "json".into(),
        timeout_s: 5,
        idempotency_key: Some(AUDIO_KEY.into()),
    }
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

/// The cache key's JSON of docs/PUBLIK-INTEGRATION.md §6.1.
fn key_json(feature: &str, version: u32, provider: &str, model: &str, input: &Value) -> String {
    canonical(&json!({
        "feature": feature,
        "v": version,
        "provider": provider,
        "model": model,
        "input": input,
    }))
}

/// The cache key's JSON with one more field, `"attempt": n`: the input of the idempotency key of a
/// Try again after an answer failed validation (docs/PUBLIK-INTEGRATION.md §6.1).
fn attempt_key_json(
    feature: &str,
    version: u32,
    provider: &str,
    model: &str,
    input: &Value,
    attempt: u32,
) -> String {
    canonical(&json!({
        "attempt": attempt,
        "feature": feature,
        "v": version,
        "provider": provider,
        "model": model,
        "input": input,
    }))
}

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The idempotency key of attempt 1: the first 32 hex of the SHA-256 of the cache key's JSON.
fn first_32(text: &str) -> String {
    sha256_hex(text)[..32].to_string()
}

fn is_lower_hex_32(text: &str) -> bool {
    text.len() == 32
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

fn is_uuid_v4(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            14 => c == '4',
            _ => c.is_ascii_digit() || ('a'..='f').contains(&c),
        })
}

/// The `os` the reference module sends.
fn expected_os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

fn expected_device_label() -> &'static str {
    if cfg!(target_os = "android") {
        "Android phone"
    } else if cfg!(target_os = "macos") {
        "Mac"
    } else if cfg!(windows) {
        "Windows PC"
    } else {
        "Linux PC"
    }
}

/// The request body of docs/PUBLIK-INTEGRATION.md §6.2, word for word.
fn p0_body() -> Value {
    serde_json::from_str(r#"{"model":"publik-fast","max_tokens":10,"temperature":0,"messages":[{"role":"system","content":"You are a connectivity check. Reply with exactly this JSON and nothing else: {\"ok\": true}"},{"role":"user","content":"ping"}]}"#)
        .expect("the §6.2 body is JSON")
}

/// The install ids of every `POST /installs` a fake received.
fn install_ids(fake: &Fake) -> Vec<String> {
    fake.requests_to("POST", "/installs")
        .iter()
        .map(|r| {
            r.json()["install_id"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

fn refused<T: std::fmt::Debug>(result: Result<T, AppError>, code: &str, message: &str) {
    let error = result.expect_err(message);
    assert_eq!(
        (error.code.as_str(), error.message.as_str()),
        (code, message)
    );
}

/// Scripts the local model server of test 22: `GET /v1/models` in the OpenAI list shape and
/// Ollama's `GET /api/tags` with sizes.
fn script_local_models(local: &Fake) {
    local.on(
        "GET",
        "/v1/models",
        Reply::json(
            200,
            &json!({"object": "list", "data": [
                {"id": LOCAL_SMALL, "object": "model", "owned_by": "library"},
                {"id": "nomic-embed-text", "object": "model", "owned_by": "library"},
                {"id": LOCAL_LARGE, "object": "model", "owned_by": "library"},
            ]}),
        ),
    );
    local.on(
        "GET",
        "/api/tags",
        Reply::json(
            200,
            &json!({"models": [
                {"name": LOCAL_SMALL, "model": LOCAL_SMALL, "size": 2_000_000_000_u64},
                {"name": "nomic-embed-text", "model": "nomic-embed-text", "size": 274_000_000_u64},
                {"name": LOCAL_LARGE, "model": LOCAL_LARGE, "size": 4_700_000_000_u64},
            ]}),
        ),
    );
}

/// Every text an error shows or carries: its `Debug` and `message()`, and the `AppError` it
/// becomes, displayed and serialised.
fn record_error(seen: &mut Vec<String>, error: AiError) {
    seen.push(format!("{error:?}"));
    seen.push(error.message());
    record_app_error(seen, &AppError::from(error));
}

fn record_app_error(seen: &mut Vec<String>, error: &AppError) {
    seen.push(error.to_string());
    seen.push(json_of!(error));
}

// ---------------------------------------------------------------------------------------------
// The reference module's constants.
// ---------------------------------------------------------------------------------------------

#[test]
fn the_reference_constants_are_kept_with_parley_s_slug() {
    assert_eq!(publik::APP_SLUG, "parley");
    assert_eq!(publik::DEFAULT_BASE_URL, "https://publikhq.com/api/v1");
    assert_eq!(
        publik::MODELS,
        ["publik-fast", "publik-balanced", "publik-smart"]
    );
    assert_eq!(DISCLOSURE, REFERENCE_DISCLOSURE, "DISCLOSURE is verbatim");
    let path = publik::credential_path();
    assert!(
        path.ends_with(Path::new("publik").join("apps").join("parley.json")),
        "{}",
        path.display()
    );
    assert_eq!(PublikEnv::from_process().credential_path, path);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 1: resolution.
// ---------------------------------------------------------------------------------------------

#[test]
fn t01_resolution_own_key_beats_env_beats_credential_file_and_none_needs_consent() {
    let dir = tempdir();
    let path = credential_path(dir.path());
    write_json(
        &path,
        &credential("http://127.0.0.1:9/from-file", "anonymous", 0),
    );
    let own = Some(("https://own.example/v1".to_string(), OWN_KEY.to_string()));
    let with_env = penv(
        Some("pk_live_env"),
        Some("http://127.0.0.1:9/from-env"),
        &path,
    );

    match publik::resolve_provider_in(&with_env, own) {
        Provider::Own { base_url, api_key } => {
            assert_eq!(base_url, "https://own.example/v1");
            assert_eq!(api_key, OWN_KEY);
        }
        _ => panic!("the own key comes first"),
    }
    match publik::resolve_provider_in(
        &with_env,
        Some(("https://own.example/v1".to_string(), String::new())),
    ) {
        Provider::Publik { api_key, .. } => assert_eq!(api_key, "pk_live_env"),
        _ => panic!("an empty own key is no own key"),
    }
    match publik::resolve_provider_in(&with_env, None) {
        Provider::Publik {
            base_url,
            api_key,
            credential,
        } => {
            assert_eq!(base_url, "http://127.0.0.1:9/from-env");
            assert_eq!(api_key, "pk_live_env");
            assert!(
                credential.is_none(),
                "PUBLIK_API_KEY beats the credential file"
            );
        }
        _ => panic!("PUBLIK_API_KEY comes second"),
    }
    match publik::resolve_provider_in(&penv(Some("pk_live_env"), None, &path), None) {
        Provider::Publik { base_url, .. } => assert_eq!(base_url, publik::DEFAULT_BASE_URL),
        _ => panic!("PUBLIK_API_KEY without a base URL uses the default"),
    }
    match publik::resolve_provider_in(&penv(None, None, &path), None) {
        Provider::Publik {
            base_url,
            api_key,
            credential,
        } => {
            assert_eq!(base_url, "http://127.0.0.1:9/from-file");
            assert_eq!(api_key, KEY);
            assert_eq!(
                credential.expect("the credential comes with it")["install_id"],
                INSTALL_ID
            );
        }
        _ => panic!("the credential file comes third"),
    }

    let empty = tempdir();
    assert!(matches!(
        publik::resolve_provider_in(&penv(None, None, &credential_path(empty.path())), None),
        Provider::NeedsConsent
    ));
    // A credential whose key is not a pk_ key is no credential.
    write_json(
        &path,
        &json!({"key": "not-a-key", "base_url": "http://127.0.0.1:9/from-file", "install_id": INSTALL_ID}),
    );
    assert!(matches!(
        publik::resolve_provider_in(&penv(None, None, &path), None),
        Provider::NeedsConsent
    ));
}

#[test]
fn t01_the_service_resolves_by_the_chosen_provider() {
    let fake = Fake::start();
    let url = fake.url();
    let mut rig = Rig::new(&url);
    let unset = AiSettings::default();
    let own = own_settings(&format!("{url}/v1"));

    let status = rig.service.status(&unset, false);
    assert_eq!(status.provider, "unset");
    assert!(
        status.needs_consent,
        "nothing resolves, so consent is needed"
    );
    assert_eq!(
        rig.service.client(&unset).err(),
        Some(AiError::NeedsConsent)
    );
    assert_eq!(
        rig.service.client(&publik_settings()).err(),
        Some(AiError::NeedsConsent)
    );
    assert_eq!(rig.service.client(&own).err(), Some(AiError::OwnKeyMissing));
    assert!(!rig.service.status(&own, false).needs_consent);

    rig.write_credential(&credential(&url, "anonymous", 0));
    rig.restart();
    assert!(!rig.service.status(&unset, false).needs_consent);
    assert_eq!(rig.client(&unset).kind(), ProviderKind::Publik);
    assert_eq!(rig.client(&publik_settings()).kind(), ProviderKind::Publik);

    rig.store_own_key();
    assert_eq!(rig.client(&own).kind(), ProviderKind::OwnKey);
    assert_eq!(
        rig.client(&publik_settings()).kind(),
        ProviderKind::Publik,
        "a stored own key is never used while publik API is chosen"
    );
    assert_eq!(fake.count(), 0, "resolving sends nothing");
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 2: provisioning.
// ---------------------------------------------------------------------------------------------

#[test]
fn t02_provision_sends_the_contract_body_and_writes_the_credential_before_returning() {
    let fake = Fake::start();
    let url = fake.url();
    fake.on(
        "POST",
        "/installs",
        Reply::fixture("http/201-installs.json", &url),
    );
    let dir = tempdir();
    let env = penv(None, Some(&url), &credential_path(dir.path()));
    let returned = publik::provision_in(&env, "0.1.0", None).expect("a 201 provisions");

    let requests = fake.requests();
    assert_eq!(requests.len(), 1, "one POST /installs");
    let request = &requests[0];
    assert_eq!(
        (request.method.as_str(), request.path.as_str()),
        ("POST", "/installs")
    );
    assert_eq!(
        request.header("authorization"),
        None,
        "provisioning carries no key"
    );
    let body = request.json();
    let fields: BTreeSet<&str> = body
        .as_object()
        .expect("a JSON object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        fields,
        BTreeSet::from([
            "app_token",
            "app_slug",
            "app_version",
            "os",
            "arch",
            "install_id",
            "disclosure_version",
            "dialects",
        ]),
        "the contract §5 body, with no device_name"
    );
    assert_eq!(body["app_token"], TOKEN);
    assert_eq!(body["app_slug"], "parley");
    assert_eq!(body["app_version"], "0.1.0");
    assert_eq!(body["os"], expected_os());
    assert_eq!(body["arch"], std::env::consts::ARCH);
    assert_eq!(body["disclosure_version"], 1);
    assert_eq!(body["dialects"], json!(["chat_completions"]));
    assert!(
        is_uuid_v4(body["install_id"].as_str().unwrap_or_default()),
        "a fresh install id is a UUID v4: {}",
        body["install_id"]
    );

    // The credential is on disk when provision_in returns.
    let file = read_json(&env.credential_path);
    for (field, expected) in [
        ("key", json!(KEY)),
        ("base_url", json!(url)),
        ("install_id", json!(INSTALL_ID)),
        ("claim_url", json!(CLAIM_URL)),
        ("claim_state", json!("anonymous")),
        ("add_credit_url", json!(ADD_CREDIT_URL)),
        ("cost_sentence", json!(COST_SENTENCE)),
        ("balance_micros", json!(0)),
    ] {
        assert_eq!(file[field], expected, "credential field {field}");
        assert_eq!(returned[field], expected, "returned field {field}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&env.credential_path)
            .expect("the credential file")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "the credential is mode 0600");
    }
}

#[test]
fn t02_device_name_is_sent_only_when_shared_and_then_as_the_generic_label() {
    let label = expected_device_label();
    assert_eq!(publik::device_label(), label);

    let fake = Fake::start();
    fake.on(
        "POST",
        "/installs",
        Reply::fixture("http/201-installs.json", &fake.url()),
    );
    let dir = tempdir();
    let env = penv(None, Some(&fake.url()), &credential_path(dir.path()));
    publik::provision_in(&env, "0.1.0", Some(publik::device_label())).expect("provisions");
    assert_eq!(fake.requests()[0].json()["device_name"], label);

    for (share, sent) in [(false, None), (true, Some(label))] {
        let fake = Fake::start();
        fake.on(
            "POST",
            "/installs",
            Reply::fixture("http/201-installs.json", &fake.url()),
        );
        let mut rig = Rig::new(&fake.url());
        let mut ai = AiSettings {
            share_device_name: share,
            ..AiSettings::default()
        };
        rig.service
            .consent_accept(&mut ai)
            .expect("consent provisions");
        let installs = fake.requests_to("POST", "/installs");
        assert_eq!(installs.len(), 1);
        let body = installs[0].json();
        assert_eq!(
            body.get("device_name").and_then(Value::as_str),
            sent,
            "share_device_name = {share}"
        );
    }
}

#[test]
fn t02_a_replay_without_a_key_keeps_the_key_on_disk() {
    let fake = Fake::start();
    let url = fake.url();
    fake.on(
        "POST",
        "/installs",
        Reply::json(200, &replay_body(&url, Value::Null)),
    );
    let dir = tempdir();
    let env = penv(None, Some(&url), &credential_path(dir.path()));
    write_json(&env.credential_path, &credential(&url, "anonymous", 0));

    let returned = publik::provision_in(&env, "0.1.0", None).expect("a replay provisions");
    assert_eq!(fake.count(), 1, "no second mint");
    assert_eq!(
        install_ids(&fake),
        [INSTALL_ID],
        "the install id on disk is sent again"
    );
    assert_eq!(returned["key"], KEY);
    assert_eq!(
        read_json(&env.credential_path)["key"],
        KEY,
        "the key on disk is kept"
    );
}

#[test]
fn t02_a_replay_with_no_key_on_disk_mints_once_more_under_a_fresh_id() {
    let fake = Fake::start();
    let url = fake.url();
    fake.on(
        "POST",
        "/installs",
        Reply::json(200, &replay_body(&url, Value::Null)),
    );
    fake.on(
        "POST",
        "/installs",
        Reply::fixture("http/201-installs.json", &url),
    );
    let dir = tempdir();
    let env = penv(None, Some(&url), &credential_path(dir.path()));

    let returned = publik::provision_in(&env, "0.1.0", None).expect("the second mint provisions");
    let ids = install_ids(&fake);
    assert_eq!(ids.len(), 2, "minted once more");
    assert_ne!(ids[0], ids[1], "under a fresh install id");
    assert!(ids.iter().all(|id| is_uuid_v4(id)), "{ids:?}");
    assert_eq!(returned["key"], KEY);
    assert_eq!(read_json(&env.credential_path)["key"], KEY);
}

#[test]
fn t02_install_revoked_mints_once_more_under_a_fresh_id() {
    let fake = Fake::start();
    let url = fake.url();
    fake.on(
        "POST",
        "/installs",
        Reply::fixture("http/403-install-revoked.json", &url),
    );
    fake.on(
        "POST",
        "/installs",
        Reply::fixture("http/201-installs.json", &url),
    );
    let dir = tempdir();
    let env = penv(None, Some(&url), &credential_path(dir.path()));
    write_json(&env.credential_path, &credential(&url, "anonymous", 0));

    publik::provision_in(&env, "0.1.0", None).expect("the second mint provisions");
    let ids = install_ids(&fake);
    assert_eq!(ids.len(), 2, "minted once more");
    assert_eq!(ids[0], INSTALL_ID, "first under the install id on disk");
    assert_ne!(ids[1], INSTALL_ID, "then under a fresh one");
    assert!(is_uuid_v4(&ids[1]), "{}", ids[1]);
}

/// Provisions once against a fresh fake scripted by `script`; the result and the fake.
fn provision_with(script: impl FnOnce(&Fake)) -> (Result<Value, String>, Fake) {
    let fake = Fake::start();
    script(&fake);
    let dir = tempdir();
    let env = penv(None, Some(&fake.url()), &credential_path(dir.path()));
    (publik::provision_in(&env, "0.1.0", None), fake)
}

#[test]
fn t02_a_second_failure_shows_its_message() {
    let (result, fake) = provision_with(|fake| {
        fake.on(
            "POST",
            "/installs",
            Reply::fixture("http/403-install-revoked.json", ""),
        );
        fake.on("POST", "/installs", Reply::fixture("http/429.json", ""));
    });
    assert_eq!(
        result,
        Err(RATE_MESSAGE.to_string()),
        "a 429 shows error.message"
    );
    assert_eq!(fake.count(), 2);

    let (result, fake) = provision_with(|fake| {
        fake.on(
            "POST",
            "/installs",
            Reply::json(200, &replay_body(&fake.url(), Value::Null)),
        );
        fake.on("POST", "/installs", Reply::status(500));
    });
    assert_eq!(
        result,
        Err("publik API could not set up this computer (500). Use your own key for now.".into())
    );
    assert_eq!(fake.count(), 2);

    let (result, fake) = provision_with(|fake| {
        fake.on(
            "POST",
            "/installs",
            Reply::fixture("http/403-install-revoked.json", ""),
        );
    });
    assert_eq!(
        result,
        Err("publik API could not set up this computer. Use your own key for now.".into()),
        "two revoked answers in a row"
    );
    assert_eq!(fake.count(), 2, "minted once more, and no more");

    let (result, fake) = provision_with(|fake| {
        fake.on("POST", "/installs", Reply::status(503));
    });
    assert_eq!(
        result,
        Err("publik API could not set up this computer (503). Use your own key for now.".into())
    );
    assert_eq!(fake.count(), 1);

    let dir = tempdir();
    let dead = closed_url();
    let env = penv(None, Some(&dead), &credential_path(dir.path()));
    assert_eq!(
        publik::provision_in(&env, "0.1.0", None),
        Err("publik API is unreachable right now. Use your own key for now.".into())
    );
}

#[test]
fn t02_a_failed_consent_names_the_failure_and_changes_nothing() {
    let fake = Fake::start();
    fake.on("POST", "/installs", Reply::status(503));
    let mut rig = Rig::new(&fake.url());
    let mut ai = AiSettings::default();
    refused(
        rig.service.consent_accept(&mut ai),
        "provider_error",
        "publik API could not set up this computer (503). Use your own key for now.",
    );
    assert_eq!(ai.provider, "unset");
    assert!(!rig.credential_path().exists(), "no credential is written");
    assert_eq!(fake.count(), 1);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 3: the app token.
// ---------------------------------------------------------------------------------------------

#[test]
fn t03_the_run_time_token_overrides_and_a_token_without_pat_is_refused() {
    let fake = Fake::start();
    fake.on(
        "POST",
        "/installs",
        Reply::fixture("http/201-installs.json", &fake.url()),
    );
    let dir = tempdir();
    let env = PublikEnv {
        app_token: Some("pat_parley_override".into()),
        ..penv(None, Some(&fake.url()), &credential_path(dir.path()))
    };
    publik::provision_in(&env, "0.1.0", None).expect("provisions");
    assert_eq!(
        fake.requests()[0].json()["app_token"],
        "pat_parley_override"
    );

    let dir = tempdir();
    let env = PublikEnv {
        app_token: Some("none".into()),
        ..penv(None, Some(&fake.url()), &credential_path(dir.path()))
    };
    assert_eq!(
        publik::provision_in(&env, "0.1.0", None),
        Err(NO_TOKEN.to_string())
    );
    assert_eq!(
        fake.count(),
        1,
        "a build without a pat_ token sends nothing"
    );

    let dir = tempdir();
    let keychain = MemoryKeychain::default();
    let env = PublikEnv {
        app_token: Some("none".into()),
        ..penv(None, Some(&fake.url()), &credential_path(dir.path()))
    };
    let mut service = new_service(dir.path(), env, &keychain);
    let mut ai = AiSettings::default();
    refused(service.consent_accept(&mut ai), "no_provider", NO_TOKEN);
    assert_eq!(ai.provider, "unset");
    assert_eq!(fake.count(), 1);
}

#[test]
fn t03_the_compiled_in_token_is_publik_app_token_txt() {
    let file = repo_root()
        .join("crates")
        .join("parley-publik")
        .join("publik-app-token.txt");
    assert!(
        file.is_file(),
        "publik-app-token.txt sits next to the crate's Cargo.toml"
    );
    if option_env!("PUBLIK_APP_TOKEN").is_some() {
        println!("skipped: PUBLIK_APP_TOKEN was set at build time, so it overrides the file");
        return;
    }
    let token = read_text(&file).trim().to_string();
    let fake = Fake::start();
    fake.on(
        "POST",
        "/installs",
        Reply::fixture("http/201-installs.json", &fake.url()),
    );
    let dir = tempdir();
    let env = PublikEnv {
        app_token: None,
        ..penv(None, Some(&fake.url()), &credential_path(dir.path()))
    };
    if !token.starts_with("pat_") {
        assert_eq!(
            publik::provision_in(&env, "0.1.0", None),
            Err(NO_TOKEN.to_string())
        );
        assert_eq!(fake.count(), 0);
        println!(
            "skipped the real-token check: crates/parley-publik/publik-app-token.txt holds no \
             pat_ token until the maintainer's setup run"
        );
        return;
    }
    publik::provision_in(&env, "0.1.0", None).expect("the compiled-in token provisions");
    assert_eq!(fake.requests()[0].json()["app_token"], token.as_str());
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 4: header transport, model names and the idempotency key.
// ---------------------------------------------------------------------------------------------

/// The key appears in the `Authorization` header and nowhere else in the request.
fn assert_key_only_in_authorization(request: &fake::Request, key: &str) {
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {key}").as_str())
    );
    for (name, value) in &request.headers {
        if name != "authorization" {
            assert!(!value.contains(key), "the key is in header {name}");
        }
    }
    assert!(!request.path.contains(key), "the key is in the URL");
    assert!(!request.text().contains(key), "the key is in the body");
}

#[test]
fn t04_chat_carries_the_key_only_in_authorization_a_tier_and_the_derived_idempotency_key() {
    let fake = Fake::start();
    let url = fake.url();
    fake.on(
        "POST",
        CHAT,
        Reply::fixture("http/200.json", &url).with_chat(ECHO_HOLA),
    );
    let mut rig = Rig::with_credential(&url, "claimed", 3_120_000);
    let client = rig.client(&publik_settings());
    assert_eq!(client.kind(), ProviderKind::Publik);
    let cache = rig.cache();
    let called = echo(client.as_ref(), &cache, &echo_prompt(1), "hola").expect("a valid answer");
    assert_eq!(called.value, json!({"echo": "hola"}));

    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(
        (request.method.as_str(), request.path.as_str()),
        ("POST", CHAT)
    );
    assert_key_only_in_authorization(request, KEY);
    let body = request.json();
    assert_eq!(body["model"], "publik-fast", "the prompt's tier");
    assert!(publik::MODELS.contains(&body["model"].as_str().unwrap_or_default()));
    assert_eq!(body["max_tokens"], 50);
    assert_eq!(
        body["temperature"],
        json!(0),
        "a whole temperature is an integer"
    );
    assert_eq!(body["messages"][1]["role"], "user");
    assert_eq!(body["messages"][1]["content"], r#"TEXT: "hola""#);

    let key_json = key_json(
        "test_echo",
        1,
        "publik",
        "publik-fast",
        &json!({"text": "hola"}),
    );
    let expected = first_32(&key_json);
    assert!(is_lower_hex_32(&expected));
    assert_eq!(
        request.header("x-publik-idempotency-key"),
        Some(expected.as_str()),
        "the first 32 hex of the SHA-256 of {key_json}"
    );
}

#[test]
fn t04_audio_requests_carry_exactly_the_model_id_their_caller_gives() {
    let fake = Fake::start();
    let url = fake.url();
    fake.on(
        "POST",
        "/audio/transcriptions",
        Reply::json(200, &json!({"text": "hola"})).header("x-publik-charge-micros", "400"),
    );
    fake.on(
        "POST",
        "/audio/speech",
        Reply::bytes(200, "audio/mpeg", b"ID3 fake mp3").header("x-publik-charge-micros", "630"),
    );
    let mut rig = Rig::with_credential(&url, "claimed", 3_120_000);
    let client = rig.client(&publik_settings());

    let transcript = client
        .transcribe(transcribe_request())
        .expect("the transcription answers");
    assert_eq!(transcript.body, json!({"text": "hola"}));
    assert_eq!(transcript.charge_micros, Some(400));
    let transcriptions = fake.requests_to("POST", "/audio/transcriptions");
    let request = &transcriptions[0];
    assert_key_only_in_authorization(request, KEY);
    assert!(
        request
            .header("content-type")
            .unwrap_or_default()
            .starts_with("multipart/form-data"),
        "transcription is multipart"
    );
    let text = request.text();
    assert_eq!(
        multipart_field(&text, "model").as_deref(),
        Some(TRANSCRIBE_MODEL)
    );
    assert_eq!(multipart_field(&text, "language").as_deref(), Some("es"));
    assert!(text.contains("filename=\"take.wav\""), "the file part");
    assert!(
        !publik::MODELS.iter().any(|tier| text.contains(*tier)),
        "an audio request never carries a tier"
    );
    assert_eq!(request.header("x-publik-idempotency-key"), Some(AUDIO_KEY));

    let speech = client
        .speech(SpeechRequest {
            model: SPEECH_MODEL.into(),
            input: "Quiero un café con leche.".into(),
            voice: "alloy".into(),
            response_format: "mp3".into(),
            speed: 0.75,
            timeout_s: 5,
            idempotency_key: Some(AUDIO_KEY.into()),
        })
        .expect("the speech answers");
    assert_eq!(speech.bytes, b"ID3 fake mp3");
    assert_eq!(speech.content_type, "audio/mpeg");
    assert_eq!(speech.charge_micros, Some(630));
    let speeches = fake.requests_to("POST", "/audio/speech");
    let request = &speeches[0];
    assert_key_only_in_authorization(request, KEY);
    let body = request.json();
    assert_eq!(body["model"], SPEECH_MODEL, "exactly the caller's model id");
    assert_eq!(body["input"], "Quiero un café con leche.");
    assert_eq!(body["voice"], "alloy");
    assert_eq!(body["response_format"], "mp3");
    assert_eq!(body["speed"].as_f64(), Some(0.75));
}

/// The value of the multipart field `name` in a form-data body.
fn multipart_field(body: &str, name: &str) -> Option<String> {
    let at = body.find(&format!("name=\"{name}\""))?;
    let rest = &body[at..];
    let start = rest.find("\r\n\r\n")? + 4;
    let end = rest[start..].find("\r\n")?;
    Some(rest[start..start + end].to_string())
}

#[test]
fn t04_a_streamed_chat_reports_each_delta_and_the_whole_reply() {
    let fake = Fake::start();
    fake.on(
        "POST",
        CHAT,
        Reply::sse(&["{\"echo\": ", "\"hola\"}"]).header("x-publik-charge-micros", "40"),
    );
    let mut rig = Rig::with_credential(&fake.url(), "claimed", 3_120_000);
    let client = rig.client(&publik_settings());
    let mut deltas: Vec<String> = Vec::new();
    let reply = client
        .chat_stream(chat_request("publik-balanced", 5), &mut |delta: &str| {
            deltas.push(delta.to_string())
        })
        .expect("the stream completes");
    assert_eq!(reply.content, ECHO_HOLA);
    assert_eq!(deltas.concat(), ECHO_HOLA);
    assert_eq!(reply.charge_micros, Some(40));
    let body = fake.requests()[0].json();
    assert_eq!(body["stream"], true);
    assert_eq!(body["model"], "publik-balanced");
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 5: the status headers and GET /wallet.
// ---------------------------------------------------------------------------------------------

#[test]
fn t05_every_status_header_is_parsed_and_get_wallet_refreshes_it() {
    let fake = Fake::start();
    let url = fake.url();
    fake.on("POST", CHAT, Reply::fixture("http/200.json", &url));
    fake.on(
        "POST",
        CHAT,
        Reply::fixture("http/200.json", &url)
            .header("x-publik-balance", "lots")
            .header("x-publik-charge-micros", "1500"),
    );
    fake.on("GET", "/wallet", Reply::fixture("http/wallet.json", &url));
    let mut rig = Rig::with_credential(&url, "anonymous", 0);
    let ai = publik_settings();
    let client = rig.client(&ai);

    let reply = client
        .chat(chat_request("publik-fast", 5))
        .expect("a 200 answers");
    assert_eq!(reply.content, P0_OK);
    assert_eq!(reply.charge_micros, Some(17));
    assert_eq!(reply.request_id.as_deref(), Some("req_0001"));

    let status = rig.service.status(&ai, false);
    assert_eq!(status.balance_micros, Some(3_120_000), "x-publik-balance");
    assert_eq!(
        status.last_charge_micros,
        Some(17),
        "x-publik-charge-micros"
    );
    assert_eq!(status.last_request_id.as_deref(), Some("req_0001"));
    assert_eq!(status.claim_state.as_deref(), Some("claimed"));
    assert_eq!(status.starter_remaining_micros, Some(40_000));
    assert_eq!(status.week_used_micros, Some(2_017));
    assert_eq!(status.week_budget_micros, Some(5_000_000));
    assert_eq!(
        status.week_resets_at.as_deref(),
        Some("2026-11-09T00:00:00Z")
    );
    assert_eq!(
        status.balance_line.as_deref(),
        Some("$3.12 of publik balance")
    );
    assert_eq!(client.status().balance_micros, Some(3_120_000));

    // A header that does not parse as decimal micros is ignored.
    client
        .chat(chat_request("publik-fast", 5))
        .expect("a 200 answers");
    let status = rig.service.status(&ai, false);
    assert_eq!(status.balance_micros, Some(3_120_000));
    assert_eq!(status.last_charge_micros, Some(1_500));
    assert_eq!(
        fake.count_of("GET", "/wallet"),
        0,
        "no refresh without Settings"
    );

    // Opening Settings refreshes from GET /wallet.
    let status = rig.service.status(&ai, true);
    let wallets = fake.requests_to("GET", "/wallet");
    assert_eq!(wallets.len(), 1);
    assert_key_only_in_authorization(&wallets[0], KEY);
    assert_eq!(status.balance_micros, Some(2_500_000));
    assert_eq!(
        status.balance_line.as_deref(),
        Some("$2.50 of publik balance")
    );
    assert_eq!(status.claim_state.as_deref(), Some("claimed"));
    assert_eq!(status.add_credit_url.as_deref(), Some(ADD_CREDIT_URL));
    assert_eq!(status.top_up_url.as_deref(), Some(REFRESHED_TOP_UP_URL));
    assert_eq!(status.week_used_micros, Some(4_000));
    assert_eq!(status.week_budget_micros, Some(5_000_000));
    assert_eq!(
        status.week_resets_at.as_deref(),
        Some("2026-11-16T00:00:00Z")
    );

    // The status is kept in the data directory too, without the key.
    let text = read_text(&rig.path().join("publik-status.json"));
    let saved: Value = serde_json::from_str(&text).expect("publik-status.json is JSON");
    assert_eq!(saved["balance_micros"], 2_500_000);
    assert!(!text.contains(KEY));
}

// ---------------------------------------------------------------------------------------------
// Acceptance tests 6 and 7: 402.
// ---------------------------------------------------------------------------------------------

fn needs_credit_case(fixture: &str, claim_state: &str, message: &str, top_up_url: &str) {
    let fake = Fake::start();
    let url = fake.url();
    fake.on("POST", CHAT, Reply::chat(ECHO_HOLA));
    fake.on("POST", CHAT, Reply::fixture(fixture, &url));
    let mut rig = Rig::with_credential(&url, claim_state, 0);
    let ai = publik_settings();
    let client = rig.client(&ai);
    let cache = rig.cache();
    let prompt = echo_prompt(1);
    echo(client.as_ref(), &cache, &prompt, "uno").expect("the first call is answered");

    // A batch of three stops after the one call that met the 402.
    let mut errors = Vec::new();
    for text in ["dos", "tres", "cuatro"] {
        if rig.service.status(&ai, false).needs_credit {
            break;
        }
        if let Err(error) = echo(client.as_ref(), &cache, &prompt, text) {
            errors.push(error);
        }
    }
    assert_eq!(
        errors,
        [AiError::NeedsCredit {
            message: message.to_string(),
            top_up_url: top_up_url.to_string(),
        }],
        "{fixture}"
    );
    assert_eq!(
        fake.count(),
        2,
        "{fixture}: one call met the 402, none followed"
    );
    assert_eq!(cache.len(), 1, "{fixture}: nothing of the 402 is cached");

    let shown = AppError::from(errors[0].clone());
    assert_eq!(shown.code.as_str(), "needs_credit");
    assert_eq!(shown.message, message);
    assert_eq!(shown.action, Some(json!({"top_up_url": top_up_url})));
    let text = json_of!(shown);
    assert_eq!(
        text.matches("https://").count(),
        1,
        "exactly one URL: {text}"
    );

    let status = rig.service.status(&ai, false);
    assert!(status.needs_credit, "{fixture}: needs-credit is set");
    assert_eq!(status.top_up_url.as_deref(), Some(top_up_url));
    assert_eq!(
        rig.service.link_url("top_up").expect("the top-up link"),
        top_up_url
    );

    // The rest keeps working: a cached answer is served with no call.
    let again = echo(client.as_ref(), &cache, &prompt, "uno").expect("served from the cache");
    assert!(again.cached);
    assert_eq!(fake.count(), 2);
}

#[test]
fn t06_402_while_anonymous_shows_the_message_and_one_claim_link() {
    needs_credit_case(
        "http/402-anonymous.json",
        "anonymous",
        ANONYMOUS_402_MESSAGE,
        ANONYMOUS_402_URL,
    );
}

#[test]
fn t06_402_once_claimed_shows_the_message_and_one_add_credit_link() {
    needs_credit_case(
        "http/402-claimed.json",
        "claimed",
        CLAIMED_402_MESSAGE,
        CLAIMED_402_URL,
    );
}

#[test]
fn t07_a_success_after_a_402_clears_needs_credit() {
    let ai = publik_settings();
    let prompt = echo_prompt(1);

    // A Try again that succeeds.
    let fake = Fake::start();
    let url = fake.url();
    fake.on(
        "POST",
        CHAT,
        Reply::fixture("http/402-anonymous.json", &url),
    );
    fake.on(
        "POST",
        CHAT,
        Reply::fixture("http/200.json", &url).with_chat(ECHO_HOLA),
    );
    let mut rig = Rig::with_credential(&url, "anonymous", 0);
    let client = rig.client(&ai);
    let cache = rig.cache();
    assert!(matches!(
        echo(client.as_ref(), &cache, &prompt, "hola"),
        Err(AiError::NeedsCredit { .. })
    ));
    assert!(rig.service.status(&ai, false).needs_credit);
    echo(client.as_ref(), &cache, &prompt, "hola").expect("the Try again succeeds");
    assert!(
        !rig.service.status(&ai, false).needs_credit,
        "a success clears it"
    );

    // A wallet refresh showing a positive balance.
    let fake = Fake::start();
    let url = fake.url();
    fake.on(
        "POST",
        CHAT,
        Reply::fixture("http/402-anonymous.json", &url),
    );
    fake.on("GET", "/wallet", Reply::fixture("http/wallet.json", &url));
    let mut rig = Rig::with_credential(&url, "anonymous", 0);
    let client = rig.client(&ai);
    let cache = rig.cache();
    assert!(echo(client.as_ref(), &cache, &prompt, "hola").is_err());
    assert!(rig.service.status(&ai, false).needs_credit);
    let status = rig.service.status(&ai, true);
    assert_eq!(status.balance_micros, Some(2_500_000));
    assert!(
        !status.needs_credit,
        "a refresh with a positive balance clears it"
    );

    // A provider change.
    let fake = Fake::start();
    let url = fake.url();
    fake.on(
        "POST",
        CHAT,
        Reply::fixture("http/402-anonymous.json", &url),
    );
    let own = Fake::start();
    own.on("POST", OWN_CHAT, Reply::chat(P0_OK));
    let mut rig = Rig::with_credential(&url, "anonymous", 0);
    let client = rig.client(&ai);
    let cache = rig.cache();
    assert!(echo(client.as_ref(), &cache, &prompt, "hola").is_err());
    let mut ai = publik_settings();
    let status = rig
        .service
        .set_own_key(&mut ai, &own_input(&format!("{}/v1", own.url())))
        .expect("the own key is saved");
    assert_eq!(ai.provider, "own_key");
    assert!(!status.needs_credit, "a provider change clears it");
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 8: 401 and 403 install_revoked, Disconnect and Reconnect.
// ---------------------------------------------------------------------------------------------

#[test]
fn t08_401_and_403_install_revoked_mark_disconnected_and_reconnect_provisions_again() {
    for fixture in ["http/401.json", "http/403-install-revoked.json"] {
        let fake = Fake::start();
        let url = fake.url();
        fake.on("POST", CHAT, Reply::fixture(fixture, &url));
        fake.on("POST", CHAT, Reply::chat(ECHO_HOLA));
        fake.on(
            "POST",
            "/installs",
            Reply::fixture("http/201-installs.json", &url),
        );
        let mut rig = Rig::with_credential(&url, "anonymous", 0);
        let mut ai = publik_settings();
        let cache = rig.cache();
        let prompt = echo_prompt(1);

        let client = rig.client(&ai);
        let error = echo(client.as_ref(), &cache, &prompt, "hola").expect_err("refused");
        assert_eq!(error, AiError::Disconnected, "{fixture}");
        assert_eq!(error.message(), DISCONNECTED);
        let shown = AppError::from(error);
        assert_eq!(shown.code.as_str(), "disconnected");
        assert_eq!(shown.message, DISCONNECTED);
        assert_eq!(shown.action, None);
        assert_eq!(fake.count_of("POST", CHAT), 1, "{fixture}: one attempt");
        assert!(
            !rig.service.status(&ai, false).connected,
            "{fixture}: disconnected"
        );

        let status = rig
            .service
            .reconnect_publik(&mut ai)
            .expect("Reconnect provisions again");
        assert!(status.connected, "{fixture}: connected after Reconnect");
        assert_eq!(ai.provider, "publik");
        assert_eq!(
            install_ids(&fake),
            [INSTALL_ID],
            "{fixture}: reconnecting keeps the install id"
        );
        assert_eq!(rig.credential()["key"], KEY);

        let client = rig.client(&ai);
        echo(client.as_ref(), &cache, &prompt, "hola").expect("calls work again");
        assert!(rig.service.status(&ai, false).connected);
    }
}

#[test]
fn t08_disconnect_revokes_and_forgets_the_key_and_reconnect_keeps_the_install_id() {
    for revoke in [
        Reply::json(200, &json!({"revoked": true})),
        Reply::status(500),
    ] {
        let fake = Fake::start();
        let url = fake.url();
        fake.on("POST", "/installs/revoke", revoke);
        fake.on(
            "POST",
            "/installs",
            Reply::fixture("http/201-installs.json", &url),
        );
        let mut rig = Rig::with_credential(&url, "anonymous", 0);
        let mut ai = publik_settings();

        rig.service.disconnect_publik(&ai);
        let revokes = fake.requests_to("POST", "/installs/revoke");
        assert_eq!(revokes.len(), 1, "one revoke, best effort");
        assert_key_only_in_authorization(&revokes[0], KEY);
        if rig.credential_path().exists() {
            let file = rig.credential();
            assert!(
                !file["key"].as_str().is_some_and(|k| k.starts_with("pk_")),
                "the key is forgotten even when the revoke fails"
            );
        }

        let status = rig
            .service
            .reconnect_publik(&mut ai)
            .expect("Reconnect provisions again");
        assert!(status.connected);
        assert_eq!(
            install_ids(&fake),
            [INSTALL_ID],
            "reconnecting does not create a second install"
        );
        assert_eq!(rig.credential()["key"], KEY);
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 9: 429, 503 and other errors.
// ---------------------------------------------------------------------------------------------

#[test]
fn t09_429_503_and_other_errors_make_one_attempt_with_the_spec_messages() {
    let provider_error = |code: u16, message: &str| AiError::Provider {
        provider: ProviderKind::Publik,
        code,
        message: message.to_string(),
    };
    let cases = [
        (
            Reply::fixture("http/429.json", ""),
            AiError::RateLimited {
                message: RATE_MESSAGE.to_string(),
            },
            "rate_limited",
            RATE_MESSAGE,
        ),
        (
            Reply::fixture("http/503.json", ""),
            AiError::Busy,
            "busy",
            BUSY,
        ),
        (
            Reply::fixture("http/500.json", ""),
            provider_error(500, "The model provider failed."),
            "provider_error",
            "publik API 500: The model provider failed.",
        ),
        (
            Reply::status(500),
            provider_error(500, "Internal Server Error"),
            "provider_error",
            "publik API 500: Internal Server Error",
        ),
        (
            Reply::json(
                400,
                &json!({"error": {"type": "invalid_request", "message": "max_tokens is too large"}}),
            ),
            provider_error(400, "max_tokens is too large"),
            "provider_error",
            "publik API 400: max_tokens is too large",
        ),
    ];
    for (reply, expected, code, message) in cases {
        let fake = Fake::start();
        fake.on("POST", CHAT, reply);
        let mut rig = Rig::with_credential(&fake.url(), "claimed", 3_120_000);
        let client = rig.client(&publik_settings());
        let cache = rig.cache();
        let error = echo(client.as_ref(), &cache, &echo_prompt(1), "hola").expect_err(message);
        assert_eq!(error, expected);
        assert_eq!(error.message(), message);
        let shown = AppError::from(error);
        assert_eq!(
            (shown.code.as_str(), shown.message.as_str()),
            (code, message)
        );
        assert_eq!(fake.count(), 1, "{message}: one attempt only");
        assert_eq!(cache.len(), 0);
    }
}

#[test]
fn t09_own_key_and_local_errors_name_their_provider() {
    let prompt = echo_prompt(1);

    // Own key: a 5xx is retried twice, then reported.
    let own = Fake::start();
    own.on("POST", OWN_CHAT, Reply::fixture("http/500.json", ""));
    let mut rig = Rig::new(&closed_url());
    rig.store_own_key();
    let client = rig.client(&own_settings(&format!("{}/v1", own.url())));
    let cache = rig.cache();
    let error = echo(client.as_ref(), &cache, &prompt, "hola").expect_err("a 500");
    assert_eq!(
        error,
        AiError::Provider {
            provider: ProviderKind::OwnKey,
            code: 500,
            message: "The model provider failed.".into(),
        }
    );
    assert_eq!(
        error.message(),
        "Your provider returned 500: The model provider failed."
    );
    assert_eq!(own.count(), 3, "an own-key 5xx is retried twice");

    // Own key: a 4xx is not retried.
    let own = Fake::start();
    own.on(
        "POST",
        OWN_CHAT,
        Reply::json(
            400,
            &json!({"error": {"message": "max_tokens is too large"}}),
        ),
    );
    let client = rig.client(&own_settings(&format!("{}/v1", own.url())));
    let error = echo(client.as_ref(), &cache, &prompt, "hola").expect_err("a 400");
    assert_eq!(
        error.message(),
        "Your provider returned 400: max_tokens is too large"
    );
    assert_eq!(own.count(), 1);

    // Local: a 404 with no body names the status's reason.
    let local = Fake::start();
    local.on("POST", OWN_CHAT, Reply::status(404));
    let client = rig.client(&local_settings(
        &format!("{}/v1", local.url()),
        Some(LOCAL_LARGE),
    ));
    let error = echo(client.as_ref(), &cache, &prompt, "hola").expect_err("a 404");
    assert_eq!(
        error,
        AiError::Provider {
            provider: ProviderKind::Local,
            code: 404,
            message: "Not Found".into(),
        }
    );
    assert_eq!(error.message(), "The local model returned 404: Not Found");
    assert_eq!(local.count_of("POST", OWN_CHAT), 1);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 10: timeouts and retries.
// ---------------------------------------------------------------------------------------------

#[test]
fn t10_a_publik_timeout_is_not_retried_an_own_key_timeout_is_retried_twice() {
    let slow = Duration::from_secs(2);
    let prompt = echo_prompt(1);
    assert_eq!(prompt.timeout_s, 1, "test_echo times out after one second");

    let fake = Fake::start();
    fake.on("POST", CHAT, Reply::chat(ECHO_HOLA).after(slow));
    let mut rig = Rig::with_credential(&fake.url(), "claimed", 3_120_000);
    let client = rig.client(&publik_settings());
    let cache = rig.cache();
    let error = echo(client.as_ref(), &cache, &prompt, "hola").expect_err("too slow");
    assert_eq!(
        error,
        AiError::Timeout {
            provider: ProviderKind::Publik
        }
    );
    assert_eq!(error.message(), "publik API did not answer in time.");
    assert_eq!(AppError::from(error).code.as_str(), "busy");
    assert_eq!(fake.count(), 1, "a Publik timeout is never retried");

    let own = Fake::start();
    own.on("POST", OWN_CHAT, Reply::chat(ECHO_HOLA).after(slow));
    let mut rig = Rig::new(&closed_url());
    rig.store_own_key();
    let client = rig.client(&own_settings(&format!("{}/v1", own.url())));
    let cache = rig.cache();
    let error = echo(client.as_ref(), &cache, &prompt, "hola").expect_err("too slow");
    assert_eq!(
        error,
        AiError::Timeout {
            provider: ProviderKind::OwnKey
        }
    );
    assert_eq!(error.message(), "Your provider did not answer in time.");
    assert_eq!(own.count(), 3, "an own-key timeout is retried twice");
}

#[test]
fn t10_publik_unreachable_is_offline_and_not_retried() {
    let dead = closed_url();
    let mut rig = Rig::with_credential(&dead, "claimed", 3_120_000);
    let client = rig.client(&publik_settings());
    let cache = rig.cache();
    let error = echo(client.as_ref(), &cache, &echo_prompt(1), "hola").expect_err("offline");
    assert_eq!(error, AiError::Offline);
    assert_eq!(
        error.message(),
        "You're offline. AI features need a connection or a local model."
    );
    assert_eq!(AppError::from(error).code.as_str(), "offline");
    assert_eq!(cache.len(), 0);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 11: the cache.
// ---------------------------------------------------------------------------------------------

#[test]
fn t11_caches_are_per_provider_identical_input_never_spends_twice_and_a_version_bump_misses() {
    let publik_fake = Fake::start();
    let url = publik_fake.url();
    publik_fake.on(
        "POST",
        CHAT,
        Reply::fixture("http/200.json", &url).with_chat(ECHO_HOLA),
    );
    let own = Fake::start();
    own.on("POST", OWN_CHAT, Reply::chat(ECHO_HOLA));
    let own_base = format!("{}/v1", own.url());
    let mut rig = Rig::with_credential(&url, "claimed", 3_120_000);
    rig.store_own_key();
    let cache = rig.cache();
    let (v1, v2) = (echo_prompt(1), echo_prompt(2));

    let publik_client = rig.client(&publik_settings());
    let first = echo(publik_client.as_ref(), &cache, &v1, "hola").expect("answered");
    assert!(!first.cached);
    assert_eq!(first.value, json!({"echo": "hola"}));
    assert_eq!(first.charge_micros, Some(17));
    let second = echo(publik_client.as_ref(), &cache, &v1, "hola").expect("answered");
    assert!(second.cached);
    assert_eq!(second.value, first.value);
    assert_eq!(publik_fake.count(), 1, "identical input never spends twice");

    let own_client = rig.client(&own_settings(&own_base));
    let own_first = echo(own_client.as_ref(), &cache, &v1, "hola").expect("answered");
    assert!(!own_first.cached, "caches are per provider");
    assert_eq!(own.count(), 1);
    assert!(
        echo(own_client.as_ref(), &cache, &v1, "hola")
            .expect("answered")
            .cached
    );
    assert_eq!(own.count(), 1);

    let bumped = echo(publik_client.as_ref(), &cache, &v2, "hola").expect("answered");
    assert!(!bumped.cached, "a prompt version bump misses");
    assert_eq!(publik_fake.count(), 2);
    assert_eq!(cache.len(), 3);

    for (version, provider, model) in [
        (1, "publik", "publik-fast"),
        (1, "own_key", OWN_MODEL),
        (2, "publik", "publik-fast"),
    ] {
        let key = sha256_hex(&key_json(
            "test_echo",
            version,
            provider,
            model,
            &json!({"text": "hola"}),
        ));
        let stored = cache
            .get(&key)
            .unwrap_or_else(|| panic!("no entry under {key} for {provider} v{version}"));
        let stored: Value = serde_json::from_str(&stored).expect("the stored answer is JSON");
        assert_eq!(stored, json!({"echo": "hola"}));
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 12: the local provider never contacts publik API.
// ---------------------------------------------------------------------------------------------

#[test]
fn t12_the_local_provider_never_contacts_publik_even_with_a_credential_file() {
    let publik_fake = Fake::start();
    let url = publik_fake.url();
    publik_fake.on("GET", "/wallet", Reply::fixture("http/wallet.json", &url));
    publik_fake.on("POST", CHAT, Reply::fixture("ai/P0/ok.json", &url));
    publik_fake.on(
        "POST",
        "/installs",
        Reply::fixture("http/201-installs.json", &url),
    );
    let local = Fake::start();
    script_local_models(&local);
    local.on("POST", OWN_CHAT, Reply::chat(ECHO_HOLA));
    local.on("POST", OWN_CHAT, Reply::chat(P0_OK));
    let base = format!("{}/v1", local.url());

    let mut rig = Rig::with_credential(&url, "claimed", 3_120_000);
    assert!(rig.credential_path().is_file());
    let mut ai = local_settings(&base, None);
    rig.service.on_start(&ai);
    rig.service.status(&ai, true);
    rig.service
        .local_probe(&base)
        .expect("the local server answers");
    rig.service
        .set_local(&mut ai, &base, None)
        .expect("the largest local model is chosen");
    let client = rig.client(&ai);
    assert_eq!(client.kind(), ProviderKind::Local);
    let cache = rig.cache();
    echo(client.as_ref(), &cache, &echo_prompt(1), "hola").expect("the local model answers");
    let check = rig
        .service
        .check_connection(&ai)
        .expect("the local model is connected");
    assert!(check.ok);
    assert_eq!(check.message, "Connected.");

    assert_eq!(publik_fake.count(), 0, "no request to publik API");
    assert_eq!(publik_fake.connections(), 0);
    for request in local.requests() {
        assert!(!request.everything().contains(KEY), "{}", request.path);
        assert_eq!(request.header("x-publik-idempotency-key"), None);
    }
    let chats = local.requests_to("POST", OWN_CHAT);
    assert_eq!(chats.len(), 2);
    for chat in chats {
        assert_eq!(chat.json()["model"], LOCAL_LARGE);
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 13: publik-smart while anonymous.
// ---------------------------------------------------------------------------------------------

#[test]
fn t13_publik_smart_falls_back_to_publik_balanced_while_anonymous() {
    let smart = smart_prompt();
    assert_eq!(smart.tier, "publik-smart");
    for (claim_state, expected) in [
        ("anonymous", "publik-balanced"),
        ("claimed", "publik-smart"),
    ] {
        let fake = Fake::start();
        fake.on("POST", CHAT, Reply::chat(ECHO_HOLA));
        let mut rig = Rig::with_credential(&fake.url(), claim_state, 0);
        let client = rig.client(&publik_settings());
        assert_eq!(client.chat_model("publik-smart"), expected, "{claim_state}");
        assert_eq!(client.chat_model("publik-fast"), "publik-fast");
        assert_eq!(client.chat_model("publik-balanced"), "publik-balanced");
        let cache = rig.cache();
        echo(client.as_ref(), &cache, &smart, "hola").expect("answered");
        assert_eq!(
            fake.requests()[0].json()["model"],
            expected,
            "{claim_state}"
        );
    }

    // The own key and the local model use their own model whatever the tier.
    let mut rig = Rig::new(&closed_url());
    rig.store_own_key();
    let own = rig.client(&own_settings("http://127.0.0.1:9/v1"));
    assert_eq!(own.chat_model("publik-smart"), OWN_MODEL);
    let local = rig.client(&local_settings("http://127.0.0.1:9/v1", Some(LOCAL_SMALL)));
    assert_eq!(local.chat_model("publik-smart"), LOCAL_SMALL);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 14: invalid answers.
// ---------------------------------------------------------------------------------------------

#[test]
fn t14_an_invalid_response_is_discarded_and_not_cached() {
    let fake = Fake::start();
    let url = fake.url();
    let invalid = [
        r#"{"echo": 5}"#,
        "Sorry, I can't do that.",
        r#"{"echo": "hola", "extra": 1}"#,
        r#"{"echo": "hola""#,
    ];
    for content in invalid {
        fake.on("POST", CHAT, Reply::chat(content));
    }
    fake.on("POST", CHAT, Reply::chat(ECHO_HOLA));
    let mut rig = Rig::with_credential(&url, "claimed", 3_120_000);
    let client = rig.client(&publik_settings());
    let cache = rig.cache();
    let prompt = echo_prompt(1);
    for content in invalid {
        let error = echo(client.as_ref(), &cache, &prompt, "hola").expect_err(content);
        assert_eq!(error, AiError::Invalid, "{content}");
        assert_eq!(error.message(), INVALID);
        let shown = AppError::from(error);
        assert_eq!(
            (shown.code.as_str(), shown.message.as_str()),
            ("ai_invalid", INVALID)
        );
        assert_eq!(cache.len(), 0, "{content}: nothing is cached");
    }

    // An answer that passes the schema but fails the feature's rules is discarded too.
    let reject = |_: &Value| -> Result<(), RuleFailure> {
        Err(RuleFailure {
            rule: "T1".into(),
            detail: "a rule of the test".into(),
        })
    };
    let error = ai::call::<Value>(
        client.as_ref(),
        &cache,
        &prompt,
        &echo_vars("hola"),
        &json!({"text": "hola"}),
        &reject,
    )
    .expect_err("the rule fails");
    assert_eq!(error, AiError::Invalid);
    assert_eq!(cache.len(), 0);
    assert_eq!(fake.count(), invalid.len() + 1);

    // P0 answers that are not {"ok": true}.
    let fake = Fake::start();
    let url = fake.url();
    fake.on("POST", CHAT, Reply::fixture("ai/P0/not_ok.json", &url));
    fake.on("POST", CHAT, Reply::fixture("ai/P0/prose.json", &url));
    let mut rig = Rig::with_credential(&url, "claimed", 3_120_000);
    for _ in 0..2 {
        refused(
            rig.service.check_connection(&publik_settings()),
            "ai_invalid",
            INVALID,
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 15: the 4 MB limit.
// ---------------------------------------------------------------------------------------------

#[test]
fn t15_a_request_of_max_request_bytes_or_more_is_refused_before_any_connection() {
    assert_eq!(MAX_REQUEST_BYTES, 4_000_000);
    let fake = Fake::start();
    fake.on("POST", CHAT, Reply::chat(P0_OK));
    fake.on(
        "POST",
        "/audio/transcriptions",
        Reply::json(200, &json!({"text": "hola"})),
    );
    let mut rig = Rig::with_credential(&fake.url(), "claimed", 3_120_000);
    let client = rig.client(&publik_settings());

    // A large JPEG: its base64 alone is 4,000,000 bytes.
    let jpeg = vec![0xFF_u8; 3_000_000];
    let photo = ChatRequest {
        messages: vec![ChatMessage {
            role: "user".into(),
            content: json!([
                {"type": "text", "text": "Photo for a German vocabulary deck."},
                image_part(&jpeg),
            ]),
        }],
        ..chat_request("publik-balanced", 5)
    };
    assert_eq!(client.chat(photo).err(), Some(AiError::TooLarge));
    assert_eq!(
        (fake.count(), fake.connections()),
        (0, 0),
        "nothing is sent"
    );
    assert_eq!(
        AiError::TooLarge.message(),
        "This request is larger than 4 MB, so it was not sent."
    );

    // Exactly MAX_REQUEST_BYTES is refused; one byte less is sent.
    assert_eq!(
        client.chat(sized_request(MAX_REQUEST_BYTES)).err(),
        Some(AiError::TooLarge)
    );
    assert_eq!(fake.connections(), 0);
    client
        .chat(sized_request(MAX_REQUEST_BYTES - 1))
        .expect("a body under the limit is sent");
    assert_eq!(fake.count(), 1);
    assert_eq!(fake.requests()[0].body.len(), MAX_REQUEST_BYTES - 1);

    // A small photo is sent as an OpenAI image part.
    let small = image_part(&[0xFF, 0xD8, 0xFF, 0xE0]);
    assert_eq!(
        small,
        json!({"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,/9j/4A=="}})
    );
    let request = ChatRequest {
        messages: vec![ChatMessage {
            role: "user".into(),
            content: json!([small]),
        }],
        ..chat_request("publik-balanced", 5)
    };
    client.chat(request).expect("a small photo is sent");
    assert_eq!(fake.count(), 2);
    assert_eq!(
        fake.requests()[1].json()["messages"][0]["content"][0]["image_url"]["url"],
        "data:image/jpeg;base64,/9j/4A=="
    );

    // Audio of 4 MB is refused too.
    let audio = TranscribeRequest {
        audio: vec![0; MAX_REQUEST_BYTES],
        ..transcribe_request()
    };
    assert_eq!(client.transcribe(audio).err(), Some(AiError::TooLarge));
    assert_eq!(fake.count(), 2);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 16: the key never leaks (invariant I1).
// ---------------------------------------------------------------------------------------------

#[test]
fn t16_the_key_appears_in_no_error_status_status_file_request_or_command_result() {
    let fake = Fake::start();
    let url = fake.url();
    fake.on(
        "POST",
        "/installs",
        Reply::fixture("http/201-installs.json", &url),
    );
    fake.on("GET", "/wallet", Reply::fixture("http/wallet.json", &url));
    for reply in [
        Reply::fixture("http/200.json", &url).with_chat(ECHO_HOLA),
        Reply::fixture("http/402-anonymous.json", &url),
        Reply::fixture("http/429.json", &url),
        Reply::fixture("http/503.json", &url),
        Reply::fixture("http/500.json", &url),
        Reply::chat(r#"{"echo": 5}"#),
        Reply::fixture("http/401.json", &url),
        Reply::fixture("ai/P0/ok.json", &url),
    ] {
        fake.on("POST", CHAT, reply);
    }
    let own = Fake::start();
    own.on(
        "POST",
        OWN_CHAT,
        Reply::json(401, &json!({"error": {"message": "The key was refused."}})),
    );

    let mut seen: Vec<String> = Vec::new();
    let mut rig = Rig::new(&url);
    let mut ai = AiSettings::default();
    let accepted = rig
        .service
        .consent_accept(&mut ai)
        .expect("consent provisions");
    seen.push(json_of!(accepted));
    seen.push(format!("{accepted:?}"));
    seen.push(json_of!(rig.service.on_start(&ai)));
    seen.push(json_of!(rig.service.status(&ai, true)));

    let client = rig.client(&ai);
    let cache = rig.cache();
    let prompt = echo_prompt(1);
    for text in ["uno", "dos", "tres", "cuatro", "cinco", "seis", "siete"] {
        match echo(client.as_ref(), &cache, &prompt, text) {
            Ok(called) => seen.push(format!("{called:?}")),
            Err(error) => record_error(&mut seen, error),
        }
        seen.push(json_of!(client.status()));
        seen.push(json_of!(rig.service.status(&ai, false)));
    }
    match rig.service.reconnect_publik(&mut ai) {
        Ok(status) => seen.push(json_of!(status)),
        Err(error) => record_app_error(&mut seen, &error),
    }
    match rig.service.check_connection(&ai) {
        Ok(check) => seen.push(json_of!(check)),
        Err(error) => record_app_error(&mut seen, &error),
    }
    for kind in ["claim", "top_up", "add_credit", "other"] {
        match rig.service.link_url(kind) {
            Ok(link) => seen.push(link),
            Err(error) => record_app_error(&mut seen, &error),
        }
    }
    seen.push(format!("{:?}", chat_request("publik-fast", 5)));
    seen.push(format!("{:?}", transcribe_request()));

    let saved = rig
        .service
        .set_own_key(&mut ai, &own_input(&format!("{}/v1", own.url())))
        .expect("the own key is saved");
    seen.push(json_of!(saved));
    seen.push(json_of!(ai));
    seen.push(format!("{ai:?}"));
    seen.push(
        Settings {
            ai: ai.clone(),
            ..Settings::default()
        }
        .to_file_text(),
    );
    let own_client = rig.client(&ai);
    if let Err(error) = echo(own_client.as_ref(), &cache, &prompt, "ocho") {
        record_error(&mut seen, error);
    }
    seen.push(json_of!(own_client.status()));
    seen.push(json_of!(rig.service.status(&ai, true)));
    match rig.service.check_connection(&ai) {
        Ok(check) => seen.push(json_of!(check)),
        Err(error) => record_app_error(&mut seen, &error),
    }
    seen.push(read_text(&rig.path().join("publik-status.json")));
    seen.push(json_of!(rig.service.disconnect_publik(&publik_settings())));

    assert_eq!(
        rig.keychain.secret().as_deref(),
        Some(OWN_KEY),
        "the own key is in the keychain"
    );
    assert!(seen.len() > 30, "the scan saw {} texts", seen.len());
    for text in &seen {
        assert!(!text.contains(KEY), "the publik API key is in: {text}");
        assert!(!text.contains(OWN_KEY), "the own key is in: {text}");
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 18 (core half): the consent text and Not now.
// ---------------------------------------------------------------------------------------------

#[test]
fn t18_the_disclosure_and_the_data_path_sentence_in_en_json_are_the_spec_text() {
    assert_eq!(DISCLOSURE, REFERENCE_DISCLOSURE);
    let strings = read_json(
        &repo_root()
            .join("app")
            .join("src")
            .join("strings")
            .join("en.json"),
    );
    assert_eq!(
        strings["ai.setup.disclosure"], DISCLOSURE,
        "en.json holds publik::DISCLOSURE verbatim"
    );
    assert_eq!(strings["ai.setup.data_path"], DATA_PATH);
}

#[test]
fn t18_not_now_sends_nothing_and_changes_nothing() {
    let fake = Fake::start();
    fake.on(
        "POST",
        "/installs",
        Reply::fixture("http/201-installs.json", &fake.url()),
    );
    let mut rig = Rig::new(&fake.url());
    let ai = AiSettings::default();
    let status = rig.service.consent_decline(&ai);
    assert_eq!(ai, AiSettings::default());
    assert_eq!(status.provider, "unset");
    assert!(status.needs_consent);
    assert_eq!(fake.count(), 0, "Not now sends nothing");
    assert!(!rig.credential_path().exists());
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 19: ai_open_link.
// ---------------------------------------------------------------------------------------------

#[test]
fn t19_open_link_gives_only_the_status_urls_and_refuses_any_other_kind() {
    let fake = Fake::start();
    let url = fake.url();
    fake.on(
        "POST",
        "/installs",
        Reply::fixture("http/201-installs.json", &url),
    );
    fake.on(
        "POST",
        CHAT,
        Reply::fixture("http/402-anonymous.json", &url),
    );
    let mut rig = Rig::new(&url);

    for kind in ["claim", "top_up", "add_credit"] {
        refused(rig.service.link_url(kind), "not_found", NOT_AVAILABLE);
    }
    for kind in ["other", "https://publikhq.com/claim/5f0c8a52", "url"] {
        refused(
            rig.service.link_url(kind),
            "invalid_input",
            &format!("Unknown value {kind} for kind."),
        );
    }

    let mut ai = AiSettings::default();
    rig.service
        .consent_accept(&mut ai)
        .expect("consent provisions");
    assert_eq!(
        rig.service.link_url("top_up").expect("the top-up link"),
        WALLET_TOP_UP_URL
    );
    let client = rig.client(&ai);
    let cache = rig.cache();
    assert!(echo(client.as_ref(), &cache, &echo_prompt(1), "hola").is_err());
    assert_eq!(
        rig.service.link_url("top_up").expect("the top-up link"),
        ANONYMOUS_402_URL,
        "the status's URL from the last 402"
    );
    refused(
        rig.service.link_url("https://publikhq.com/claim/5f0c8a52"),
        "invalid_input",
        "Unknown value https://publikhq.com/claim/5f0c8a52 for kind.",
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 21 (core half): the balance line.
// ---------------------------------------------------------------------------------------------

#[test]
fn t21_the_balance_line_of_the_first_run_card_and_afterwards() {
    assert_eq!(publik::balance_line(Some(0), "anonymous"), FRESH_LINE);
    assert_eq!(
        publik::balance_line(Some(0), "claimed"),
        "$0.00 of publik balance"
    );
    assert_eq!(
        publik::balance_line(Some(3_120_000), "claimed"),
        "$3.12 of publik balance"
    );
    assert_eq!(
        publik::balance_line(Some(3_120_000), "anonymous"),
        "$3.12 of publik balance"
    );
    // Adaptation 7: the amount through format_dollars, so ten cents and under is not $0.00.
    assert_eq!(
        publik::balance_line(Some(49_000), "claimed"),
        "$0.049 of publik balance"
    );
    assert_eq!(publik::balance_line(None, "anonymous"), "");

    assert_eq!(
        publik::top_up_url_from_402(&json!({"error": {"message": "M", "top_up_url": "U"}})),
        ("M".to_string(), Some("U".to_string()))
    );
    assert_eq!(
        publik::top_up_url_from_402(&json!({})),
        ("Not enough publik API balance.".to_string(), None)
    );

    let fake = Fake::start();
    fake.on(
        "POST",
        "/installs",
        Reply::fixture("http/201-installs.json", &fake.url()),
    );
    let mut rig = Rig::new(&fake.url());
    let mut ai = AiSettings::default();
    let status = rig
        .service
        .consent_accept(&mut ai)
        .expect("consent provisions");
    assert_eq!(status.balance_line.as_deref(), Some(FRESH_LINE));
    assert_eq!(status.claim_state.as_deref(), Some("anonymous"));
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 22: the local probe.
// ---------------------------------------------------------------------------------------------

#[test]
fn t22_the_local_probe_drops_embedding_models_and_chooses_the_largest() {
    let local = Fake::start();
    script_local_models(&local);
    local.on("POST", OWN_CHAT, Reply::chat(ECHO_HOLA));
    let base = format!("{}/v1", local.url());
    let mut rig = Rig::new(&closed_url());

    let models = rig
        .service
        .local_probe(&base)
        .expect("the local server answers");
    assert_eq!(
        models,
        [
            LocalModel {
                name: LOCAL_LARGE.into(),
                bytes: Some(4_700_000_000),
            },
            LocalModel {
                name: LOCAL_SMALL.into(),
                bytes: Some(2_000_000_000),
            },
        ],
        "largest first, embedding models dropped"
    );
    assert!(local.count_of("GET", "/v1/models") >= 1);
    assert!(
        local.count_of("GET", "/api/tags") >= 1,
        "sizes from /api/tags"
    );

    let mut ai = AiSettings::default();
    let status = rig
        .service
        .set_local(&mut ai, &base, None)
        .expect("the local server is chosen");
    assert_eq!(ai.provider, "local");
    assert_eq!(ai.local.base_url, base);
    assert_eq!(status.provider, "local");
    assert_eq!(status.local_model.as_deref(), Some(LOCAL_LARGE));
    let client = rig.client(&ai);
    assert_eq!(client.chat_model("publik-fast"), LOCAL_LARGE);
    let cache = rig.cache();
    echo(client.as_ref(), &cache, &echo_prompt(1), "hola").expect("the local model answers");
    assert_eq!(
        local.requests_to("POST", OWN_CHAT)[0].json()["model"],
        LOCAL_LARGE
    );

    // The learner's choice wins over the largest.
    let status = rig
        .service
        .set_local(&mut ai, &base, Some(LOCAL_SMALL))
        .expect("the local model is chosen");
    assert_eq!(status.local_model.as_deref(), Some(LOCAL_SMALL));
    assert_eq!(rig.client(&ai).chat_model("publik-fast"), LOCAL_SMALL);
}

#[test]
fn t22_sizes_the_server_does_not_give_come_last() {
    let local = Fake::start();
    local.on(
        "GET",
        "/v1/models",
        Reply::json(
            200,
            &json!({"object": "list", "data": [
                {"id": LOCAL_LARGE, "object": "model"},
                {"id": LOCAL_SMALL, "object": "model"},
                {"id": "mxbai-embed-large", "object": "model"},
            ]}),
        ),
    );
    local.on(
        "GET",
        "/api/tags",
        Reply::json(
            200,
            &json!({"models": [{"name": LOCAL_SMALL, "size": 2_000_000_000_u64}]}),
        ),
    );
    let mut rig = Rig::new(&closed_url());
    let models = rig
        .service
        .local_probe(&format!("{}/v1", local.url()))
        .expect("the local server answers");
    assert_eq!(
        models,
        [
            LocalModel {
                name: LOCAL_SMALL.into(),
                bytes: Some(2_000_000_000),
            },
            LocalModel {
                name: LOCAL_LARGE.into(),
                bytes: None,
            },
        ]
    );

    // No /api/tags at all: every size is unknown, and the probe still answers.
    let plain = Fake::start();
    plain.on(
        "GET",
        "/v1/models",
        Reply::json(
            200,
            &json!({"object": "list", "data": [{"id": "qwen3:4b", "object": "model"}]}),
        ),
    );
    let models = rig
        .service
        .local_probe(&format!("{}/v1", plain.url()))
        .expect("the local server answers");
    assert_eq!(
        models,
        [LocalModel {
            name: "qwen3:4b".into(),
            bytes: None,
        }]
    );
}

#[test]
fn t22_with_the_local_server_down_the_probe_says_so() {
    let mut rig = Rig::new(&closed_url());
    let dead = format!("{}/v1", closed_url());
    refused(
        rig.service.local_probe(&dead),
        "provider_error",
        &format!(
            "The local model server at {dead} is not running. Start it, or choose another provider."
        ),
    );
}

#[test]
fn t22_a_local_server_must_be_on_this_computer() {
    let mut rig = Rig::new(&closed_url());
    for remote in [
        "http://models.invalid:11434/v1",
        "https://ollama.invalid/v1",
    ] {
        let mut ai = AiSettings::default();
        refused(
            rig.service.set_local(&mut ai, remote, None),
            "invalid_input",
            NOT_LOCAL,
        );
        assert_eq!(ai, AiSettings::default(), "{remote}: nothing changes");
        refused(rig.service.local_probe(remote), "invalid_input", NOT_LOCAL);
    }
    let local = Fake::start();
    script_local_models(&local);
    let port = local.port();
    for host in ["localhost", "127.0.0.1", "[::1]"] {
        let base = format!("http://{host}:{port}/v1");
        let mut ai = AiSettings::default();
        if let Err(error) = rig.service.set_local(&mut ai, &base, Some(LOCAL_LARGE)) {
            assert_ne!(error.message, NOT_LOCAL, "{host} is on this computer");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 23: the own key.
// ---------------------------------------------------------------------------------------------

#[test]
fn t23_set_own_key_stores_the_key_in_the_keychain_and_nowhere_else() {
    let own = Fake::start();
    own.on(
        "GET",
        "/v1/models",
        Reply::json(
            200,
            &json!({"object": "list", "data": [{"id": OWN_MODEL, "object": "model"}]}),
        ),
    );
    own.on("POST", OWN_CHAT, Reply::chat(P0_OK));
    let base = format!("{}/v1", own.url());
    let mut rig = Rig::new(&closed_url());
    let mut ai = AiSettings::default();

    let status = rig
        .service
        .set_own_key(&mut ai, &own_input(&base))
        .expect("the own key is saved");
    assert_eq!(rig.keychain.secret().as_deref(), Some(OWN_KEY));
    assert_eq!(ai.provider, "own_key");
    assert_eq!(
        ai.own_key,
        OwnKeySettings {
            base_url: Some(base.clone()),
            chat_model: Some(OWN_MODEL.into()),
            transcribe_model: Some("whisper-own".into()),
            speech_model: Some("tts-own".into()),
            speech_voice: Some("alloy".into()),
        }
    );
    assert!(status.own_key_stored);
    assert_eq!(status.provider, "own_key");
    let settings_file = Settings {
        ai: ai.clone(),
        ..Settings::default()
    }
    .to_file_text();
    for text in [
        settings_file,
        json_of!(ai),
        format!("{ai:?}"),
        json_of!(status),
        json_of!(rig.service.status(&ai, false)),
        json_of!(rig.service.check_connection(&ai).expect("connected")),
    ] {
        assert!(!text.contains(OWN_KEY), "the own key is in: {text}");
    }
    for request in own.requests() {
        assert_key_only_in_authorization(&request, OWN_KEY);
    }

    let status = rig
        .service
        .clear_own_key(&mut ai)
        .expect("the own key is removed");
    assert_eq!(rig.keychain.secret(), None);
    assert!(!status.own_key_stored);
    assert_eq!(
        rig.service.client(&own_settings(&base)).err(),
        Some(AiError::OwnKeyMissing)
    );
}

#[test]
fn t23_own_key_input_is_checked_before_anything_is_stored() {
    let base = "https://own.invalid/v1";
    let cases = [
        (
            OwnKeyInput {
                base_url: "own.invalid/v1".into(),
                ..own_input(base)
            },
            BASE_URL_MESSAGE,
        ),
        (
            OwnKeyInput {
                base_url: "ftp://own.invalid/v1".into(),
                ..own_input(base)
            },
            BASE_URL_MESSAGE,
        ),
        (
            OwnKeyInput {
                base_url: String::new(),
                ..own_input(base)
            },
            BASE_URL_MESSAGE,
        ),
        (
            OwnKeyInput {
                key: String::new(),
                ..own_input(base)
            },
            "Enter your key.",
        ),
        (
            OwnKeyInput {
                chat_model: String::new(),
                ..own_input(base)
            },
            "Enter the chat model name.",
        ),
    ];
    for (input, message) in cases {
        let mut rig = Rig::new(&closed_url());
        let mut ai = AiSettings::default();
        refused(
            rig.service.set_own_key(&mut ai, &input),
            "invalid_input",
            message,
        );
        assert_eq!(rig.keychain.secret(), None, "{message}: nothing is stored");
        assert_eq!(ai, AiSettings::default(), "{message}: settings unchanged");
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 24 (service half): Check connection.
// ---------------------------------------------------------------------------------------------

#[test]
fn t24_check_connection_reports_the_measured_charge() {
    let fake = Fake::start();
    let url = fake.url();
    fake.on("POST", CHAT, Reply::fixture("ai/P0/ok.json", &url));
    let mut rig = Rig::with_credential(&url, "claimed", 3_120_000);
    let check = rig
        .service
        .check_connection(&publik_settings())
        .expect("connected");
    assert_eq!(
        check,
        ConnectionCheck {
            ok: true,
            charge_micros: Some(17),
            message: "Connected. This check cost $0.000017.".into(),
        }
    );
    let requests = fake.requests();
    let request = &requests[0];
    assert_eq!(request.json(), p0_body(), "P0's body is §6.2's exactly");
    assert!(is_lower_hex_32(
        request
            .header("x-publik-idempotency-key")
            .unwrap_or_default()
    ));
    assert_eq!(format_dollars(17), "$0.000017");

    // The own key reports no charge: "Connected."
    let own = Fake::start();
    own.on("POST", OWN_CHAT, Reply::chat(P0_OK));
    let mut rig = Rig::new(&closed_url());
    rig.store_own_key();
    let check = rig
        .service
        .check_connection(&own_settings(&format!("{}/v1", own.url())))
        .expect("connected");
    assert_eq!(
        check,
        ConnectionCheck {
            ok: true,
            charge_micros: None,
            message: "Connected.".into(),
        }
    );
    let body = own.requests()[0].json();
    assert_eq!(body["model"], OWN_MODEL);
    assert_eq!(body["messages"], p0_body()["messages"]);
    assert_eq!(own.requests()[0].header("x-publik-idempotency-key"), None);

    // Before any provider is set up, Check connection asks for consent.
    let mut rig = Rig::new(&closed_url());
    refused(
        rig.service.check_connection(&AiSettings::default()),
        "needs_consent",
        "AI features are not set up yet.",
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 25: the cost of a run.
// ---------------------------------------------------------------------------------------------

#[test]
fn t25_the_cost_of_a_run_is_the_sum_of_the_charge_headers() {
    let fake = Fake::start();
    for charge in ["17", "1500", "2300"] {
        fake.on(
            "POST",
            CHAT,
            Reply::chat(ECHO_HOLA).header("x-publik-charge-micros", charge),
        );
    }
    let mut rig = Rig::with_credential(&fake.url(), "claimed", 3_120_000);
    let ai = publik_settings();
    let client = rig.client(&ai);
    let cache = rig.cache();
    let prompt = echo_prompt(1);
    let mut total: i64 = 0;
    for text in ["uno", "dos", "uno", "tres"] {
        let called = echo(client.as_ref(), &cache, &prompt, text).expect("answered");
        if called.cached {
            assert_eq!(
                called.charge_micros.unwrap_or(0),
                0,
                "a cache hit adds nothing"
            );
        }
        total += called.charge_micros.unwrap_or(0);
    }
    assert_eq!(fake.count(), 3);
    assert_eq!(total, 3_817, "17 + 1,500 + 2,300");
    assert_eq!(format_dollars(total), "$0.0038");
    assert_eq!(
        rig.service.status(&ai, false).last_charge_micros,
        Some(2_300)
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 28 (HTTP half): idempotency keys on the wire.
// ---------------------------------------------------------------------------------------------

#[test]
fn t28_two_check_connection_clicks_send_two_random_keys() {
    let fake = Fake::start();
    let url = fake.url();
    fake.on("POST", CHAT, Reply::fixture("ai/P0/ok.json", &url));
    let mut rig = Rig::with_credential(&url, "claimed", 3_120_000);
    let ai = publik_settings();
    rig.service.check_connection(&ai).expect("connected");
    rig.service.check_connection(&ai).expect("connected");
    let keys: Vec<String> = fake
        .requests()
        .iter()
        .map(|r| {
            r.header("x-publik-idempotency-key")
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(keys.len(), 2, "P0 is never cached");
    assert!(keys.iter().all(|k| is_lower_hex_32(k)), "{keys:?}");
    assert_ne!(keys[0], keys[1], "a new random key per click");
}

#[test]
fn t28_a_try_again_after_a_timeout_sends_the_same_key() {
    let fake = Fake::start();
    fake.on(
        "POST",
        CHAT,
        Reply::chat(ECHO_HOLA).after(Duration::from_secs(2)),
    );
    fake.on("POST", CHAT, Reply::chat(ECHO_HOLA));
    let mut rig = Rig::with_credential(&fake.url(), "claimed", 3_120_000);
    let client = rig.client(&publik_settings());
    let cache = rig.cache();
    let prompt = echo_prompt(1);
    assert!(matches!(
        echo(client.as_ref(), &cache, &prompt, "hola"),
        Err(AiError::Timeout { .. })
    ));
    echo(client.as_ref(), &cache, &prompt, "hola").expect("the Try again is answered");
    let expected = first_32(&key_json(
        "test_echo",
        1,
        "publik",
        "publik-fast",
        &json!({"text": "hola"}),
    ));
    let keys: Vec<Option<String>> = fake
        .requests()
        .iter()
        .map(|r| r.header("x-publik-idempotency-key").map(str::to_string))
        .collect();
    assert_eq!(keys, [Some(expected.clone()), Some(expected)]);
}

#[test]
fn t14_t28_a_reply_with_no_content_is_invalid_and_a_try_again_sends_the_attempt_2_key() {
    let input = json!({"text": "hola"});
    let attempt_1 = first_32(&key_json("test_echo", 1, "publik", "publik-fast", &input));
    let attempt_2 = first_32(&attempt_key_json(
        "test_echo",
        1,
        "publik",
        "publik-fast",
        &input,
        2,
    ));
    assert_ne!(attempt_1, attempt_2);
    let null_content = json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "created": 1_762_387_200,
        "model": "publik-fast",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": null},
            "finish_reason": "stop",
        }],
    });
    let no_choices = json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "created": 1_762_387_200,
        "model": "publik-fast",
    });
    for body in [null_content, no_choices] {
        let fake = Fake::start();
        fake.on(
            "POST",
            CHAT,
            Reply::json(200, &body).header("x-publik-charge-micros", "17"),
        );
        fake.on("POST", CHAT, Reply::chat(ECHO_HOLA));
        let mut rig = Rig::with_credential(&fake.url(), "claimed", 3_120_000);
        let client = rig.client(&publik_settings());
        let cache = rig.cache();
        let prompt = echo_prompt(1);

        let error = echo(client.as_ref(), &cache, &prompt, "hola").expect_err("no content");
        assert_eq!(error, AiError::Invalid, "{body}");
        assert_eq!(error.message(), INVALID);
        assert_eq!(cache.len(), 0, "{body}: nothing is cached");

        echo(client.as_ref(), &cache, &prompt, "hola").expect("the Try again is answered");
        let keys: Vec<Option<String>> = fake
            .requests()
            .iter()
            .map(|r| r.header("x-publik-idempotency-key").map(str::to_string))
            .collect();
        assert_eq!(
            keys,
            [Some(attempt_1.clone()), Some(attempt_2.clone())],
            "{body}: the Try again is attempt 2, not a replay of the unusable answer"
        );
        assert_eq!(cache.len(), 1);
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 29: start-up and Settings contact publik API only while it is chosen.
// ---------------------------------------------------------------------------------------------

#[test]
fn t29_start_up_calls_get_wallet_only_while_publik_is_chosen() {
    for provider in ["unset", "own_key", "local"] {
        let publik_fake = Fake::start();
        let url = publik_fake.url();
        publik_fake.on("GET", "/wallet", Reply::fixture("http/wallet.json", &url));
        let other = Fake::start();
        script_local_models(&other);
        let base = format!("{}/v1", other.url());
        let mut rig = Rig::with_credential(&url, "claimed", 3_120_000);
        rig.store_own_key();
        let ai = match provider {
            "own_key" => own_settings(&base),
            "local" => local_settings(&base, Some(LOCAL_LARGE)),
            _ => AiSettings::default(),
        };
        let status = rig.service.on_start(&ai);
        assert_eq!(status.provider, provider);
        assert_eq!(
            publik_fake.count(),
            0,
            "{provider}: start-up sends nothing to publik API"
        );
        assert_eq!(
            other.count(),
            0,
            "{provider}: start-up sends nothing at all"
        );
        rig.service.status(&ai, true);
        assert_eq!(
            publik_fake.count(),
            0,
            "{provider}: opening Settings sends nothing to publik API"
        );
    }

    let publik_fake = Fake::start();
    let url = publik_fake.url();
    publik_fake.on("GET", "/wallet", Reply::fixture("http/wallet.json", &url));
    let mut rig = Rig::with_credential(&url, "claimed", 3_120_000);
    let ai = publik_settings();
    let status = rig.service.on_start(&ai);
    assert_eq!(status.provider, "publik");
    assert_eq!(
        publik_fake.count_of("GET", "/wallet"),
        1,
        "one GET /wallet at start-up"
    );
    assert_eq!(publik_fake.count(), 1);
    rig.service.status(&ai, false);
    assert_eq!(
        publik_fake.count(),
        1,
        "ai_status without refresh sends nothing"
    );
    rig.service.status(&ai, true);
    assert_eq!(
        publik_fake.count_of("GET", "/wallet"),
        2,
        "opening Settings makes another"
    );
    for request in publik_fake.requests() {
        assert_key_only_in_authorization(&request, KEY);
    }

    // publik API chosen but no key resolves: nothing is sent.
    let empty = Fake::start();
    let mut rig = Rig::new(&empty.url());
    rig.service.on_start(&publik_settings());
    assert_eq!(empty.count(), 0);
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 30: only the chosen provider, with its own key (invariant I9).
// ---------------------------------------------------------------------------------------------

#[test]
fn t30_only_the_chosen_provider_is_contacted_and_with_its_own_key() {
    let publik_fake = Fake::start();
    let url = publik_fake.url();
    publik_fake.on("GET", "/wallet", Reply::fixture("http/wallet.json", &url));
    publik_fake.on(
        "POST",
        CHAT,
        Reply::fixture("http/200.json", &url).with_chat(ECHO_HOLA),
    );
    publik_fake.on("POST", CHAT, Reply::fixture("ai/P0/ok.json", &url));
    let own = Fake::start();
    own.on("POST", OWN_CHAT, Reply::chat(ECHO_HOLA));
    own.on("POST", OWN_CHAT, Reply::chat(P0_OK));
    let own_base = format!("{}/v1", own.url());
    let mut rig = Rig::with_credential(&url, "claimed", 3_120_000);
    rig.store_own_key();
    let cache = rig.cache();
    let prompt = echo_prompt(1);

    // publik API chosen, with an own key stored and configured.
    let ai = AiSettings {
        provider: "publik".into(),
        ..own_settings(&own_base)
    };
    rig.service.on_start(&ai);
    rig.service.status(&ai, true);
    let client = rig.client(&ai);
    assert_eq!(client.kind(), ProviderKind::Publik);
    echo(client.as_ref(), &cache, &prompt, "hola").expect("answered");
    rig.service.check_connection(&ai).expect("connected");
    assert_eq!(
        own.count(),
        0,
        "the own key is never used while publik API is chosen"
    );
    let requests = publik_fake.requests();
    assert!(requests.len() >= 4, "{} requests", requests.len());
    for request in &requests {
        assert_key_only_in_authorization(request, KEY);
        assert!(!request.everything().contains(OWN_KEY));
    }

    // The own key chosen.
    publik_fake.forget();
    let ai = own_settings(&own_base);
    rig.service.on_start(&ai);
    rig.service.status(&ai, true);
    let client = rig.client(&ai);
    assert_eq!(client.kind(), ProviderKind::OwnKey);
    echo(client.as_ref(), &cache, &prompt, "hola").expect("answered");
    rig.service.check_connection(&ai).expect("connected");
    assert_eq!(publik_fake.count(), 0, "publik API is not contacted");
    assert_eq!(publik_fake.connections(), 0);
    for request in &own.requests() {
        assert!(request.path.starts_with("/v1/"), "the own base URL");
        assert_key_only_in_authorization(request, OWN_KEY);
        assert!(!request.everything().contains(KEY));
        assert_eq!(
            request.header("x-publik-idempotency-key"),
            None,
            "the idempotency key is for publik API only"
        );
    }
    let chats = own.requests_to("POST", OWN_CHAT);
    assert_eq!(chats.len(), 2);
    for chat in &chats {
        assert_eq!(chat.json()["model"], OWN_MODEL);
    }
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 31: the 201 and 402 bodies in the status.
// ---------------------------------------------------------------------------------------------

#[test]
fn t31_the_201_and_402_bodies_fill_the_status_and_open_link_uses_them() {
    let fake = Fake::start();
    let url = fake.url();
    fake.on(
        "POST",
        "/installs",
        Reply::fixture("http/201-installs.json", &url),
    );
    fake.on(
        "POST",
        CHAT,
        Reply::fixture("http/402-anonymous.json", &url),
    );
    let mut rig = Rig::new(&url);
    let mut ai = AiSettings::default();

    let status = rig
        .service
        .consent_accept(&mut ai)
        .expect("consent provisions");
    assert_eq!(ai.provider, "publik");
    assert_eq!(status.provider, "publik");
    assert!(!status.needs_consent);
    assert!(status.connected);
    assert_eq!(status.cost_sentence.as_deref(), Some(COST_SENTENCE));
    assert_eq!(status.claim_url.as_deref(), Some(CLAIM_URL));
    assert_eq!(status.add_credit_url.as_deref(), Some(ADD_CREDIT_URL));
    assert_eq!(status.top_up_url.as_deref(), Some(WALLET_TOP_UP_URL));
    assert_eq!(status.claim_state.as_deref(), Some("anonymous"));
    assert_eq!(status.balance_micros, Some(0));
    assert_eq!(status.balance_line.as_deref(), Some(FRESH_LINE));
    assert_eq!(
        rig.service.link_url("claim").expect("the claim link"),
        CLAIM_URL
    );
    assert_eq!(
        rig.service
            .link_url("add_credit")
            .expect("the add-credit link"),
        ADD_CREDIT_URL
    );

    // Accepting again with a key that resolves sends nothing.
    rig.service
        .consent_accept(&mut ai)
        .expect("consent with a key");
    assert_eq!(fake.count_of("POST", "/installs"), 1);

    // After a restart the status comes from the credential file.
    rig.restart();
    let status = rig.service.status(&ai, false);
    assert_eq!(status.cost_sentence.as_deref(), Some(COST_SENTENCE));
    assert_eq!(status.claim_url.as_deref(), Some(CLAIM_URL));
    assert_eq!(status.add_credit_url.as_deref(), Some(ADD_CREDIT_URL));
    assert_eq!(status.claim_state.as_deref(), Some("anonymous"));
    assert_eq!(status.balance_micros, Some(0));
    assert_eq!(
        rig.service.link_url("claim").expect("the claim link"),
        CLAIM_URL
    );

    // A 402 gives the top-up link of its body.
    let client = rig.client(&ai);
    let cache = rig.cache();
    assert!(matches!(
        echo(client.as_ref(), &cache, &echo_prompt(1), "hola"),
        Err(AiError::NeedsCredit { .. })
    ));
    let status: AiStatus = rig.service.status(&ai, false);
    assert_eq!(status.top_up_url.as_deref(), Some(ANONYMOUS_402_URL));
    assert_eq!(
        rig.service.link_url("top_up").expect("the top-up link"),
        ANONYMOUS_402_URL
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 32 (in-process half): the built-app self-test (contract test 18).
// ---------------------------------------------------------------------------------------------

#[test]
fn t32_the_self_test_prints_one_line_and_runs_only_when_asked() {
    let fake = Fake::start();
    fake.on(
        "POST",
        "/installs",
        Reply::fixture("http/201-installs.json", &fake.url()),
    );
    let dir = tempdir();
    let env = penv(None, Some(&fake.url()), &credential_path(dir.path()));

    assert_eq!(
        parley_publik::startup_self_test_in(&env, None, "0.1.0"),
        None
    );
    assert_eq!(fake.count(), 0, "without PUBLIK_SELF_TEST nothing is sent");

    assert_eq!(
        parley_publik::startup_self_test_in(&env, Some(OsStr::new("1")), "0.1.0").as_deref(),
        Some("publik self-test: ok parley 0.1.0 (install 5f0c8a52)")
    );
    assert_eq!(fake.count_of("POST", "/installs"), 1);
    assert!(
        env.credential_path.is_file(),
        "the self-test provisions for real"
    );

    let failing = Fake::start();
    failing.on("POST", "/installs", Reply::status(503));
    let dir = tempdir();
    let env = penv(None, Some(&failing.url()), &credential_path(dir.path()));
    assert_eq!(
        parley_publik::startup_self_test_in(&env, Some(OsStr::new("1")), "0.1.0").as_deref(),
        Some(
            "publik self-test: failed: publik API could not set up this computer (503). Use your \
             own key for now."
        )
    );
    assert_eq!(failing.count(), 1);
    assert_eq!(
        publik::self_test_in(&env, "0.1.0"),
        "publik self-test: failed: publik API could not set up this computer (503). Use your \
         own key for now."
    );

    let env = PublikEnv {
        app_token: Some("none".into()),
        ..penv(None, Some(&failing.url()), &credential_path(dir.path()))
    };
    let expected = format!("publik self-test: failed: {NO_TOKEN}");
    assert_eq!(
        parley_publik::startup_self_test_in(&env, Some(OsStr::new("1")), "0.1.0").as_deref(),
        Some(expected.as_str())
    );
    assert_eq!(failing.count(), 2, "no request without a pat_ token");
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 16 (invariant I1): a provider message that quotes a key.
// ---------------------------------------------------------------------------------------------

#[test]
fn t16_a_provider_message_that_quotes_a_key_never_shows_it() {
    let prompt = echo_prompt(1);
    let mut seen: Vec<String> = Vec::new();

    // The learner's provider quotes the own key back in its 401.
    let own = Fake::start();
    own.on(
        "POST",
        OWN_CHAT,
        Reply::json(
            401,
            &json!({"error": {"message": format!("Incorrect API key provided: {OWN_KEY}")}}),
        ),
    );
    let mut rig = Rig::new(&closed_url());
    rig.store_own_key();
    let ai = own_settings(&format!("{}/v1", own.url()));
    let client = rig.client(&ai);
    let cache = rig.cache();
    let error = echo(client.as_ref(), &cache, &prompt, "hola").expect_err("a 401");
    record_error(&mut seen, error);
    let error = rig.service.check_connection(&ai).expect_err("a 401");
    record_app_error(&mut seen, &error);
    seen.push(format!("{error:?}"));

    // publik API quotes its key in a 402, a 500 and a 429.
    for reply in [
        Reply::json(
            402,
            &json!({"error": {
                "type": "insufficient_balance",
                "message": format!("The key {KEY} has $0.00 of publik balance."),
                "top_up_url": ANONYMOUS_402_URL,
            }}),
        ),
        Reply::json(
            500,
            &json!({"error": {"type": "server_error", "message": format!("upstream refused {KEY}")}}),
        ),
        Reply::json(
            429,
            &json!({"error": {"type": "rate_limited", "message": format!("Slow down, {KEY}.")}}),
        ),
    ] {
        let fake = Fake::start();
        fake.on("POST", CHAT, reply);
        let mut rig = Rig::with_credential(&fake.url(), "claimed", 3_120_000);
        let ai = publik_settings();
        let client = rig.client(&ai);
        let cache = rig.cache();
        let error = echo(client.as_ref(), &cache, &prompt, "hola").expect_err("refused");
        record_error(&mut seen, error);
        seen.push(json_of!(rig.service.status(&ai, false)));
        let error = rig.service.check_connection(&ai).expect_err("refused");
        record_app_error(&mut seen, &error);
        seen.push(format!("{error:?}"));
    }

    assert!(seen.len() >= 20, "the scan saw {} texts", seen.len());
    for text in &seen {
        assert!(!text.contains(KEY), "the publik API key is in: {text}");
        assert!(!text.contains(OWN_KEY), "the own key is in: {text}");
    }
}

// ---------------------------------------------------------------------------------------------
// Messages of the service decided during implementation: the keychain and the local server.
// ---------------------------------------------------------------------------------------------

#[test]
fn t23_a_keychain_that_refuses_to_keep_or_remove_the_key_says_so() {
    let own = Fake::start();
    own.on(
        "GET",
        "/v1/models",
        Reply::json(
            200,
            &json!({"object": "list", "data": [{"id": OWN_MODEL, "object": "model"}]}),
        ),
    );
    own.on("POST", OWN_CHAT, Reply::chat(P0_OK));
    let base = format!("{}/v1", own.url());

    let locked = DoubleKeychain {
        refuse_set: true,
        ..DoubleKeychain::default()
    };
    let dir = tempdir();
    let env = penv(None, Some(&closed_url()), &credential_path(dir.path()));
    let mut service = new_service_with(dir.path(), env, Box::new(locked.clone()));
    let mut ai = AiSettings::default();
    refused(
        service.set_own_key(&mut ai, &own_input(&base)),
        "storage",
        "Parley could not keep your key in this computer's keychain.",
    );
    assert_eq!(locked.slot.secret(), None);
    assert_eq!(ai.provider, "unset", "the provider stays as it was");

    let stuck = DoubleKeychain {
        refuse_delete: true,
        ..DoubleKeychain::default()
    };
    stuck.slot.set(OWN_KEY).expect("the memory slot stores");
    let dir = tempdir();
    let env = penv(None, Some(&closed_url()), &credential_path(dir.path()));
    let mut service = new_service_with(dir.path(), env, Box::new(stuck.clone()));
    let mut ai = own_settings(&base);
    refused(
        service.clear_own_key(&mut ai),
        "storage",
        "Parley could not remove your key from this computer's keychain.",
    );
    assert_eq!(stuck.slot.secret().as_deref(), Some(OWN_KEY));
}

#[test]
fn t22_a_local_server_with_no_chat_model_says_so() {
    let local = Fake::start();
    local.on(
        "GET",
        "/v1/models",
        Reply::json(
            200,
            &json!({"object": "list", "data": [{"id": "nomic-embed-text", "object": "model"}]}),
        ),
    );
    local.on(
        "GET",
        "/api/tags",
        Reply::json(
            200,
            &json!({"models": [{"name": "nomic-embed-text", "model": "nomic-embed-text", "size": 274_000_000_u64}]}),
        ),
    );
    let base = format!("{}/v1", local.url());
    let mut rig = Rig::new(&closed_url());
    let Err(error) = rig.service.client(&local_settings(&base, None)) else {
        panic!("a server with only an embedding model has no chat model");
    };
    let message = format!(
        "The local model server at {base} has no chat model. Install one, or choose another provider."
    );
    assert_eq!(error.message(), message);
    assert_eq!(AppError::from(error).message, message);
    assert_eq!(
        local.count_of("POST", OWN_CHAT),
        0,
        "no chat request is sent"
    );
}

// ---------------------------------------------------------------------------------------------
// Delete everything forgets the AI status.
// ---------------------------------------------------------------------------------------------

#[test]
fn delete_everything_forgets_the_ai_status_and_keeps_the_credential_and_the_own_key() {
    let fake = Fake::start();
    let url = fake.url();
    fake.on(
        "POST",
        CHAT,
        Reply::fixture("http/200.json", &url).with_chat(ECHO_HOLA),
    );
    fake.on(
        "POST",
        CHAT,
        Reply::fixture("http/402-anonymous.json", &url),
    );
    let mut rig = Rig::new(&url);
    // A credential with no balance of its own, so any balance shown comes from the status.
    let mut written = credential(&url, "anonymous", 0);
    written
        .as_object_mut()
        .expect("a JSON object")
        .remove("balance_micros");
    rig.write_credential(&written);
    rig.store_own_key();
    let ai = publik_settings();
    let client = rig.client(&ai);
    let cache = rig.cache();
    let prompt = echo_prompt(1);
    echo(client.as_ref(), &cache, &prompt, "uno").expect("answered");
    assert!(matches!(
        echo(client.as_ref(), &cache, &prompt, "dos"),
        Err(AiError::NeedsCredit { .. })
    ));
    let before = rig.service.status(&ai, false);
    assert!(before.needs_credit);
    assert!(before.balance_micros.is_some());
    assert_eq!(before.last_charge_micros, Some(17));
    let status_file = rig.path().join("publik-status.json");
    assert!(
        status_file.is_file(),
        "the status is kept in the data directory"
    );

    rig.service.forget_status();
    assert!(!status_file.exists(), "publik-status.json is removed");
    let after = rig.service.status(&ai, false);
    assert!(!after.needs_credit);
    assert!(after.connected);
    assert_eq!(after.balance_micros, None);
    assert_eq!(after.last_charge_micros, None);
    assert!(
        rig.credential_path().is_file(),
        "the publik credential lives outside the data directory and stays"
    );
    assert_eq!(
        rig.keychain.secret().as_deref(),
        Some(OWN_KEY),
        "the own key stays in the keychain"
    );
}

// ---------------------------------------------------------------------------------------------
// The keychain is read only when Settings opens.
// ---------------------------------------------------------------------------------------------

#[test]
fn t29_start_up_and_ai_status_read_the_keychain_only_when_settings_opens() {
    for provider in ["unset", "publik", "local"] {
        let publik_fake = Fake::start();
        let url = publik_fake.url();
        publik_fake.on("GET", "/wallet", Reply::fixture("http/wallet.json", &url));
        let local = Fake::start();
        script_local_models(&local);
        let dir = tempdir();
        write_json(
            &credential_path(dir.path()),
            &credential(&url, "claimed", 3_120_000),
        );
        let keychain = DoubleKeychain::default();
        keychain.slot.set(OWN_KEY).expect("the memory slot stores");
        let env = penv(None, Some(&url), &credential_path(dir.path()));
        let mut service = new_service_with(dir.path(), env, Box::new(keychain.clone()));
        let ai = match provider {
            "publik" => publik_settings(),
            "local" => local_settings(&format!("{}/v1", local.url()), Some(LOCAL_LARGE)),
            _ => AiSettings::default(),
        };

        let before = keychain.reads();
        service.on_start(&ai);
        service.status(&ai, false);
        assert_eq!(
            keychain.reads(),
            before,
            "{provider}: start-up and ai_status do not read the keychain"
        );
        service.status(&ai, true);
        assert!(
            keychain.reads() > before,
            "{provider}: opening Settings reads the keychain"
        );
    }
}
