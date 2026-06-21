//! OS-keychain-backed storage for secret variable values. Plaintext secret
//! values never touch the `variables.value` SQLite column — only this module
//! talks to the platform credential store, keyed by `var:{variable_id}`.
//!
//! The real backend is swapped for an in-memory fake under `cfg(test)`: the
//! `keyring` v1 API installs its native store via a process-global `Once`,
//! so unit tests would otherwise hit the developer's actual login keychain
//! (permission prompts, leftover `var:{uuid}` entries, failures on a
//! headless/CI box with no keychain at all). Every test variable id is a
//! fresh UUID, so the fake needs no per-test isolation beyond the map itself.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SecretBackendStatus {
    Available,
    Unavailable,
}

#[cfg(not(test))]
mod backend {
    use super::SecretBackendStatus;
    use crate::error::{AppError, AppResult};
    use keyring::{Entry, Error as KeyringError};

    const SERVICE: &str = "restman";

    fn entry(variable_id: &str) -> AppResult<Entry> {
        Entry::new(SERVICE, &format!("var:{variable_id}"))
            .map_err(|e| AppError::Other(format!("keychain unavailable: {e}")))
    }

    pub fn set(variable_id: &str, value: &str) -> AppResult<()> {
        if value.is_empty() {
            return delete(variable_id);
        }
        entry(variable_id)?
            .set_password(value)
            .map_err(|e| AppError::Other(format!("failed to write secret to keychain: {e}")))
    }

    pub fn get(variable_id: &str) -> AppResult<Option<String>> {
        match entry(variable_id)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(e) => Err(AppError::Other(format!("failed to read secret from keychain: {e}"))),
        }
    }

    pub fn delete(variable_id: &str) -> AppResult<()> {
        match entry(variable_id)?.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(e) => Err(AppError::Other(format!("failed to delete secret from keychain: {e}"))),
        }
    }

    /// Whether a platform credential store is actually wired up.
    /// `Entry::new` fails fast with `NoDefaultStore` when none is — e.g.
    /// Linux with no Secret Service provider (gnome-keyring/KWallet)
    /// running. Cheap: this never touches the store itself, just checks
    /// whether one was registered.
    pub fn backend_status() -> SecretBackendStatus {
        match Entry::new(SERVICE, "__restman_backend_probe__") {
            Ok(_) => SecretBackendStatus::Available,
            Err(_) => SecretBackendStatus::Unavailable,
        }
    }
}

pub use backend::{backend_status, delete, get, set};

#[cfg(test)]
mod backend {
    use super::SecretBackendStatus;
    use crate::error::AppResult;
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};

    static STORE: LazyLock<Mutex<HashMap<String, String>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    pub fn set(variable_id: &str, value: &str) -> AppResult<()> {
        if value.is_empty() {
            return delete(variable_id);
        }
        STORE.lock().unwrap().insert(variable_id.to_string(), value.to_string());
        Ok(())
    }

    pub fn get(variable_id: &str) -> AppResult<Option<String>> {
        Ok(STORE.lock().unwrap().get(variable_id).cloned())
    }

    pub fn delete(variable_id: &str) -> AppResult<()> {
        STORE.lock().unwrap().remove(variable_id);
        Ok(())
    }

    pub fn backend_status() -> SecretBackendStatus {
        SecretBackendStatus::Available
    }
}
