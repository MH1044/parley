//! Everything the `ai_*` commands do (docs/modules/06-publik-and-providers.md, Interfaces): provider
//! choice, consent and provisioning, the own key in the keychain, the local model probe, the
//! connection check, disconnecting and reconnecting publik API, and the status.
//!
//! Only the chosen provider is ever contacted (invariant I9; docs/PUBLIK-INTEGRATION.md §3):
//! `own_key` passes the keychain's key to the resolver; `publik` and `unset` resolve without it, so
//! a stored own key is never used while another provider is chosen; `local` calls no resolver and
//! reads no credential. Settings stay in `parley-core`: a method that changes the provider takes
//! the AI settings by `&mut`, and the command saves them.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parley_core::ai::{AiClient, AiError, AiStatus, ConnectionCheck, LocalModel, check_connection};
use parley_core::env::{HttpRequest, Rng};
use parley_core::error::{AppError, ErrorCode};
use parley_core::settings::AiSettings;
use serde_json::Value;

use crate::errors::{body_json, is_revoked, setup_error};
use crate::http::{self, is_loopback};
use crate::keychain::Keychain;
use crate::providers::{ProviderClient, trim_base};
use crate::publik::{
    Provider, PublikEnv, device_label, provision_reply_in, resolve_provider_in, write_credential_in,
};
use crate::status::{CredentialView, StatusHandle};

/// How long a wallet refresh, a revoke or a probe may take.
const QUICK_TIMEOUT: Duration = Duration::from_secs(15);

/// What the service is given.
pub struct AiServiceConfig {
    /// `publik-status.json` is written here.
    pub data_dir: PathBuf,
    pub publik: PublikEnv,
    pub keychain: Box<dyn Keychain>,
    /// P0's random idempotency key.
    pub rng: Box<dyn Rng + Send>,
    /// Own-key and local retries: [1 s, 4 s] in the app.
    pub backoff: [Duration; 2],
    pub app_version: String,
}

/// The learner's own endpoint and key, as `ai_set_own_key` receives them.
#[derive(Clone, PartialEq, Eq)]
pub struct OwnKeyInput {
    pub base_url: String,
    pub key: String,
    pub chat_model: String,
    pub transcribe_model: Option<String>,
    pub speech_model: Option<String>,
    pub speech_voice: Option<String>,
}

// The key is never printed.
impl std::fmt::Debug for OwnKeyInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnKeyInput")
            .field("base_url", &self.base_url)
            .field("chat_model", &self.chat_model)
            .field("transcribe_model", &self.transcribe_model)
            .field("speech_model", &self.speech_model)
            .field("speech_voice", &self.speech_voice)
            .finish_non_exhaustive()
    }
}

/// The AI service of one app.
pub struct AiService {
    data_dir: PathBuf,
    publik: PublikEnv,
    keychain: Box<dyn Keychain>,
    rng: Box<dyn Rng + Send>,
    backoff: [Duration; 2],
    app_version: String,
    status: StatusHandle,
    /// The last local probe's models, largest first.
    probed: Vec<LocalModel>,
    /// Whether the keychain held an own key when it was last read; false until it is read.
    own_key_stored: bool,
}

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message)
}

fn not_local() -> AppError {
    invalid("The local model server must be on this computer: use a localhost address.")
}

