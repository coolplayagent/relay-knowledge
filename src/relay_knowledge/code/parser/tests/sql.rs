use crate::domain::{CodeIndexSnapshot, CodeParseStatus, CodeRepositoryRegistration};

use super::*;

#[test]
fn sql_files_extract_schema_symbols_references_calls_and_chunks() {
    let snapshot = parse_source_snapshot(
        "schema/main.sql",
        br#"
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    organization_id INTEGER REFERENCES organizations(id)
);

CREATE VIEW active_users AS
SELECT id FROM users;

CREATE TRIGGER users_update
AFTER UPDATE ON users
FOR EACH ROW
EXECUTE FUNCTION notify_user(NEW.id);
"#,
    );

    assert_eq!(snapshot.files[0].language_id, "sql");
    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert_sql_symbol(&snapshot, "users", "table");
    assert_sql_symbol(&snapshot, "active_users", "view");
    assert_sql_symbol(&snapshot, "users_update", "trigger");
    assert!(
        snapshot
            .references
            .iter()
            .any(|reference| reference.name == "organizations" && reference.kind == "reference"),
        "foreign key target should be indexed as a SQL object reference: {:?}",
        snapshot.references
    );
    assert!(
        snapshot
            .references
            .iter()
            .any(|reference| reference.name == "users" && reference.kind == "reference"),
        "view and trigger source table should be indexed as SQL object references: {:?}",
        snapshot.references
    );
    assert!(
        snapshot
            .references
            .iter()
            .any(|reference| reference.name == "notify_user" && reference.kind == "call"),
        "trigger function invocation should be indexed as a call reference: {:?}",
        snapshot.references
    );
    assert!(
        snapshot
            .chunks
            .iter()
            .any(|chunk| chunk.content.contains("CREATE TABLE users")),
        "SQL schema definitions should create retrievable chunks: {:?}",
        snapshot.chunks
    );
}

#[test]
fn sql_files_extract_functions_and_types() {
    let function = parse_source_snapshot(
        "schema/routines.sql",
        br#"
CREATE FUNCTION add_one(x INTEGER) RETURNS INTEGER RETURN x + 1;
"#,
    );
    let type_definition = parse_source_snapshot(
        "schema/types.sql",
        br#"
CREATE TYPE currency;
"#,
    );

    assert_eq!(function.files[0].language_id, "sql");
    assert_eq!(function.files[0].parse_status, CodeParseStatus::Parsed);
    assert_eq!(type_definition.files[0].language_id, "sql");
    assert_eq!(
        type_definition.files[0].parse_status,
        CodeParseStatus::Parsed
    );
    assert_sql_symbol(&function, "add_one", "function");
    assert_sql_symbol(&type_definition, "currency", "type");
}

