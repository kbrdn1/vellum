//! Unit tests for `vellum::secrets`. The secret-handling contract: a password
//! round-trips through the `SecretStore` port, `VELLUM_DSN_<NAME>` overrides
//! the store, and an in-memory secret never leaks through `Debug`. Tests use
//! the process-local `MemoryStore` — never the OS keyring (no global state, no
//! real keychain in CI).

use vellum::secrets::{
  env_var_name, resolve, resolve_with, Credential, ExposeSecret, MemoryStore, SecretStore,
  SecretString,
};

#[test]
fn memory_store_round_trips_set_get_delete() {
  let store = MemoryStore::default();
  assert!(
    store.get("conn-a").unwrap().is_none(),
    "an unknown connection has no stored secret"
  );

  store
    .set("conn-a", &SecretString::from("s3cr3t".to_string()))
    .unwrap();
  let got = store
    .get("conn-a")
    .unwrap()
    .expect("the secret is present right after set");
  assert_eq!(got.expose_secret(), "s3cr3t");

  store.delete("conn-a").unwrap();
  assert!(
    store.get("conn-a").unwrap().is_none(),
    "the secret is gone after delete"
  );
}

#[test]
fn env_var_name_is_uppercased_with_separators_normalised() {
  // The frozen contract: `VELLUM_DSN_<NAME>`, name uppercased with every
  // non-alphanumeric char folded to `_` (so a hyphenated connection name maps
  // to a legal env var).
  assert_eq!(env_var_name("local-pg"), "VELLUM_DSN_LOCAL_PG");
  assert_eq!(env_var_name("proj.sqlite"), "VELLUM_DSN_PROJ_SQLITE");
}

#[test]
fn resolve_prefers_the_env_dsn_over_the_store() {
  // A `VELLUM_DSN_<NAME>` override wins even when a password is stored — the
  // CI / scripting path bypasses the keyring. Driven through `resolve_with`
  // with an injected env so the test never mutates the process environment.
  let store = MemoryStore::default();
  store
    .set("envwins", &SecretString::from("stored-pw".to_string()))
    .unwrap();

  let env = |key: &str| (key == "VELLUM_DSN_ENVWINS").then(|| "postgres://dsn-from-env".to_string());
  let resolved = resolve_with("envwins", &store, env)
    .unwrap()
    .expect("env supplies a credential");
  match resolved {
    Credential::Dsn(dsn) => assert_eq!(dsn.expose_secret(), "postgres://dsn-from-env"),
    other => panic!("expected a DSN from env, got {other:?}"),
  }
}

#[test]
fn resolve_falls_back_to_the_stored_password() {
  // No env override → the stored password, as a `Password` credential. Uses
  // the real `resolve`: with no override set, the lookup misses and the store
  // wins (read-only env access — nothing here writes the environment).
  let store = MemoryStore::default();
  store
    .set("fallback", &SecretString::from("stored-pw".to_string()))
    .unwrap();

  let resolved = resolve("fallback", &store)
    .unwrap()
    .expect("the store supplies a credential");
  match resolved {
    Credential::Password(pw) => assert_eq!(pw.expose_secret(), "stored-pw"),
    other => panic!("expected a stored password, got {other:?}"),
  }
}

#[test]
fn resolve_returns_none_when_nothing_is_configured() {
  // No env, no stored password → no credential.
  let store = MemoryStore::default();
  assert!(resolve("absent", &store).unwrap().is_none());
}
