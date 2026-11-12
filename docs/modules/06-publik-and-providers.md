# 06: Publik and providers

## Goal

Parley can make AI calls safely through one client: publik API (provisioned per install, with the
balance line and account link), the learner's own OpenAI-compatible key, or a local model server.
The consent dialog appears at the first AI action; Settings shows all three providers and a
**Check connection** button (feature P0). Every later AI module calls this client and nothing else.

## Depends on

04. The live Publik path needs the app token, which the maintainer's one-time Publik setup run
(`docs/PUBLIK-INTEGRATION.md` §2) provides once this module is done and a release build works; until
then the module is completed and tested against the fake server.

## Scope

- The Publik contract in `docs/PUBLIK-CONTRACT.md` in full, with the adaptations of
  `docs/PUBLIK-INTEGRATION.md` §2. Feature-map row: **P0** (connection check).
- `crates/parley-publik/src/publik.rs`: the reference `publik.rs.md` verbatim except `APP_SLUG`,
  `APP_VERSION` and the marked adaptations; `publik-app-token.txt` next to its `Cargo.toml`.
- `http.rs` (reqwest with rustls, server-sent events), `providers.rs` (Publik, own key, local),
  `status.rs` (headers of contract §7, `publik-status.json`, `GET /wallet` on start and when
  Settings opens, only while `ai.provider = publik`), `errors.rs`, `keychain.rs`, `service.rs`.
- Provider choice by `ai.provider` (PUBLIK-INTEGRATION §3): the own key reaches
  `resolve_provider` only when `ai.provider = own_key`; `local` never calls the resolver.
- Idempotency keys and the `ai_attempts` table (PUBLIK-INTEGRATION §6.1), including P0's random key.
- The `AiClient` trait and `ai::call` pipeline in `parley-core`: render the prompt, compute the cache
  key, look up the cache, call once, parse, validate against the schema, run the feature's rules,
  store only valid responses, return.
- Prompt files and loader (format of PUBLIK-INTEGRATION §6.1), `prompts/ping.v1.md`, JSON Schema
  validation with `jsonschema`.
- Own key in the OS keychain (`keyring`); local model probe and choice.
- Commands marked 06 in ARCHITECTURE §5 and the event `ai://status`.
- The desktop app's `Net` (pack downloads, module 04) becomes the real transport of `http.rs` in
  place of `FailingNet`.
- UI: the AI setup dialog and first-run card (strings of PUBLIK-INTEGRATION §4), Settings > AI,
  the AI chip on the path, the error banners of §9, the balance line.
- `scripts/publik-proof` → `parley publik proof` (maintainer only: one real P0 call).
- The built-app self-test (contract §3): the first lines of `main` in `app/src-tauri/src/main.rs`,
  before logging, the database, `AppState` and `tauri::Builder`, print the one line of
  `publik::self_test` and return when `PUBLIK_SELF_TEST` is set. Desktop only; normal start-up never
  runs it. The setup run of PUBLIK-INTEGRATION §2 uses it to prove the release build.
- `SECURITY.md`: every network call and the `device_name` field.

## Files

- `crates/parley-publik/{Cargo.toml,publik-app-token.txt}`,
  `crates/parley-publik/src/{lib,publik,http,providers,status,errors,keychain,service,proof}.rs`
- `crates/parley-core/src/ai/{mod,client,cache,prompts,schema,call,money}.rs`, `prompts/ping.v1.md`,
  `schemas/ai/ping.v1.schema.json`
- `app/src-tauri/src/commands/ai.rs`; `app/src-tauri/src/main.rs` (the self-test hook, first lines of
  `main`); `app/src/components/{AiSetupDialog,BalanceLine,AiChip,AiBanner}.tsx`;
  `app/src/screens/Settings/AiSection.tsx`; `crates/parley-cli` (`parley publik proof`),
  `scripts/publik-proof`
- Tests: `crates/parley-publik/tests/{fake.rs,m06_contract.rs}`,
  `crates/parley-core/tests/m06_publik_and_providers.rs`,
  `app/src-tauri/tests/m06_publik_and_providers.rs` (starts the desktop binary for test 32),
  `crates/parley-cli/tests/m06_publik_and_providers.rs` (test 26), `app/src/test/m06/*.test.tsx`,
  `tests/fixtures/ai/P0/`, `tests/fixtures/http/`, `tests/fixtures/prompts/` (the test-only prompts)

## Interfaces

### parley-core: `parley_core::ai`

`crates/parley-core/src/ai/mod.rs` re-exports every public item below, so tests write
`parley_core::ai::{...}`.

