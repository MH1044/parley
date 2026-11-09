//! The status object (docs/PUBLIK-CONTRACT.md §7; docs/PUBLIK-INTEGRATION.md §9; adaptation 5):
//! what the interface knows about AI, kept in memory and in `publik-status.json` in the data
//! directory, never with a key (invariant I1).
//!
//! Its publik API fields come first from the credential file (the 201 body: claim state, balance,
//! cost sentence, claim and add-credit links), then from fresher sources: the `x-publik-*` headers
//! of every publik API response and `GET /wallet`. A 402 sets `needs_credit` and keeps its one
//! top-up link; a success, a wallet refresh with a positive balance, or a provider change clears it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use parley_core::ai::AiStatus;
use serde_json::Value;

use crate::publik::balance_line;

/// The status file in the data directory.
pub const STATUS_FILE: &str = "publik-status.json";

/// The fields the credential file holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CredentialView {
    pub claim_state: Option<String>,
    pub balance_micros: Option<i64>,
    pub cost_sentence: Option<String>,
    pub claim_url: Option<String>,
    pub add_credit_url: Option<String>,
}

impl CredentialView {
    /// The fields of a credential (as `provision()` writes it).
    pub fn of(credential: &Value) -> CredentialView {
        CredentialView {
            claim_state: text(&credential["claim_state"]),
            balance_micros: credential["balance_micros"].as_i64(),
            cost_sentence: text(&credential["cost_sentence"]),
            claim_url: text(&credential["claim_url"]),
            add_credit_url: text(&credential["add_credit_url"]),
        }
    }
}

fn text(value: &Value) -> Option<String> {
    value.as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

/// Everything the status shows, by source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusState {
    pub provider: String,
    pub needs_consent: bool,
    pub connected: bool,
    pub own_key_stored: bool,
    pub local_model: Option<String>,
    pub needs_credit: bool,
    /// From the credential file, while publik API resolves through it.
    pub credential: Option<CredentialView>,
    /// From the headers and `GET /wallet`, fresher than the credential.
    pub claim_state: Option<String>,
    pub balance_micros: Option<i64>,
    pub add_credit_url: Option<String>,
    /// The last 402's link, which wins over the wallet's.
    pub top_up_url_402: Option<String>,
    /// `wallet.top_up_url` of the 201 body or `GET /wallet`.
    pub top_up_url_wallet: Option<String>,
    pub last_charge_micros: Option<i64>,
    pub last_request_id: Option<String>,
    pub starter_remaining_micros: Option<i64>,
    pub week_used_micros: Option<i64>,
    pub week_budget_micros: Option<i64>,
    pub week_resets_at: Option<String>,
}

impl Default for StatusState {
    fn default() -> Self {
        StatusState {
            provider: "unset".into(),
            needs_consent: false,
            connected: true,
            own_key_stored: false,
            local_model: None,
            needs_credit: false,
            credential: None,
            claim_state: None,
            balance_micros: None,
            add_credit_url: None,
            top_up_url_402: None,
            top_up_url_wallet: None,
            last_charge_micros: None,
            last_request_id: None,
            starter_remaining_micros: None,
            week_used_micros: None,
            week_budget_micros: None,
            week_resets_at: None,
        }
    }
}

/// Integer headers are decimal micros; a value that does not parse is ignored.
fn micros(value: &str) -> Option<i64> {
    value.trim().parse().ok()
}