/// `localhost`, `127.0.0.1` or `[::1]`, over http or https.
fn local_host(base_url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(base_url.trim()) else {
        return false;
    };
    matches!(url.scheme(), "http" | "https")
        && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

fn some_text(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

impl AiService {
    pub fn new(config: AiServiceConfig) -> AiService {
        let status = StatusHandle::new(Some(config.data_dir.clone()));
        AiService {
            data_dir: config.data_dir,
            publik: config.publik,
            keychain: config.keychain,
            rng: config.rng,
            backoff: config.backoff,
            app_version: config.app_version,
            status,
            probed: Vec::new(),
            own_key_stored: false,
        }
    }

    /// "Delete everything": forgets the status (back to its defaults: connected, no needs-credit,
    /// no headers, balance or links), the last probe and what the keychain was last seen to hold,
    /// and removes `publik-status.json`. The publik credential (outside the data directory; it
    /// holds this install's balance) and the own key in the keychain are kept.
    pub fn forget_status(&mut self) {
        self.status.reset();
        self.probed.clear();
        self.own_key_stored = false;
    }

    /// The folder `publik-status.json` lives in.
    pub fn data_dir(&self) -> &std::path::Path {
        &self.data_dir
    }

    /// Reads the own key from the keychain and remembers whether there is one. The keychain is
    /// read only when it is needed: with `own_key` chosen, when Settings > AI opens, and when the
    /// key is saved or removed. A launch with another provider never touches it (on Linux a locked
    /// keyring would ask for its password at launch).
    fn own_key(&mut self) -> Option<String> {
        let key = self
            .keychain
            .get()
            .ok()
            .flatten()
            .filter(|k| !k.trim().is_empty());
        self.own_key_stored = key.is_some();
        key
    }

    /// The publik API key and base URL when they resolve (without the own key).
    fn publik_resolved(&self) -> Option<(String, String, Option<Value>)> {
        match resolve_provider_in(&self.publik, None) {
            Provider::Publik {
                base_url,
                api_key,
                credential,
            } => Some((base_url, api_key, credential)),
            _ => None,
        }
    }

    /// Brings the status up to date with the settings and, for `publik` and `unset`, the resolver
    /// (which reads the credential file; `local` and `own_key` read none). The keychain is read
    /// with `own_key` chosen or when `read_keychain` is set; otherwise `own_key_stored` is the last
    /// value read.
    fn refresh_context(
        &mut self,
        ai: &AiSettings,
        read_keychain: bool,
    ) -> Option<(String, String)> {
        if read_keychain || ai.provider == "own_key" {
            self.own_key();
        }
        let own_key_stored = self.own_key_stored;
        let local_model =
            some_text(&ai.local.chat_model).or_else(|| self.probed.first().map(|m| m.name.clone()));
        let resolved = match ai.provider.as_str() {
            "publik" | "unset" => Some(self.publik_resolved()),
            _ => None,
        };
        let provider = ai.provider.clone();
        self.status.update(|s| {
            s.provider = provider;
            s.own_key_stored = own_key_stored;
            s.local_model = local_model;
            match &resolved {
                Some(Some((_, _, credential))) => {
                    s.needs_consent = false;
                    s.credential = credential.as_ref().map(CredentialView::of);
                }
                Some(None) => {
                    s.needs_consent = true;
                    s.credential = None;
                }
                None => s.needs_consent = false,
            }
        });
        resolved.flatten().map(|(base, key, _)| (base, key))
    }

    /// One `GET /wallet`; a failure leaves the status as it was.
    fn refresh_wallet(&mut self, base_url: &str, key: &str) {
        let request = HttpRequest {
            method: "GET".into(),
            url: format!("{}/wallet", trim_base(base_url)),
            headers: vec![("authorization".into(), format!("Bearer {key}"))],
            body: Vec::new(),
        };
        let Ok(response) = http::send(&request, QUICK_TIMEOUT) else {
            return;
        };
        let body = body_json(&response.body);
        self.status.update(|s| {
            s.apply_headers(&response.headers);
            if response.status == 200 {
                s.apply_wallet(&body);
            } else if is_revoked(response.status, &body) {
                s.connected = false;
            }
        });
    }

    /// Start-up: one GET /wallet when ai.provider = publik and a key resolves; nothing otherwise.
    pub fn on_start(&mut self, ai: &AiSettings) -> AiStatus {
        let resolved = self.refresh_context(ai, false);
        if ai.provider == "publik"
            && let Some((base, key)) = resolved
        {
            self.refresh_wallet(&base, &key);
        }
        self.status.snapshot()
    }

    /// ai_status; `refresh` is true when Settings > AI opens (one GET /wallet while ai.provider = publik).
    pub fn status(&mut self, ai: &AiSettings, refresh: bool) -> AiStatus {
        let resolved = self.refresh_context(ai, refresh);
        if refresh
            && ai.provider == "publik"
            && let Some((base, key)) = resolved
        {
            self.refresh_wallet(&base, &key);
        }
        self.status.snapshot()
    }

    /// Marks a provider change, which clears needs-credit.
    fn choose(&mut self, ai: &mut AiSettings, provider: &str) {
        if ai.provider != provider {
            ai.provider = provider.to_string();
            self.status.update(|s| s.provider_changed());
        }
    }

    /// Provisions this install (the learner said yes, or asked to reconnect).
    fn provision(&mut self, ai: &AiSettings) -> Result<(), AppError> {
        let device = ai.share_device_name.then(device_label);
        let (credential, reply) = provision_reply_in(&self.publik, &self.app_version, device)
            .map_err(|message| AppError::from(setup_error(message)))?;
        self.status.update(|s| s.provisioned(&credential, &reply));
        Ok(())
    }

    /// ai_consent_accept: provisions (no request when a key already resolves); sets ai.provider = publik.
    pub fn consent_accept(&mut self, ai: &mut AiSettings) -> Result<AiStatus, AppError> {
        if self.publik_resolved().is_none() {
            self.provision(ai)?;
        }
        self.choose(ai, "publik");
        Ok(self.status(ai, false))
    }

    /// ai_consent_decline: sends nothing, changes nothing.
    pub fn consent_decline(&mut self, ai: &AiSettings) -> AiStatus {
        self.status(ai, false)
    }

    pub fn set_own_key(
        &mut self,
        ai: &mut AiSettings,
        input: &OwnKeyInput,
    ) -> Result<AiStatus, AppError> {
        let base_url = input.base_url.trim();
        let lower = base_url.to_ascii_lowercase();
        let is_url = (lower.starts_with("https://") || lower.starts_with("http://"))
            && reqwest::Url::parse(base_url).is_ok_and(|u| u.host_str().is_some());
        if !is_url {
            return Err(invalid(
                "Enter the base URL of an OpenAI-compatible API, starting with https:// or http://.",
            ));
        }
        let key = input.key.trim();
        if key.is_empty() {
            return Err(invalid("Enter your key."));
        }
        let chat_model = input.chat_model.trim();
        if chat_model.is_empty() {
            return Err(invalid("Enter the chat model name."));
        }
        self.keychain.set(key).map_err(|_| {
            AppError::new(
                ErrorCode::Storage,
                "Parley could not keep your key in this computer's keychain.",
            )
        })?;
        self.own_key_stored = true;
        ai.own_key.base_url = Some(trim_base(base_url));
        ai.own_key.chat_model = Some(chat_model.to_string());
        ai.own_key.transcribe_model = some_text(&input.transcribe_model);
        ai.own_key.speech_model = some_text(&input.speech_model);
        ai.own_key.speech_voice = some_text(&input.speech_voice);
        self.choose(ai, "own_key");
        Ok(self.status(ai, false))
    }

    /// ai_clear_own_key: removes the key from the keychain; with the own key chosen, no provider is
    /// chosen any more.
    pub fn clear_own_key(&mut self, ai: &mut AiSettings) -> Result<AiStatus, AppError> {
        self.keychain.delete().map_err(|_| {
            AppError::new(
                ErrorCode::Storage,
                "Parley could not remove your key from this computer's keychain.",
            )
        })?;
        self.own_key_stored = false;
        if ai.provider == "own_key" {
            self.choose(ai, "unset");
        }
        Ok(self.status(ai, false))
    }

    /// `GET {base_url}/models`, then `GET {root}/api/tags` for sizes; names with `embed` dropped;
    /// largest first, unknown sizes last.
    fn probe(&self, base_url: &str) -> Result<Vec<LocalModel>, AiError> {
        let base = trim_base(base_url);
        let get = |url: String| {
            http::send(
                &HttpRequest {
                    method: "GET".into(),
                    url,
                    headers: Vec::new(),
                    body: Vec::new(),
                },
                QUICK_TIMEOUT,
            )
        };
        let listed = get(format!("{base}/models"));
        let root = base.strip_suffix("/v1").unwrap_or(&base).to_string();
        let tags = get(format!("{root}/api/tags"));
        if listed.is_err() && tags.is_err() {
            return Err(AiError::LocalDown { base_url: base });
        }
        let mut names: Vec<String> = Vec::new();
        if let Ok(response) = &listed
            && response.status == 200
        {
            let body = body_json(&response.body);
            for model in body["data"].as_array().into_iter().flatten() {
                if let Some(id) = model["id"].as_str() {
                    names.push(id.to_string());
                }
            }
        }
        let mut sizes: Vec<(String, u64)> = Vec::new();
        if let Ok(response) = &tags
            && response.status == 200
        {
            let body = body_json(&response.body);
            for model in body["models"].as_array().into_iter().flatten() {
                let name = model["name"].as_str().or_else(|| model["model"].as_str());
                if let (Some(name), Some(size)) = (name, model["size"].as_u64()) {
                    sizes.push((name.to_string(), size));
                }
            }
        }
        if names.is_empty() {
            names = sizes.iter().map(|(name, _)| name.clone()).collect();
        }
        let mut models: Vec<LocalModel> = Vec::new();
        for name in names {
            if name.to_ascii_lowercase().contains("embed") || models.iter().any(|m| m.name == name)
            {
                continue;
            }
            let bytes = sizes.iter().find(|(n, _)| *n == name).map(|(_, s)| *s);
            models.push(LocalModel { name, bytes });
        }
        // Largest first; unknown sizes last; equal sizes by name.
        models.sort_by(|a, b| match (a.bytes, b.bytes) {
            (Some(x), Some(y)) => y.cmp(&x).then_with(|| a.name.cmp(&b.name)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.name.cmp(&b.name),
        });
        Ok(models)
    }

    /// ai_local_probe: the models of a local server, largest first.
    pub fn local_probe(&mut self, base_url: &str) -> Result<Vec<LocalModel>, AppError> {
        if !local_host(base_url) {
            return Err(not_local());
        }
        let models = self.probe(base_url).map_err(AppError::from)?;
        self.probed = models.clone();
        Ok(models)
    }

    /// ai_set_local: chooses the local server, and a model or none (the largest it offers).
    pub fn set_local(
        &mut self,
        ai: &mut AiSettings,
        base_url: &str,
        chat_model: Option<&str>,
    ) -> Result<AiStatus, AppError> {
        if !local_host(base_url) {
            return Err(not_local());
        }
        ai.local.base_url = trim_base(base_url);
        ai.local.chat_model = chat_model
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_string);
        self.choose(ai, "local");
        Ok(self.status(ai, false))
    }

    /// ai_check_connection: P0 through the chosen provider.
    pub fn check_connection(&mut self, ai: &AiSettings) -> Result<ConnectionCheck, AppError> {
        let client = self.client(ai).map_err(AppError::from)?;
        let checked = check_connection(&*client, &mut *self.rng).map_err(AppError::from)?;
        self.refresh_context(ai, false);
        Ok(checked)
    }

    /// ai_disconnect_publik: revokes the key (best effort) and forgets it, keeping the install id
    /// so reconnecting makes no second install (contract §5).
    pub fn disconnect_publik(&mut self, ai: &AiSettings) -> AiStatus {
        if let Some((base, key, Some(mut credential))) = self.publik_resolved() {
            let revoke = HttpRequest {
                method: "POST".into(),
                url: format!("{}/installs/revoke", trim_base(&base)),
                headers: vec![("authorization".into(), format!("Bearer {key}"))],
                body: Vec::new(),
            };
            let _ = http::send(&revoke, QUICK_TIMEOUT);
            if let Some(map) = credential.as_object_mut() {
                map.remove("key");
            }
            let _ = write_credential_in(&self.publik, &credential);
        }
        self.status.update(|s| s.connected = false);
        self.status(ai, false)
    }

    /// ai_reconnect_publik: forgets the key that stopped working and provisions again; chooses
    /// publik API.
    pub fn reconnect_publik(&mut self, ai: &mut AiSettings) -> Result<AiStatus, AppError> {
        if let Some((_, _, Some(mut credential))) = self.publik_resolved() {
            if let Some(map) = credential.as_object_mut() {
                map.remove("key");
            }
            let _ = write_credential_in(&self.publik, &credential);
        }
        self.provision(ai)?;
        self.choose(ai, "publik");
        Ok(self.status(ai, false))
    }

    /// ai_open_link: the URL of `claim`, `top_up` or `add_credit` from the status.
    pub fn link_url(&self, kind: &str) -> Result<String, AppError> {
        let status = self.status.snapshot();
        let url = match kind {
            "claim" => status.claim_url,
            "top_up" => status.top_up_url,
            "add_credit" => status.add_credit_url,
            other => return Err(invalid(&format!("Unknown value {other} for kind."))),
        };
        url.filter(|u| !u.is_empty())
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "This link is not available yet."))
    }

    /// The client of the chosen provider; modules 07 to 12 make their calls through it.
    pub fn client(&mut self, ai: &AiSettings) -> Result<Arc<dyn AiClient>, AiError> {
        match ai.provider.as_str() {
            "own_key" => {
                let key = self.own_key().ok_or(AiError::OwnKeyMissing)?;
                let base = some_text(&ai.own_key.base_url).ok_or(AiError::OwnKeyMissing)?;
                let model = some_text(&ai.own_key.chat_model).ok_or(AiError::OwnKeyMissing)?;
                match resolve_provider_in(&self.publik, Some((base, key))) {
                    Provider::Own { base_url, api_key } => Ok(Arc::new(ProviderClient::own_key(
                        &base_url,
                        &api_key,
                        &model,
                        self.backoff,
                        self.status.clone(),
                    ))),
                    _ => Err(AiError::OwnKeyMissing),
                }
            }
            "local" => {
                let base = trim_base(&ai.local.base_url);
                if !is_loopback(&base) || !local_host(&base) {
                    return Err(AiError::LocalDown { base_url: base });
                }
                let model = match some_text(&ai.local.chat_model) {
                    Some(model) => model,
                    None => {
                        let models = self.probe(&base)?;
                        self.probed = models.clone();
                        models.first().map(|m| m.name.clone()).ok_or_else(|| {
                            AiError::Setup {
                                message: format!(
                                    "The local model server at {base} has no chat model. Install one, or choose another provider."
                                ),
                            }
                        })?
                    }
                };
                self.status.update(|s| s.local_model = Some(model.clone()));
                Ok(Arc::new(ProviderClient::local(
                    &base,
                    &model,
                    self.backoff,
                    self.status.clone(),
                )))
            }
            _ => {
                let (base, key) = self
                    .refresh_context(ai, false)
                    .ok_or(AiError::NeedsConsent)?;
                if !self.status.read(|s| s.connected) {
                    return Err(AiError::Disconnected);
                }
                Ok(Arc::new(ProviderClient::publik(
                    &base,
                    &key,
                    self.status.clone(),
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::test_server::{Reply, Server, closed_url, serve};
    use crate::keychain::MemoryKeychain;
    use parley_core::env::SplitMix64;
    use serde_json::json;

    struct Fixture {
        dir: tempfile::TempDir,
        keychain: MemoryKeychain,
        service: AiService,
    }

    fn fixture(api_key: Option<&str>, base_url: Option<String>) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let keychain = MemoryKeychain::default();
        let service = AiService::new(AiServiceConfig {
            data_dir: dir.path().join("data"),
            publik: PublikEnv {
                api_key: api_key.map(str::to_string),
                base_url,
                app_token: Some("pat_parley_test".into()),
                credential_path: dir.path().join("publik").join("parley.json"),
            },
            keychain: Box::new(keychain.clone()),
            rng: Box::new(SplitMix64::new(5)),
            backoff: [Duration::from_millis(1); 2],
            app_version: "0.1.0".into(),
        });
        Fixture {
            dir,
            keychain,
            service,
        }
    }

    const CREATED: &str = r#"{"key":"pk_live_a","key_id":"k1","base_url":"{BASE}/api/v1","install_id":"5f0c8a52-6e0b-4c55-9a3e-2a3f1d9b7c11","claim_url":"https://example.invalid/claim","claim_state":"anonymous","balance_micros":0,"wallet":{"balance_micros":0,"add_credit_url":"https://example.invalid/add","top_up_url":"https://example.invalid/top","claim_state":"anonymous"},"disclosure":{"version":1,"cost":"A lesson costs about a cent.","data_path":"x"}}"#;

    fn created() -> Reply {
        Reply::json(201, CREATED)
    }

    fn base_of(server: &Server) -> String {
        server.url("/api/v1")
    }

    #[test]
    fn consent_provisions_once_and_the_status_carries_the_201_body_but_no_key() {
        let server = serve(vec![created()]);
        let mut f = fixture(None, Some(base_of(&server)));
        let mut ai = AiSettings::default();
        let before = f.service.status(&ai, false);
        assert!(before.needs_consent);
        assert_eq!(before.provider, "unset");
        let status = f.service.consent_accept(&mut ai).unwrap();
        assert_eq!(ai.provider, "publik");
        assert!(!status.needs_consent);
        assert_eq!(
            status.balance_line.as_deref(),
            Some("$0.00 · link this computer for $0.05 of free use")
        );
        assert_eq!(
            status.cost_sentence.as_deref(),
            Some("A lesson costs about a cent.")
        );
        assert_eq!(
            f.service.link_url("claim").unwrap(),
            "https://example.invalid/claim"
        );
        assert_eq!(
            f.service.link_url("add_credit").unwrap(),
            "https://example.invalid/add"
        );
        assert_eq!(
            f.service.link_url("top_up").unwrap(),
            "https://example.invalid/top"
        );
        assert_eq!(
            f.service.link_url("elsewhere").unwrap_err().message,
            "Unknown value elsewhere for kind."
        );
        let seen = server.requests();
        assert_eq!(seen.len(), 1);
        let body: Value = serde_json::from_slice(&seen[0].body).unwrap();
        assert_eq!(body["app_slug"], "parley");
        assert!(body.get("device_name").is_none());
        let saved =
            std::fs::read_to_string(f.service.data_dir().join("publik-status.json")).unwrap();
        assert!(!saved.contains("pk_live_a"));
        assert!(
            !serde_json::to_string(&status)
                .unwrap()
                .contains("pk_live_a")
        );
        // A key now resolves: accepting again sends nothing.
        f.service.consent_accept(&mut ai).unwrap();
        assert_eq!(server.requests().len(), 1);
    }

    #[test]
    fn a_shared_device_name_is_the_generic_label() {
        let server = serve(vec![Reply::json(
            500,
            r#"{"error":{"message":"Try later."}}"#,
        )]);
        let mut f = fixture(None, Some(base_of(&server)));
        let mut ai = AiSettings {
            share_device_name: true,
            ..AiSettings::default()
        };
        let err = f.service.consent_accept(&mut ai).unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (ErrorCode::ProviderError, "Try later.")
        );
        assert_eq!(ai.provider, "unset");
        let body: Value = serde_json::from_slice(&server.requests()[0].body).unwrap();
        assert_eq!(body["device_name"], json!(device_label()));
    }

    #[test]
    fn start_up_and_settings_refresh_the_wallet_only_while_publik_is_chosen() {
        let server = serve(vec![
            Reply::json(
                200,
                r#"{"balance_micros":3120000,"claim_state":"claimed","top_up_url":"https://example.invalid/t"}"#,
            ),
            Reply::json(200, r#"{"balance_micros":3110000}"#),
        ]);
        let mut f = fixture(Some("pk_live_a"), Some(base_of(&server)));
        let mut ai = AiSettings::default();
        for provider in ["unset", "own_key", "local"] {
            ai.provider = provider.into();
            f.service.on_start(&ai);
        }
        assert_eq!(server.requests().len(), 0);
        ai.provider = "publik".into();
        let status = f.service.on_start(&ai);
        assert_eq!(
            status.balance_line.as_deref(),
            Some("$3.12 of publik balance")
        );
        f.service.status(&ai, false);
        assert_eq!(server.requests().len(), 1);
        let status = f.service.status(&ai, true);
        assert_eq!(status.balance_micros, Some(3_110_000));
        let seen = server.requests();
        assert_eq!(seen.len(), 2);
        assert!(
            seen.iter()
                .all(|r| r.path == "/api/v1/wallet" && r.method == "GET")
        );
        assert_eq!(seen[0].header("authorization"), Some("Bearer pk_live_a"));
    }

    #[test]
    fn the_own_key_is_checked_kept_in_the_keychain_and_used_only_when_chosen() {
        let server = serve(vec![Reply::json(
            200,
            r#"{"choices":[{"message":{"content":"{\"ok\":true}"}}]}"#,
        )]);
        let mut f = fixture(Some("pk_live_b"), Some("http://127.0.0.1:9/never".into()));
        let mut ai = AiSettings::default();
        let input = |base: &str, key: &str, model: &str| OwnKeyInput {
            base_url: base.into(),
            key: key.into(),
            chat_model: model.into(),
            transcribe_model: Some(" ".into()),
            speech_model: None,
            speech_voice: None,
        };
        for (given, message) in [
            (
                input("api.example.invalid", "k", "m"),
                "Enter the base URL of an OpenAI-compatible API, starting with https:// or http://.",
            ),
            (
                input("https://api.example.invalid", " ", "m"),
                "Enter your key.",
            ),
            (
                input("https://api.example.invalid", "k", ""),
                "Enter the chat model name.",
            ),
        ] {
            let err = f.service.set_own_key(&mut ai, &given).unwrap_err();
            assert_eq!(
                (err.code, err.message.as_str()),
                (ErrorCode::InvalidInput, message)
            );
        }
        let status = f
            .service
            .set_own_key(
                &mut ai,
                &input(&format!("{}/", server.url("/v1")), "sk-test-a", "gpt-x"),
            )
            .unwrap();
        assert!(status.own_key_stored);
        assert_eq!(f.keychain.secret().as_deref(), Some("sk-test-a"));
        assert_eq!(ai.provider, "own_key");
        assert_eq!(
            ai.own_key.base_url.as_deref(),
            Some(server.url("/v1").as_str())
        );
        assert_eq!(ai.own_key.transcribe_model, None);
        let settings_text = serde_json::to_string(&ai).unwrap();
        assert!(!settings_text.contains("sk-test-a"));
        assert!(
            !serde_json::to_string(&status)
                .unwrap()
                .contains("sk-test-a")
        );
        let check = f.service.check_connection(&ai).unwrap();
        assert_eq!(check.message, "Connected.");
        let seen = server.requests();
        assert_eq!(seen[0].header("authorization"), Some("Bearer sk-test-a"));
        // With publik API chosen, the stored own key is never used.
        ai.provider = "publik".into();
        let client = f.service.client(&ai).unwrap();
        assert_eq!(client.kind(), parley_core::ai::ProviderKind::Publik);
        ai.provider = "own_key".into();
        f.service.clear_own_key(&mut ai).unwrap();
        assert_eq!(ai.provider, "unset");
        assert_eq!(f.keychain.secret(), None);
        ai.provider = "own_key".into();
        assert_eq!(f.service.client(&ai).err(), Some(AiError::OwnKeyMissing));
    }

    #[test]
    fn the_local_probe_drops_embeddings_and_puts_the_largest_first() {
        let server = serve(vec![
            Reply::json(
                200,
                r#"{"object":"list","data":[{"id":"llama3.2:3b"},{"id":"nomic-embed-text"},{"id":"llama3.1:8b"},{"id":"phi"}]}"#,
            ),
            Reply::json(
                200,
                r#"{"models":[{"name":"llama3.1:8b","size":4700000000},{"name":"llama3.2:3b","size":2000000000},{"name":"nomic-embed-text","size":274000000}]}"#,
            ),
        ]);
        let mut f = fixture(None, None);
        let base = format!("http://localhost:{}/v1", server.addr.port());
        let models = f.service.local_probe(&base).unwrap();
        assert_eq!(
            models,
            vec![
                LocalModel {
                    name: "llama3.1:8b".into(),
                    bytes: Some(4_700_000_000)
                },
                LocalModel {
                    name: "llama3.2:3b".into(),
                    bytes: Some(2_000_000_000)
                },
                LocalModel {
                    name: "phi".into(),
                    bytes: None
                },
            ]
        );
        let paths: Vec<String> = server.requests().iter().map(|r| r.path.clone()).collect();
        assert_eq!(paths, ["/v1/models", "/api/tags"]);
        let mut ai = AiSettings::default();
        let status = f.service.set_local(&mut ai, &base, None).unwrap();
        assert_eq!(status.local_model.as_deref(), Some("llama3.1:8b"));
        assert_eq!(ai.provider, "local");
        let down = closed_url("/v1").replace("127.0.0.1", "localhost");
        let err = f.service.local_probe(&down).unwrap_err();
        assert_eq!(
            err.message,
            format!(
                "The local model server at {down} is not running. Start it, or choose another provider."
            )
        );
        let err = f
            .service
            .local_probe("http://192.168.1.20:11434/v1")
            .unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (
                ErrorCode::InvalidInput,
                "The local model server must be on this computer: use a localhost address."
            )
        );
        assert!(
            f.service
                .set_local(&mut ai, "https://example.invalid/v1", None)
                .is_err()
        );
    }

    /// A keychain that counts its reads.
    #[derive(Clone, Default)]
    struct CountingKeychain {
        inner: MemoryKeychain,
        reads: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl CountingKeychain {
        fn reads(&self) -> usize {
            self.reads.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl Keychain for CountingKeychain {
        fn get(&self) -> Result<Option<String>, String> {
            self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.get()
        }
        fn set(&self, secret: &str) -> Result<(), String> {
            self.inner.set(secret)
        }
        fn delete(&self) -> Result<(), String> {
            self.inner.delete()
        }
    }

    #[test]
    fn the_keychain_is_read_only_when_the_own_key_is_needed() {
        let dir = tempfile::tempdir().unwrap();
        let keychain = CountingKeychain::default();
        keychain.inner.set("sk-own-a").unwrap();
        let mut service = AiService::new(AiServiceConfig {
            data_dir: dir.path().to_path_buf(),
            publik: PublikEnv {
                api_key: None,
                base_url: Some(closed_url("/api/v1")),
                app_token: None,
                credential_path: dir.path().join("cred.json"),
            },
            keychain: Box::new(keychain.clone()),
            rng: Box::new(SplitMix64::new(2)),
            backoff: [Duration::ZERO; 2],
            app_version: "0.1.0".into(),
        });
        let mut ai = AiSettings::default();
        for provider in ["unset", "publik", "local"] {
            ai.provider = provider.into();
            let status = service.on_start(&ai);
            assert!(!status.own_key_stored, "not read yet");
            service.status(&ai, false);
            service.consent_decline(&ai);
        }
        assert_eq!(
            keychain.reads(),
            0,
            "a launch with another provider never reads it"
        );
        ai.provider = "local".into();
        assert!(
            service.status(&ai, true).own_key_stored,
            "Settings > AI reads it"
        );
        assert_eq!(keychain.reads(), 1);
        assert!(
            service.status(&ai, false).own_key_stored,
            "the last value read"
        );
        assert_eq!(keychain.reads(), 1);
        ai.provider = "own_key".into();
        ai.own_key.base_url = Some("https://api.example.invalid/v1".into());
        ai.own_key.chat_model = Some("m".into());
        service.on_start(&ai);
        assert_eq!(keychain.reads(), 2, "with the own key chosen it is read");
        assert!(service.client(&ai).is_ok());
        assert_eq!(keychain.reads(), 3);
        service.clear_own_key(&mut ai).unwrap();
        assert_eq!(ai.provider, "unset");
        assert!(!service.status(&ai, false).own_key_stored);
        assert_eq!(keychain.reads(), 3);
    }

    #[test]
    fn forgetting_the_status_resets_it_and_removes_its_file_but_keeps_the_credential_and_key() {
        let server = serve(vec![
            created(),
            Reply::json(
                402,
                r#"{"error":{"message":"No balance.","top_up_url":"https://example.invalid/t"}}"#,
            )
            .header("x-publik-charge-micros", "0")
            .header("x-publik-balance", "0"),
            Reply::json(200, r#"{"object":"list","data":[{"id":"m1"}]}"#),
            Reply::json(200, r#"{"models":[]}"#),
        ]);
        let mut f = fixture(None, Some(base_of(&server)));
        f.keychain.set("sk-own-a").unwrap();
        let mut ai = AiSettings::default();
        f.service.consent_accept(&mut ai).unwrap();
        assert!(f.service.check_connection(&ai).is_err());
        let base = format!("http://localhost:{}/v1", server.addr.port());
        f.service.local_probe(&base).unwrap();
        let before = f.service.status(&ai, false);
        assert!(before.needs_credit);
        assert_eq!(before.last_charge_micros, Some(0));
        let mut local = ai.clone();
        local.provider = "local".into();
        assert!(f.service.status(&local, false).local_model.is_some());
        assert!(f.service.status(&local, true).own_key_stored);
        let file = f.service.data_dir().join("publik-status.json");
        assert!(file.exists());
        f.service.forget_status();
        assert!(!file.exists());
        let fresh = AiSettings::default();
        let after = f.service.status(&fresh, false);
        assert!(after.connected && !after.needs_credit && !after.own_key_stored);
        assert_eq!(
            (
                after.last_charge_micros,
                after.top_up_url,
                after.local_model
            ),
            (None, None, None)
        );
        assert!(f.dir.path().join("publik").join("parley.json").exists());
        assert_eq!(f.keychain.secret().as_deref(), Some("sk-own-a"));
        // The credential still resolves, so its own fields come back from it, not from before.
        assert_eq!(
            after.claim_url.as_deref(),
            Some("https://example.invalid/claim")
        );
        assert_eq!(after.balance_micros, Some(0));
    }

    #[test]
    fn disconnect_forgets_the_key_and_reconnect_keeps_the_install_id() {
        let server = serve(vec![created(), Reply::json(200, "{}"), created()]);
        let mut f = fixture(None, Some(base_of(&server)));
        let mut ai = AiSettings::default();
        f.service.consent_accept(&mut ai).unwrap();
        let path = f.dir.path().join("publik").join("parley.json");
        let status = f.service.disconnect_publik(&ai);
        assert!(!status.connected);
        let left: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(left.get("key").is_none());
        assert_eq!(left["install_id"], "5f0c8a52-6e0b-4c55-9a3e-2a3f1d9b7c11");
        let status = f.service.reconnect_publik(&mut ai).unwrap();
        assert!(status.connected);
        let seen = server.requests();
        assert_eq!(seen[1].path, "/api/v1/installs/revoke");
        assert_eq!(seen[1].header("authorization"), Some("Bearer pk_live_a"));
        let again: Value = serde_json::from_slice(&seen[2].body).unwrap();
        assert_eq!(again["install_id"], "5f0c8a52-6e0b-4c55-9a3e-2a3f1d9b7c11");
    }

    #[test]
    fn without_a_token_or_a_provider_the_errors_say_so() {
        let dir = tempfile::tempdir().unwrap();
        let mut service = AiService::new(AiServiceConfig {
            data_dir: dir.path().to_path_buf(),
            publik: PublikEnv {
                api_key: None,
                base_url: Some(closed_url("/api/v1")),
                app_token: Some("none".into()),
                credential_path: dir.path().join("cred.json"),
            },
            keychain: Box::new(MemoryKeychain::default()),
            rng: Box::new(SplitMix64::new(1)),
            backoff: [Duration::ZERO; 2],
            app_version: "0.1.0".into(),
        });
        let mut ai = AiSettings::default();
        let err = service.consent_accept(&mut ai).unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (
                ErrorCode::NoProvider,
                "This build has no publik API app token. Use your own key for now."
            )
        );
        assert_eq!(service.client(&ai).err(), Some(AiError::NeedsConsent));
        assert_eq!(
            service.check_connection(&ai).unwrap_err().code,
            ErrorCode::NeedsConsent
        );
        assert_eq!(
            service.link_url("claim").unwrap_err().message,
            "This link is not available yet."
        );
        assert!(!dir.path().join("cred.json").exists());
    }
}
