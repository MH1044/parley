//! Acceptance tests of module 10 in parley-publik (docs/modules/10-speaking-and-pronunciation.md,
//! Acceptance test 7, and contract test 29 of docs/PUBLIK-INTEGRATION.md §11): the P6 request on
//! the wire. A take is sent as multipart form data with exactly the text parts of
//! `TranscribeRequest::fields` (`model`, `language`, `response_format`, `temperature`, in that
//! order), then the `file` part, and never a `prompt` part; the body stays under 4,000,000 bytes,
//! and a body of 4,000,000 bytes or more is refused before any connection (I10).
//!
//! All HTTP goes to the fake server of `fake.rs` on 127.0.0.1; no test reaches publikhq.com. The
//! service is built with a temporary data directory and credential path, a keychain fake, seeded
//! randomness and no retry back-off. Expected values come from §6.8 (the fields and the 15 s take
//! of 480,044 bytes) and §6.1 (the idempotency key, computed with the core's definition), and the
//! multipart form is read back here by its boundary, part by part, as RFC 7578 lays it out.

mod fake;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fake::{Fake, Reply};
use parley_core::ai::{
    AiClient, AiError, MAX_REQUEST_BYTES, PUBLIK_TRANSCRIBE_MODEL, ProviderKind, TranscribeRequest,
    idempotency_key, transcribe_key_json, transcribe_request,
};
use parley_core::content::Lang;
use parley_core::env::SplitMix64;
use parley_core::settings::{AiSettings, OwnKeySettings};
use parley_publik::keychain::{Keychain, MemoryKeychain};
use parley_publik::publik::PublikEnv;
use parley_publik::service::{AiService, AiServiceConfig};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const KEY: &str = "pk_live_a";
const OWN_KEY: &str = "sk-own-a";
const TOKEN: &str = "pat_parley_test";
const INSTALL_ID: &str = "5f0c8a52-6e0b-4c55-9a3e-2a3f1d9b7c11";
const TRANSCRIPTIONS: &str = "/audio/transcriptions";
const OWN_TRANSCRIPTIONS: &str = "/v1/audio/transcriptions";
const OWN_MODEL: &str = "whisper-own";

// ---------------------------------------------------------------------------------------------
// Takes.
// ---------------------------------------------------------------------------------------------

