use super::*;

fn fixture() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("CREATE TABLE code_repository_symbols (
        source_scope TEXT, canonical_symbol_id TEXT, symbol_snapshot_id TEXT,
        kind TEXT, signature TEXT, language_id TEXT, callable_signature_key TEXT);
        CREATE INDEX canonical_lookup ON code_repository_symbols (source_scope, canonical_symbol_id, symbol_snapshot_id);").unwrap();
    connection
}

fn insert(
    connection: &Connection,
    id: &str,
    kind: &str,
    signature: &str,
    language: &str,
    key: Option<&str>,
) {
    connection
        .execute(
            "INSERT INTO code_repository_symbols VALUES ('scope', 'canonical', ?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, kind, signature, language, key],
        )
        .unwrap();
}

#[test]
fn canonical_callers_expand_only_matching_structured_declarations() {
    let connection = fixture();
    let key = Some("c-family-callable-v1|3:int|");
    insert(
        &connection,
        "definition",
        "function",
        "int helper(int value) {}",
        "cpp",
        key,
    );
    insert(
        &connection,
        "declaration",
        "function_declaration",
        "helper(int old_name)",
        "cpp",
        key,
    );
    insert(
        &connection,
        "different",
        "function_declaration",
        "helper(double value)",
        "cpp",
        Some("c-family-callable-v1|6:double|"),
    );
    assert_eq!(
        snapshots(&connection, "scope", "canonical", true).unwrap(),
        vec!["definition", "declaration"]
    );
    assert_eq!(
        snapshots(&connection, "scope", "canonical", false).unwrap(),
        vec!["definition"]
    );
    assert!(
        snapshots(&connection, "other", "canonical", true)
            .unwrap()
            .is_empty()
    );
    insert(
        &connection,
        "duplicate",
        "function_declaration",
        "helper(int)",
        "cpp",
        key,
    );
    assert_eq!(
        snapshots(&connection, "scope", "canonical", true)
            .unwrap()
            .len(),
        3
    );
    insert(
        &connection,
        "second",
        "function",
        "int helper(double value) {}",
        "cpp",
        Some("c-family-callable-v1|6:double|"),
    );
    assert!(
        snapshots(&connection, "scope", "canonical", true)
            .unwrap_err()
            .to_string()
            .contains("multiple definitions")
    );
}

#[test]
fn canonical_resolution_keeps_declaration_only_and_non_c_family_policies() {
    let connection = fixture();
    insert(
        &connection,
        "a",
        "function_declaration",
        "def helper(value): ...",
        "python",
        None,
    );
    assert_eq!(
        snapshots(&connection, "scope", "canonical", true).unwrap(),
        vec!["a"]
    );
    insert(
        &connection,
        "b",
        "function_declaration",
        "def helper(value): ...",
        "python",
        None,
    );
    assert!(
        snapshots(&connection, "scope", "canonical", true)
            .unwrap_err()
            .to_string()
            .contains("multiple callable declarations")
    );
    insert(
        &connection,
        "body",
        "function",
        "def helper(value): return value",
        "python",
        None,
    );
    assert_eq!(
        snapshots(&connection, "scope", "canonical", true).unwrap(),
        vec!["body"]
    );
    connection
        .execute("UPDATE code_repository_symbols SET kind = 'variable'", [])
        .unwrap();
    assert!(
        snapshots(&connection, "scope", "canonical", true)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn canonical_unknown_or_oversized_evidence_never_silently_discards_declaration_edges() {
    let connection = fixture();
    insert(
        &connection,
        "body",
        "function",
        "int helper(int value) {}",
        "cpp",
        None,
    );
    insert(
        &connection,
        "decl",
        "function_declaration",
        "helper(int)",
        "cpp",
        None,
    );
    assert!(
        snapshots(&connection, "scope", "canonical", true)
            .unwrap_err()
            .to_string()
            .contains("structured callable signatures")
    );
    assert_eq!(
        snapshots(&connection, "scope", "canonical", false).unwrap(),
        vec!["body"]
    );
    connection
        .execute(
            "UPDATE code_repository_symbols SET callable_signature_key = ?1",
            ["x".repeat(MAX_CALLABLE_SIGNATURE_KEY_BYTES + 1)],
        )
        .unwrap();
    assert!(
        snapshots(&connection, "scope", "canonical", true)
            .unwrap_err()
            .to_string()
            .contains("byte budget")
    );
}

#[test]
fn canonical_resolution_charges_definitions_and_declarations_to_one_candidate_budget() {
    let connection = fixture();
    let key = Some("c-family-callable-v1|3:int|");
    insert(
        &connection,
        "body",
        "function",
        "void helper() {}",
        "cpp",
        key,
    );
    for number in 0..MAX_CANONICAL_SYMBOL_CANDIDATES - 1 {
        insert(
            &connection,
            &format!("decl-{number:04}"),
            "function_declaration",
            "void helper();",
            "cpp",
            key,
        );
    }
    assert_eq!(
        snapshots(&connection, "scope", "canonical", true)
            .unwrap()
            .len(),
        MAX_CANONICAL_SYMBOL_CANDIDATES
    );
    insert(
        &connection,
        "overflow",
        "function_declaration",
        "void helper();",
        "cpp",
        key,
    );
    let error = snapshots(&connection, "scope", "canonical", true).unwrap_err();
    assert!(error.to_string().contains("1024-candidate"));
    assert!(error.to_string().contains("symbol_snapshot_id"));
}

#[test]
fn canonical_resolution_recognizes_signature_only_legacy_callable_kinds() {
    let connection = fixture();
    insert(
        &connection,
        "a",
        "function_declaration",
        "void helper();",
        "c",
        None,
    );
    insert(&connection, "b", "function", "void helper();", "c", None);
    insert(
        &connection,
        "c",
        "function",
        "void helper() { leaf(); }",
        "c",
        None,
    );
    assert_eq!(
        snapshots(&connection, "scope", "canonical", false).unwrap(),
        vec!["c"]
    );
}

#[test]
fn canonical_candidate_budget_excludes_non_callable_snapshots_before_limiting() {
    let connection = fixture();
    for kind in ["constant", "variable", "field", "module"] {
        for number in 0..MAX_CANONICAL_SYMBOL_CANDIDATES + 1 {
            insert(
                &connection,
                &format!("noise-{kind}-{number:04}"),
                kind,
                "target = 1",
                "python",
                None,
            );
        }
    }
    for include_declarations in [false, true] {
        assert!(
            snapshots(&connection, "scope", "canonical", include_declarations)
                .unwrap()
                .is_empty()
        );
    }
    for kind in CALLABLE_TARGET_SYMBOL_KINDS {
        insert(&connection, "target", kind, "target() {}", "python", None);
        for include_declarations in [false, true] {
            assert_eq!(
                snapshots(&connection, "scope", "canonical", include_declarations).unwrap(),
                vec!["target"]
            );
        }
        connection
            .execute(
                "DELETE FROM code_repository_symbols WHERE symbol_snapshot_id = 'target'",
                [],
            )
            .unwrap();
    }
    insert(
        &connection,
        "first",
        "function",
        "def target(): pass",
        "python",
        None,
    );
    insert(
        &connection,
        "second",
        "function",
        "def target(): pass",
        "python",
        None,
    );
    assert!(
        snapshots(&connection, "scope", "canonical", false)
            .unwrap_err()
            .to_string()
            .contains("multiple definitions")
    );
}
