// publik.rs — publik API as one provider option. Resolution order: the user's
// own key > PUBLIK_API_KEY / PUBLIK_API_BASE_URL env > this app's credential
// file > provision on first launch (after the user accepts DISCLOSURE) > the
// user's own key again if provisioning fails.
//
// The reference module from https://publikhq.com/sdk/publik.rs.md (fetched 2026-11-06), copied
// verbatim except APP_SLUG, APP_VERSION and the adaptations marked `ADAPTATION n`
// (docs/modules/06-publik-and-providers.md, Design notes). Keep it close to the reference so
// publik's later versions merge easily; rustfmt leaves this file as it is (see lib.rs).
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;
// ADAPTATION 3: `std::io::Write` and `std::process` are gone with curl: http.rs (reqwest with
// rustls) carries every request, and the key rides only in the Authorization header.

pub const APP_SLUG: &str = "parley";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_BASE_URL: &str = "https://publikhq.com/api/v1";
/// Ask for a tier, never a vendor model name.
pub const MODELS: [&str; 3] = ["publik-fast", "publik-balanced", "publik-smart"];
/// Show this and get a yes BEFORE provision().
pub const DISCLOSURE: &str = "This app can use publik API for its AI features: no key to paste, priced per use in dollars from a publik balance that starts at $0.00. Linking this computer to a publik account gives $0.05 of free use, once. Your prompts go through publik's servers to the model; you can switch to your own key at any time.";
// Public by design: it can only mint capped keys attributed to this app, and
// every user who builds from source needs it, so it lives in the repo.
const APP_TOKEN: &str = match option_env!("PUBLIK_APP_TOKEN") {
    Some(t) => t,
    None => include_str!("../publik-app-token.txt"),
};

// ADAPTATION 1: the key never reaches the web view. A Provider stays inside this crate's client
// (providers.rs); the interface only ever receives AiStatus, which has no key field, and the debug
// form below names the variant and base URL but never the key.
pub enum Provider {
    Own { base_url: String, api_key: String },
    Publik { base_url: String, api_key: String, credential: Option<Value> },
    NeedsConsent,
}

impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::Own { base_url, .. } => f.debug_struct("Own").field("base_url", base_url).finish_non_exhaustive(),
            Provider::Publik { base_url, .. } => f.debug_struct("Publik").field("base_url", base_url).finish_non_exhaustive(),
            Provider::NeedsConsent => f.write_str("NeedsConsent"),
        }
    }
}

// ADAPTATION 6: every function that reads the process environment has an `_in` twin that takes it
// as a PublikEnv, so tests run against a fake server and a temporary credential folder without
// touching the process environment. Provisioning posts to `base_url` when it is set.
pub struct PublikEnv {
    /// PUBLIK_API_KEY
    pub api_key: Option<String>,
    /// PUBLIK_API_BASE_URL; provisioning also posts there when set
    pub base_url: Option<String>,
    /// PUBLIK_APP_TOKEN at run time (overrides the compiled-in token)
    pub app_token: Option<String>,
    /// credential_path()
    pub credential_path: PathBuf,
}

impl PublikEnv {
    pub fn from_process() -> PublikEnv {
        PublikEnv {
            api_key: std::env::var("PUBLIK_API_KEY").ok(),
            base_url: std::env::var("PUBLIK_API_BASE_URL").ok(),
            app_token: std::env::var("PUBLIK_APP_TOKEN").ok(),
            credential_path: credential_path(),
        }
    }
}

impl std::fmt::Debug for PublikEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PublikEnv")
            .field("api_key", &self.api_key.as_ref().map(|_| "(set)"))
            .field("base_url", &self.base_url)
            .field("app_token", &self.app_token.as_ref().map(|_| "(set)"))
            .field("credential_path", &self.credential_path)
            .finish()
    }
}

/// "Windows PC", "Mac", "Linux PC" or "Android phone": the only device_name ever sent (ADAPTATION 2).
pub fn device_label() -> &'static str {
    if cfg!(target_os = "android") { "Android phone" } else if cfg!(target_os = "macos") { "Mac" } else if cfg!(windows) { "Windows PC" } else { "Linux PC" }
}