```rust
// client.rs
pub enum ProviderKind { Publik, OwnKey, Local }      // serde snake_case: "publik", "own_key", "local"
impl ProviderKind { pub fn as_str(self) -> &'static str; }
pub struct ChatMessage { pub role: String, pub content: serde_json::Value } // a JSON string, or an array of content parts
pub struct ChatRequest {
    pub model: String,                    // from AiClient::chat_model(prompt tier)
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
    pub temperature: f64,
    pub timeout_s: u64,
    pub idempotency_key: Option<String>,  // sent as x-publik-idempotency-key, on Publik calls only
}
impl ChatRequest {
    /// {"model", "max_tokens", "temperature", "messages"} (+ "stream": true when streaming). A whole
    /// temperature is written as a JSON integer, so P0's body is exactly PUBLIK-INTEGRATION §6.2's.
    pub fn body(&self, stream: bool) -> serde_json::Value;
}
pub struct ChatReply { pub content: String, pub charge_micros: Option<i64>, pub request_id: Option<String> }
pub struct TranscribeRequest { pub model: String, pub audio: Vec<u8>, pub file_name: String, pub mime: String,
    pub language: String, pub response_format: String, pub timeout_s: u64, pub idempotency_key: Option<String> }
pub struct TranscriptReply { pub body: serde_json::Value, pub charge_micros: Option<i64> }
pub struct SpeechRequest { pub model: String, pub input: String, pub voice: String, pub response_format: String,
    pub speed: f64, pub timeout_s: u64, pub idempotency_key: Option<String> }
pub struct AudioReply { pub bytes: Vec<u8>, pub content_type: String, pub charge_micros: Option<i64> }
pub struct LocalModel { pub name: String, pub bytes: Option<u64> }   // serde as written

pub trait AiClient: Send + Sync {
    fn chat(&self, req: ChatRequest) -> Result<ChatReply, AiError>;
    fn chat_stream(&self, req: ChatRequest, on_delta: &mut dyn FnMut(&str)) -> Result<ChatReply, AiError>;
    fn transcribe(&self, req: TranscribeRequest) -> Result<TranscriptReply, AiError>;
    fn speech(&self, req: SpeechRequest) -> Result<AudioReply, AiError>;
    fn kind(&self) -> ProviderKind;
    fn status(&self) -> AiStatus;
    /// The chat `model` for a prompt tier: the tier itself on Publik (`publik-smart` becomes
    /// `publik-balanced` while `claim_state` is `anonymous`), the configured chat model on the own
    /// key, the chosen local model on Local.
    fn chat_model(&self, tier: &str) -> String;
}

/// Every request body is under this many bytes, base64 included (invariant I10). A client refuses
/// a body of this size or more with `AiError::TooLarge` before opening any connection.
pub const MAX_REQUEST_BYTES: usize = 4_000_000;
/// The OpenAI content part of a JPEG (module 12): {"type":"image_url","image_url":{"url":"data:image/jpeg;base64,..."}}.
pub fn image_part(jpeg: &[u8]) -> serde_json::Value;

pub enum AiError {
    NeedsConsent, NoToken, OwnKeyMissing,
    NeedsCredit { message: String, top_up_url: String },
    Disconnected, RateLimited { message: String }, Busy,
    Timeout { provider: ProviderKind }, Offline, Invalid,
    Provider { provider: ProviderKind, code: u16, message: String },
    LocalDown { base_url: String },
    Setup { message: String },      // provisioning failures (contract §5)
    TooLarge,
}
impl AiError { pub fn message(&self) -> String; }   // the table below
impl From<AiError> for AppError;                     // code, message and action from the table below
```

| `AiError` | `AppError.code` | `message()` | `action` |
|---|---|---|---|
| `NeedsConsent` | `needs_consent` | "AI features are not set up yet." | null |
| `NoToken` | `no_provider` | "This build has no publik API app token. Use your own key for now." | null |
| `OwnKeyMissing` | `no_provider` | "Add your own key in Settings first." | null |
| `NeedsCredit` | `needs_credit` | `error.message` | `{"top_up_url": error.top_up_url}` |
| `Disconnected` | `disconnected` | "publik API is disconnected." | null |
| `RateLimited` | `rate_limited` | `error.message` | null |
| `Busy` | `busy` | "publik API is busy; try again in a moment" | null |
| `Timeout{Publik}` | `busy` | "publik API did not answer in time." | null |
| `Timeout{OwnKey}` | `busy` | "Your provider did not answer in time." | null |
| `Timeout{Local}` | `busy` | "The local model did not answer in time." | null |
| `Offline` | `offline` | "You're offline. AI features need a connection or a local model." | null |
| `Invalid` | `ai_invalid` | "The AI answer could not be used." | null |
| `Provider{Publik}` | `provider_error` | "publik API {code}: {message}" | null |
| `Provider{OwnKey}` | `provider_error` | "Your provider returned {code}: {message}" | null |
| `Provider{Local}` | `provider_error` | "The local model returned {code}: {message}" | null |
| `LocalDown` | `provider_error` | "The local model server at {base_url} is not running. Start it, or choose another provider." | null |
| `Setup` | `provider_error` | the message | null |
| `TooLarge` | `file_too_large` | "This request is larger than 4 MB, so it was not sent." | null |

`{message}` in a `Provider` error is the body's `error.message`, or the HTTP status's canonical
reason ("Internal Server Error") when the body has none.

`AiStatus` (serde field names as written, `Option` as null), with no key field, and where each
field comes from:

| Field | Type | Source |
|---|---|---|
| `provider` | String | `ai.provider` in settings: `unset`, `publik`, `own_key` or `local` |
| `needs_consent` | bool | `provider` is `unset` or `publik` and `resolve_provider(None)` returned `NeedsConsent` |
| `connected` | bool | false after a 401 or 403 `install_revoked` until **Reconnect** succeeds; true otherwise |
| `claim_state` | Option<String> | the credential file (from the 201 body), then `x-publik-claim-state` and `GET /wallet` |
| `balance_micros` | Option<i64> | the credential file, then `x-publik-balance` and `GET /wallet` |
| `balance_line` | Option<String> | `balance_line(balance_micros, claim_state or "anonymous")`; null when that is empty |
| `cost_sentence` | Option<String> | the credential file (written by `provision()` from the 201 body's `disclosure.cost`) |
| `claim_url` | Option<String> | the credential file (201 body `claim_url`) |
| `add_credit_url` | Option<String> | the credential file (201 body `wallet.add_credit_url`), then `GET /wallet` |
| `top_up_url` | Option<String> | the last 402 body through `top_up_url_from_402`, else `wallet.top_up_url` from the 201 body or `GET /wallet` |
| `needs_credit` | bool | set by a 402, cleared by a success, a wallet refresh with a positive balance, or a provider change |
| `last_charge_micros`, `last_request_id`, `starter_remaining_micros`, `week_used_micros`, `week_budget_micros`, `week_resets_at` | Option (i64, String for the id and the reset time) | the `x-publik-*` headers of the last call (contract §7) |
| `own_key_stored` | bool | the keychain holds an own key |
| `local_model` | Option<String> | the chosen local model, else the largest the last probe found |

Integer headers are read as decimal micros; a value that does not parse is ignored. The UI shows
the URLs as labels only; `ai_open_link(kind)` opens the core's own copy of `claim_url`,
`add_credit_url` or `top_up_url` and never a URL passed from the UI.

```rust
// prompts.rs and schema.rs
pub struct Prompt { pub id: String, pub version: u32, pub tier: String, pub max_tokens: u32,
    pub temperature: f64, pub timeout_s: u64, pub schema: String, pub system: String, pub user: String, .. }
impl Prompt {
    /// The file format of PUBLIK-INTEGRATION §6.1 (front matter id, version, tier, max_tokens,
    /// temperature (default 0), timeout_s, schema; then `## system` and `## user`), with the text of
    /// the schema the front matter names, which is compiled here.
    pub fn parse(text: &str, schema_json: &str) -> Result<Prompt, PromptError>;
    /// [system, user] messages. A placeholder is `{name}` with `name` matching [a-z][a-z0-9_]*; any
    /// other brace is text. A value whose name ends in `_json` is inserted as a JSON string literal.
    pub fn render(&self, vars: &BTreeMap<String, String>) -> Result<Vec<ChatMessage>, RenderError>;
    pub fn schema_valid(&self, value: &serde_json::Value) -> bool;
}
pub enum PromptError { FrontMatter(String), Schema(String) }
pub enum RenderError { Missing(String), Unused(String) }       // the placeholder's name
pub fn ping() -> &'static Prompt;    // prompts/ping.v1.md and schemas/ai/ping.v1.schema.json, compiled in
/// The JSON of an answer: from the first `{` to the last `}`, parsed as it is; nothing else is tolerated.
pub fn parse_reply(content: &str) -> Option<serde_json::Value>;

