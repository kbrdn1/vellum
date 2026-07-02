//! The OS-keyring `SecretStore` backend and the storing seam `vellum connect`
//! uses (#72). The keyring adapter itself can't be exercised in CI (no real
//! keychain), so the *storing path* is covered through the port with the
//! in-process `MemoryStore` — the same contract the keyring impl satisfies — and
//! the keyring backend is pinned at compile time to implement the port.

use vellum::keyring_store::{store_secret, KeyringStore};
use vellum::secrets::{ExposeSecret, MemoryStore, SecretStore, SecretString};

#[test]
fn store_secret_persists_the_password_through_the_port() {
  let store = MemoryStore::default();
  store_secret(&store, "prod-pg", &SecretString::from("s3cr3t".to_string())).unwrap();
  let got = store.get("prod-pg").unwrap().expect("a secret is stored");
  assert_eq!(
    got.expose_secret(),
    "s3cr3t",
    "connect stores the password under the connection name"
  );
}

#[test]
fn store_secret_replaces_an_existing_password() {
  // Re-running `vellum connect <name>` overwrites, it doesn't error or append.
  let store = MemoryStore::default();
  store_secret(&store, "c", &SecretString::from("old".to_string())).unwrap();
  store_secret(&store, "c", &SecretString::from("new".to_string())).unwrap();
  assert_eq!(store.get("c").unwrap().unwrap().expose_secret(), "new");
}

/// The OS keyring backend implements the same `SecretStore` port. Compile-time
/// only — constructing a `KeyringStore` touches no keychain, and CI never calls
/// `set`/`get` on it.
#[allow(dead_code)]
fn keyring_store_implements_the_port(s: &KeyringStore) -> &dyn SecretStore {
  s
}
