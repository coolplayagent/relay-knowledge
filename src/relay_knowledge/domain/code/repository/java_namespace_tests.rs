use super::*;
use crate::domain::{CodeFeatureFlagMetadata, RepositoryCodeFileRecord};

#[test]
fn legacy_metadata_and_file_records_do_not_invent_java_namespace_proof() {
    let metadata: CodeFeatureFlagMetadata = serde_json::from_str("{}").unwrap();
    assert!(metadata.java_implicit_platform.is_none());
    let file: RepositoryCodeFileRecord = serde_json::from_value(serde_json::json!({
        "repository_id": "repo", "source_scope": "scope", "file_id": "file",
        "path": "App.java", "language_id": "java", "blob_hash": "hash",
        "byte_len": 12, "line_count": 1, "parse_status": "parsed"
    }))
    .unwrap();
    assert!(file.java_namespace.is_none());
}

#[test]
fn explicit_namespace_and_read_proofs_round_trip_without_losing_completeness() {
    let evidence = JavaNamespaceEvidence {
        package: "p".to_owned(),
        top_level_types: vec!["System".to_owned()],
        complete: false,
    };
    assert_eq!(
        serde_json::from_str::<JavaNamespaceEvidence>(&serde_json::to_string(&evidence).unwrap())
            .unwrap(),
        evidence
    );
    let metadata = CodeFeatureFlagMetadata {
        java_implicit_platform: Some(JavaImplicitPlatformRead {
            type_name: "System".to_owned(),
        }),
        ..Default::default()
    };
    assert_eq!(
        serde_json::from_str::<CodeFeatureFlagMetadata>(&serde_json::to_string(&metadata).unwrap())
            .unwrap(),
        metadata
    );
}

#[test]
fn projection_name_bytes_accept_exact_limit_and_reject_one_extra_byte() {
    let mut evidence = JavaNamespaceEvidence {
        package: "p".repeat(32_767),
        top_level_types: vec!["AB".to_owned()],
        complete: true,
    };
    assert_eq!(evidence.projected_name_bytes(), Some(65_536));
    evidence.top_level_types[0].push('C');
    assert_eq!(evidence.projected_name_bytes(), None);
    evidence.complete = false;
    assert_eq!(evidence.projected_name_bytes(), Some(32_767));
}