// cache.rs: cache/ai.sqlite3 with the ai_cache, audio_cache and ai_attempts tables of ARCHITECTURE §4
pub struct Cache { .. }
impl Cache {
    pub fn open(path: &Path, clock: Box<dyn Clock + Send + Sync>) -> Result<Cache, StoreError>;
    pub fn in_memory(clock: Box<dyn Clock + Send + Sync>) -> Cache;   // P6's session-only cache
    pub fn get(&self, key: &str) -> Option<String>;                    // the stored answer JSON
    pub fn len(&self) -> usize;                                        // rows in ai_cache
    pub fn attempts(&self, key: &str) -> u32;                          // 0 when none failed
}
/// Sorted keys, no insignificant whitespace, every string NFC.
pub fn canonical_json(value: &serde_json::Value) -> String;
/// canonical_json of {"feature", "v", "provider", "model", "input"}.
pub fn cache_key_json(feature: &str, version: u32, provider: ProviderKind, model: &str, input: &serde_json::Value) -> String;
pub fn cache_key(key_json: &str) -> String;                            // SHA-256, 64 lowercase hex
/// The first 32 lowercase hex of the SHA-256 of `key_json` for attempt 1; for attempt n ≥ 2, of
/// the canonical JSON with one more field "attempt": n.
pub fn idempotency_key(key_json: &str, attempt: u32) -> String;

// call.rs
pub struct Called<T> { pub value: T, pub cached: bool, pub charge_micros: Option<i64> }
pub struct RuleFailure { pub rule: String, pub detail: String }
/// Renders `feature` with `vars`; the key is cache_key_json(feature.id, feature.version,
/// client.kind(), client.chat_model(&feature.tier), key_input). A hit returns it with no request. A
/// miss sends one request with the idempotency key of attempt `attempts(key) + 1`; the answer is
/// parsed (parse_reply), checked against the schema, deserialised and checked by `rules`; only an
/// answer that passes all four is stored, under the attempt-free key. Any failure stores nothing,
/// adds one to `ai_attempts` for the key and returns `AiError::Invalid`. A transport error, timeout
/// or HTTP error changes no count, so a Try again after it sends the same key.
pub fn call<T: DeserializeOwned>(client: &dyn AiClient, cache: &Cache, feature: &Prompt,
    vars: &BTreeMap<String, String>, key_input: &serde_json::Value,
    rules: &dyn Fn(&T) -> Result<(), RuleFailure>) -> Result<Called<T>, AiError>;
pub struct ConnectionCheck { pub ok: bool, pub charge_micros: Option<i64>, pub message: String }
/// P0: never cached; a fresh idempotency key of 32 random lowercase hex from `rng` on every call;
/// message "Connected. This check cost {format_dollars(charge)}." or "Connected." with no charge.
pub fn check_connection(client: &dyn AiClient, rng: &mut dyn Rng) -> Result<ConnectionCheck, AiError>;

// money.rs
/// Two decimals from $0.10 up (`$3.12`, `$0.10`), otherwise two significant figures (`$0.049`,
/// `$0.0058`, `$0.000017`); 0 is `$0.00`. Integer arithmetic, half up.
pub fn format_dollars(micros: i64) -> String;
```

Every public type derives `Debug`, `Clone` and `PartialEq` where its fields allow (`Prompt` and
`Cache` derive `Debug` only); `ProviderKind`, `AiStatus` (also `Default` and `Deserialize`),
`LocalModel` and `ConnectionCheck` also derive `Serialize`.

### parley-publik

Every file of `crates/parley-publik/src` is a public module of the same name
(`parley_publik::publik::DISCLOSURE`, `parley_publik::service::AiService`,
`parley_publik::keychain::MemoryKeychain`).

```rust
// publik.rs: the reference, with APP_SLUG = "parley", APP_VERSION = env!("CARGO_PKG_VERSION"),
// DEFAULT_BASE_URL, MODELS, DISCLOSURE (verbatim), Provider, credential_path, read_credential,
// resolve_provider, provision, self_test, balance_line, top_up_url_from_402, and these adaptations:
pub struct PublikEnv {
    pub api_key: Option<String>,       // PUBLIK_API_KEY
    pub base_url: Option<String>,      // PUBLIK_API_BASE_URL; provisioning also posts there when set
    pub app_token: Option<String>,     // PUBLIK_APP_TOKEN at run time (overrides the compiled-in token)
    pub credential_path: PathBuf,      // credential_path()
}
impl PublikEnv { pub fn from_process() -> PublikEnv; }
pub fn resolve_provider_in(env: &PublikEnv, user_key: Option<(String, String)>) -> Provider;
pub fn provision_in(env: &PublikEnv, app_version: &str, device_name: Option<&str>) -> Result<Value, String>;
pub fn self_test_in(env: &PublikEnv, app_version: &str) -> String;
pub fn device_label() -> &'static str;   // "Windows PC", "Mac", "Linux PC" or "Android phone"

// lib.rs, outside the copied publik.rs.
// None when `var` is None; otherwise Some(self_test_in(env, version)), the one line
// "publik self-test: ok parley <version> (install <8 chars>)" or "publik self-test: failed: <message>".
pub fn startup_self_test_in(env: &PublikEnv, var: Option<&std::ffi::OsStr>, version: &str) -> Option<String>;
// The same with PublikEnv::from_process(), read only when `var` is Some.
pub fn startup_self_test(var: Option<&std::ffi::OsStr>, version: &str) -> Option<String>;

