//! Where passwords and tokens live: the operating system's credential store
//! (Windows Credential Manager, macOS Keychain, Secret Service on Linux).
//!
//! Secrets never touch the SQLite store, logs or error messages. Callers name a
//! secret by account address; this module owns the service name.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::brand;
use crate::domain::Provider;
use crate::oauth::{OAuthConfig, OAuthError, ReqwestHttp, Tokens};

/// A credential-store failure. The message never contains the secret.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    /// The platform store refused the operation.
    #[error("the system credential store failed: {0}")]
    Store(String),
}

/// Read and write secrets by key.
pub trait Secrets: Send + Sync {
    /// The secret for `key`, or `None` if none is saved.
    fn get(&self, key: &str) -> Result<Option<String>, SecretError>;
    /// Saves or replaces the secret for `key`.
    fn set(&self, key: &str, secret: &str) -> Result<(), SecretError>;
    /// Removes the secret for `key`. Removing a missing secret is fine.
    fn delete(&self, key: &str) -> Result<(), SecretError>;

    /// Gets fresh OAuth tokens. Plain stores can't; [`Refreshing`] can.
    fn refresh_tokens(&self, provider: Provider, _: &Tokens) -> Result<Tokens, OAuthError> {
        Err(OAuthError::Provider(format!(
            "{provider:?} tokens can't be refreshed here"
        )))
    }
}

/// Refreshes OAuth tokens for a provider.
pub type TokenRefresher =
    Arc<dyn Fn(Provider, &Tokens) -> Result<Tokens, OAuthError> + Send + Sync>;

/// A secret store that can also refresh OAuth tokens.
#[derive(Clone)]
pub struct Refreshing {
    inner: Arc<dyn Secrets>,
    refresh: TokenRefresher,
}

impl Refreshing {
    /// Wraps `inner`, refreshing with `refresh`.
    pub fn new(inner: Arc<dyn Secrets>, refresh: TokenRefresher) -> Self {
        Self { inner, refresh }
    }

    /// The production refresher: real token endpoints over TLS.
    pub fn networked(inner: Arc<dyn Secrets>) -> Self {
        Self::new(
            inner,
            Arc::new(|provider, tokens| {
                let config = OAuthConfig::for_provider(provider)?;
                crate::oauth::refresh(&ReqwestHttp::new()?, &config, tokens, chrono::Utc::now())
            }),
        )
    }
}

impl Secrets for Refreshing {
    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        self.inner.get(key)
    }

    fn set(&self, key: &str, secret: &str) -> Result<(), SecretError> {
        self.inner.set(key, secret)
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        self.inner.delete(key)
    }

    fn refresh_tokens(&self, provider: Provider, tokens: &Tokens) -> Result<Tokens, OAuthError> {
        (self.refresh)(provider, tokens)
    }
}

/// The operating system's credential store.
#[derive(Debug, Default, Clone, Copy)]
pub struct Keychain;

impl Keychain {
    fn entry(key: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(brand::PRODUCT_NAME, key).map_err(|e| SecretError::Store(e.to_string()))
    }
}

impl Secrets for Keychain {
    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        match Self::entry(key)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(SecretError::Store(e.to_string())),
        }
    }

    fn set(&self, key: &str, secret: &str) -> Result<(), SecretError> {
        Self::entry(key)?
            .set_password(secret)
            .map_err(|e| SecretError::Store(e.to_string()))
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(SecretError::Store(e.to_string())),
        }
    }
}

/// Secrets held in memory only, for tests and previews.
#[derive(Debug, Default)]
pub struct MemorySecrets(Mutex<HashMap<String, String>>);

impl MemorySecrets {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        // A poisoned lock only means another test thread panicked mid-write;
        // the map itself is still usable.
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Secrets for MemorySecrets {
    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        Ok(self.map().get(key).cloned())
    }

    fn set(&self, key: &str, secret: &str) -> Result<(), SecretError> {
        self.map().insert(key.to_owned(), secret.to_owned());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        self.map().remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_secrets_round_trip() {
        let s = MemorySecrets::default();
        assert_eq!(s.get("you@x.com").unwrap(), None);
        s.set("you@x.com", "hunter2").unwrap();
        assert_eq!(s.get("you@x.com").unwrap().as_deref(), Some("hunter2"));
        s.delete("you@x.com").unwrap();
        s.delete("you@x.com").unwrap();
        assert_eq!(s.get("you@x.com").unwrap(), None);
    }

    #[test]
    fn errors_never_contain_secrets() {
        let err = SecretError::Store("access denied".into());
        assert_eq!(
            err.to_string(),
            "the system credential store failed: access denied"
        );
    }

    #[test]
    fn keychain_reports_or_works_but_never_panics() {
        // CI machines may have no credential store; either outcome is fine,
        // as long as a missing entry reads as None rather than an error when it works.
        let key = format!("sift-test-{}", std::process::id());
        match Keychain.get(&key) {
            Ok(value) => assert_eq!(value, None),
            Err(SecretError::Store(_)) => {}
        }
        let _ = Keychain.delete(&key);
    }
}
