use super::*;
use crate::domain::JavaNamespaceEvidence;

#[test]
fn insertion_cost_counts_repeated_owner_strings_and_unknown_rows() {
    let mut file = RepositoryCodeFileRecord {
        repository_id: "repo".into(),
        source_scope: "scope".into(),
        file_id: "file".into(),
        path: "App.java".into(),
        language_id: "java".into(),
        blob_hash: "blob".into(),
        byte_len: 1,
        line_count: 1,
        parse_status: CodeParseStatus::Parsed,
        is_generated: false,
        degraded_reason: None,
        java_namespace: None,
    };
    let common = 5 + 8 + 128;
    assert_eq!(file.namespace_projection_cost(), (1, common + 7));
    file.java_namespace = Some(JavaNamespaceEvidence {
        source_set: crate::domain::JavaSourceSet::Repository,
        package: "demo".into(),
        top_level_types: vec!["One".into(), "Two".into()],
        complete: true,
    });
    assert_eq!(
        file.namespace_projection_cost(),
        (3, 3 * (common + 4 + 10) + 6)
    );
    file.java_namespace.as_mut().unwrap().complete = false;
    assert_eq!(file.namespace_projection_cost(), (1, common + 10));
    file.language_id = "rust".into();
    assert_eq!(file.namespace_projection_cost(), (0, 0));
}

fn legacy_symbol() -> serde_json::Value {
    serde_json::json!({
        "repository_id": "repo", "source_scope": "scope", "symbol_snapshot_id": "symbol",
        "canonical_symbol_id": "repo://repo/helper", "file_id": "file", "path": "helper.c",
        "language_id": "c", "name": "helper", "qualified_name": "helper", "kind": "function",
        "signature": "int helper(int value)",
        "byte_range": {"start": 0, "end": 25}, "line_range": {"start": 1, "end": 1}
    })
}

#[test]
fn legacy_symbols_keep_unknown_callable_proof_without_changing_display_signature() {
    let mut value = legacy_symbol();
    for explicit_null in [false, true] {
        if explicit_null {
            value["callable_signature_key"] = serde_json::Value::Null;
        }
        let symbol: RepositoryCodeSymbolRecord = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(symbol.callable_signature_key, None);
        assert_eq!(symbol.signature, "int helper(int value)");
        assert!(
            serde_json::to_value(symbol)
                .unwrap()
                .get("callable_signature_key")
                .is_none()
        );
    }
}

#[test]
fn callable_proofs_round_trip_at_utf8_byte_boundary_and_reject_oversize() {
    let mut value = legacy_symbol();
    let key = "é".repeat(MAX_CALLABLE_SIGNATURE_KEY_BYTES / 2);
    assert_eq!(key.len(), MAX_CALLABLE_SIGNATURE_KEY_BYTES);
    value["callable_signature_key"] = key.clone().into();
    let symbol: RepositoryCodeSymbolRecord = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(symbol.callable_signature_key.as_deref(), Some(key.as_str()));
    assert_eq!(
        serde_json::to_value(symbol).unwrap()["callable_signature_key"],
        key
    );
    value["callable_signature_key"] = format!("{key}x").into();
    let error = serde_json::from_value::<RepositoryCodeSymbolRecord>(value).unwrap_err();
    assert!(error.to_string().contains("exceeds byte budget"));
}