// keychain.rs
pub trait Keychain: Send {
    fn get(&self) -> Result<Option<String>, String>;
    fn set(&self, secret: &str) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}
pub struct OsKeychain;                       // keyring entry: service "org.parley.app", user "own_key"
#[derive(Clone, Default)] pub struct MemoryKeychain { .. }   // clones share one slot
impl MemoryKeychain { pub fn secret(&self) -> Option<String>; }

// service.rs: everything the ai_* commands do. Settings stay in parley-core: a method that changes
// the provider takes the AI settings by &mut, and the command saves them.
pub struct AiServiceConfig {
    pub data_dir: PathBuf,                     // publik-status.json is written here
    pub publik: PublikEnv,
    pub keychain: Box<dyn Keychain>,
    pub rng: Box<dyn Rng + Send>,              // P0's random idempotency key
    pub backoff: [Duration; 2],                // own-key and local retries: [1 s, 4 s] in the app
    pub app_version: String,
}
pub struct OwnKeyInput { pub base_url: String, pub key: String, pub chat_model: String,
    pub transcribe_model: Option<String>, pub speech_model: Option<String>, pub speech_voice: Option<String> }
pub struct AiService { .. }
impl AiService {
    pub fn new(config: AiServiceConfig) -> AiService;
    /// Start-up: one GET /wallet when ai.provider = publik and a key resolves; nothing otherwise.
    pub fn on_start(&mut self, ai: &AiSettings) -> AiStatus;
    /// ai_status; `refresh` is true when Settings > AI opens (one GET /wallet while ai.provider = publik).
    pub fn status(&mut self, ai: &AiSettings, refresh: bool) -> AiStatus;
    /// ai_consent_accept: provisions (no request when a key already resolves); sets ai.provider = publik.
    pub fn consent_accept(&mut self, ai: &mut AiSettings) -> Result<AiStatus, AppError>;
    pub fn consent_decline(&mut self, ai: &AiSettings) -> AiStatus;                 // sends nothing, changes nothing
    pub fn set_own_key(&mut self, ai: &mut AiSettings, input: &OwnKeyInput) -> Result<AiStatus, AppError>;
    pub fn clear_own_key(&mut self, ai: &mut AiSettings) -> Result<AiStatus, AppError>;
    pub fn local_probe(&mut self, base_url: &str) -> Result<Vec<LocalModel>, AppError>;  // largest first
    pub fn set_local(&mut self, ai: &mut AiSettings, base_url: &str, chat_model: Option<&str>) -> Result<AiStatus, AppError>;
    pub fn check_connection(&mut self, ai: &AiSettings) -> Result<ConnectionCheck, AppError>;
    pub fn disconnect_publik(&mut self, ai: &AiSettings) -> AiStatus;
    pub fn reconnect_publik(&mut self, ai: &mut AiSettings) -> Result<AiStatus, AppError>;
    /// ai_open_link: the URL of `claim`, `top_up` or `add_credit` from the status.
    pub fn link_url(&self, kind: &str) -> Result<String, AppError>;
    /// The client of the chosen provider; modules 07 to 12 make their calls through it.
    pub fn client(&mut self, ai: &AiSettings) -> Result<Arc<dyn AiClient>, AiError>;
}
```

Service messages (`invalid_input` unless stated): "Enter the base URL of an OpenAI-compatible API,
starting with https:// or http://."; "Enter your key."; "Enter the chat model name."; "The local
model server must be on this computer: use a localhost address." (a local base URL whose host is
not `localhost`, `127.0.0.1` or `[::1]`); "Unknown value {kind} for kind." (`link_url`); "This link
is not available yet." (`not_found`, a known kind with no URL).

Provider resolution in `client` and `status`: `own_key` passes the keychain's key to
`resolve_provider_in(env, Some((base_url, key)))` (no key stored → `OwnKeyMissing`); `publik` and
`unset` call `resolve_provider_in(env, None)`; `local` calls no resolver and reads no credential. Own
key requests go to `ai.own_key.base_url`, local requests to `ai.local.base_url`, with the chat model
of the settings, or for local the largest model a probe finds when none is chosen. The local probe is
`GET {base_url}/models` (OpenAI list shape), then `GET {root}/api/tags` for sizes, where `root` is
the base URL without a trailing `/v1` (a failure there leaves sizes unknown); names containing
`embed` are dropped; largest first, unknown sizes last; nothing answering → `LocalDown`.

### Commands and events

Appended to `COMMANDS` (and `commands.ts`) after `backup_import`, in this order:

| Command | Arguments | Result |
|---|---|---|
| `ai_status` | `refresh: bool` | `AiStatus` |
| `ai_consent_accept` | | `AiStatus` |
| `ai_consent_decline` | | `AiStatus` |
| `ai_set_own_key` | `base_url, key, chat_model, transcribe_model?, speech_model?, speech_voice?` | `AiStatus` |
| `ai_clear_own_key` | | `AiStatus` |
| `ai_local_probe` | `base_url` | `[LocalModel]` |
| `ai_set_local` | `base_url, chat_model?` | `AiStatus` |
| `ai_check_connection` | | `ConnectionCheck {ok, charge_micros, message}` |
| `ai_disconnect_publik` | | `AiStatus` |
| `ai_reconnect_publik` | | `AiStatus` |
| `ai_open_link` | `kind: claim/top_up/add_credit` | `()` |

Every command that changes the status also emits it as `ai://status`. `app_start` runs
`AiService::on_start`. AI commands make their network calls off the window's main thread.

The built-app self-test, desktop only:

```rust
// app/src-tauri/src/main.rs: the first lines of main(), so no window and no engine starts.
fn main() {
    if let Some(line) = parley_publik::startup_self_test(
        std::env::var_os("PUBLIK_SELF_TEST").as_deref(), env!("CARGO_PKG_VERSION")) {
        println!("{line}");
        return;
    }
    // ... logging, the database, AppState and tauri::Builder follow
}
```

