//! The app's state: one `Parley` behind a mutex, built from the data directory, the bundled
//! course and the real environment, and one `AiService` behind another.
//!
//! Lock order: an AI command takes the AI lock, then the `Parley` lock briefly inside it (to read
//! and save the AI settings); nothing takes them the other way round, so the two cannot deadlock.

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use parley_core::app::{Config, Env, Parley};
use parley_publik::PublikEnv;
use parley_publik::http::HttpNet;
use parley_publik::keychain::OsKeychain;
use parley_publik::service::{AiService, AiServiceConfig};

use crate::bundled;
use crate::env::{DirFs, SystemClock, time_seeded_rng};

/// Own-key and local calls retry after these waits (docs/PUBLIK-INTEGRATION.md §3).
pub const AI_BACKOFF: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(4)];

pub struct AppState {
    parley: Mutex<Parley>,
    ai: Mutex<AiService>,
}

impl AppState {
    /// The state of an app whose data lives in `data_dir`, with the real network (pack downloads)
    /// and the real AI providers.
    pub fn new(data_dir: PathBuf) -> AppState {
        let config = Config {
            data_dir: data_dir.clone(),
            courses: bundled::courses(),
            packs_lock: bundled::PACKS_LOCK.to_string(),
        };
        let env = Env {
            clock: Box::new(SystemClock),
            rng: Box::new(time_seeded_rng()),
            fs: Box::new(DirFs::new(data_dir.clone())),
            net: Box::new(HttpNet::new()),
        };
        let ai = AiService::new(AiServiceConfig {
            data_dir,
            publik: PublikEnv::from_process(),
            keychain: Box::new(OsKeychain),
            rng: Box::new(time_seeded_rng()),
            backoff: AI_BACKOFF,
            app_version: parley_core::api::VERSION.to_string(),
        });
        AppState {
            parley: Mutex::new(Parley::new(config, env)),
            ai: Mutex::new(ai),
        }
    }

    /// Runs `f` with the service. A command that panicked leaves the mutex poisoned; the state
    /// itself is still consistent (every store write is one transaction), so it is used as is.
    pub fn with<T>(&self, f: impl FnOnce(&mut Parley) -> T) -> T {
        let mut parley = self.parley.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut parley)
    }

    /// Runs `f` with the AI service.
    pub fn with_ai<T>(&self, f: impl FnOnce(&mut AiService) -> T) -> T {
        let mut ai = self.ai.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut ai)
    }
}