impl StatusState {
    /// The status the interface receives.
    pub fn snapshot(&self) -> AiStatus {
        let cred = self.credential.as_ref();
        let claim_state = self
            .claim_state
            .clone()
            .or_else(|| cred.and_then(|c| c.claim_state.clone()));
        let balance_micros = self
            .balance_micros
            .or_else(|| cred.and_then(|c| c.balance_micros));
        let line = balance_line(
            balance_micros,
            claim_state.as_deref().unwrap_or("anonymous"),
        );
        AiStatus {
            provider: self.provider.clone(),
            needs_consent: self.needs_consent,
            connected: self.connected,
            claim_state,
            balance_micros,
            balance_line: (!line.is_empty()).then_some(line),
            cost_sentence: cred.and_then(|c| c.cost_sentence.clone()),
            claim_url: cred.and_then(|c| c.claim_url.clone()),
            add_credit_url: self
                .add_credit_url
                .clone()
                .or_else(|| cred.and_then(|c| c.add_credit_url.clone())),
            top_up_url: self
                .top_up_url_402
                .clone()
                .or_else(|| self.top_up_url_wallet.clone()),
            needs_credit: self.needs_credit,
            last_charge_micros: self.last_charge_micros,
            last_request_id: self.last_request_id.clone(),
            starter_remaining_micros: self.starter_remaining_micros,
            week_used_micros: self.week_used_micros,
            week_budget_micros: self.week_budget_micros,
            week_resets_at: self.week_resets_at.clone(),
            own_key_stored: self.own_key_stored,
            local_model: self.local_model.clone(),
        }
    }

    /// The `x-publik-*` headers of a publik API response (contract §7).
    pub fn apply_headers(&mut self, headers: &[(String, String)]) {
        for (name, value) in headers {
            match name.to_ascii_lowercase().as_str() {
                "x-publik-balance" => {
                    if let Some(m) = micros(value) {
                        self.balance_micros = Some(m);
                    }
                }
                "x-publik-charge-micros" => {
                    if let Some(m) = micros(value) {
                        self.last_charge_micros = Some(m);
                    }
                }
                "x-publik-request-id" => self.last_request_id = Some(value.trim().to_string()),
                "x-publik-claim-state" => self.claim_state = Some(value.trim().to_string()),
                "x-publik-starter-remaining" => {
                    if let Some(m) = micros(value) {
                        self.starter_remaining_micros = Some(m);
                    }
                }
                "x-publik-week-used" => {
                    if let Some(m) = micros(value) {
                        self.week_used_micros = Some(m);
                    }
                }
                "x-publik-week-budget" => {
                    if let Some(m) = micros(value) {
                        self.week_budget_micros = Some(m);
                    }
                }
                "x-publik-week-resets-at" => self.week_resets_at = Some(value.trim().to_string()),
                _ => {}
            }
        }
    }

    /// A `GET /wallet` body: the fields of the 201 body's `wallet` object plus this week's usage,
    /// budget and reset time (read at the top level, or inside a `wallet` object).
    pub fn apply_wallet(&mut self, body: &Value) {
        let wallet = if body["wallet"].is_object() {
            &body["wallet"]
        } else {
            body
        };
        if let Some(m) = wallet["balance_micros"].as_i64() {
            self.balance_micros = Some(m);
            if m > 0 {
                self.needs_credit = false;
            }
        }
        if let Some(s) = text(&wallet["claim_state"]) {
            self.claim_state = Some(s);
        }
        if let Some(s) = text(&wallet["add_credit_url"]) {
            self.add_credit_url = Some(s);
        }
        if let Some(s) = text(&wallet["top_up_url"]) {
            self.top_up_url_wallet = Some(s);
        }
        if let Some(m) = wallet["week_used_micros"].as_i64() {
            self.week_used_micros = Some(m);
        }
        if let Some(m) = wallet["week_budget_micros"].as_i64() {
            self.week_budget_micros = Some(m);
        }
        if let Some(s) = text(&wallet["week_resets_at"]) {
            self.week_resets_at = Some(s);
        }
    }

    /// A new install's credential and the body it came from: what was known of the old install
    /// goes, the new one's links and balance come in.
    pub fn provisioned(&mut self, credential: &Value, reply: &Value) {
        self.credential = Some(CredentialView::of(credential));
        self.claim_state = None;
        self.balance_micros = None;
        self.add_credit_url = None;
        self.top_up_url_402 = None;
        self.top_up_url_wallet = text(&reply["wallet"]["top_up_url"]);
        self.needs_credit = false;
        self.connected = true;
    }