`parley publik proof` (maintainer only): without `PUBLIK_PROOF=1` in the environment it writes
"This makes one real publik API call. Run it with PUBLIK_PROOF=1." to standard error, nothing to
standard output, sends nothing and exits 1. With it, it resolves publik API (provisioning if it must,
the person who runs it being the consent), makes one P0 call, and prints the reply, the charge line
and every `x-publik-*` header name and value, then the ids `GET /models` lists; never the key.
`scripts/publik-proof` runs it through `cargo run -q -p parley-cli -- publik proof`.

### UI

Strings live in `app/src/strings/en.json`; `ai.setup.disclosure` holds `publik::DISCLOSURE`
verbatim and `ai.setup.data_path` the data-path sentence of PUBLIK-INTEGRATION §4.

- **Path**: while `ai_status` says `provider: "unset"`, a card with the text "Parley works without
  AI. Set it up for fresh lessons, answer checks and conversations." and the button **Set up AI**;
  otherwise the AI chip: "AI: publik API" (with the balance line under it), "AI: your own key" or
  "AI: local model". Nothing AI-related shows while `ai_status` fails.
- **Consent dialog** (`role="dialog"`, named by its title): title **Set up AI features**, the
  data-path sentence, `DISCLOSURE`, "Everything else in Parley works without AI.", buttons **Continue
  with publik API**, **Use my own key**, **Use a local model**, and **Not now**
  (`ai_consent_decline`, closes). It opens from **Set up AI** and whenever an AI command fails with
  `needs_consent`; never at launch or in onboarding. **Continue with publik API** calls
  `ai_consent_accept`; a failure shows its message with **Use my own key** and **Use a local model**.
- **First-run card** (in the dialog after a successful `ai_consent_accept`): `balance_line`,
  `cost_sentence`, the primary button **Sign in with publik** (`ai_open_link` `claim`) while
  `claim_state` is not `claimed`, else **Add a plan or pack** (`add_credit`); **Use my own key**, **Use
  a local model** and **Continue** (closes, running the action that opened the dialog, if any).
- **Settings > AI** (a tab **AI** after **Learning**): calls `ai_status` with `refresh: true` on
  opening; sections **publik API**, **Your own key**, **Local model** in this order, the chosen one
  marked "In use"; **Check connection** shows `ConnectionCheck.message` or the error banner. publik
  API: the balance line, **Sign in with publik** or **Add a plan or pack**, **Use publik API** (when
  another provider is chosen; `ai_consent_accept`, through the dialog when `needs_consent`),
  **Disconnect** or **Reconnect**. Own key: fields **Base URL**, **Key** (a password field, never
  filled back), **Chat model**, the three optional audio fields, **Save my key**, **Remove my key**.
  Local: **Server address** (default `http://localhost:11434/v1`), **Find models**, the models with
  their sizes, **Use this model**.
- **Error banner** (`AiBanner`, `role="alert"`): `needs_credit` shows the message, exactly one link
  button (**Sign in with publik** while anonymous, **Add a plan or pack** once claimed;
  `ai_open_link` `top_up`) and "or use your own key or a local model in Settings"; `disconnected`
  shows "publik API is disconnected." with **Reconnect** (`ai_reconnect_publik`); `busy` and
  `ai_invalid` show the message with **Try again**; any other code shows the message.

## Out of scope

- Every AI feature except P0 (07 to 12); `publik-smart` use (none in v1; the fallback exists).
- A self-test on Android: the Android build starts through Tauri's mobile entry point, not `main`.
- Usage reports to publik (`POST /telemetry`): module 14, see Design notes.

## Design notes

- Live checks at the start of implementation (record the results here): fetch the contract's sources with `curl`; confirm the header
  names with a `-D` dump in the proof call; probe whether `publik-fast` and `publik-balanced` honour
  `response_format`; call `GET /models` and record the ids it lists for `/audio/transcriptions` and
  `/audio/speech`, for modules 09 and 10 (if it lists none, those modules stop until the maintainer has decided).
- A 402 sets `needs_credit`; automatic calls stop until a success, a wallet refresh with a positive
  balance, or a provider change (PUBLIK-INTEGRATION §9).
- Local choice: `GET {base}/models`; for Ollama also `GET /api/tags` for sizes; ignore names
  containing `embed`; pick the largest; the learner may override it.
- Own-key and local calls retry twice (after 1 s and 4 s) on connection errors, timeouts and 5xx;
  Publik calls never retry.
- The self-test provisions for real (`POST /installs`, then the credential file) and runs only when
  the app is started with `PUBLIK_SELF_TEST` set: whoever sets it is the consent. Normal start-up is
  unchanged, so invariant I12 and test 29 still hold.
- Windows: the release build uses the GUI subsystem (`windows_subsystem = "windows"`), so a console
  that starts it does not see its stdout. On the self-test path only, when stdout has no handle,
  attach to the parent console (`AttachConsole(ATTACH_PARENT_PROCESS)` from `windows-sys`) before
  printing; check by hand that the built `.exe` prints the line in PowerShell and in `cmd`.
- Sources fetched again with `curl` on 2026-11-06 (`/developers`, `/pricing`, `/llms.txt`,
  `skills/setup.md`, `publish.md`, `api.md`, `search.md`, `sdk/publik.rs.md`). What changed since the
  contract's reading of 2026-10-26:
  - `llms.txt` and a new `skills/telemetry.md` add usage telemetry: an installed app sends
    `app_opened` and, every 30 minutes, `active_session` to `POST /telemetry` with no key and no
    content, which a user can turn off; publik counts weekly users from it. Sent at every launch,
    it would contact publik before any provider is chosen, against invariant I9 and ARCHITECTURE
    §8. Maintainer decision (2026-11-09): Parley sends these reports only while
    `ai.provider = publik`, never before the learner has chosen publik API, with a switch in
    Settings to stop them, so invariant I9 and the promise that nothing goes to publik before a
    provider is chosen still hold. Module 14 builds it, before the listing (its Scope and
    acceptance tests 13 to 15); module 06 sends nothing.
  - The Rust reference now carries a `search()` helper (web search over `curl`). Parley uses no web
    search and its token will not list `search`, so the helper is not copied.
  - `/developers` names what serves each tier (`publik-fast` z-ai/glm-5.3-flash, `publik-balanced`
    xiaomi/mimo-v2.6-pro, `publik-smart` openai/gpt-6-sol) and their aliases, and describes
    `GET /models` as "the tiers and every alias, OpenAI list shape", which may list no audio model.
    Parley still sends only tier names on chat.
  - Pricing, the $0.05 of free use once (expiring after 30 days), the 4 MB limit, the 402 shape,
    the credential paths and the self-test line are unchanged. The API skill's own proof block reads
    `x-publik-charge-micros`, so that header name is confirmed by the documentation.
