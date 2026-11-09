//! The one pipeline every AI feature goes through (docs/PUBLIK-INTEGRATION.md §6.1): render the
//! prompt, compute the cache key, look up the cache, call once, parse, validate against the
//! schema, run the feature's rules, store only valid responses, return.
//!
//! Nothing is stored or returned before it passes all four checks (invariant I2). A hit makes no
//! request (I5). One call makes one request, never an automatic retry (I4): the own-key and local
//! clients may retry a failed connection inside `chat`, publik API never does.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::cache::{Cache, CacheEntry, cache_key, cache_key_json, canonical_json, idempotency_key};
use super::client::{AiClient, AiError, ChatRequest};
use super::money::format_dollars;
use super::prompts::{Prompt, ping};
use super::schema::parse_reply;
use crate::env::Rng;

/// A feature's answer, whether it came from the cache, and what the call cost (none for a hit).
#[derive(Debug, Clone, PartialEq)]
pub struct Called<T> {
    pub value: T,
    pub cached: bool,
    pub charge_micros: Option<i64>,
}

/// A feature rule an answer broke: the rule's name (`J2`) and what was wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleFailure {
    pub rule: String,
    pub detail: String,
}

/// Renders `feature` with `vars`; the key is cache_key_json(feature.id, feature.version,
/// client.kind(), client.chat_model(&feature.tier), key_input). A hit returns it with no request. A
/// miss sends one request with the idempotency key of attempt `attempts(key) + 1`; the answer is
/// parsed (parse_reply), checked against the schema, deserialised and checked by `rules`; only an
/// answer that passes all four is stored, under the attempt-free key. Any failure stores nothing,
/// adds one to `ai_attempts` for the key and returns `AiError::Invalid`. A transport error, timeout
/// or HTTP error changes no count, so a Try again after it sends the same key.
///
/// A 200 whose body has no usable answer (the client's own `AiError::Invalid`) counts like an
/// answer that fails its checks: publik API settled it, so a Try again must be a new request and
/// not a replay of it.
pub fn call<T: DeserializeOwned>(
    client: &dyn AiClient,
    cache: &Cache,
    feature: &Prompt,
    vars: &BTreeMap<String, String>,
    key_input: &Value,
    rules: &dyn Fn(&T) -> Result<(), RuleFailure>,
) -> Result<Called<T>, AiError> {
    // A prompt that does not render is a fault in the caller, caught by its tests; nothing is sent.
    let messages = feature.render(vars).map_err(|_| AiError::Invalid)?;
    let kind = client.kind();
    let model = client.chat_model(&feature.tier);
    let key_json = cache_key_json(&feature.id, feature.version, kind, &model, key_input);
    let key = cache_key(&key_json);

    if let Some(stored) = cache.get(&key)
        && let Ok(value) = serde_json::from_str::<T>(&stored)
    {
        return Ok(Called {
            value,
            cached: true,
            charge_micros: None,
        });
    }

    let attempt = cache.attempts(&key).saturating_add(1);
    let reply = match client.chat(ChatRequest {
        model: model.clone(),
        messages,
        max_tokens: feature.max_tokens,
        temperature: feature.temperature,
        timeout_s: feature.timeout_s,
        idempotency_key: Some(idempotency_key(&key_json, attempt)),
    }) {
        Ok(reply) => reply,
        Err(AiError::Invalid) => {
            let _ = cache.add_attempt(&key);
            return Err(AiError::Invalid);
        }
        Err(other) => return Err(other),
    };

    let checked = parse_reply(&reply.content)
        .filter(|value| feature.schema_valid(value))
        .and_then(|value| {
            let typed = serde_json::from_value::<T>(value.clone()).ok()?;
            rules(&typed).ok()?;
            Some((value, typed))
        });
    let Some((value, typed)) = checked else {
        // The count enters the next idempotency key, so Try again is a new request and not a
        // replay of the unusable answer. A count that cannot be written only costs that.
        let _ = cache.add_attempt(&key);
        return Err(AiError::Invalid);
    };
    // A cache that cannot be written loses only the saving; the valid answer is still returned.
    let _ = cache.put(&CacheEntry {
        key: &key,
        provider: kind,
        model: &model,
        prompt_id: &feature.id,
        prompt_version: feature.version,
        response: &canonical_json(&value),
        charge_micros: reply.charge_micros,
    });
    Ok(Called {
        value: typed,
        cached: false,
        charge_micros: reply.charge_micros,
    })
}

/// The result of P0, the connection check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionCheck {
    pub ok: bool,
    pub charge_micros: Option<i64>,
    pub message: String,
}

/// 32 lowercase hex from the generator.
fn random_key(rng: &mut dyn Rng) -> String {
    format!("{:016x}{:016x}", rng.next_u64(), rng.next_u64())
}