    /// A 402: its message's one link, and automatic calls stop.
    pub fn needs_credit(&mut self, top_up_url: Option<String>) {
        self.needs_credit = true;
        if top_up_url.is_some() {
            self.top_up_url_402 = top_up_url;
        }
    }

    /// Any successful answer.
    pub fn succeeded(&mut self) {
        self.needs_credit = false;
    }

    /// The provider changed.
    pub fn provider_changed(&mut self) {
        self.needs_credit = false;
    }

    /// Reads back a saved status: the fields that do not come from the credential.
    pub fn restore(&mut self, saved: &AiStatus) {
        self.connected = saved.connected;
        self.needs_credit = saved.needs_credit;
        self.claim_state = saved.claim_state.clone();
        self.balance_micros = saved.balance_micros;
        self.add_credit_url = saved.add_credit_url.clone();
        self.top_up_url_wallet = saved.top_up_url.clone();
        self.last_charge_micros = saved.last_charge_micros;
        self.last_request_id = saved.last_request_id.clone();
        self.starter_remaining_micros = saved.starter_remaining_micros;
        self.week_used_micros = saved.week_used_micros;
        self.week_budget_micros = saved.week_budget_micros;
        self.week_resets_at = saved.week_resets_at.clone();
    }
}

/// The status shared by the service and the clients it hands out, saved to `publik-status.json`
/// after every change. Saving never fails the learner's action.
#[derive(Debug, Clone)]
pub struct StatusHandle {
    state: Arc<Mutex<StatusState>>,
    data_dir: Option<PathBuf>,
}

impl StatusHandle {
    /// A status saved under `data_dir` (none: kept in memory only), starting from the saved file
    /// when there is one.
    pub fn new(data_dir: Option<PathBuf>) -> StatusHandle {
        let mut state = StatusState::default();
        if let Some(saved) = data_dir.as_deref().and_then(load) {
            state.restore(&saved);
        }
        StatusHandle {
            state: Arc::new(Mutex::new(state)),
            data_dir,
        }
    }

    /// Reads the state.
    pub fn read<T>(&self, f: impl FnOnce(&StatusState) -> T) -> T {
        f(&self.state.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Changes the state and saves it.
    pub fn update<T>(&self, f: impl FnOnce(&mut StatusState) -> T) -> T {
        let (value, snapshot) = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let value = f(&mut state);
            (value, state.snapshot())
        };
        if let Some(dir) = &self.data_dir {
            save(dir, &snapshot);
        }
        value
    }

    pub fn snapshot(&self) -> AiStatus {
        self.read(StatusState::snapshot)
    }

    /// Forgets everything: the state goes back to its defaults and the saved file is removed.
    /// Clients holding a clone see the reset too.
    pub fn reset(&self) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = StatusState::default();
        if let Some(dir) = &self.data_dir {
            let _ = std::fs::remove_file(dir.join(STATUS_FILE));
        }
    }
}

