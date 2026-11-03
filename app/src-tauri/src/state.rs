//! The app's state: one `Parley` behind a mutex, built from the data directory, the bundled
//! course and the real environment.

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use parley_core::app::{Config, Env, Parley};
use parley_core::env::FailingNet;

use crate::bundled;
use crate::env::{DirFs, SystemClock, time_seeded_rng};

pub struct AppState {
    parley: Mutex<Parley>,
}

impl AppState {
    /// The state of an app whose data lives in `data_dir`. There is no network until the HTTP
    /// transport arrives (module 06): pack downloads answer offline.
    pub fn new(data_dir: PathBuf) -> AppState {
        let config = Config {
            data_dir: data_dir.clone(),
            courses: bundled::courses(),
            packs_lock: bundled::PACKS_LOCK.to_string(),
        };
        let env = Env {
            clock: Box::new(SystemClock),
            rng: Box::new(time_seeded_rng()),
            fs: Box::new(DirFs::new(data_dir)),
            net: Box::new(FailingNet),
        };
        AppState {
            parley: Mutex::new(Parley::new(config, env)),
        }
    }

    /// Runs `f` with the service. A command that panicked leaves the mutex poisoned; the state
    /// itself is still consistent (every store write is one transaction), so it is used as is.
    pub fn with<T>(&self, f: impl FnOnce(&mut Parley) -> T) -> T {
        let mut parley = self.parley.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut parley)
    }
}
