//! Where the learner's own key lives (docs/ARCHITECTURE.md §4): the operating system's keychain on
//! desktop (Windows Credential Manager, macOS Keychain, the Secret Service on Linux), never
//! `settings.json`, a backup, a log or anything the interface receives (invariant I1).
//!
//! Errors are short descriptions without the secret.

use std::sync::{Arc, Mutex, PoisonError};

/// A slot for one secret.
pub trait Keychain: Send {
    fn get(&self) -> Result<Option<String>, String>;
    fn set(&self, secret: &str) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

/// The keychain entry's service name.
pub const SERVICE: &str = "org.parley.app";
/// The keychain entry's user name.
pub const USER: &str = "own_key";

/// The operating system's keychain: service `org.parley.app`, user `own_key`.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsKeychain;

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, USER).map_err(|e| e.to_string())
}

impl Keychain for OsKeychain {
    fn get(&self) -> Result<Option<String>, String> {
        match entry()?.get_password() {
            Ok(secret) => Ok(Some(secret).filter(|s| !s.is_empty())),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, secret: &str) -> Result<(), String> {
        entry()?.set_password(secret).map_err(|e| e.to_string())
    }

    fn delete(&self) -> Result<(), String> {
        match entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// A keychain in memory, for tests; clones share one slot.
#[derive(Clone, Default)]
pub struct MemoryKeychain {
    slot: Arc<Mutex<Option<String>>>,
}

impl MemoryKeychain {
    /// What the slot holds.
    pub fn secret(&self) -> Option<String> {
        self.slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Keychain for MemoryKeychain {
    fn get(&self) -> Result<Option<String>, String> {
        Ok(self.secret())
    }

    fn set(&self, secret: &str) -> Result<(), String> {
        *self.slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(secret.to_string());
        Ok(())
    }

    fn delete(&self) -> Result<(), String> {
        *self.slot.lock().unwrap_or_else(PoisonError::into_inner) = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_of_a_memory_keychain_share_one_slot() {
        let keychain = MemoryKeychain::default();
        let view = keychain.clone();
        assert_eq!(keychain.get(), Ok(None));
        keychain.set("pk_live_a").unwrap();
        assert_eq!(view.secret().as_deref(), Some("pk_live_a"));
        view.delete().unwrap();
        assert_eq!(keychain.get(), Ok(None));
    }
}