/// P0: never cached; a fresh idempotency key of 32 random lowercase hex from `rng` on every call;
/// message "Connected. This check cost {format_dollars(charge)}." or "Connected." with no charge.
pub fn check_connection(
    client: &dyn AiClient,
    rng: &mut dyn Rng,
) -> Result<ConnectionCheck, AiError> {
    let prompt = ping();
    let messages = prompt
        .render(&BTreeMap::new())
        .map_err(|_| AiError::Invalid)?;
    let reply = client.chat(ChatRequest {
        model: client.chat_model(&prompt.tier),
        messages,
        max_tokens: prompt.max_tokens,
        temperature: prompt.temperature,
        timeout_s: prompt.timeout_s,
        idempotency_key: Some(random_key(rng)),
    })?;
    let valid = parse_reply(&reply.content).is_some_and(|value| prompt.schema_valid(&value));
    if !valid {
        return Err(AiError::Invalid);
    }
    let message = match reply.charge_micros {
        Some(charge) => format!("Connected. This check cost {}.", format_dollars(charge)),
        None => "Connected.".to_string(),
    };
    Ok(ConnectionCheck {
        ok: true,
        charge_micros: reply.charge_micros,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{
        AiStatus, AudioReply, ChatReply, ProviderKind, SpeechRequest, TranscribeRequest,
        TranscriptReply,
    };
    use crate::env::{Clock, SplitMix64};
    use std::sync::Mutex;

    /// A client that answers chat from a script and records each request.
    struct Scripted {
        replies: Mutex<Vec<Result<ChatReply, AiError>>>,
        seen: Mutex<Vec<ChatRequest>>,
    }

    impl Scripted {
        fn new(replies: Vec<Result<ChatReply, AiError>>) -> Scripted {
            Scripted {
                replies: Mutex::new(replies),
                seen: Mutex::new(Vec::new()),
            }
        }
        fn keys(&self) -> Vec<String> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .map(|r| r.idempotency_key.clone().unwrap())
                .collect()
        }
    }

    fn reply(content: &str, charge: Option<i64>) -> Result<ChatReply, AiError> {
        Ok(ChatReply {
            content: content.into(),
            charge_micros: charge,
            request_id: None,
        })
    }

    impl AiClient for Scripted {
        fn chat(&self, req: ChatRequest) -> Result<ChatReply, AiError> {
            self.seen.lock().unwrap().push(req);
            self.replies.lock().unwrap().remove(0)
        }
        fn chat_stream(
            &self,
            req: ChatRequest,
            _on_delta: &mut dyn FnMut(&str),
        ) -> Result<ChatReply, AiError> {
            self.chat(req)
        }
        fn transcribe(&self, _req: TranscribeRequest) -> Result<TranscriptReply, AiError> {
            Err(AiError::Invalid)
        }
        fn speech(&self, _req: SpeechRequest) -> Result<AudioReply, AiError> {
            Err(AiError::Invalid)
        }
        fn kind(&self) -> ProviderKind {
            ProviderKind::Publik
        }
        fn status(&self) -> AiStatus {
            AiStatus::default()
        }
        fn chat_model(&self, tier: &str) -> String {
            tier.to_string()
        }
    }

    struct Fixed;

    impl Clock for Fixed {
        fn now_ms(&self) -> i64 {
            0
        }
        fn utc_offset_minutes(&self) -> i32 {
            0
        }
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Verdict {
        verdict: String,
    }

    fn judge() -> Prompt {
        Prompt::parse(
            "---\nid: judge\nversion: 1\ntier: publik-fast\nmax_tokens: 200\ntimeout_s: 10\nschema: s\n---\n## system\nJudge.\n## user\n{answer_json}\n",
            r#"{"type":"object","required":["verdict"],"properties":{"verdict":{"enum":["correct","wrong"]}},"additionalProperties":false}"#,
        )
        .unwrap()
    }

    fn not_almost(v: &Verdict) -> Result<(), RuleFailure> {
        if v.verdict == "wrong" {
            Err(RuleFailure {
                rule: "J9".into(),
                detail: "test rule".into(),
            })
        } else {
            Ok(())
        }
    }

    #[test]
    fn a_valid_answer_is_cached_and_a_second_call_makes_no_request() {
        let client = Scripted::new(vec![reply("{\"verdict\":\"correct\"}", Some(150))]);
        let cache = Cache::in_memory(Box::new(Fixed));
        let vars: BTreeMap<String, String> = [("answer_json".to_string(), "hola".to_string())]
            .into_iter()
            .collect();
        let input = serde_json::json!({"answer_norm": "hola"});
        let first: Called<Verdict> =
            call(&client, &cache, &judge(), &vars, &input, &not_almost).unwrap();
        assert_eq!((first.cached, first.charge_micros), (false, Some(150)));
        let second: Called<Verdict> =
            call(&client, &cache, &judge(), &vars, &input, &not_almost).unwrap();
        assert_eq!((second.cached, second.charge_micros), (true, None));
        assert_eq!(second.value.verdict, "correct");
        assert_eq!(client.seen.lock().unwrap().len(), 1);
        let key_json = cache_key_json("judge", 1, ProviderKind::Publik, "publik-fast", &input);
        assert_eq!(client.keys(), [idempotency_key(&key_json, 1)]);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn failures_store_nothing_and_move_the_attempt_but_transport_errors_do_not() {
        let client = Scripted::new(vec![
            reply("not json", None),
            reply("{\"verdict\":\"maybe\"}", None),
            reply("{\"verdict\":\"wrong\"}", None),
            Err(AiError::Timeout {
                provider: ProviderKind::Publik,
            }),
            reply("{\"verdict\":\"correct\"}", None),
        ]);
        let cache = Cache::in_memory(Box::new(Fixed));
        let vars: BTreeMap<String, String> = [("answer_json".to_string(), "x".to_string())]
            .into_iter()
            .collect();
        let input = serde_json::json!({"a": 1});
        for _ in 0..3 {
            let result: Result<Called<Verdict>, AiError> =
                call(&client, &cache, &judge(), &vars, &input, &not_almost);
            assert_eq!(result.unwrap_err(), AiError::Invalid);
            assert_eq!(cache.len(), 0);
        }
        let timeout: Result<Called<Verdict>, AiError> =
            call(&client, &cache, &judge(), &vars, &input, &not_almost);
        assert!(matches!(timeout, Err(AiError::Timeout { .. })));
        let ok: Called<Verdict> =
            call(&client, &cache, &judge(), &vars, &input, &not_almost).unwrap();
        assert!(!ok.cached);
        let key_json = cache_key_json("judge", 1, ProviderKind::Publik, "publik-fast", &input);
        assert_eq!(
            client.keys(),
            [
                idempotency_key(&key_json, 1),
                idempotency_key(&key_json, 2),
                idempotency_key(&key_json, 3),
                idempotency_key(&key_json, 4),
                idempotency_key(&key_json, 4),
            ]
        );
        assert!(cache.get(&cache_key(&key_json)).is_some());
    }

    #[test]
    fn an_answer_the_client_could_not_read_moves_the_attempt_and_other_errors_do_not() {
        let client = Scripted::new(vec![
            Err(AiError::Invalid),
            Err(AiError::Busy),
            Err(AiError::Offline),
            reply("{\"verdict\":\"correct\"}", None),
        ]);
        let cache = Cache::in_memory(Box::new(Fixed));
        let vars: BTreeMap<String, String> = [("answer_json".to_string(), "x".to_string())]
            .into_iter()
            .collect();
        let input = serde_json::json!({"b": 2});
        let key_json = cache_key_json("judge", 1, ProviderKind::Publik, "publik-fast", &input);
        let first: Result<Called<Verdict>, AiError> =
            call(&client, &cache, &judge(), &vars, &input, &not_almost);
        assert_eq!(first.unwrap_err(), AiError::Invalid);
        assert_eq!(cache.attempts(&cache_key(&key_json)), 1);
        assert_eq!(cache.len(), 0);
        for expected in [AiError::Busy, AiError::Offline] {
            let failed: Result<Called<Verdict>, AiError> =
                call(&client, &cache, &judge(), &vars, &input, &not_almost);
            assert_eq!(failed.unwrap_err(), expected);
        }
        let ok: Called<Verdict> =
            call(&client, &cache, &judge(), &vars, &input, &not_almost).unwrap();
        assert!(!ok.cached);
        assert_eq!(cache.attempts(&cache_key(&key_json)), 1);
        assert_eq!(
            client.keys(),
            [
                idempotency_key(&key_json, 1),
                idempotency_key(&key_json, 2),
                idempotency_key(&key_json, 2),
                idempotency_key(&key_json, 2),
            ]
        );
    }

    #[test]
    fn the_connection_check_uses_a_fresh_random_key_and_names_the_charge() {
        let client = Scripted::new(vec![
            reply("{\"ok\": true}", Some(17)),
            reply("{\"ok\": true}", None),
            reply("{\"ok\": false}", None),
        ]);
        let mut rng = SplitMix64::new(1);
        let first = check_connection(&client, &mut rng).unwrap();
        assert_eq!(
            first,
            ConnectionCheck {
                ok: true,
                charge_micros: Some(17),
                message: "Connected. This check cost $0.000017.".into()
            }
        );
        assert_eq!(
            check_connection(&client, &mut rng).unwrap().message,
            "Connected."
        );
        assert_eq!(
            check_connection(&client, &mut rng).unwrap_err(),
            AiError::Invalid
        );
        let keys = client.keys();
        assert_eq!(keys.len(), 3);
        assert_ne!(keys[0], keys[1]);
        assert!(keys.iter().all(|k| {
            k.len() == 32
                && k.chars()
                    .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        }));
        let seen = client.seen.lock().unwrap();
        assert_eq!(seen[0].model, "publik-fast");
        assert_eq!(
            seen[0].body(false),
            serde_json::json!({"model":"publik-fast","max_tokens":10,"temperature":0,"messages":[{"role":"system","content":"You are a connectivity check. Reply with exactly this JSON and nothing else: {\"ok\": true}"},{"role":"user","content":"ping"}]})
        );
    }
}
