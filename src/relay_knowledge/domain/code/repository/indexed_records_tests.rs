use super::{MAX_CALLABLE_SIGNATURE_KEY_BYTES, RepositoryCodeSymbolRecord};

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
