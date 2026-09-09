use crate::domain::{
    CodeFeatureFlagMetadata, CodeFeatureFlagRecord, CodeFeatureFlagRequest, CodeRepositorySelector,
    CodeRepositoryStatus, FreshnessPolicy, RepositoryCodeRange,
};

pub(in crate::storage::sqlite::code) fn record(
    id: &str,
    key: &str,
    kind: &str,
) -> CodeFeatureFlagRecord {
    CodeFeatureFlagRecord {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        feature_flag_id: id.to_owned(),
        usage_id: id.to_owned(),
        file_id: id.to_owned(),
        path: format!("src/{id}.java"),
        language_id: "java".to_owned(),
        name: key.to_owned(),
        source_kind: kind.to_owned(),
        source_key: key.to_owned(),
        edge_kind: "reads_config".to_owned(),
        confidence_basis_points: 9000,
        confidence_tier: "extracted".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 5 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        excerpt: key.to_owned(),
        metadata: CodeFeatureFlagMetadata {
            java_implicit_platform: None,
            source_format: "java".to_owned(),
            ..Default::default()
        },
    }
}

pub(super) fn status() -> CodeRepositoryStatus {
    CodeRepositoryStatus {
        repository_id: "repo".to_owned(),
        alias: "repo".to_owned(),
        root_path: "/repo".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        last_indexed_scope_id: Some("scope".to_owned()),
        last_indexed_commit: Some("commit".to_owned()),
        tree_hash: Some("tree".to_owned()),
        state: "indexed".to_owned(),
        indexed_file_count: 1,
        symbol_count: 0,
        reference_count: 0,
        chunk_count: 0,
        stale: false,
        degraded_reason: None,
    }
}

pub(super) fn request() -> CodeFeatureFlagRequest {
    CodeFeatureFlagRequest::new(
        None,
        CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new()).unwrap(),
        100,
        FreshnessPolicy::AllowStale,
    )
    .unwrap()
}
