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
  Settings opens, only while `ai.provider = publik`), `errors.rs`.
- Provider choice by `ai.provider` (PUBLIK-INTEGRATION §3): the own key reaches
  `resolve_provider` only when `ai.provider = own_key`; `local` never calls the resolver.
- Idempotency keys and the `ai_attempts` table (PUBLIK-INTEGRATION §6.1), including P0's random key.
- The `AiClient` trait and `ai::call` pipeline in `parley-core`: render the prompt, compute the cache
  key, look up the cache, call once, parse, validate against the schema, run the feature's rules,
  store only schema-valid responses, return.
- Prompt files and loader (format of PUBLIK-INTEGRATION §6.1), `prompts/ping.v1.md`, JSON Schema
  validation with `jsonschema`.
- Own key in the OS keychain (`keyring`); local model probe and choice.
- Commands marked 06 in ARCHITECTURE §5 and the event `ai://status`.
- UI: the AI setup dialog and first-run card (strings of PUBLIK-INTEGRATION §4), Settings > AI,
  the AI chip on the path, the error banners of §9, the balance line.
- `scripts/publik-proof` → `parley publik proof` (maintainer only: one real P0 call).
- The built-app self-test (contract §3): the first lines of `main` in `app/src-tauri/src/main.rs`,
  before logging, the database, `AppState` and `tauri::Builder`, print the one line of
  `publik::self_test` and return when `PUBLIK_SELF_TEST` is set. Desktop only; normal start-up never
  runs it. The setup run of PUBLIK-INTEGRATION §2 uses it to prove the release build.
- `SECURITY.md`: every network call and the `device_name` field.

## Files

- `crates/parley-publik/{Cargo.toml,publik-app-token.txt,src/*.rs,tests/{fake.rs,m06_contract.rs}}`
- `crates/parley-core/src/ai/{client.rs,cache.rs,prompts.rs,schema.rs,call.rs}`, `prompts/ping.v1.md`,
  `schemas/ai/ping.v1.schema.json`
- `app/src-tauri/src/commands/ai.rs`; `app/src-tauri/src/main.rs` (the self-test hook, first lines of
  `main`); `app/src/components/{AiSetupDialog,BalanceLine,AiChip,AiBanner}.tsx`;
  `app/src/screens/Settings/AiSection.tsx`
- `crates/parley-core/tests/m06_publik_and_providers.rs`, `app/src-tauri/tests/m06_publik_and_providers.rs`
  (starts the desktop binary for test 32), `tests/fixtures/ai/P0/`, `tests/fixtures/http/`

## Interfaces

```rust
pub trait AiClient: Send + Sync {
    fn chat(&self, req: ChatRequest) -> Result<ChatReply, AiError>;
    fn chat_stream(&self, req: ChatRequest, on_delta: &mut dyn FnMut(&str)) -> Result<ChatReply, AiError>;
    fn transcribe(&self, req: TranscribeRequest) -> Result<TranscriptReply, AiError>;
    fn speech(&self, req: SpeechRequest) -> Result<AudioReply, AiError>;
    fn kind(&self) -> ProviderKind;   // Publik | OwnKey | Local
    fn status(&self) -> AiStatus;
}
pub fn call<T: DeserializeOwned>(client: &dyn AiClient, cache: &Cache, feature: &Prompt,
    input: &serde_json::Value, rules: &dyn Fn(&T) -> Result<(), RuleFailure>) -> Result<Called<T>, AiError>;
// Called { value, cached: bool, charge_micros: Option<i64> }
pub enum AiError { NeedsConsent, NoToken, NeedsCredit { message: String, top_up_url: String },
    Disconnected, RateLimited { message: String }, Busy, Timeout, Offline, Invalid,
    Provider { code: u16, message: String }, LocalDown { base_url: String } }
```

`AiStatus`, with no key field, and where each field comes from:

