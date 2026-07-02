//! OS-keyring [`SecretStore`] backend + the storing seam `vellum connect` uses
//! (#72).
//!
//! [`KeyringStore`] adapts the OS keychain (macOS Keychain, Windows Credential
//! Manager, Linux `keyutils`) to the [`SecretStore`] port from
//! [`crate::secrets`]. It is a thin adapter — its correctness is the `keyring`
//! crate's; the port *contract* is exercised via `MemoryStore`, and the storing
//! path `connect` takes is covered through the port by [`store_secret`] (no real
//! keychain in CI). Passwords are namespaced under the service `vellum`, keyed
//! by the connection name — i.e. `vellum:<name>`.

use keyring::Entry;

use crate::error::{Result, VellumError};
use crate::secrets::{ExposeSecret, SecretStore, SecretString};

/// The keychain service every vellum secret is filed under; the connection name
/// is the per-entry key, giving the `vellum:<name>` namespacing the port
/// documents.
const SERVICE: &str = "vellum";

/// A [`SecretStore`] over the OS keychain. Stateless — each operation opens a
/// fresh keyring [`Entry`] for `SERVICE` + the connection name.
#[derive(Debug, Default)]
pub struct KeyringStore;

impl KeyringStore {
  /// A keyring store over the OS keychain.
  pub fn new() -> Self {
    Self
  }

  fn entry(&self, connection: &str) -> Result<Entry> {
    Entry::new(SERVICE, connection)
      .map_err(|e| VellumError::Secret(format!("keyring: could not open entry for `{connection}`: {e}")))
  }
}

impl SecretStore for KeyringStore {
  fn set(&self, connection: &str, secret: &SecretString) -> Result<()> {
    self
      .entry(connection)?
      .set_password(secret.expose_secret())
      .map_err(|e| VellumError::Secret(format!("keyring: could not store `{connection}`: {e}")))
  }

  fn get(&self, connection: &str) -> Result<Option<SecretString>> {
    match self.entry(connection)?.get_password() {
      Ok(password) => Ok(Some(SecretString::from(password))),
      // A missing entry is "no secret stored", not an error — mirrors
      // `MemoryStore::get` returning `None`.
      Err(keyring::Error::NoEntry) => Ok(None),
      Err(e) => Err(VellumError::Secret(format!(
        "keyring: could not read `{connection}`: {e}"
      ))),
    }
  }

  fn delete(&self, connection: &str) -> Result<()> {
    match self.entry(connection)?.delete_credential() {
      // Removing an absent entry is not an error (port contract).
      Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
      Err(e) => Err(VellumError::Secret(format!(
        "keyring: could not delete `{connection}`: {e}"
      ))),
    }
  }
}

/// Store (or replace) `secret` for `name` through `store` — the exact seam
/// `vellum connect` calls after prompting for the password. Generic over the
/// [`SecretStore`] port so the storing path is covered by a `MemoryStore`
/// round-trip without touching a real keychain.
pub fn store_secret(store: &dyn SecretStore, name: &str, secret: &SecretString) -> Result<()> {
  store.set(name, secret)
}
