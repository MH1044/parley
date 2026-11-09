//! One OpenAI-compatible client for the three providers (docs/PUBLIK-INTEGRATION.md §2 adaptation
//! 4, §3): publik API, the learner's own key, and a local model server. They differ only in where
//! requests go, which key rides in the `Authorization` header, what `model` a chat request names,
//! and what happens when something fails.
//!
//! - publik API: the tier is the model (`publik-smart` falls back to `publik-balanced` until this
//!   computer is linked); every response's `x-publik-*` headers update the status; the
//!   idempotency key rides in `x-publik-idempotency-key`; a failure is never retried, because
//!   publik settles usage even when the client gives up (invariant I4).
//! - Own key and local: the configured or chosen model; connection errors, timeouts and 5xx are
//!   retried twice, after the configured backoff (1 s and 4 s in the app).
//!
//! A request body of [`MAX_REQUEST_BYTES`] or more is refused before any connection (I10).

use std::time::Duration;

use parley_core::ai::{
    AiClient, AiError, AiStatus, AudioReply, ChatReply, ChatRequest, MAX_REQUEST_BYTES,
    ProviderKind, SpeechRequest, TranscribeRequest, TranscriptReply,
};
use parley_core::env::{HttpRequest, HttpResponse, NetError};
use serde_json::{Value, json};

use crate::errors::{body_json, net_error, provider_error, publik_error, redact_error};
use crate::http::{self, header};
use crate::status::StatusHandle;

/// The client of the chosen provider.
pub struct ProviderClient {
    kind: ProviderKind,
    base_url: String,
    /// Never printed: the debug form leaves it out.
    api_key: Option<String>,
    chat_model: Option<String>,
    backoff: [Duration; 2],
    status: StatusHandle,
}

impl std::fmt::Debug for ProviderClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderClient")
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field("chat_model", &self.chat_model)
            .finish_non_exhaustive()
    }
}

