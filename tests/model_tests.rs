//! Unit tests for the pure domain model: the cross-DB `Value` enum, its
//! `TypeKind` mapping, and the `QueryResult` container. No I/O — these
//! exercise the "mini-contract" that absorbs per-backend type divergence
//! (ARCHITECTURE risk #1).

use vellum::model::{Column, QueryResult, TypeKind, Value};

#[test]
fn kind_maps_every_value_variant() {
  // Totality: every `Value` variant reports its `TypeKind`. Witnessing all
  // eight here is the proof that the mapping is total (issue #4 acceptance).
  assert_eq!(Value::Null.kind(), TypeKind::Null);
  assert_eq!(Value::Bool(true).kind(), TypeKind::Bool);
  assert_eq!(Value::Int(42).kind(), TypeKind::Int);
  assert_eq!(Value::Float(1.5).kind(), TypeKind::Float);
  assert_eq!(Value::Text("hi".into()).kind(), TypeKind::Text);
  assert_eq!(Value::Bytes(vec![1, 2, 3]).kind(), TypeKind::Bytes);
  assert_eq!(Value::Json("{}".into()).kind(), TypeKind::Json);
  assert_eq!(
    Value::Timestamp("2026-06-26T00:00:00Z".into()).kind(),
    TypeKind::Timestamp
  );
}

#[test]
fn display_renders_each_variant() {
  assert_eq!(Value::Null.to_string(), "NULL");
  assert_eq!(Value::Bool(true).to_string(), "true");
  assert_eq!(Value::Bool(false).to_string(), "false");
  assert_eq!(Value::Int(-7).to_string(), "-7");
  // Rust renders `1.0_f64` as "1" and `3.25` as "3.25" — no trailing ".0".
  assert_eq!(Value::Float(3.25).to_string(), "3.25");
  assert_eq!(Value::Float(1.0).to_string(), "1");
  assert_eq!(Value::Text("hello".into()).to_string(), "hello");
  assert_eq!(Value::Bytes(vec![0xde, 0xad, 0xbe, 0xef]).to_string(), "<4 bytes>");
  assert_eq!(Value::Json(r#"{"a":1}"#.into()).to_string(), r#"{"a":1}"#);
  assert_eq!(
    Value::Timestamp("2026-06-26T13:00:00Z".into()).to_string(),
    "2026-06-26T13:00:00Z"
  );
}

#[test]
fn query_result_holds_columns_rows_and_affected() {
  let result = QueryResult {
    columns: vec![
      Column {
        name: "id".into(),
        kind: TypeKind::Int,
      },
      Column {
        name: "name".into(),
        kind: TypeKind::Text,
      },
    ],
    rows: vec![
      vec![Value::Int(1), Value::Text("alice".into())],
      vec![Value::Int(2), Value::Null],
    ],
    affected: None,
  };
  assert_eq!(result.columns.len(), 2);
  assert_eq!(result.columns[0].name, "id");
  assert_eq!(result.columns[1].kind, TypeKind::Text);
  assert_eq!(result.rows.len(), 2);
  assert_eq!(result.rows[1][1], Value::Null);
  assert_eq!(result.affected, None);
}

#[test]
fn affected_tracks_write_row_count() {
  // INSERT/UPDATE/DELETE report affected rows; SELECT leaves it `None`.
  let write = QueryResult {
    columns: vec![],
    rows: vec![],
    affected: Some(3),
  };
  assert_eq!(write.affected, Some(3));
}
