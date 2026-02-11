use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use thiserror::Error;

/// Keychain service name used for all secret storage.
const KEYRING_SERVICE: &str = "com.amberize.app";

/// Process-wide in-memory cache so the macOS Keychain is only accessed once per
/// secret per app session. Without this, every background sync cycle (every 15
/// minutes) triggers a Keychain access prompt on macOS — especially during
/// development where the binary signature changes on each rebuild.
static SECRET_CACHE: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Error)]
pub enum SecretStoreError {
    #[error("keychain error: {0}")]
    Keychain(String),

    #[error("internal error: {0}")]
    Internal(String),
}

pub type SecretStoreResult<T> = Result<T, SecretStoreError>;

pub trait SecretStore: Send + Sync {
    fn set_secret(&self, secret_ref: &str, secret: &str) -> SecretStoreResult<()>;
    fn get_secret(&self, secret_ref: &str) -> SecretStoreResult<Option<String>>;
    fn delete_secret(&self, secret_ref: &str) -> SecretStoreResult<()>;
}

#[derive(Debug, Clone)]
pub struct KeychainSecretStore;

impl KeychainSecretStore {
    pub fn new() -> Self {
        Self
    }
}

impl Default for KeychainSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for KeychainSecretStore {
    fn set_secret(&self, secret_ref: &str, secret: &str) -> SecretStoreResult<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, secret_ref)
            .map_err(|err| SecretStoreError::Keychain(err.to_string()))?;
        entry
            .set_password(secret)
            .map_err(|err| SecretStoreError::Keychain(err.to_string()))?;

        // Update the in-memory cache so subsequent reads avoid the Keychain entirely.
        if let Ok(mut cache) = SECRET_CACHE.lock() {
            cache.insert(secret_ref.to_string(), secret.to_string());
        }

        Ok(())
    }

    fn get_secret(&self, secret_ref: &str) -> SecretStoreResult<Option<String>> {
        // Serve from the process-wide cache when available (avoids macOS Keychain prompts).
        if let Ok(cache) = SECRET_CACHE.lock() {
            if let Some(cached) = cache.get(secret_ref) {
                return Ok(Some(cached.clone()));
            }
        }

        // Cache miss — read from Keychain (may trigger a macOS prompt on first access).
        let entry = keyring::Entry::new(KEYRING_SERVICE, secret_ref)
            .map_err(|err| SecretStoreError::Keychain(err.to_string()))?;

        let secret = match entry.get_password() {
            Ok(value) => Some(value),
            Err(keyring::Error::NoEntry) => None,
            Err(err) => return Err(SecretStoreError::Keychain(err.to_string())),
        };

        if let Some(ref value) = secret {
            if let Ok(mut cache) = SECRET_CACHE.lock() {
                cache.insert(secret_ref.to_string(), value.clone());
            }
        }

        Ok(secret)
    }

    fn delete_secret(&self, secret_ref: &str) -> SecretStoreResult<()> {
        // Remove from cache first.
        if let Ok(mut cache) = SECRET_CACHE.lock() {
            cache.remove(secret_ref);
        }

        let entry = keyring::Entry::new(KEYRING_SERVICE, secret_ref)
            .map_err(|err| SecretStoreError::Keychain(err.to_string()))?;

        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(SecretStoreError::Keychain(err.to_string())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemorySecretStore {
    secrets_by_ref: Arc<Mutex<HashMap<String, String>>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self {
            secrets_by_ref: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Default for MemorySecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for MemorySecretStore {
    fn set_secret(&self, secret_ref: &str, secret: &str) -> SecretStoreResult<()> {
        let mut guard = self
            .secrets_by_ref
            .lock()
            .map_err(|_| SecretStoreError::Internal("poisoned mutex".to_string()))?;
        guard.insert(secret_ref.to_string(), secret.to_string());
        Ok(())
    }

    fn get_secret(&self, secret_ref: &str) -> SecretStoreResult<Option<String>> {
        let guard = self
            .secrets_by_ref
            .lock()
            .map_err(|_| SecretStoreError::Internal("poisoned mutex".to_string()))?;
        Ok(guard.get(secret_ref).cloned())
    }

    fn delete_secret(&self, secret_ref: &str) -> SecretStoreResult<()> {
        let mut guard = self
            .secrets_by_ref
            .lock()
            .map_err(|_| SecretStoreError::Internal("poisoned mutex".to_string()))?;
        guard.remove(secret_ref);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_roundtrip() {
        let store = MemorySecretStore::new();
        store.set_secret("a", "secret").unwrap();
        assert_eq!(store.get_secret("a").unwrap(), Some("secret".to_string()));
        store.delete_secret("a").unwrap();
        assert_eq!(store.get_secret("a").unwrap(), None);
    }
}
