//! Unit tests for the pure catalog model (`vellum::model::catalog`). Zero I/O —
//! build a tree from fixture data, navigate `database → schema → relation →
//! column`, and resolve foreign keys (issue #12). Ordering is insertion order
//! (deterministic for diffs / tests).

use vellum::model::catalog::{Catalog, Column, Database, ForeignKey, Reference, Relation, RelationKind, Schema};

/// `app` database, `public` schema, with `orders` (FK → `users.id`) inserted
/// *before* `users` — so iteration order can be asserted as insertion order,
/// not alphabetical.
fn sample_catalog() -> Catalog {
  Catalog {
    databases: vec![Database {
      name: "app".to_string(),
      schemas: vec![Schema {
        name: "public".to_string(),
        relations: vec![
          Relation {
            name: "orders".to_string(),
            kind: RelationKind::Table,
            columns: vec![
              Column {
                name: "id".to_string(),
                data_type: "bigint".to_string(),
                nullable: false,
                primary_key: true,
              },
              Column {
                name: "user_id".to_string(),
                data_type: "bigint".to_string(),
                nullable: false,
                primary_key: false,
              },
            ],
            foreign_keys: vec![ForeignKey {
              name: Some("orders_user_id_fkey".to_string()),
              columns: vec!["user_id".to_string()],
              references: Reference {
                schema: None,
                relation: "users".to_string(),
                columns: vec!["id".to_string()],
              },
            }],
          },
          Relation {
            name: "users".to_string(),
            kind: RelationKind::Table,
            columns: vec![
              Column {
                name: "id".to_string(),
                data_type: "bigint".to_string(),
                nullable: false,
                primary_key: true,
              },
              Column {
                name: "email".to_string(),
                data_type: "text".to_string(),
                nullable: true,
                primary_key: false,
              },
            ],
            foreign_keys: vec![],
          },
        ],
      }],
    }],
  }
}

#[test]
fn navigates_database_schema_relation_column() {
  let catalog = sample_catalog();

  let db = catalog.database("app").expect("database `app`");
  let schema = db.schema("public").expect("schema `public`");
  let users = schema.relation("users").expect("relation `users`");
  assert_eq!(users.kind, RelationKind::Table);

  let email = users.column("email").expect("column `email`");
  assert_eq!(email.data_type, "text");
  assert!(email.nullable);
  assert!(!email.primary_key);

  let id = users.column("id").expect("column `id`");
  assert!(id.primary_key);
  assert!(!id.nullable);

  // Absent lookups are `None`, not a panic.
  assert!(catalog.database("nope").is_none());
  assert!(db.schema("nope").is_none());
  assert!(schema.relation("nope").is_none());
  assert!(users.column("nope").is_none());
}

#[test]
fn preserves_insertion_order() {
  // Deterministic ordering: relations come back in the order inserted
  // (`orders`, then `users`), not alphabetised — stable for diffs and tests.
  let catalog = sample_catalog();
  let schema = catalog.database("app").unwrap().schema("public").unwrap();

  let names: Vec<&str> = schema.relations.iter().map(|r| r.name.as_str()).collect();
  assert_eq!(names, ["orders", "users"]);
}

#[test]
fn resolves_a_same_schema_foreign_key() {
  // `orders.user_id` → `users.id`, both in `public` (reference omits a schema).
  let catalog = sample_catalog();
  let db = catalog.database("app").unwrap();
  let orders = db.schema("public").unwrap().relation("orders").unwrap();
  let fk = &orders.foreign_keys[0];

  let target = db.resolve(fk, "public").expect("the FK resolves to a relation");
  assert_eq!(target.name, "users");

  // The referenced columns actually exist on the target.
  for col in &fk.references.columns {
    assert!(
      target.column(col).is_some(),
      "referenced column `{col}` must exist on the target"
    );
  }
}

#[test]
fn resolves_a_cross_schema_foreign_key() {
  // A reference that names a schema is followed across schemas, not assumed to
  // be in the owning relation's schema.
  let db = Database {
    name: "app".to_string(),
    schemas: vec![
      Schema {
        name: "sales".to_string(),
        relations: vec![Relation {
          name: "orders".to_string(),
          kind: RelationKind::Table,
          columns: vec![Column {
            name: "customer_id".to_string(),
            data_type: "bigint".to_string(),
            nullable: false,
            primary_key: false,
          }],
          foreign_keys: vec![ForeignKey {
            name: None,
            columns: vec!["customer_id".to_string()],
            references: Reference {
              schema: Some("crm".to_string()),
              relation: "customers".to_string(),
              columns: vec!["id".to_string()],
            },
          }],
        }],
      },
      Schema {
        name: "crm".to_string(),
        relations: vec![Relation {
          name: "customers".to_string(),
          kind: RelationKind::Table,
          columns: vec![Column {
            name: "id".to_string(),
            data_type: "bigint".to_string(),
            nullable: false,
            primary_key: true,
          }],
          foreign_keys: vec![],
        }],
      },
    ],
  };

  let fk = &db.schema("sales").unwrap().relation("orders").unwrap().foreign_keys[0];
  let target = db.resolve(fk, "sales").expect("cross-schema FK resolves");
  assert_eq!(target.name, "customers");
  assert!(target.column("id").is_some());
}

#[test]
fn unresolvable_foreign_key_is_none() {
  // A reference to a relation that isn't in the database resolves to `None`,
  // not a panic.
  let catalog = sample_catalog();
  let db = catalog.database("app").unwrap();
  let dangling = ForeignKey {
    name: None,
    columns: vec!["user_id".to_string()],
    references: Reference {
      schema: None,
      relation: "ghost".to_string(),
      columns: vec!["id".to_string()],
    },
  };
  assert!(db.resolve(&dangling, "public").is_none());
}
