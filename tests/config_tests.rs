//! Unit tests for the `.vellum.toml` parser (`vellum::config`). Pure parse —
//! a TOML string in, a typed `Config` out, no I/O. This pins the connection
//! manager contract we **freeze for 1.0** (issue #8): the named-connection
//! shape, `[ui]` defaults, the closed backend set, and the safety gates
//! (unknown keys / plaintext secrets rejected).

use vellum::config::Config;
use vellum::error::VellumError;
use vellum::model::Backend;

#[test]
fn parses_a_multi_connection_file() {
  // The frozen schema, exercised in full: a server connection (every field)
  // and a file-backed one (path only), plus an explicit `[ui]` block.
  let toml = r#"
    [connections.local-pg]
    backend  = "postgres"
    host     = "localhost"
    port     = 5432
    user     = "kbrdn1"
    database = "app_dev"
    sslmode  = "prefer"

    [connections.proj-sqlite]
    backend = "sqlite"
    path    = "./data/app.db"

    [ui]
    page_size = 50
    theme     = "midnight"
  "#;

  let config = Config::from_toml_str(toml).expect("valid file should parse");

  assert_eq!(config.connections.len(), 2);

  let pg = &config.connections["local-pg"];
  assert_eq!(pg.backend, Backend::Postgres);
  assert_eq!(pg.host.as_deref(), Some("localhost"));
  assert_eq!(pg.port, Some(5432));
  assert_eq!(pg.user.as_deref(), Some("kbrdn1"));
  assert_eq!(pg.database.as_deref(), Some("app_dev"));
  assert_eq!(pg.sslmode.as_deref(), Some("prefer"));
  assert_eq!(pg.path, None);

  let sqlite = &config.connections["proj-sqlite"];
  assert_eq!(sqlite.backend, Backend::Sqlite);
  assert_eq!(sqlite.path.as_deref(), Some("./data/app.db"));
  assert_eq!(sqlite.host, None);

  // The explicit `[ui]` values win over the defaults.
  assert_eq!(config.ui.page_size, 50);
  assert_eq!(config.ui.theme, "midnight");
}

#[test]
fn applies_ui_defaults_when_section_omitted() {
  // No `[ui]` at all → the whole block defaults (page_size 200, theme
  // "vellum"). A connection with no optional fields parses too.
  let toml = r#"
    [connections.only]
    backend = "mysql"
  "#;

  let config = Config::from_toml_str(toml).expect("file without [ui] should parse");

  assert_eq!(config.connections["only"].backend, Backend::MySql);
  assert_eq!(config.ui.page_size, 200);
  assert_eq!(config.ui.theme, "vellum");
}

#[test]
fn rejects_an_unknown_backend() {
  // The backend set is closed — an unrecognised engine name is a config
  // error, not a silently-accepted string.
  let toml = r#"
    [connections.bad]
    backend = "oracle"
  "#;

  let err = Config::from_toml_str(toml).expect_err("unknown backend must error");
  assert!(
    matches!(err, VellumError::Config(_)),
    "expected VellumError::Config, got {err:?}"
  );
}

#[test]
fn rejects_unknown_keys() {
  // The schema is frozen and hand-edited — a typo'd key (here `hostname`
  // instead of `host`) must be loud, not silently dropped.
  let toml = r#"
    [connections.typo]
    backend  = "postgres"
    hostname = "localhost"
  "#;

  let err = Config::from_toml_str(toml).expect_err("unknown key must error");
  assert!(
    matches!(err, VellumError::Config(_)),
    "expected VellumError::Config, got {err:?}"
  );
}

#[test]
fn rejects_a_plaintext_password() {
  // A secret never lives in the file. Reject it on presence with a message
  // that points at the real channels (keyring / VELLUM_DSN), rather than a
  // generic "unknown field".
  let toml = r#"
    [connections.leaky]
    backend  = "postgres"
    user     = "kbrdn1"
    password = "hunter2"
  "#;

  let err = Config::from_toml_str(toml).expect_err("a plaintext password must be refused");
  let VellumError::Config(message) = err else {
    panic!("expected VellumError::Config, got {err:?}");
  };
  let lower = message.to_lowercase();
  assert!(
    lower.contains("password"),
    "message should name the offending key: {message}"
  );
  assert!(
    lower.contains("keyring"),
    "message should point at the keyring: {message}"
  );
}

#[test]
fn rejects_a_non_string_password() {
  // "Refused on presence" must not depend on the secret's TOML type — a
  // non-string `password` (here an integer; tables / dotted keys are the same
  // class) must still hit the keyring gate, not fall through to a generic
  // type error that may echo the value.
  let toml = r#"
    [connections.leaky]
    backend  = "postgres"
    password = 12345
  "#;

  let err = Config::from_toml_str(toml).expect_err("a non-string password must be refused");
  let VellumError::Config(message) = err else {
    panic!("expected VellumError::Config, got {err:?}");
  };
  let lower = message.to_lowercase();
  assert!(
    lower.contains("password") && lower.contains("keyring"),
    "should hit the keyring gate, not a generic type error: {message}"
  );
  // And never echo the secret value back.
  assert!(
    !message.contains("12345"),
    "the error must not reflect the secret value: {message}"
  );
}

#[test]
fn parsed_connection_carries_no_password_surface() {
  // Safe by construction: the public `Connection` has no `password` field at
  // all, so its derived `Debug` cannot leak one and there is no field for a
  // direct-deserialise bypass to populate.
  let toml = r#"
    [connections.c]
    backend = "postgres"
    host    = "localhost"
  "#;

  let config = Config::from_toml_str(toml).expect("valid file should parse");
  let rendered = format!("{:?}", config.connections["c"]);
  assert!(
    !rendered.to_lowercase().contains("password"),
    "public Connection must carry no password surface: {rendered}"
  );
}

#[test]
fn rejects_connection_names_that_collide_under_the_env_override() {
  // `local-pg` and `local_pg` both normalise to `VELLUM_DSN_LOCAL_PG` (#9), so
  // one connection's env override would silently apply to the other — refused
  // on a frozen security contract, before it can mis-route a credential.
  let toml = r#"
    [connections.local-pg]
    backend = "postgres"

    [connections.local_pg]
    backend = "postgres"
  "#;

  let err =
    Config::from_toml_str(toml).expect_err("colliding env-override names must be rejected");
  assert!(
    matches!(err, VellumError::Config(_)),
    "expected VellumError::Config, got {err:?}"
  );
}