- Live checks that need a key are to confirm in the maintainer's setup run (`parley publik proof`
  prints what they need): the other `x-publik-*` header names (taken from contract §7 meanwhile); the
  field names of the `GET /wallet` body (taken as those of the 201 body's `wallet` object, plus
  `week_used_micros`, `week_budget_micros` and `week_resets_at`); whether the tiers honour
  `response_format` (until then it is never sent, and the prompts ask for JSON); and the
  `GET /models` ids for `/audio/*`. The audio methods of the client send the `model` their caller
  gives, unchanged; modules 09 and 10 record those ids in their live checks.
- Results of the maintainer's setup run (2026-11-12), against the live service:
  - The listing is live at https://publikhq.com/parley with the products `ai.chat`,
    `ai.transcribe` and `ai.speech`, category Study (publik's categories are AI, Productivity,
    Creative, Health, Study and Lifestyle) and the run command `npm run desktop`. Parley has no
    sign-in gate at launch: the course works offline with no account, and Sign in with publik is
    offered in the AI setup dialog when a learner first chooses publik API (maintainer decision).
  - The release build prints `publik self-test: ok parley 0.1.0 (install <8 characters>)` from
    PowerShell and from cmd, so the console attach of the self-test path works.
  - `GET /models` lists 43 ids, all chat models: the four tiers and vendor aliases. Each entry has
    `id`, `kind`, `object`, `owned_by`, `resolves_to`, `tier`, `context_window` and `created`. It
    lists **no id for `/audio/transcriptions` or `/audio/speech`**. The maintainer decided to probe
    the standard ids once from a linked install instead (see the last item).
  - `GET /wallet` answers with `account`, `add_credit_url`, `app_slug`, `available_micros`,
    `balance_micros`, `base_url`, `claim_code`, `claim_expires_at`, `claim_reissued`,
    `claim_state`, `claim_url`, `daily_cap_micros`, `install_id`, `key_id`, `linked_at`, `pack`,
    `plan`, `plans_url`, `price_epoch`, `smart_locked`, `spent_today_micros`, `starter`,
    `top_up_url` and `week`.
  - A `publik-fast` call from an anonymous install at $0.00 answers 402 with `error.message` and
    `error.top_up_url`, as contract §8 says, and charges nothing.
  - From a linked install, charged calls confirm every header name of contract §7:
    `x-publik-balance`, `x-publik-charge-micros`, `x-publik-request-id`, `x-publik-claim-state`,
    `x-publik-starter-remaining`, `x-publik-week-used`, `x-publik-week-budget` (the text `none`
    when there is no weekly budget) and `x-publik-week-resets-at`, plus `x-publik-model`, the
    model that served the call. A five-word `publik-fast` reply cost 15 micros.
  - `response_format: {"type": "json_object"}` is honoured by `publik-fast` and
    `publik-balanced`: both returned valid JSON (33 and 328 micros). Parley may send it; every
    response is still validated against its schema.
  - Audio ids, probed once by maintainer decision because `GET /models` lists none: `/audio/speech`
    accepts **`tts-1`** (voice `alloy`, MP3, 32 KB for one short sentence, 540 micros) and refuses
    `gpt-4o-mini-tts` with `400 unknown_model`; `/audio/transcriptions` accepts
    **`gpt-4o-mini-transcribe`** (multipart `file`, `model`, `language=es`, `response_format=json`,
    `temperature=0`), which returned the spoken sentence exactly (538 micros). Modules 09 and 10
    use these two ids.
- Adaptations of `publik.rs`, each marked `// ADAPTATION n` in the code: 1 the key never reaches the
  web view (the UI gets `AiStatus`, which has no key); 2 `device_name` is sent only when
  `ai.share_device_name` is on, and then as `device_label()`; 3 `http.rs` (reqwest with rustls)
  replaces `curl`, the key only in the `Authorization` header; 4 one OpenAI-compatible client for the
  three providers (`providers.rs`); 5 status and error mapping live in `status.rs` and `errors.rs`;
  6 every function that reads the process environment has an `_in` twin that takes it as a
  `PublikEnv`, so tests run against a fake server and a temporary credential folder without touching
  the process environment, and provisioning posts to `PUBLIK_API_BASE_URL` when it is set; 7
  `balance_line` writes the amount with `format_dollars`, so a balance under ten cents is not shown
  as $0.00 (the words around it are unchanged); 8 `search()` is left out.
