use super::*;
use crate::domain::{
    CodeConfigFilter, CodeConfigMetadata, CodeRepositorySelector, FreshnessPolicy,
};
fn fixture() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("CREATE TABLE code_repository_feature_flags (feature_flag_id TEXT, usage_id TEXT, file_id TEXT, path TEXT, language_id TEXT, name TEXT, source_kind TEXT, source_key TEXT, edge_kind TEXT, confidence_basis_points INTEGER, confidence_tier TEXT, byte_start INTEGER, byte_end INTEGER, line_start INTEGER, line_end INTEGER, excerpt TEXT, metadata_json TEXT, source_scope TEXT);
    CREATE TABLE code_repository_files(source_scope TEXT,path TEXT,language_id TEXT);
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

#[test]
fn empty_template_inventory_drives_scoped_missing_format_diagnostics() {
    let db = fixture();
    db.execute_batch("INSERT INTO code_repository_files VALUES ('scope','cfg/empty.ctmpl','gotemplate'),('other','elsewhere.ini','ini');").unwrap();
    add(
        &db,
        "feature_y",
        "config_key",
        "declares_config_key",
        CodeConfigMetadata {
            source_format: "java".into(),
            ..Default::default()
        },
    );
    let query = request(
        Some("feature_y"),
        CodeConfigFilter {
            consistency: true,
            ..Default::default()
        },
    );
    let groups = search(&db, &status(), &query).unwrap();
    assert!(
        groups[0]
            .consistency_diagnostics
            .contains(&"missing_from_format: ctmpl".into())
    );
    assert!(
        !groups[0]
            .consistency_diagnostics
            .contains(&"missing_from_format: ini".into())
    );
    let mut restricted = query;
    restricted.repository.language_filters = vec!["java".into()];
    let groups = search(&db, &status(), &restricted).unwrap();
    assert!(
        groups[0]
            .consistency_diagnostics
            .contains(&"missing_from_format: ctmpl".into())
    );
    let mut authorized = status();
    authorized.language_filters = vec!["java".into()];
    let groups = search(&db, &authorized, &restricted).unwrap();
    assert!(
        !groups[0]
            .consistency_diagnostics
            .contains(&"missing_from_format: ctmpl".into())
    );
}
#[test]
fn ordinary_constants_require_visible_read_evidence_and_keep_key_lookup() {
    let db = fixture();
    add(
        &db,
        "application.name",
        "config_key",
        "declares_string_constant",
        CodeConfigMetadata {
            source_format: "java".into(),
            bindings: vec!["Messages.NAME".into()],
            ..Default::default()
        },
    );
    let query = request(Some("application.name"), CodeConfigFilter::default());
    assert!(search(&db, &status(), &query).unwrap().is_empty());
    add(
        &db,
        "Messages.NAME",
        "config_symbol",
        "reads_config",
        CodeConfigMetadata {
            source_format: "java".into(),
            reference: Some("Messages.NAME".into()),
            target_kind: Some("config_key".into()),
            ..Default::default()
        },
    );
    let groups = search(&db, &status(), &query).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].source_key, "application.name");
    assert!(
        groups[0]
            .usages
            .iter()
            .any(|u| u.edge_kind == "declares_config_key")
    );
    assert!(
        groups[0]
            .usages
            .iter()
            .any(|u| u.edge_kind == "reads_config")
    );
}
#[test]
fn conflicts_include_located_sources_and_unknown_flow_cannot_claim_completeness() {
    let db = fixture();
    for value in ["true", "false"] {
        add(
            &db,
            "feature_x",
            "config_key",
            "reads_config",
            CodeConfigMetadata {
                source_format: "java".into(),
                default_value: Some(value.into()),
                ..Default::default()
            },
        );
    }
    let query = request(
        Some("feature_x"),
        CodeConfigFilter {
            consistency: true,
            ..Default::default()
        },
    );
    let groups = search(&db, &status(), &query).unwrap();
    assert_eq!(groups[0].conflicting_default_sources.len(), 2);
    for source in &groups[0].conflicting_default_sources {
        assert!(!source.path.is_empty());
        assert!(
            groups[0]
                .usages
                .iter()
                .any(|u| u.usage_id == source.usage_id)
        );
    }
    add(
        &db,
        "feature_x",
        "config_key",
        "reads_config",
        CodeConfigMetadata {
            source_format: "java".into(),
            flow_incomplete: Some("unsupported_getter_value_flow".into()),
            ..Default::default()
        },
    );
    let groups = search(&db, &status(), &query).unwrap();
    assert!(!groups[0].analysis_complete);
    assert_eq!(groups[0].conflicting_default_sources.len(), 2);
    assert!(
        groups[0]
            .consistency_diagnostics
            .iter()
            .any(|d| d.starts_with("conflicting_defaults:"))
    );
    assert!(
        !groups[0]
            .consistency_diagnostics
            .iter()
            .any(|d| d.starts_with("missing_from_format:") || d == "read_without_definition")
    );
    assert!(
        groups[0]
            .consistency_diagnostics
            .iter()
            .any(|d| d.starts_with("incomplete_analysis"))
    );
}

#[path = "dispatch_tests.rs"]
mod dispatch_tests;
#[path = "review_tests.rs"]
mod review_tests;
