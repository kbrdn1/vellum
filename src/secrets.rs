//! Connection secrets — never in `.vellum.toml`.
//!
//! A connection's password lives in the OS keyring (or is supplied as a full
//! DSN via `VELLUM_DSN_<NAME>` for CI / scripting); `.vellum.toml` only ever
//! names the connection. This module owns that contract:
//!
//! - [`SecretStore`] is the port — set / get / delete a password by connection
//!   name. [`KeyringStore`] is the OS-backed impl; [`MemoryStore`] is a
//!   process-local impl (handy for `VELLUM_DSN`-only runs and for tests,
//!   without the keyring's process-global state).
//! - [`resolve`] is the precedence rule a driver consumes: a `VELLUM_DSN_<NAME>`
//!   environment override wins, otherwise the stored password.
//!
//! In-memory secrets are [`SecretString`]s: zeroized on drop and redacted in
//! `Debug`, so a password never lands in a log.

use std::collections::BTreeMap;
use std::sync::Mutex;

pub use secrecy::{ExposeSecret, SecretString};

use crate::error::Result;

/// Store / fetch / delete a connection's password. The key is the connection
/// name; implementations namespace it (the keyring uses `vellum:<name>`).
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
    map.insert(connection.to_string(), clone_secret(secret));
    Ok(())
  }

  fn get(&self, connection: &str) -> Result<Option<SecretString>> {
    let map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
    Ok(map.get(connection).map(clone_secret))
  }

  fn delete(&self, connection: &str) -> Result<()> {
    let mut map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
    map.remove(connection);
    Ok(())
  }
}

/// `SecretString` is not `Clone`, so duplicate it through its exposed bytes.
/// The transient `String` is the only extra copy and is dropped immediately.
fn clone_secret(secret: &SecretString) -> SecretString {
  SecretString::from(secret.expose_secret().to_string())
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
pub fn env_var_name(_connection: &str) -> String {
  // stub — the transform is pinned by the red test first
  String::new()
}

/// Resolve a connection's credential: a `VELLUM_DSN_<NAME>` override wins,
/// otherwise the password held by `store`. `None` if neither is configured.
pub fn resolve(_connection: &str, _store: &dyn SecretStore) -> Result<Option<Credential>> {
  // stub
  Ok(None)
}