- Decisions made during implementation:
  - The live checks that need a key were not run during implementation: no request reaches
    publikhq.com outside the maintainer's proof. `parley publik proof` prints the header names,
    the charge and the `GET /models` ids they need.
  - TLS is rustls with the ring provider (reqwest's `rustls-no-provider`, rustls with `ring`); the
    certificate is checked against the operating system's store by `rustls-platform-verifier`.
    ring builds with the C compiler alone on every platform the app ships for, and its licence
    (Apache-2.0 AND ISC) is already allowed by `deny.toml`. reqwest's default provider, aws-lc-rs,
    was not taken: it needs CMake or NASM on some platforms and carries the OpenSSL licence.
  - Every request runs on a plain thread of its own (`http.rs`), and the blocking client is built
    once and never dropped: reqwest's blocking client panics inside an async runtime, and Tauri's
    async commands run on one. The AI commands and `pack_download` also run on a blocking thread,
    and `app_start` starts `AiService::on_start` in the background and emits its status as
    `ai://status`, so the first screen never waits for the network. A test calls the transport
    from inside a runtime.
  - A request to `localhost`, `127.0.0.1` or `[::1]` never goes through a proxy, so a local model's
    traffic stays on the machine; other requests honour the proxy environment variables.
  - A connection that is never made (refused, unreachable, or not made within three quarters of
    the request's time limit, at most 10 s) is `Offline` (`LocalDown` for the local server), not a
    timeout; only a server that accepted the connection and did not answer in time is `Timeout`.
    On Windows a refused connection takes about two seconds to say so, longer than a short limit.
  - `cache/ai.sqlite3` is opened for each operation and closed straight after (its folder and
    tables are created when missing), so clearing the cache or deleting everything can remove the
    file at any time, Windows included. `Cache::open` creates the folder of its SQLite file, as
    opening the store and the packs by path does: the one direct file operation of the core,
    beside opening SQLite files.
  - Adaptation 9 of `publik.rs`: `provision()` reads `prior` from the credential file as it is, with
    or without a key, so an install whose key Disconnect forgot keeps its `install_id` (contract §5).
    Disconnect posts `/installs/revoke` (best effort) and rewrites the credential without `key`;
    Reconnect forgets a key that stopped working, then provisions. Under adaptation 6, a 201 without
    `base_url` is stored with the base it was posted to, so a fake server's install never points at
    publikhq.com. `provision_reply_in` also returns the 201 body, whose `wallet.top_up_url` the
    status keeps; the credential file has exactly the reference's fields.
  - `publik-status.json` is read back at start (connected, needs-credit, the last headers, the
    balance and links), so the last known balance shows before the wallet refresh. While
    `connected` is false, the publik client is not built (`Disconnected`, no request) until
    Reconnect succeeds.
  - **Delete everything** also forgets the AI status: after the core's delete succeeds, the
    `data_delete_all` command calls `AiService::forget_status`, which puts the status back to its
    defaults (connected, no needs-credit, no headers, balance, links or probe results) and removes
    `publik-status.json`. The two locks are taken one after the other, never together, and the
    command now runs on a blocking thread so a running AI request never holds up the window. The
    publik credential (outside the data directory; it holds the install's balance) and the own key
    in the keychain are kept, and the handbook says so.
  - The keychain is read only when it is needed: with `ai.provider = own_key` (to build the client
    or the status), when Settings > AI opens (`status(.., refresh: true)`), and when the key is
    saved or removed. Otherwise `own_key_stored` is the last value read (false until the first
    read), so a launch with another provider never touches the keychain (on Linux a locked keyring
    would ask for its password at launch).
  - Text from a server's body (`error.message` of any provider, a 402's message and link, a 429's
    message) has every occurrence of the key in use replaced by `…` before it reaches an error, so
    a server that echoes the key it was sent cannot leak it (I1). The proof does the same with
    everything it prints. Provisioning sends no key, so its messages have none to replace.
  - A 200 whose body has no usable `choices[0].message.content` (the client's `AiError::Invalid`)
    adds one attempt for the key in `call`, like an answer that fails parsing, schema or rules, so
    Try again sends the next attempt's key and is not a replay of the unusable answer.
  - Every AI command reads the AI settings after taking the AI service's lock and saves them before
    releasing it, so two overlapping changes cannot lose one; the settings lock is taken only
    briefly inside it, always in that order.
  - `pack_download` holds the service lock for the whole download, so other commands wait while a
    pack downloads. Packs are not published yet; to revisit when module 14 publishes them.
  - P0's answer that fails its schema is `AiError::Invalid`; `ConnectionCheck.ok` is true whenever
    the check succeeds. In `call`, a prompt that does not render is `Invalid` with no request and no
    count, and a stored answer that no longer deserialises is treated as a miss.
  - Own-key and local calls retry on a refused connection, a timeout and a 5xx, not on other
    transport failures (a certificate error stays an error), and a streamed reply is never retried
    after its first delta. A publik 429 without a message says "publik API is busy; try again in a
    moment".
  - Own key: base URLs are stored without a trailing slash, blank optional fields as null.
    **Remove my key** with the own key chosen sets `ai.provider = unset`. A keychain that refuses
    gives `storage` with "Parley could not keep your key in this computer's keychain." or "Parley
    could not remove your key from this computer's keychain."
  - Local: the probe falls back to the names `/api/tags` lists when `/models` lists none; a model
    already listed is not listed twice; equal sizes sort by name. `client()` with no chosen model
    probes and takes the largest; a server with no chat model gives "The local model server at
    {base_url} has no chat model. Install one, or choose another provider."
  - `ai_open_link` opens only an `http` or `https` URL from the status, with
    `tauri_plugin_opener::open_url` (the plugin is not registered, so the web view has no opener
    permission); a browser that does not open gives `not_found` "Parley could not open the link in
    your browser."
  - Settings > AI fills the own key's base URL and models from `settings_get` (never the key) and
    shows **Remove my key** only while a key is stored; on the path, **Use my own key** and **Use a
    local model** in the dialog open Settings > AI.
  - The keychain is `keyring` 4 with its platform stores; on Linux it reaches the Secret Service
    over zbus, so no system D-Bus library is needed (ADR-0013 names the exit, as its platform
    crates have one maintainer).
  - `publik.rs` and the self-test lines of `main` are kept out of rustfmt (`#[rustfmt::skip]`), so
    the reference merges line by line and the hook reads exactly as above. The Windows console
    attach runs on the self-test path only, when standard output has no handle; the check by hand
    of the built `.exe` in PowerShell and cmd is part of the maintainer's setup run.
  - `parley publik proof` prints plain lines, not JSON: the reply, `charge: $0.000017 (17 micros)`,
    each `x-publik-*` header, then `models: <ids>`; it exits 0 only when the check answered 200.

## Acceptance tests

All HTTP is the fake server (`crates/parley-publik/tests/fake.rs`, a small HTTP/1.1 server on
127.0.0.1 that records every request and replays status, headers, body and delay; reused by the
other test files through `#[path]`); references are the contract and `docs/PUBLIK-INTEGRATION.md`.
Tests 1 to 17, 19, 22, 23, 25, 29 to 31 and the in-process part of 32 are in
`crates/parley-publik/tests/m06_contract.rs`; 11 (with a scripted client), 14, 20, 24 and 28 also or
only in `crates/parley-core/tests/m06_publik_and_providers.rs`, with 17 and 27; 26 in
`crates/parley-cli/tests/m06_publik_and_providers.rs`; the desktop-binary part of 32 and the command
lists in `app/src-tauri/tests/m06_publik_and_providers.rs`; the UI parts of 6, 8, 18, 21 and 24 in
`app/src/test/m06/*.test.tsx`.

1. Resolution: own key beats `PUBLIK_API_KEY` beats the credential file; none → `NeedsConsent`.
2. `provision()` sends the contract §5 body with no `device_name` (with `share_device_name`, exactly
   `device_label()`); 201 writes the credential with mode 0600 (on Unix) before returning; 200 with
   `key: null` keeps the key on disk; 200 with none on disk mints once more under a fresh id;
   `install_revoked` mints once more; a second failure shows its message.
3. The token comes from `publik-app-token.txt`, overridden by `PUBLIK_APP_TOKEN`; without a `pat_`
   token → "This build has no publik API app token. Use your own key for now." (the real-token test
   skips with a message until the maintainer step).
4. Requests carry the key only in `Authorization`; chat requests carry a tier name as `model` and
   no vendor model name, while audio requests carry exactly the model id their caller gives (the
   `GET /models` ids modules 09 and 10 record), never a tier; the idempotency key is 32 lowercase hex
   equal to the first 32 of the SHA-256 of the cache key's JSON.
5. Every header of contract §7 is parsed into the status; `GET /wallet` refreshes it.
6. 402 (anonymous and claimed): the message, exactly one URL (`top_up_url`), one call, needs-credit
   set, nothing cached, the batch stopped, the app still usable.
7. A success after a 402 clears needs-credit.
8. 401 and 403 `install_revoked` mark disconnected and offer **Reconnect**.
9. 429, 503 and other errors: one attempt and the messages above.
10. A Publik timeout is not retried; an own-key timeout is retried twice.
11. Caches are per provider; identical input never spends twice (the second call makes no request);
    a prompt version bump misses (test-only prompt `test_echo.v1` and `.v2`).
12. The local provider makes no request to publikhq.com with a credential file present.
13. `publik-smart` falls back to `publik-balanced` while `claim_state` is `anonymous`.
14. An invalid response is discarded with "The AI answer could not be used." and not cached.
15. A request of `MAX_REQUEST_BYTES` or more (an `image_part` of a large JPEG) is refused with
    `TooLarge` and nothing is sent; a small one is sent.
16. The key appears in no error (its `Display`, `Debug` and serialised forms), `AiStatus`,
    `publik-status.json`, request `Debug` form or command result (a scan for the fake key
    `pk_live_a` over all of them; invariant I1).
17. The copy rule over `en.json`, `docs/` (except `docs/PUBLIK-CONTRACT.md` and the test itself),
    `README.md` and `content/`.
18. The consent dialog opens from the path's "Set up AI" card and not at launch or in onboarding;
    its title, sentence and buttons equal PUBLIK-INTEGRATION §4, `DISCLOSURE` shown verbatim (the
    Rust test compares `ai.setup.disclosure` in `en.json` with `publik::DISCLOSURE`).
19. `ai_open_link("top_up")` opens the status URL; there is no command that opens an arbitrary URL
    (`ai_open_link` takes only `kind`, and any other value is refused).
20. The renderer fails on a missing or unused placeholder; learner text is JSON-escaped.
21. The first-run card at $0.00 anonymous shows `balance_line(0, "anonymous")` and **Sign in with
    publik**; claimed shows **Add a plan or pack**.
22. Local probe with `llama3.1:8b` (4.7 GB), `llama3.2:3b` (2.0 GB) and `nomic-embed-text` → chooses
    `llama3.1:8b`; with the server down → the local-down message.
23. `ai_set_own_key` stores the key in the keychain fake; `settings.json`, `AiStatus` and every
    command result contain no key.
24. Check connection with a charge of 17 micros → "Connected. This check cost $0.000017.";
    `format_dollars` of 3,120,000, 100,000, 49,000 and 5,800 micros → `$3.12`, `$0.10`, `$0.049`,
    `$0.0058`.
25. The cost of a run of calls (`Called::charge_micros` summed, a cache hit adding nothing) equals the
    sum of the `x-publik-charge-micros` headers sent (17, 1,500 and 2,300 → 3,817 micros); modules 07
    and 11 store it in `sessions.cost_micros` and `conversations.cost_micros`.
26. `scripts/publik-proof` without `PUBLIK_PROOF=1` exits 1 with "This makes one real publik API
    call. Run it with PUBLIK_PROOF=1." and sends nothing.
27. `SECURITY.md` names the `device_name` field and every endpoint listed in ARCHITECTURE §8.
28. Idempotency: a **Try again** after a response that failed validation sends a different key
    (`"attempt": 2`, then 3), and the count survives a restart; a **Try again** after a timeout sends
    the same key; a later valid answer is cached under the attempt-free key; two P0 clicks send two
    different random keys (PUBLIK-INTEGRATION test 36).
29. Start-up with `ai.provider` = `unset`, `own_key` or `local` makes no request to publik API;
    with `publik` it makes one `GET /wallet`, and opening Settings makes another (test 37).
30. With a stored own key and `ai.provider = publik`, every request carries the Publik key and goes to
    publik API; with `ai.provider = own_key`, every request goes to the own base URL (invariant I9).
31. After a 201 fixture, `AiStatus` has `cost_sentence`, `claim_url` and `add_credit_url` from the
    body; after a 402 fixture, `top_up_url` equals `error.top_up_url`; `ai_open_link("claim")` opens
    `claim_url`.
32. Built-app self-test (contract test 18). With the fake server answering `POST /installs` with a
    201 whose `install_id` is `5f0c8a52-6e0b-4c55-9a3e-2a3f1d9b7c11`,
    `startup_self_test_in(&env, Some(OsStr::new("1")), "0.1.0")` returns exactly
    `publik self-test: ok parley 0.1.0 (install 5f0c8a52)`; with the fake answering 503 with an empty
    body it returns `publik self-test: failed: publik API could not set up this computer (503). Use
    your own key for now.`; with `None` it returns `None` and the fake records no request. The
    desktop binary, started with `PUBLIK_SELF_TEST=1`, `PUBLIK_APP_TOKEN=none` (so the token check
    fails before any request) and `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `APPDATA` and
    `LOCALAPPDATA` pointing at an empty temporary directory, prints exactly one line,
    `publik self-test: failed: This build has no publik API app token. Use your own key for now.`,
    exits with code 0 within 10 s, and leaves that directory empty (no window, database, log or
    credential file).

## Status

Done (2026-11-09)
