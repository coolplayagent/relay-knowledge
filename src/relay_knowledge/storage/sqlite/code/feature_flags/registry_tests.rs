use super::*;
use crate::domain::{
    CodeConfigFilter, CodeConfigMetadata, CodeRepositorySelector, FreshnessPolicy,
};
fn fixture() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("CREATE TABLE code_repository_feature_flags (feature_flag_id TEXT, usage_id TEXT, file_id TEXT, path TEXT, language_id TEXT, name TEXT, source_kind TEXT, source_key TEXT, edge_kind TEXT, confidence_basis_points INTEGER, confidence_tier TEXT, byte_start INTEGER, byte_end INTEGER, line_start INTEGER, line_end INTEGER, excerpt TEXT, metadata_json TEXT, source_scope TEXT);
    CREATE INDEX scope_flags ON code_repository_feature_flags(source_scope,feature_flag_id);
    CREATE TABLE code_repository_symbols(source_scope TEXT,path TEXT,line_start INTEGER,line_end INTEGER,symbol_snapshot_id TEXT,name TEXT);").unwrap();
    connection
}
fn status() -> CodeRepositoryStatus {
    CodeRepositoryStatus {
        repository_id: "repo".into(),
        alias: "fixture".into(),
        root_path: "/tmp/repo".into(),
        path_filters: vec![],
        language_filters: vec![],
        last_indexed_scope_id: Some("scope".into()),
        last_indexed_commit: Some("commit".into()),
        tree_hash: Some("tree".into()),
        state: "indexed".into(),
        indexed_file_count: 1,
        symbol_count: 0,
        reference_count: 0,
        chunk_count: 0,
        stale: false,
        degraded_reason: None,
    }
}
fn add(connection: &Connection, key: &str, kind: &str, edge: &str, metadata: CodeConfigMetadata) {
    let id = connection.last_insert_rowid() + 1;
    connection.execute("INSERT INTO code_repository_feature_flags VALUES (?1,?2,'file','src/config','java',?3,?4,?3,?5,9000,'extracted',0,1,1,1,?3,?6,'scope')",params![format!("{kind}:{key}"),id.to_string(),key,kind,edge,serde_json::to_string(&metadata).unwrap()]).unwrap();
}
fn request(query: Option<&str>, filters: CodeConfigFilter) -> CodeFeatureFlagRequest {
    CodeFeatureFlagRequest::new(
        query.map(str::to_owned),
        CodeRepositorySelector::new("fixture", "HEAD", vec![], vec![]).unwrap(),
        1,
        FreshnessPolicy::AllowStale,
    )
    .unwrap()
    .with_filters(filters)
    .unwrap()
}
#[test]
fn group_filters_keep_resolved_constant_reads_and_definition_metadata() {
    let db = fixture();
    add(
        &db,
        "feature_x",
        "config_key",
        "defines_config",
        CodeConfigMetadata {
            source_format: "properties".into(),
            domain: Some("business".into()),
            hot_reload: Some(true),
            default_value: Some("true".into()),
            ..Default::default()
        },
    );
    add(
        &db,
        "feature_x",
        "config_key",
        "declares_config_key",
        CodeConfigMetadata {
            source_format: "java".into(),
            bindings: vec!["Keys.X".into()],
            ..Default::default()
        },
    );
    add(
        &db,
        "Keys.X",
        "config_symbol",
        "reads_config",
        CodeConfigMetadata {
            source_format: "java".into(),
            reference: Some("Keys.X".into()),
            target_kind: Some("config_key".into()),
            ..Default::default()
        },
    );
    let result = search(
        &db,
        &status(),
        &request(
            Some("feature_x"),
            CodeConfigFilter {
                domain: Some("business".into()),
                source: Some("properties".into()),
                hot_reload: Some(true),
                consistency: false,
            },
        ),
    )
    .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].usages.len(), 3);
    assert_eq!(result[0].source_key, "feature_x");
    assert!(result[0].analysis_complete);
}
#[test]
fn ordinary_getters_cannot_displace_known_configuration_from_seed_limit() {
    let db = fixture();
    for index in 0..150 {
        let key = format!("Ordinary{index}.getName");
        add(
            &db,
            &key,
            "config_symbol",
            "guards_code",
            CodeConfigMetadata {
                reference: Some(key.clone()),
                ..Default::default()
            },
        );
    }
    add(
        &db,
        "real_flag",
        "config_key",
        "defines_config",
        CodeConfigMetadata::default(),
    );
    let result = search(&db, &status(), &request(None, CodeConfigFilter::default())).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].source_key, "real_flag");
}
#[test]
fn ambiguous_getter_targets_suppress_absence_diagnostics() {
    let db = fixture();
    for key in ["feature_a", "feature_b"] {
        add(
            &db,
            key,
            "config_key",
            "reads_config",
            CodeConfigMetadata {
                source_format: "java".into(),
                bindings: vec!["Config.getX".into()],
                ..Default::default()
            },
        );
    }
    add(
        &db,
        "Config.getX",
        "config_symbol",
        "reads_config",
        CodeConfigMetadata {
            source_format: "java".into(),
            reference: Some("Config.getX".into()),
            ..Default::default()
        },
    );
    add(
        &db,
        "other",
        "config_key",
        "defines_config",
        CodeConfigMetadata {
            source_format: "properties".into(),
            ..Default::default()
        },
    );
    let result = search(
        &db,
        &status(),
        &request(
            Some("feature_a"),
            CodeConfigFilter {
                consistency: true,
                ..Default::default()
            },
        ),
    )
    .unwrap();
    assert_eq!(result.len(), 1);
    assert!(!result[0].analysis_complete);
    assert!(
        !result[0]
            .consistency_diagnostics
            .iter()
            .any(|d| d.starts_with("missing_from_format"))
    );
}
#[test]
fn stale_scope_cannot_claim_consistency_or_absence() {
    let db = fixture();
    add(
        &db,
        "flag",
        "config_key",
        "reads_config",
        CodeConfigMetadata::default(),
    );
    let mut status = status();
    status.stale = true;
    let result = search(
        &db,
        &status,
        &request(
            Some("flag"),
            CodeConfigFilter {
                consistency: true,
                ..Default::default()
            },
        ),
    )
    .unwrap();
    assert!(!result[0].analysis_complete);
    assert!(
        result[0]
            .consistency_diagnostics
            .iter()
            .any(|d| d.starts_with("incomplete_analysis"))
    );
}
#[test]
fn malformed_metadata_is_an_error_instead_of_an_empty_registry() {
    let db = fixture();
    add(
        &db,
        "flag",
        "config_key",
        "reads_config",
        CodeConfigMetadata::default(),
    );
    db.execute(
        "UPDATE code_repository_feature_flags SET metadata_json='{'",
        [],
    )
    .unwrap();
    assert!(
        search(
            &db,
            &status(),
            &request(Some("flag"), CodeConfigFilter::default())
        )
        .is_err()
    );
}
