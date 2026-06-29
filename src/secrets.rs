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
  fn set(&self, _connection: &str, _secret: &SecretString) -> Result<()> {
    // stub — round-trip is pinned by the red test first
    Ok(())
  }

  fn get(&self, _connection: &str) -> Result<Option<SecretString>> {
    Ok(None)
  }

  fn delete(&self, _connection: &str) -> Result<()> {
    Ok(())
  }
}
