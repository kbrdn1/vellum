//! Connection secrets — never in `.vellum.toml`.
//!
//! A connection's password lives in a secret store (or is supplied as a full
//! DSN via `VELLUM_DSN_<NAME>` for CI / scripting); `.vellum.toml` only ever
//! names the connection. This module owns that contract:
//!
//! - [`SecretStore`] is the port — set / get / delete a password by connection
//!   name. [`MemoryStore`] is the process-local impl backing `VELLUM_DSN`-only
//!   runs and the tests (no global state, no real keychain in CI). The OS
//!   keyring backend and the `vellum connect` command that populates it land
//!   in a follow-up, behind this same port.
//! - [`resolve`] is the precedence rule a driver consumes: a `VELLUM_DSN_<NAME>`
//!   environment override wins, otherwise the stored password.
//!
//! In-memory secrets are [`SecretString`]s: zeroized on drop and redacted in
//! `Debug`, so a password never lands in a log.

use std::collections::BTreeMap;
use std::sync::Mutex;

pub use secrecy::{ExposeSecret, SecretString};

use crate::error::{Result, VellumError};

/// Store / fetch / delete a connection's password. The key is the connection
/// name; implementations namespace it (the keyring backend will key it as
/// `vellum:<name>`).
pub trait SecretStore {
  /// Store (or replace) the password for `connection`.
  fn set(&self, connection: &str, secret: &SecretString) -> Result<()>;
  /// Fetch the password for `connection`, or `None` if none is stored.
  fn get(&self, connection: &str) -> Result<Option<SecretString>>;
  /// Remove the password for `connection`. Removing an absent entry is not an
  /// error.
  fn delete(&self, connection: &str) -> Result<()>;
}

/// Process-local, in-memory [`SecretStore`]. Nothing is persisted — it backs
/// `VELLUM_DSN`-only runs and tests without touching the OS keyring.
#[derive(Default)]
pub struct MemoryStore {
  inner: Mutex<BTreeMap<String, SecretString>>,
}

impl SecretStore for MemoryStore {
  fn set(&self, connection: &str, secret: &SecretString) -> Result<()> {
    let mut map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
    map.insert(connection.to_string(), secret.clone());
    Ok(())
  }

  fn get(&self, connection: &str) -> Result<Option<SecretString>> {
    let map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
    Ok(map.get(connection).cloned())
  }

  fn delete(&self, connection: &str) -> Result<()> {
    let mut map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
    map.remove(connection);
    Ok(())
  }
}

/// The credential a driver should use for a connection. Holds a redacted
/// secret — `Debug` shows no value, and it is intentionally not comparable.
#[derive(Debug)]
pub enum Credential {
  /// A full DSN from `VELLUM_DSN_<NAME>` — used as-is, keyring bypassed.
  Dsn(SecretString),
  /// A password from the store — the driver combines it with the
  /// `.vellum.toml` connection fields.
  Password(SecretString),
}

/// The environment variable that overrides a connection's stored secret with a
/// full DSN: `VELLUM_DSN_<NAME>`, the name uppercased with every
/// non-alphanumeric character folded to `_`. This is part of the frozen 1.0
/// contract.
pub fn env_var_name(connection: &str) -> String {
  let suffix: String = connection
    .chars()
    .map(|c| {
      if c.is_ascii_alphanumeric() {
        c.to_ascii_uppercase()
      } else {
        '_'
      }
    })
    .collect();
  format!("VELLUM_DSN_{suffix}")
}

/// Resolve a connection's credential: a `VELLUM_DSN_<NAME>` override wins,
/// otherwise the password held by `store`. `None` if neither is configured.
///
/// A `VELLUM_DSN_<NAME>` that is *set but not valid UTF-8* is an error, not a
/// silent fall-through to the store — an override must not quietly swap the
/// credential source. The error never echoes the (unreadable) value.
pub fn resolve(connection: &str, store: &dyn SecretStore) -> Result<Option<Credential>> {
  resolve_with(connection, store, |key| match std::env::var(key) {
    Ok(value) => Ok(Some(value)),
    Err(std::env::VarError::NotPresent) => Ok(None),
    Err(std::env::VarError::NotUnicode(_)) => Err(VellumError::Secret(format!(
      "`{key}` is set but is not valid UTF-8 — fix or unset it"
    ))),
  })
}

/// [`resolve`] with the environment lookup injected. This is the seam tests
/// drive: they pin the precedence deterministically without mutating the
/// process environment (a write that races parallel readers — the same
/// global-state hazard that pushed the keyring impl behind this port). `env`
/// maps an env var name to its value (`Ok(None)` when unset), and may report a
/// present-but-unreadable override as an error — which [`resolve`] surfaces
/// rather than falling back to the store.
pub fn resolve_with(
  connection: &str,
  store: &dyn SecretStore,
  env: impl Fn(&str) -> Result<Option<String>>,
) -> Result<Option<Credential>> {
  if let Some(dsn) = env(&env_var_name(connection)).ok().flatten() {
    return Ok(Some(Credential::Dsn(SecretString::from(dsn))));
  }
  Ok(store.get(connection)?.map(Credential::Password))
}
