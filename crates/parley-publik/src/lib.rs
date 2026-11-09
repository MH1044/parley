//! Parley's AI client crate (docs/modules/06-publik-and-providers.md; docs/PUBLIK-INTEGRATION.md;
//! docs/PUBLIK-CONTRACT.md).
//!
//! - [`publik`]: the publik API reference module, copied verbatim except the slug, the version and
//!   the marked adaptations; the app token `publik-app-token.txt`, public by design, sits next to
//!   this crate's Cargo.toml.
//! - [`http`]: the transport (reqwest with rustls, server-sent events), and the desktop app's real
//!   network for pack downloads.
//! - [`providers`]: one OpenAI-compatible client for publik API, the learner's own key and a local
//!   model server, implementing `parley_core::ai::AiClient`.
//! - [`status`] and [`errors`]: the status object and the error mapping.
//! - [`keychain`]: the own key in the operating system's keychain.
//! - [`service`]: everything the desktop app's `ai_*` commands do.
//! - [`proof`]: `parley publik proof`, the maintainer's one real call.
//!
//! No key ever leaves this crate toward the interface: the interface receives `AiStatus`, which has
//! no key field (invariant I1).

pub mod errors;
pub mod http;
pub mod keychain;
pub mod proof;
pub mod providers;
// Kept in the reference's own layout, so publik's later versions merge line by line.
#[rustfmt::skip]
pub mod publik;
pub mod service;
pub mod status;

use std::ffi::OsStr;

pub use publik::PublikEnv;

/// The built-app self-test (contract §3): None when `var` is None; otherwise
/// Some(self_test_in(env, version)), the one line
/// "publik self-test: ok parley <version> (install <8 chars>)" or "publik self-test: failed: <message>".
pub fn startup_self_test_in(env: &PublikEnv, var: Option<&OsStr>, version: &str) -> Option<String> {
    var.map(|_| publik::self_test_in(env, version))
}

/// The same with `PublikEnv::from_process()`, read only when `var` is Some.
pub fn startup_self_test(var: Option<&OsStr>, version: &str) -> Option<String> {
    var.map(|_| publik::self_test_in(&PublikEnv::from_process(), version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::test_server::{Reply, serve};

    fn env(base_url: &str, dir: &std::path::Path, token: &str) -> PublikEnv {
        PublikEnv {
            api_key: None,
            base_url: Some(base_url.into()),
            app_token: Some(token.into()),
            credential_path: dir.join("publik").join("apps").join("parley.json"),
        }
    }

    #[test]
    fn the_self_test_prints_one_line_and_runs_only_when_asked() {
        let server = serve(vec![
            Reply::json(
                201,
                r#"{"key":"pk_live_a","base_url":"{BASE}/api/v1","install_id":"5f0c8a52-6e0b-4c55-9a3e-2a3f1d9b7c11","claim_state":"anonymous","balance_micros":0}"#,
            ),
            Reply::json(503, ""),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let env = env(&server.url("/api/v1"), dir.path(), "pat_parley_test");
        assert_eq!(startup_self_test_in(&env, None, "0.1.0"), None);
        assert!(server.requests().is_empty());
        assert_eq!(
            startup_self_test_in(&env, Some(OsStr::new("1")), "0.1.0").as_deref(),
            Some("publik self-test: ok parley 0.1.0 (install 5f0c8a52)")
        );
        std::fs::remove_file(&env.credential_path).unwrap();
        assert_eq!(
            startup_self_test_in(&env, Some(OsStr::new("1")), "0.1.0").as_deref(),
            Some(
                "publik self-test: failed: publik API could not set up this computer (503). Use your own key for now."
            )
        );
        let no_token = PublikEnv {
            app_token: Some("none".into()),
            ..env
        };
        assert_eq!(
            startup_self_test_in(&no_token, Some(OsStr::new("1")), "0.1.0").as_deref(),
            Some(
                "publik self-test: failed: This build has no publik API app token. Use your own key for now."
            )
        );
        assert_eq!(server.requests().len(), 2);
    }

    #[test]
    fn the_credential_is_written_before_provision_returns_and_names_no_device_by_default() {
        let server = serve(vec![Reply::json(
            201,
            r#"{"key":"pk_live_a","install_id":"i-1","claim_url":"https://example.invalid/c","claim_state":"anonymous","balance_micros":0,"wallet":{"add_credit_url":"https://example.invalid/a"},"disclosure":{"cost":"Cheap."}}"#,
        )]);
        let dir = tempfile::tempdir().unwrap();
        let env = env(&server.url("/api/v1"), dir.path(), "pat_parley_test");
        let credential = publik::provision_in(&env, "0.1.0", None).unwrap();
        let on_disk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&env.credential_path).unwrap()).unwrap();
        assert_eq!(on_disk, credential);
        assert_eq!(credential["base_url"], server.url("/api/v1"));
        assert_eq!(credential["cost_sentence"], "Cheap.");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&env.credential_path)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let body: serde_json::Value = serde_json::from_slice(&server.requests()[0].body).unwrap();
        assert!(body.get("device_name").is_none());
        assert_eq!(body["app_token"], "pat_parley_test");
        assert_eq!(body["dialects"], serde_json::json!(["chat_completions"]));
        assert!(matches!(
            publik::resolve_provider_in(&env, None),
            publik::Provider::Publik { .. }
        ));
        let debug = format!("{:?}", publik::resolve_provider_in(&env, None));
        assert!(!debug.contains("pk_live_a"));
        assert!(matches!(
            publik::resolve_provider_in(
                &env,
                Some(("https://own.example.invalid".into(), "sk".into()))
            ),
            publik::Provider::Own { .. }
        ));
    }

    #[test]
    fn the_balance_line_writes_small_amounts_in_full() {
        assert_eq!(publik::balance_line(None, "anonymous"), "");
        assert_eq!(
            publik::balance_line(Some(0), "anonymous"),
            "$0.00 · link this computer for $0.05 of free use"
        );
        assert_eq!(
            publik::balance_line(Some(0), "claimed"),
            "$0.00 of publik balance"
        );
        assert_eq!(
            publik::balance_line(Some(49_000), "claimed"),
            "$0.049 of publik balance"
        );
        assert_eq!(
            publik::balance_line(Some(3_120_000), "anonymous"),
            "$3.12 of publik balance"
        );
        assert_eq!(
            publik::device_label(),
            if cfg!(windows) {
                "Windows PC"
            } else if cfg!(target_os = "macos") {
                "Mac"
            } else {
                "Linux PC"
            }
        );
    }
}