/// The saved status, when the file can be read.
pub fn load(data_dir: &Path) -> Option<AiStatus> {
    let text = std::fs::read_to_string(data_dir.join(STATUS_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Writes the status file; a failure is ignored.
pub fn save(data_dir: &Path, status: &AiStatus) {
    if let Ok(mut text) = serde_json::to_string_pretty(status) {
        text.push('\n');
        let _ = std::fs::create_dir_all(data_dir);
        let _ = std::fs::write(data_dir.join(STATUS_FILE), text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn every_header_of_the_contract_reaches_its_field() {
        let mut state = StatusState::default();
        state.apply_headers(&headers(&[
            ("X-Publik-Balance", "3120000"),
            ("x-publik-charge-micros", "17"),
            ("x-publik-request-id", "req_1"),
            ("x-publik-claim-state", "claimed"),
            ("x-publik-starter-remaining", "40000"),
            ("x-publik-week-used", "1500"),
            ("x-publik-week-budget", "2000000"),
            ("x-publik-week-resets-at", "2026-11-09T00:00:00Z"),
            ("x-publik-model", "publik-fast"),
        ]));
        let status = state.snapshot();
        assert_eq!(status.balance_micros, Some(3_120_000));
        assert_eq!(
            status.balance_line.as_deref(),
            Some("$3.12 of publik balance")
        );
        assert_eq!(status.last_charge_micros, Some(17));
        assert_eq!(status.last_request_id.as_deref(), Some("req_1"));
        assert_eq!(status.claim_state.as_deref(), Some("claimed"));
        assert_eq!(status.starter_remaining_micros, Some(40_000));
        assert_eq!(status.week_used_micros, Some(1_500));
        assert_eq!(status.week_budget_micros, Some(2_000_000));
        assert_eq!(
            status.week_resets_at.as_deref(),
            Some("2026-11-09T00:00:00Z")
        );
        state.apply_headers(&headers(&[
            ("x-publik-balance", "lots"),
            ("x-publik-charge-micros", ""),
        ]));
        assert_eq!(
            state.snapshot().balance_micros,
            Some(3_120_000),
            "unparsable values are ignored"
        );
        assert_eq!(state.snapshot().last_charge_micros, Some(17));
    }

    #[test]
    fn the_credential_comes_first_then_fresher_sources() {
        let mut state = StatusState::default();
        let credential = json!({"key": "pk_live_a", "claim_state": "anonymous", "balance_micros": 0,
            "cost_sentence": "About a cent a lesson.", "claim_url": "https://example.invalid/claim",
            "add_credit_url": "https://example.invalid/add"});
        state.provisioned(
            &credential,
            &json!({"wallet": {"top_up_url": "https://example.invalid/top"}}),
        );
        let status = state.snapshot();
        assert_eq!(
            status.balance_line.as_deref(),
            Some("$0.00 · link this computer for $0.05 of free use")
        );
        assert_eq!(
            status.cost_sentence.as_deref(),
            Some("About a cent a lesson.")
        );
        assert_eq!(
            status.claim_url.as_deref(),
            Some("https://example.invalid/claim")
        );
        assert_eq!(
            status.top_up_url.as_deref(),
            Some("https://example.invalid/top")
        );
        assert!(
            !serde_json::to_string(&status)
                .unwrap()
                .contains("pk_live_a")
        );
        state.apply_wallet(&json!({"balance_micros": 50_000, "claim_state": "claimed",
            "add_credit_url": "https://example.invalid/add2", "week_used_micros": 0}));
        let status = state.snapshot();
        assert_eq!(
            status.balance_line.as_deref(),
            Some("$0.050 of publik balance")
        );
        assert_eq!(
            status.add_credit_url.as_deref(),
            Some("https://example.invalid/add2")
        );
        state.needs_credit(Some("https://example.invalid/402".into()));
        assert!(state.snapshot().needs_credit);
        assert_eq!(
            state.snapshot().top_up_url.as_deref(),
            Some("https://example.invalid/402")
        );
        state.apply_wallet(&json!({"balance_micros": 0}));
        assert!(state.snapshot().needs_credit, "an empty wallet keeps it");
        state.apply_wallet(&json!({"wallet": {"balance_micros": 10}}));
        assert!(
            !state.snapshot().needs_credit,
            "a positive balance clears it"
        );
    }

    #[test]
    fn the_status_file_round_trips_and_a_bad_one_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let handle = StatusHandle::new(Some(dir.path().to_path_buf()));
        handle.update(|s| {
            s.connected = false;
            s.apply_headers(&headers(&[("x-publik-balance", "5")]));
        });
        let saved = load(dir.path()).unwrap();
        assert!(!saved.connected);
        let again = StatusHandle::new(Some(dir.path().to_path_buf()));
        assert_eq!(again.snapshot().balance_micros, Some(5));
        assert!(!again.snapshot().connected);
        std::fs::write(dir.path().join(STATUS_FILE), "{not json").unwrap();
        assert!(
            StatusHandle::new(Some(dir.path().to_path_buf()))
                .snapshot()
                .connected
        );
    }
}