/// A base URL without its trailing slashes.
pub fn trim_base(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

impl ProviderClient {
    /// publik API at `base_url` with the install's key.
    pub fn publik(base_url: &str, api_key: &str, status: StatusHandle) -> ProviderClient {
        ProviderClient {
            kind: ProviderKind::Publik,
            base_url: trim_base(base_url),
            api_key: Some(api_key.to_string()),
            chat_model: None,
            backoff: [Duration::ZERO; 2],
            status,
        }
    }

    /// The learner's own OpenAI-compatible endpoint.
    pub fn own_key(
        base_url: &str,
        api_key: &str,
        chat_model: &str,
        backoff: [Duration; 2],
        status: StatusHandle,
    ) -> ProviderClient {
        ProviderClient {
            kind: ProviderKind::OwnKey,
            base_url: trim_base(base_url),
            api_key: Some(api_key.to_string()),
            chat_model: Some(chat_model.to_string()),
            backoff,
            status,
        }
    }

    /// A model server on this computer; no key.
    pub fn local(
        base_url: &str,
        chat_model: &str,
        backoff: [Duration; 2],
        status: StatusHandle,
    ) -> ProviderClient {
        ProviderClient {
            kind: ProviderKind::Local,
            base_url: trim_base(base_url),
            api_key: None,
            chat_model: Some(chat_model.to_string()),
            backoff,
            status,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        content_type: Option<String>,
        body: Vec<u8>,
        idempotency_key: Option<&str>,
    ) -> Result<HttpRequest, AiError> {
        if body.len() >= MAX_REQUEST_BYTES {
            return Err(AiError::TooLarge);
        }
        let mut headers = Vec::new();
        if let Some(key) = &self.api_key {
            headers.push(("authorization".to_string(), format!("Bearer {key}")));
        }
        if let Some(content_type) = content_type {
            headers.push(("content-type".to_string(), content_type));
        }
        if self.kind == ProviderKind::Publik
            && let Some(key) = idempotency_key
        {
            headers.push(("x-publik-idempotency-key".to_string(), key.to_string()));
        }
        Ok(HttpRequest {
            method: method.to_string(),
            url: format!("{}{path}", self.base_url),
            headers,
            body,
        })
    }

    /// Tries a request: once on publik API, up to three times on the others (a connection error,
    /// a timeout or a 5xx is retried after the backoff). `send` reports whether anything reached
    /// the caller already (a streamed delta), after which nothing is retried.
    fn exchange(
        &self,
        send: &mut dyn FnMut() -> (Result<HttpResponse, NetError>, bool),
    ) -> Result<HttpResponse, AiError> {
        let tries = if self.kind == ProviderKind::Publik {
            1
        } else {
            3
        };
        let mut attempt = 0;
        loop {
            let (result, delivered) = send();
            let retryable = match &result {
                Ok(response) => response.status >= 500,
                Err(NetError::Offline | NetError::Timeout) => true,
                Err(_) => false,
            };
            if retryable && !delivered && attempt + 1 < tries {
                std::thread::sleep(self.backoff[attempt.min(1)]);
                attempt += 1;
                continue;
            }
            return match result {
                Ok(response) => self.answered(response),
                Err(e) => Err(net_error(self.kind, &e, &self.base_url)),
            };
        }
    }

    /// Updates the status from a response and turns an error status into its error.
    fn answered(&self, response: HttpResponse) -> Result<HttpResponse, AiError> {
        if self.kind == ProviderKind::Publik {
            self.status.update(|s| s.apply_headers(&response.headers));
        }
        if (200..300).contains(&response.status) {
            self.status.update(|s| s.succeeded());
            return Ok(response);
        }
        // A server's text can echo the key it was sent; it never reaches a message (I1).
        let key = self.api_key.as_deref();
        if self.kind != ProviderKind::Publik {
            let error = provider_error(self.kind, response.status, &response.body);
            return Err(redact_error(error, key));
        }
        let fallback = self.status.read(|s| s.top_up_url_wallet.clone());
        let error = redact_error(publik_error(response.status, &response.body, fallback), key);
        match &error {
            AiError::NeedsCredit { top_up_url, .. } => {
                let link = Some(top_up_url.clone()).filter(|u| !u.is_empty());
                self.status.update(|s| s.needs_credit(link));
            }
            AiError::Disconnected => self.status.update(|s| s.connected = false),
            _ => {}
        }
        Err(error)
    }

    fn charge(&self, response: &HttpResponse) -> Option<i64> {
        header(&response.headers, "x-publik-charge-micros").and_then(|v| v.trim().parse().ok())
    }

    fn json_post(
        &self,
        path: &str,
        body: Vec<u8>,
        timeout_s: u64,
        idempotency_key: Option<&str>,
    ) -> Result<HttpResponse, AiError> {
        let req = self.request(
            "POST",
            path,
            Some("application/json".into()),
            body,
            idempotency_key,
        )?;
        let timeout = Duration::from_secs(timeout_s.max(1));
        self.exchange(&mut || (http::send(&req, timeout), false))
    }
}

/// The assistant's text of a chat completion.
fn completion_text(body: &Value) -> Option<String> {
    match &body["choices"][0]["message"]["content"] {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => Some(
            parts
                .iter()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join(""),
        ),
        _ => None,
    }
}

impl AiClient for ProviderClient {
    fn chat(&self, req: ChatRequest) -> Result<ChatReply, AiError> {
        let response = self.json_post(
            "/chat/completions",
            req.body_bytes(false),
            req.timeout_s,
            req.idempotency_key.as_deref(),
        )?;
        let content = completion_text(&body_json(&response.body)).ok_or(AiError::Invalid)?;
        Ok(ChatReply {
            content,
            charge_micros: self.charge(&response),
            request_id: header(&response.headers, "x-publik-request-id").map(str::to_string),
        })
    }

    fn chat_stream(
        &self,
        req: ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<ChatReply, AiError> {
        let http_req = self.request(
            "POST",
            "/chat/completions",
            Some("application/json".into()),
            req.body_bytes(true),
            req.idempotency_key.as_deref(),
        )?;
        let timeout = Duration::from_secs(req.timeout_s.max(1));
        let mut content = String::new();
        let response = self.exchange(&mut || {
            content.clear();
            let mut delivered = false;
            let result = http::send_events(&http_req, timeout, &mut |data| {
                if data.trim() == "[DONE]" {
                    return;
                }
                let event: Value = serde_json::from_str(data).unwrap_or(Value::Null);
                if let Some(piece) = event["choices"][0]["delta"]["content"].as_str()
                    && !piece.is_empty()
                {
                    content.push_str(piece);
                    delivered = true;
                    on_delta(piece);
                }
            });
            (result, delivered)
        })?;
        // A server that ignored "stream" answers with one whole completion.
        if content.is_empty() && !response.body.is_empty() {
            content = completion_text(&body_json(&response.body)).ok_or(AiError::Invalid)?;
            on_delta(&content);
        }
        Ok(ChatReply {
            content,
            charge_micros: self.charge(&response),
            request_id: header(&response.headers, "x-publik-request-id").map(str::to_string),
        })
    }

    fn transcribe(&self, req: TranscribeRequest) -> Result<TranscriptReply, AiError> {
        let boundary = format!("parley-{}", uuid::Uuid::new_v4().simple());
        let mut body = Vec::with_capacity(req.audio.len() + 1024);
        for (name, value) in [
            ("model", req.model.as_str()),
            ("language", req.language.as_str()),
            ("response_format", req.response_format.as_str()),
        ] {
            body.extend_from_slice(
                format!(
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
                )
                .as_bytes(),
            );
        }
        let file_name = req.file_name.replace(['"', '\r', '\n'], "_");
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: {}\r\n\r\n",
                req.mime
            )
            .as_bytes(),
        );
        body.extend_from_slice(&req.audio);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let http_req = self.request(
            "POST",
            "/audio/transcriptions",
            Some(format!("multipart/form-data; boundary={boundary}")),
            body,
            req.idempotency_key.as_deref(),
        )?;
        let timeout = Duration::from_secs(req.timeout_s.max(1));
        let response = self.exchange(&mut || (http::send(&http_req, timeout), false))?;
        let body: Value = serde_json::from_slice(&response.body).map_err(|_| AiError::Invalid)?;
        Ok(TranscriptReply {
            body,
            charge_micros: self.charge(&response),
        })
    }

    fn speech(&self, req: SpeechRequest) -> Result<AudioReply, AiError> {
        let body = serde_json::to_vec(&json!({
            "model": req.model,
            "input": req.input,
            "voice": req.voice,
            "response_format": req.response_format,
            "speed": req.speed,
        }))
        .expect("a speech request serialises");
        let response = self.json_post(
            "/audio/speech",
            body,
            req.timeout_s,
            req.idempotency_key.as_deref(),
        )?;
        Ok(AudioReply {
            content_type: header(&response.headers, "content-type")
                .unwrap_or("application/octet-stream")
                .to_string(),
            charge_micros: self.charge(&response),
            bytes: response.body,
        })
    }

    fn kind(&self) -> ProviderKind {
        self.kind
    }

    fn status(&self) -> AiStatus {
        self.status.snapshot()
    }

    fn chat_model(&self, tier: &str) -> String {
        match self.kind {
            ProviderKind::Publik => {
                let claimed = self.status.snapshot().claim_state.as_deref() == Some("claimed");
                if tier == "publik-smart" && !claimed {
                    "publik-balanced".to_string()
                } else {
                    tier.to_string()
                }
            }
            ProviderKind::OwnKey | ProviderKind::Local => {
                self.chat_model.clone().unwrap_or_default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::test_server::{Reply, closed_url, serve};
    use parley_core::ai::{ChatMessage, image_part};
    use std::time::Instant;

    fn ping(model: &str, key: Option<&str>) -> ChatRequest {
        ChatRequest {
            model: model.into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: json!("ping"),
            }],
            max_tokens: 10,
            temperature: 0.0,
            timeout_s: 5,
            idempotency_key: key.map(str::to_string),
        }
    }

    const OK: &str = r#"{"choices":[{"message":{"role":"assistant","content":"{\"ok\": true}"}}]}"#;
    const FAST: [Duration; 2] = [Duration::from_millis(1), Duration::from_millis(1)];

    fn base(server: &crate::http::test_server::Server) -> String {
        server.url("/v1")
    }

    #[test]
    fn publik_calls_carry_the_key_tier_and_idempotency_key_and_read_the_headers() {
        let server = serve(vec![
            Reply::json(200, OK)
                .header("x-publik-charge-micros", "17")
                .header("x-publik-request-id", "req_9")
                .header("x-publik-balance", "49983"),
        ]);
        let status = StatusHandle::new(None);
        let client = ProviderClient::publik(&base(&server), "pk_live_a", status.clone());
        let reply = client
            .chat(ping(
                "publik-fast",
                Some("0123456789abcdef0123456789abcdef"),
            ))
            .unwrap();
        assert_eq!(reply.content, "{\"ok\": true}");
        assert_eq!(reply.charge_micros, Some(17));
        assert_eq!(reply.request_id.as_deref(), Some("req_9"));
        assert_eq!(status.snapshot().balance_micros, Some(49_983));
        let seen = &server.requests()[0];
        assert_eq!(seen.path, "/v1/chat/completions");
        assert_eq!(seen.header("authorization"), Some("Bearer pk_live_a"));
        assert_eq!(
            seen.header("x-publik-idempotency-key"),
            Some("0123456789abcdef0123456789abcdef")
        );
        let body: Value = serde_json::from_slice(&seen.body).unwrap();
        assert_eq!(body["model"], "publik-fast");
        assert!(!format!("{client:?}").contains("pk_live_a"));
    }

    #[test]
    fn publik_smart_waits_for_a_linked_computer() {
        let status = StatusHandle::new(None);
        let client = ProviderClient::publik("http://127.0.0.1:9/v1", "pk_live_a", status.clone());
        assert_eq!(client.chat_model("publik-smart"), "publik-balanced");
        assert_eq!(client.chat_model("publik-fast"), "publik-fast");
        status.update(|s| s.claim_state = Some("claimed".into()));
        assert_eq!(client.chat_model("publik-smart"), "publik-smart");
        let own = ProviderClient::own_key(
            "https://api.example.invalid/v1/",
            "k",
            "gpt-x",
            FAST,
            status,
        );
        assert_eq!(own.chat_model("publik-fast"), "gpt-x");
        assert_eq!(own.base_url(), "https://api.example.invalid/v1");
    }

    #[test]
    fn publik_never_retries_but_the_own_key_retries_twice() {
        let server = serve(vec![Reply::json(500, ""); 4]);
        let status = StatusHandle::new(None);
        let publik = ProviderClient::publik(&base(&server), "pk_live_a", status.clone());
        assert_eq!(
            publik
                .chat(ping("publik-fast", None))
                .unwrap_err()
                .to_string(),
            "publik API 500: Internal Server Error"
        );
        assert_eq!(server.requests().len(), 1);
        let own = ProviderClient::own_key(&base(&server), "sk", "m", FAST, status.clone());
        assert_eq!(
            own.chat(ping("m", Some("abc"))).unwrap_err().to_string(),
            "Your provider returned 500: Internal Server Error"
        );
        let seen = server.requests();
        assert_eq!(
            seen.len(),
            4,
            "one publik API attempt, then three own-key attempts"
        );
        assert!(
            seen[1..]
                .iter()
                .all(|r| r.header("x-publik-idempotency-key").is_none())
        );
        let slow = serve(vec![
            Reply::json(200, OK).after(Duration::from_millis(1500)),
            Reply::json(200, OK).after(Duration::from_millis(1500)),
            Reply::json(200, OK).after(Duration::from_millis(1500)),
        ]);
        let mut req = ping("m", None);
        req.timeout_s = 1;
        let started = Instant::now();
        let publik = ProviderClient::publik(&base(&slow), "pk_live_a", status.clone());
        assert_eq!(
            publik.chat(req.clone()).unwrap_err(),
            AiError::Timeout {
                provider: ProviderKind::Publik
            }
        );
        assert!(started.elapsed() < Duration::from_millis(2500));
        let local = ProviderClient::local(&closed_url("/v1"), "m", FAST, status);
        assert!(matches!(
            local.chat(req).unwrap_err(),
            AiError::LocalDown { .. }
        ));
    }

    #[test]
    fn a_402_sets_needs_credit_with_its_link_and_a_success_clears_it() {
        let server = serve(vec![
            Reply::json(
                402,
                r#"{"error":{"message":"Link this computer for $0.05 of free use.","top_up_url":"https://example.invalid/claim"}}"#,
            ),
            Reply::json(401, r#"{"error":{"type":"key_revoked"}}"#),
            Reply::json(200, OK),
        ]);
        let status = StatusHandle::new(None);
        let client = ProviderClient::publik(&base(&server), "pk_live_a", status.clone());
        let err = client.chat(ping("publik-fast", None)).unwrap_err();
        assert_eq!(err.to_string(), "Link this computer for $0.05 of free use.");
        let snapshot = status.snapshot();
        assert!(snapshot.needs_credit);
        assert_eq!(
            snapshot.top_up_url.as_deref(),
            Some("https://example.invalid/claim")
        );
        assert_eq!(
            client.chat(ping("publik-fast", None)).unwrap_err(),
            AiError::Disconnected
        );
        assert!(!status.snapshot().connected);
        client.chat(ping("publik-fast", None)).unwrap();
        assert!(!status.snapshot().needs_credit);
    }

    #[test]
    fn a_key_echoed_in_an_error_body_is_replaced() {
        let server = serve(vec![
            Reply::json(
                401,
                r#"{"error":{"message":"Incorrect API key provided: sk-own-a"}}"#,
            ),
            Reply::json(
                400,
                r#"{"error":{"message":"key pk_live_a cannot use this model"}}"#,
            ),
        ]);
        let status = StatusHandle::new(None);
        let own = ProviderClient::own_key(&base(&server), "sk-own-a", "m", FAST, status.clone());
        let err = own.chat(ping("m", None)).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Your provider returned 401: Incorrect API key provided: …"
        );
        let publik = ProviderClient::publik(&base(&server), "pk_live_a", status);
        let err = publik.chat(ping("publik-fast", None)).unwrap_err();
        assert_eq!(
            err.to_string(),
            "publik API 400: key … cannot use this model"
        );
        assert!(!format!("{err:?}").contains("pk_live_a"));
    }

    #[test]
    fn a_body_of_four_million_bytes_or_more_is_never_sent() {
        let server = serve(vec![Reply::json(200, OK)]);
        let client = ProviderClient::publik(&base(&server), "pk_live_a", StatusHandle::new(None));
        let mut req = ping("publik-balanced", None);
        req.messages[0].content = json!([image_part(&vec![7u8; 3_000_000])]);
        assert_eq!(client.chat(req).unwrap_err(), AiError::TooLarge);
        let mut small = ping("publik-balanced", None);
        small.messages[0].content = json!([image_part(&[1, 2, 3])]);
        client.chat(small).unwrap();
        assert_eq!(server.requests().len(), 1);
    }

    #[test]
    fn streamed_deltas_arrive_in_order_and_audio_goes_out_as_given() {
        let events = concat!(
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"{\\\"reply\\\": \\\"Ho\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"la\\\"}\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let server = serve(vec![
            Reply {
                status: 200,
                headers: vec![("content-type".into(), "text/event-stream".into())],
                body: events.as_bytes().to_vec(),
                delay: Duration::ZERO,
            },
            Reply::json(200, r#"{"text":"hola"}"#),
            Reply {
                status: 200,
                headers: vec![("content-type".into(), "audio/mpeg".into())],
                body: vec![1, 2, 3],
                delay: Duration::ZERO,
            },
        ]);
        let client = ProviderClient::publik(&base(&server), "pk_live_a", StatusHandle::new(None));
        let mut deltas = Vec::new();
        let reply = client
            .chat_stream(ping("publik-balanced", None), &mut |d| {
                deltas.push(d.to_string())
            })
            .unwrap();
        assert_eq!(reply.content, "{\"reply\": \"Hola\"}");
        assert_eq!(deltas, ["{\"reply\": \"Ho", "la\"}"]);
        let transcript = client
            .transcribe(TranscribeRequest {
                model: "whisper-x".into(),
                audio: b"RIFF".to_vec(),
                file_name: "take.wav".into(),
                mime: "audio/wav".into(),
                language: "es".into(),
                response_format: "json".into(),
                timeout_s: 5,
                idempotency_key: None,
            })
            .unwrap();
        assert_eq!(transcript.body, json!({"text": "hola"}));
        let audio = client
            .speech(SpeechRequest {
                model: "tts-x".into(),
                input: "Hola.".into(),
                voice: "v1".into(),
                response_format: "mp3".into(),
                speed: 0.75,
                timeout_s: 5,
                idempotency_key: None,
            })
            .unwrap();
        assert_eq!(
            (audio.bytes, audio.content_type.as_str()),
            (vec![1, 2, 3], "audio/mpeg")
        );
        let seen = server.requests();
        let body: Value = serde_json::from_slice(&seen[0].body).unwrap();
        assert_eq!(body["stream"], json!(true));
        let multipart = String::from_utf8_lossy(&seen[1].body).to_string();
        assert!(multipart.contains("name=\"model\"\r\n\r\nwhisper-x\r\n"));
        assert!(multipart.contains("filename=\"take.wav\""));
        assert!(
            seen[1]
                .header("content-type")
                .unwrap()
                .starts_with("multipart/form-data; boundary=")
        );
        let speech: Value = serde_json::from_slice(&seen[2].body).unwrap();
        assert_eq!(speech["model"], "tts-x");
        assert_eq!(seen[2].path, "/v1/audio/speech");
    }
}