/// RIFF WAVE, 16 kHz mono 16-bit PCM: the 44-byte header written field by field, then `ms`
/// milliseconds of an integer square wave (period 40 samples, +/-16,000).
fn take(ms: usize) -> Vec<u8> {
    let samples = 16 * ms;
    let data_len = u32::try_from(samples * 2).expect("a short take");
    let mut out = Vec::with_capacity(44 + samples * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&16_000u32.to_le_bytes());
    out.extend_from_slice(&32_000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..samples {
        let level: i16 = if i % 40 < 20 { 16_000 } else { -16_000 };
        out.extend_from_slice(&level.to_le_bytes());
    }
    out
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The service.
// ---------------------------------------------------------------------------------------------

/// A credential file as `provision()` writes it from a 201 (the contract's fields).
fn credential(base_url: &str) -> Value {
    json!({
        "key": KEY,
        "base_url": base_url,
        "install_id": INSTALL_ID,
        "claim_url": "https://publikhq.com/claim/5f0c8a52",
        "claim_state": "claimed",
        "add_credit_url": "https://publikhq.com/wallet/add-credit",
        "cost_sentence": "A typical use costs about $0.01 on publik-balanced and $0.001 on publik-fast.",
        "balance_micros": 3_120_000,
    })
}

fn credential_path(dir: &Path) -> PathBuf {
    dir.join("publik").join("apps").join("parley.json")
}

/// A service over its own data directory whose credential file points publik API at
/// `base_url`, with `own_key` in the keychain fake when given.
struct Rig {
    _dir: tempfile::TempDir,
    service: AiService,
}

impl Rig {
    fn new(base_url: &str, own_key: Option<&str>) -> Rig {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = credential_path(dir.path());
        fs::create_dir_all(path.parent().expect("a parent folder")).expect("the folder is made");
        fs::write(
            &path,
            serde_json::to_string_pretty(&credential(base_url)).expect("JSON"),
        )
        .expect("the credential is written");
        let keychain = MemoryKeychain::default();
        if let Some(key) = own_key {
            keychain.set(key).expect("the keychain fake stores");
        }
        let service = AiService::new(AiServiceConfig {
            data_dir: dir.path().to_path_buf(),
            publik: PublikEnv {
                api_key: None,
                base_url: Some(base_url.to_string()),
                app_token: Some(TOKEN.to_string()),
                credential_path: path,
            },
            keychain: Box::new(keychain),
            rng: Box::new(SplitMix64::new(10)),
            backoff: [Duration::ZERO; 2],
            app_version: "0.1.0".to_string(),
        });
        Rig { _dir: dir, service }
    }

    fn client(&mut self, ai: &AiSettings) -> Arc<dyn AiClient> {
        self.service
            .client(ai)
            .unwrap_or_else(|e| panic!("a client for {}: {e:?}", ai.provider))
    }
}

fn publik_settings() -> AiSettings {
    AiSettings {
        provider: "publik".to_string(),
        ..AiSettings::default()
    }
}

fn own_settings(base_url: &str) -> AiSettings {
    AiSettings {
        provider: "own_key".to_string(),
        own_key: OwnKeySettings {
            base_url: Some(base_url.to_string()),
            chat_model: Some("gpt-own-mini".to_string()),
            transcribe_model: Some(OWN_MODEL.to_string()),
            ..OwnKeySettings::default()
        },
        ..AiSettings::default()
    }
}

/// A 200 transcription reply charged 538 micros.
fn transcript() -> Reply {
    Reply::json(200, &json!({"text": "Yo es estudiante."})).header("x-publik-charge-micros", "538")
}

// ---------------------------------------------------------------------------------------------
// Reading a multipart form.
// ---------------------------------------------------------------------------------------------

/// One part of a multipart/form-data body.
struct Part {
    name: String,
    file_name: Option<String>,
    content_type: Option<String>,
    body: Vec<u8>,
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The value of the parameter `key` (`key="..."`) of a Content-Disposition header.
fn quoted(header: &str, key: &str) -> Option<String> {
    header.split(';').find_map(|param| {
        let (name, value) = param.trim().split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case(key)
            .then(|| value.trim().trim_matches('"').to_string())
    })
}

/// The parts of a request's multipart/form-data body, in order. Every part sits between two
/// delimiter lines `--<boundary>`; the last delimiter is followed by `--`.
fn parts(request: &fake::Request) -> Vec<Part> {
    let content_type = request
        .header("content-type")
        .expect("the request has a Content-Type");
    assert!(
        content_type.starts_with("multipart/form-data"),
        "transcription is multipart: {content_type}"
    );
    let boundary = content_type
        .split("boundary=")
        .nth(1)
        .expect("a boundary")
        .trim_matches('"')
        .to_string();
    let delimiter = format!("--{boundary}").into_bytes();
    let body = &request.body;
    let mut at = Vec::new();
    let mut from = 0;
    while let Some(found) = find(&body[from..], &delimiter) {
        at.push(from + found);
        from += found + delimiter.len();
    }
    assert!(at.len() >= 2, "the body has its delimiters");
    assert_eq!(at[0], 0, "the body starts with the first delimiter");
    let last = at[at.len() - 1] + delimiter.len();
    assert_eq!(&body[last..last + 2], b"--", "the closing delimiter");
    at.windows(2)
        .map(|pair| {
            let chunk = &body[pair[0] + delimiter.len()..pair[1]];
            let chunk = chunk
                .strip_prefix(b"\r\n")
                .expect("a line break after the delimiter");
            let chunk = chunk
                .strip_suffix(b"\r\n")
                .expect("a line break before the next one");
            let split = find(chunk, b"\r\n\r\n").expect("the part's headers end");
            let head = String::from_utf8_lossy(&chunk[..split]).into_owned();
            let mut name = None;
            let mut file_name = None;
            let mut content_type = None;
            for line in head.split("\r\n") {
                let (header, value) = line.split_once(':').expect("a header line");
                match header.trim().to_ascii_lowercase().as_str() {
                    "content-disposition" => {
                        name = quoted(value, "name");
                        file_name = quoted(value, "filename");
                    }
                    "content-type" => content_type = Some(value.trim().to_string()),
                    _ => {}
                }
            }
            Part {
                name: name.expect("every part is named"),
                file_name,
                content_type,
                body: chunk[split + 4..].to_vec(),
            }
        })
        .collect()
}

/// The text parts as (name, value) and the one `file` part.
fn text_parts_and_file(request: &fake::Request) -> (Vec<(String, String)>, Part) {
    let mut texts = Vec::new();
    let mut file = None;
    for part in parts(request) {
        if part.name == "file" {
            assert!(file.is_none(), "one file part");
            file = Some(part);
        } else {
            assert!(file.is_none(), "the text parts come before the file part");
            texts.push((
                part.name,
                String::from_utf8(part.body).expect("a text part is UTF-8"),
            ));
        }
    }
    (texts, file.expect("a file part"))
}

// ---------------------------------------------------------------------------------------------
// Acceptance test 7 and contract test 29.
// ---------------------------------------------------------------------------------------------

/// Acceptance 7: "The P6 request is multipart with file (take.wav, audio/wav, the take's bytes),
/// model (gpt-4o-mini-transcribe), language=es, response_format=json, temperature=0, no prompt
/// field, and a body under 4,000,000 bytes". Contract test 29: the request carries `language`, no
/// prompt field, and a WAV under 4 MB.
#[test]
fn t07_the_p6_request_is_the_multipart_form_of_section_6_8() {
    let fake = Fake::start();
    fake.on("POST", TRANSCRIPTIONS, transcript());
    let mut rig = Rig::new(&fake.url(), None);
    let client = rig.client(&publik_settings());

    let wav = take(15_000);
    assert_eq!(wav.len(), 480_044, "a 15 s take");
    let request = transcribe_request(
        ProviderKind::Publik,
        PUBLIK_TRANSCRIBE_MODEL,
        &wav,
        Lang::Es,
        1,
    );
    let reply = client
        .transcribe(request.clone())
        .unwrap_or_else(|e| panic!("the transcription answers: {e:?}"));
    assert_eq!(reply.body, json!({"text": "Yo es estudiante."}));
    assert_eq!(reply.charge_micros, Some(538));

    let sent = fake.requests_to("POST", TRANSCRIPTIONS);
    assert_eq!(sent.len(), 1, "one call");
    let sent = &sent[0];
    let (texts, file) = text_parts_and_file(sent);
    assert_eq!(
        texts,
        [
            ("model".to_string(), "gpt-4o-mini-transcribe".to_string()),
            ("language".to_string(), "es".to_string()),
            ("response_format".to_string(), "json".to_string()),
            ("temperature".to_string(), "0".to_string()),
        ]
    );
    let fields: Vec<(String, String)> = request
        .fields()
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect();
    assert_eq!(texts, fields, "exactly the text parts of fields()");
    assert!(
        texts.iter().all(|(name, _)| name != "prompt"),
        "no prompt field: the expected sentence is never sent"
    );
    assert_eq!(file.file_name.as_deref(), Some("take.wav"));
    assert_eq!(file.content_type.as_deref(), Some("audio/wav"));
    assert!(file.body == wav, "the take's bytes, as they are");
    assert!(
        sent.body.len() < MAX_REQUEST_BYTES,
        "{} bytes is under 4,000,000",
        sent.body.len()
    );
    assert!(!sent.text().contains("prompt"), "no prompt anywhere");

    // §6.1: the idempotency key of the key JSON at attempt 1, and the key in Authorization only.
    let key_json = transcribe_key_json(
        ProviderKind::Publik,
        PUBLIK_TRANSCRIBE_MODEL,
        &sha256_hex(&wav),
        Lang::Es,
    );
    assert_eq!(
        sent.header("x-publik-idempotency-key"),
        Some(idempotency_key(&key_json, 1).as_str())
    );
    assert_eq!(
        sent.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
}

/// Acceptance 7: every language goes out as its code, and attempt 2's request carries attempt 2's
/// idempotency key (a Try again after an unusable reply).
#[test]
fn t07_the_language_and_the_attempt_reach_the_wire() {
    let fake = Fake::start();
    fake.on("POST", TRANSCRIPTIONS, transcript());
    let mut rig = Rig::new(&fake.url(), None);
    let client = rig.client(&publik_settings());
    let wav = take(1_000);
    for (lang, code) in [(Lang::Fr, "fr"), (Lang::De, "de")] {
        client
            .transcribe(transcribe_request(
                ProviderKind::Publik,
                PUBLIK_TRANSCRIBE_MODEL,
                &wav,
                lang,
                2,
            ))
            .unwrap_or_else(|e| panic!("the transcription answers: {e:?}"));
        let sent = fake.requests_to("POST", TRANSCRIPTIONS);
        let last = sent.last().expect("a request");
        let (texts, _) = text_parts_and_file(last);
        assert_eq!(texts[1], ("language".to_string(), code.to_string()));
        let key_json = transcribe_key_json(
            ProviderKind::Publik,
            PUBLIK_TRANSCRIBE_MODEL,
            &sha256_hex(&wav),
            lang,
        );
        assert_eq!(
            last.header("x-publik-idempotency-key"),
            Some(idempotency_key(&key_json, 2).as_str())
        );
    }
}

/// Acceptance 7, Interfaces: "parley-publik writes exactly these text parts, then the file part,
/// and never a prompt part" for every provider: the own key sends its transcription model.
#[test]
fn t07_the_own_key_sends_the_same_form_with_its_model() {
    let fake = Fake::start();
    fake.on("POST", OWN_TRANSCRIPTIONS, transcript());
    let base = format!("{}/v1", fake.url());
    let mut rig = Rig::new(&fake.url(), Some(OWN_KEY));
    let client = rig.client(&own_settings(&base));
    let wav = take(2_000);
    client
        .transcribe(transcribe_request(
            ProviderKind::OwnKey,
            OWN_MODEL,
            &wav,
            Lang::Es,
            1,
        ))
        .unwrap_or_else(|e| panic!("the transcription answers: {e:?}"));
    let sent = fake.requests_to("POST", OWN_TRANSCRIPTIONS);
    assert_eq!(sent.len(), 1);
    let (texts, file) = text_parts_and_file(&sent[0]);
    assert_eq!(
        texts,
        [
            ("model".to_string(), OWN_MODEL.to_string()),
            ("language".to_string(), "es".to_string()),
            ("response_format".to_string(), "json".to_string()),
            ("temperature".to_string(), "0".to_string()),
        ]
    );
    assert!(file.body == wav);
    assert_eq!(
        sent[0].header("authorization"),
        Some(format!("Bearer {OWN_KEY}").as_str())
    );
}

/// Contract test 29: "a body of 4,000,000 bytes or more is refused before any connection" (I10).
/// Audio of 4,000,000 bytes is refused; so is audio 100 bytes short of it, whose form (the parts'
/// headers and delimiters add more than 100 bytes) is over the limit; a 30 s turn of 960,044 bytes
/// is sent.
#[test]
fn contract_29_a_body_of_4_mb_or_more_is_refused_before_any_connection() {
    assert_eq!(MAX_REQUEST_BYTES, 4_000_000);
    let fake = Fake::start();
    fake.on("POST", TRANSCRIPTIONS, transcript());
    let mut rig = Rig::new(&fake.url(), None);
    let client = rig.client(&publik_settings());
    let base = transcribe_request(
        ProviderKind::Publik,
        PUBLIK_TRANSCRIBE_MODEL,
        &take(1_000),
        Lang::Es,
        1,
    );
    for size in [MAX_REQUEST_BYTES, MAX_REQUEST_BYTES - 100] {
        let large = TranscribeRequest {
            audio: vec![0; size],
            ..base.clone()
        };
        assert_eq!(
            client.transcribe(large).err(),
            Some(AiError::TooLarge),
            "{size} bytes of audio"
        );
        assert_eq!(
            (fake.count(), fake.connections()),
            (0, 0),
            "nothing is sent for {size} bytes of audio"
        );
    }

    let turn = take(30_000);
    assert_eq!(turn.len(), 960_044, "a 30 s turn");
    client
        .transcribe(TranscribeRequest {
            audio: turn.clone(),
            ..base
        })
        .unwrap_or_else(|e| panic!("a 30 s turn is sent: {e:?}"));
    let sent = fake.requests_to("POST", TRANSCRIPTIONS);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].body.len() < MAX_REQUEST_BYTES);
    let (_, file) = text_parts_and_file(&sent[0]);
    assert!(file.body == turn);
}