/// ~/Library/Application Support/publik/apps/<slug>.json, %LOCALAPPDATA%\publik\apps\<slug>.json, ~/.config/publik/apps/<slug>.json
pub fn credential_path() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    let base = if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or(home.join("AppData/Local"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or(home.join(".config"))
    };
    base.join("publik").join("apps").join(format!("{APP_SLUG}.json"))
}

pub fn read_credential() -> Option<Value> {
    read_credential_in(&PublikEnv::from_process()) // ADAPTATION 6
}

pub fn read_credential_in(env: &PublikEnv) -> Option<Value> {
    // ADAPTATION 6: the credential path comes from the PublikEnv.
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&env.credential_path).ok()?).ok()?;
    v["key"].as_str().filter(|k| k.starts_with("pk_"))?;
    Some(v)
}

pub(crate) fn write_credential_in(env: &PublikEnv, v: &Value) -> Result<(), String> {
    let path = &env.credential_path; // ADAPTATION 6
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(path, serde_json::to_string_pretty(v).unwrap()).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn resolve_provider(user_key: Option<(String, String)>) -> Provider {
    resolve_provider_in(&PublikEnv::from_process(), user_key) // ADAPTATION 6
}

pub fn resolve_provider_in(env: &PublikEnv, user_key: Option<(String, String)>) -> Provider {
    if let Some((base_url, api_key)) = user_key.filter(|(_, k)| !k.is_empty()) {
        return Provider::Own { base_url, api_key };
    }
    // ADAPTATION 6: PUBLIK_API_KEY and PUBLIK_API_BASE_URL come from the PublikEnv.
    if let Some(key) = env.api_key.clone() {
        let base_url = env.base_url.clone().unwrap_or_else(|| DEFAULT_BASE_URL.into());
        return Provider::Publik { base_url, api_key: key, credential: None };
    }
    match read_credential_in(env) {
        Some(c) => Provider::Publik {
            base_url: c["base_url"].as_str().unwrap_or(DEFAULT_BASE_URL).into(),
            api_key: c["key"].as_str().unwrap_or_default().into(),
            credential: Some(c),
        },
        None => Provider::NeedsConsent,
    }
}

/// POST JSON through http.rs; the status code arrives with the body.
fn post_json(url: &str, body: &Value) -> Result<(u16, Value), String> {
    // ADAPTATION 3: reqwest with rustls in place of curl, with curl's 20 s limit. No response at
    // all is the reference's "code 0".
    let (code, payload) = crate::http::post_json(url, body, Duration::from_secs(20))
        .map_err(|_| "publik API is unreachable right now. Use your own key for now.".to_string())?;
    Ok((code, serde_json::from_slice(&payload).unwrap_or(json!({}))))
}

/// Call only after the user accepted DISCLOSURE. Err is a readable message; offer the user's own key then.
pub fn provision(app_version: &str) -> Result<Value, String> {
    provision_in(&PublikEnv::from_process(), app_version, None) // ADAPTATION 6
}

pub fn provision_in(env: &PublikEnv, app_version: &str, device_name: Option<&str>) -> Result<Value, String> {
    provision_reply_in(env, app_version, device_name).map(|(c, _)| c)
}

/// provision_in, with the 200 or 201 body the credential came from: its `wallet` holds the
/// top-up link the status shows before any 402 (ADAPTATION 5).
pub fn provision_reply_in(env: &PublikEnv, app_version: &str, device_name: Option<&str>) -> Result<(Value, Value), String> {
    let runtime = env.app_token.clone().unwrap_or_default(); // ADAPTATION 6
    let token = if runtime.trim().is_empty() { APP_TOKEN.trim() } else { runtime.trim() };
    if !token.starts_with("pat_") { return Err("This build has no publik API app token. Use your own key for now.".into()); }
    // ADAPTATION 9: `prior` is the credential file as it is, with or without a key, so an install
    // whose key was forgotten by Disconnect keeps its install_id (contract §5, Disconnect).
    let mut prior = std::fs::read_to_string(&env.credential_path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object).unwrap_or(json!({}));
    let mut install_id = prior["install_id"].as_str().map(String::from).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let os = if cfg!(target_os = "macos") { "macos" } else if cfg!(windows) { "windows" } else { "linux" };
    let base = env.base_url.clone().unwrap_or_else(|| DEFAULT_BASE_URL.into()); // ADAPTATION 6
    for _ in 0..2 {
        let mut body = json!({
            "app_token": token, "app_slug": APP_SLUG, "app_version": app_version, "os": os,
            "arch": std::env::consts::ARCH, "install_id": install_id, "disclosure_version": 1,
            "dialects": ["chat_completions"],
        });
        // ADAPTATION 2: device_name only when the learner turned ai.share_device_name on, and then
        // only device_label(), never the hostname.
        if let Some(name) = device_name { body["device_name"] = json!(name); }
        let (code, res) = post_json(&format!("{base}/installs"), &body)?;
        let key = res["key"].as_str().or(if code == 200 { prior["key"].as_str() } else { None }).map(String::from);
        if let (200 | 201, Some(key)) = (code, key) {
            let c = json!({
                // ADAPTATION 6: with no base_url in the answer, the base it posted to.
                "key": key, "base_url": res["base_url"].as_str().unwrap_or(&base),
                "install_id": res["install_id"].as_str().unwrap_or(&install_id), "claim_url": res["claim_url"],
                "claim_state": res["claim_state"].as_str().unwrap_or("anonymous"),
                "add_credit_url": res["wallet"]["add_credit_url"], "cost_sentence": res["disclosure"]["cost"],
                "balance_micros": res["balance_micros"],
            });
            write_credential_in(env, &c)?; // the key reaches disk before anything else sees it
            return Ok((c, res));
        }
        if code == 200 || res["error"]["type"] == "install_revoked" {
            install_id = uuid::Uuid::new_v4().to_string();
            prior = json!({});
            continue;
        }
        return Err(res["error"]["message"].as_str().map(String::from)
            .unwrap_or_else(|| format!("publik API could not set up this computer ({code}). Use your own key for now.")));
    }
    Err("publik API could not set up this computer. Use your own key for now.".into())
}

/// Builder check, run by the BUILT app: start it with PUBLIK_SELF_TEST=1, print this line, exit.
/// It provisions for real; the builder who runs it is the consent.
pub fn self_test(app_version: &str) -> String {
    self_test_in(&PublikEnv::from_process(), app_version) // ADAPTATION 6
}

pub fn self_test_in(env: &PublikEnv, app_version: &str) -> String {
    match provision_in(env, app_version, None) {
        Ok(c) => format!("publik self-test: ok {APP_SLUG} {app_version} (install {:.8})", c["install_id"].as_str().unwrap_or("")),
        Err(e) => format!("publik self-test: failed: {e}"),
    }
}

/// First-run card, line 1. Then keep it live from the x-publik-balance header. A new install is $0.00 until it is linked.
pub fn balance_line(micros: Option<i64>, claim_state: &str) -> String {
    let Some(m) = micros else { return String::new() };
    // ADAPTATION 7: format_dollars, so a balance under ten cents is not shown as $0.00.
    let dollars = parley_core::ai::format_dollars(m);
    if m == 0 && claim_state == "anonymous" { format!("{dollars} · link this computer for $0.05 of free use") } else { format!("{dollars} of publik balance") }
}

/// A 402 renders its message plus exactly ONE link: this one.
pub fn top_up_url_from_402(body: &Value) -> (String, Option<String>) {
    let e = &body["error"];
    (e["message"].as_str().unwrap_or("Not enough publik API balance.").into(), e["top_up_url"].as_str().map(String::from))
}

// ADAPTATION 4: chat, transcription and speech go through one OpenAI-compatible client for the
// three providers (providers.rs). ADAPTATION 5: the x-publik-* status and the error mapping live in
// status.rs and errors.rs.
// ADAPTATION 8: the reference's search() is left out: Parley uses no web search, and its token
// does not list `search`.