| Field | Source |
|---|---|
| `provider` | `ai.provider` in settings |
| `needs_consent` | `resolve_provider(None)` returned `NeedsConsent` |
| `connected` | false after a 401 or 403 `install_revoked` until **Reconnect** succeeds |
| `claim_state` | the credential file (from the 201 body), then `x-publik-claim-state` and `GET /wallet` |
| `balance_micros`, `balance_line` | the credential file, then `x-publik-balance` and `GET /wallet`, through `balance_line(micros, claim_state)` |
| `cost_sentence` | the credential file (written by `provision()` from the 201 body's `disclosure.cost`) |
| `claim_url` | the credential file (201 body `claim_url`) |
| `add_credit_url` | the credential file (201 body `wallet.add_credit_url`), then `GET /wallet` |
| `top_up_url` | the last 402 body through `top_up_url_from_402`, else `wallet.top_up_url` from the 201 body or `GET /wallet` |
| `needs_credit` | set by a 402, cleared by a success, a wallet refresh with a positive balance, or a provider change |
| `last_charge_micros`, `week_used_micros`, `week_budget_micros`, `week_resets_at` | the `x-publik-*` headers of the last call |
| `local_model` | the chosen or largest local model |

The UI shows the URLs as labels only; `ai_open_link(kind)` opens the core's own copy of
`claim_url`, `add_credit_url` or `top_up_url` and never a URL passed from the UI.

The built-app self-test (contract §3), desktop only:

```rust
// crates/parley-publik/src/lib.rs, outside the copied publik.rs.
// None when the variable is absent; otherwise Some(publik::self_test(version)), the one line
// "publik self-test: ok parley <version> (install <8 chars>)" or "publik self-test: failed: <message>".
pub fn startup_self_test(var: Option<&std::ffi::OsStr>, version: &str) -> Option<String>;

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

Messages (UI strings): 402 → `error.message`, one link (**Sign in with publik** while
anonymous, **Add a plan or pack** once claimed) opening `error.top_up_url`, and "or use your own key
or a local model in Settings"; 401 and 403 `install_revoked` → "publik API is disconnected." with
**Reconnect**; 429 → `error.message`; 503 → "publik API is busy; try again in a moment" with **Try
again**; timeout → "publik API did not answer in time." with **Try again**; other → "publik API
{code}: {error.message}"; invalid → "The AI answer could not be used."; own key → "Your provider
returned {code}: {message}"; local down → "The local model server at {base_url} is not running.
Start it, or choose another provider."; no token → "This build has no publik API app token. Use
your own key for now."; Check connection → "Connected. This check cost {amount}." or the matching
error. Every amount in the UI goes through `format_dollars(micros)`: two decimals from $0.10 up
(`$3.12`, `$0.10`), otherwise two significant figures (`$0.049`, `$0.0058`, `$0.000017`).

## Out of scope

- Every AI feature except P0 (07 to 12); `publik-smart` use (none in v1; the fallback exists).
- A self-test on Android: the Android build starts through Tauri's mobile entry point, not `main`.

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
- Decisions are filled in here during implementation.

## Acceptance tests

Contract tests in `crates/parley-publik/tests/m06_contract.rs` (the desktop-binary part of test 32
in `app/src-tauri/tests/m06_publik_and_providers.rs`); the rest in
`crates/parley-core/tests/m06_publik_and_providers.rs` and `app/src/**/*.test.tsx`. All HTTP is the
fake server; references are the contract and `docs/PUBLIK-INTEGRATION.md`.

1. Resolution: own key beats `PUBLIK_API_KEY` beats the credential file; none → `NeedsConsent`.
2. `provision()` sends the contract §5 body with no `device_name`; 201 writes the credential with
   mode 0600 before returning; 200 with `key: null` keeps the key on disk; 200 with none on disk
   mints once more under a fresh id; `install_revoked` mints once more; a second failure shows its message.
3. The token comes from `publik-app-token.txt`, overridden by `PUBLIK_APP_TOKEN`; without a `pat_`
   token → "This build has no publik API app token. Use your own key for now." (the real-token test
   skips with a message until the maintainer step).
4. Requests carry the key only in `Authorization`; chat requests carry a tier name as `model` and
   no vendor model name, while audio requests carry exactly the ids recorded from `GET /models`; the
   idempotency key is 32 lowercase hex equal to the first 32 of the SHA-256 of the cache key's JSON.
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
15. (Images) The helper that module 12 uses rejects any request over 4,000,000 bytes.
16. The key appears in no log line, error, `AiStatus`, `publik-status.json` or command result
    (a scan for the fake key `pk_live_a` over all of them; invariant I1).
17. The copy rule over `en.json`, `docs/` (except `docs/PUBLIK-CONTRACT.md` and the test itself),
    `README.md` and `content/`.
18. The consent dialog opens from the path's "Set up AI" card and not at launch or in onboarding;
    its title, sentence and buttons equal PUBLIK-INTEGRATION §4, `DISCLOSURE` shown verbatim.
19. `ai_open_link("top_up")` opens the status URL; there is no command that opens an arbitrary URL.
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
25. Session cost equals the sum of `x-publik-charge-micros` of its calls.
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
    `startup_self_test(Some(OsStr::new("1")), "0.1.0")` returns exactly
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

Planned