#[test]
fn sql_files_preserve_qualified_names_and_normalize_identifiers() {
    let snapshot = parse_source_snapshot(
        "schema/qualified.sql",
        br#"
CREATE TABLE Sales.Orders (
    id INTEGER PRIMARY KEY
);

CREATE TABLE "Support"."Orders" (
    id INTEGER PRIMARY KEY
);

CREATE VIEW Sales.OpenOrders AS
SELECT id FROM Sales.Orders;

CREATE MATERIALIZED VIEW Sales.OrderSummary AS
SELECT id FROM "Support"."Orders";
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert_sql_symbol_record(&snapshot, "sales.orders", "table");
    assert_sql_symbol_record(&snapshot, "Support.Orders", "table");
    assert_sql_symbol_record(&snapshot, "sales.openorders", "view");
    assert_sql_symbol_record(&snapshot, "sales.ordersummary", "view");
    assert_sql_reference(&snapshot, "sales.orders", "reference");
    assert_sql_reference(&snapshot, "Support.Orders", "reference");
    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "Orders"),
        "qualified SQL names should not be collapsed to the terminal identifier: {:?}",
        snapshot
            .symbols
            .iter()
            .map(|symbol| symbol.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn sql_files_recover_procedure_symbols_as_call_targets() {
    let snapshot = parse_source_snapshot(
        "schema/procedures.sql",
        br#"
CREATE PROCEDURE Sales.RefreshCache() LANGUAGE SQL AS 'SELECT 1';

CREATE TRIGGER refresh_cache_after_insert
AFTER INSERT ON Sales.Orders
FOR EACH ROW
EXECUTE PROCEDURE Sales.RefreshCache(NEW.id);
"#,
    );

    assert_eq!(snapshot.files[0].language_id, "sql");
    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Partial);
    assert_sql_symbol_record(&snapshot, "sales.refreshcache", "function");
    assert_sql_symbol_record(&snapshot, "refresh_cache_after_insert", "trigger");
    assert_sql_reference(&snapshot, "sales.refreshcache", "call");
}

#[test]
fn sql_files_preserve_qualified_invocation_call_targets() {
    let snapshot = parse_source_snapshot(
        "schema/invocation.sql",
        br#"
SELECT Sales.RefreshCache(1);
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert_sql_reference(&snapshot, "sales.refreshcache", "call");
    assert_no_sql_reference(&snapshot, "refreshcache", "call");
}

#[test]
fn sql_files_do_not_record_unsupported_sequence_declarations_as_references() {
    let snapshot = parse_source_snapshot(
        "schema/sequence.sql",
        br#"
CREATE SEQUENCE Sales.OrderIdSeq;
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "sales.orderidseq"),
        "unsupported SQL sequence declarations should not create symbols: {:?}",
        snapshot
            .symbols
            .iter()
            .map(|symbol| (symbol.name.as_str(), symbol.kind.as_str()))
            .collect::<Vec<_>>()
    );
    assert_no_sql_reference(&snapshot, "sales.orderidseq", "reference");
}

#[test]
fn sql_files_preserve_function_return_type_references() {
    let snapshot = parse_source_snapshot(
        "schema/function_return_type.sql",
        br#"
CREATE TYPE Sales.Currency;

CREATE FUNCTION Sales.Balance() RETURNS Sales.Currency RETURN NULL;
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert_sql_symbol_record(&snapshot, "sales.currency", "type");
    assert_sql_symbol_record(&snapshot, "sales.balance", "function");
    assert_sql_reference(&snapshot, "sales.currency", "reference");
    assert_no_sql_reference(&snapshot, "sales.balance", "reference");
}

fn assert_sql_symbol(snapshot: &CodeIndexSnapshot, name: &str, kind: &str) {
    assert_sql_symbol_record(snapshot, name, kind);
    assert!(
        snapshot
            .chunks
            .iter()
            .any(|chunk| chunk.content.contains(name)),
        "missing SQL chunk for {name}; got {:?}",
        snapshot
            .chunks
            .iter()
            .map(|chunk| chunk.content.as_str())
            .collect::<Vec<_>>()
    );
}

fn assert_sql_symbol_record(snapshot: &CodeIndexSnapshot, name: &str, kind: &str) {
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == name && symbol.kind == kind),
        "missing SQL {kind} symbol {name}; got {:?}",
        snapshot
            .symbols
            .iter()
            .map(|symbol| (symbol.name.as_str(), symbol.kind.as_str()))
            .collect::<Vec<_>>()
    );
}

fn assert_sql_reference(snapshot: &CodeIndexSnapshot, name: &str, kind: &str) {
    assert!(
        snapshot
            .references
            .iter()
            .any(|reference| reference.name == name && reference.kind == kind),
        "missing SQL {kind} reference {name}; got {:?}",
        snapshot
            .references
            .iter()
            .map(|reference| (reference.name.as_str(), reference.kind.as_str()))
            .collect::<Vec<_>>()
    );
}

fn assert_no_sql_reference(snapshot: &CodeIndexSnapshot, name: &str, kind: &str) {
    assert!(
        !snapshot
            .references
            .iter()
            .any(|reference| reference.name == name && reference.kind == kind),
        "unexpected SQL {kind} reference {name}; got {:?}",
        snapshot
            .references
            .iter()
            .map(|reference| (reference.name.as_str(), reference.kind.as_str()))
            .collect::<Vec<_>>()
    );
}

fn parse_source_snapshot(path: &str, source: &[u8]) -> CodeIndexSnapshot {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );

    parse_indexed_file(&mut build, path, source).expect("file should parse");

    build.finish()
}
