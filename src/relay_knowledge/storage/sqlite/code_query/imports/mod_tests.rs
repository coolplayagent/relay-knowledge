use rusqlite::params;

use super::{
    import_excerpt, import_identifier_patterns, import_resolution_confidence_bonus,
    search_import_path_rows,
};
use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryRegistration, CodeRepositorySelector, CodeRepositoryStatus,
        CodeRetrievalRequest, FreshnessPolicy,
    },
    storage::{SqliteGraphStore, code::CodeRepositoryStore},
};

const TEST_SCOPE: &str = "code:test:import-direct-path:commit:tree";

#[test]
fn grouped_go_import_excerpts_include_source_like_siblings() {
    let excerpt = import_excerpt(
        "ctxalias context",
        None,
        &[
            ". strings".to_owned(),
            "_ embed".to_owned(),
            "ctxalias context".to_owned(),
        ],
    );

    assert!(excerpt.contains("ctxalias \"context\""), "{excerpt}");
    assert!(excerpt.contains(". \"strings\""), "{excerpt}");
    assert!(excerpt.contains("_ \"embed\""), "{excerpt}");
}

#[test]
fn import_excerpts_keep_target_symbol_context() {
    let excerpt = import_excerpt(
        "#include \"leveldb/filter_policy.h\"",
        Some("FilterPolicy"),
        &[],
    );

    assert!(excerpt.contains("leveldb/filter_policy.h"));
    assert!(excerpt.contains("FilterPolicy"));
}

#[test]
fn import_identifier_patterns_keep_alias_and_target_terms() {
    let patterns = import_identifier_patterns("disposeInstance as runDisposers instance registry");

    assert!(patterns.contains(&"%disposeinstance%".to_owned()));
    assert!(patterns.contains(&"%rundisposers%".to_owned()));
    assert!(patterns.contains(&"%registry%".to_owned()));
    assert!(!patterns.contains(&"%as%".to_owned()));
}

#[test]
fn import_identifier_patterns_drop_import_syntax_keywords() {
    let patterns = import_identifier_patterns("import serde using System include vector");

    assert!(patterns.contains(&"%serde%".to_owned()));
    assert!(patterns.contains(&"%system%".to_owned()));
    assert!(patterns.contains(&"%vector%".to_owned()));
    assert!(!patterns.contains(&"%import%".to_owned()));
    assert!(!patterns.contains(&"%using%".to_owned()));
    assert!(!patterns.contains(&"%include%".to_owned()));
}

#[test]
fn import_resolution_confidence_scores_resolved_edges_above_unresolved_edges() {
    assert!(
        import_resolution_confidence_bonus(2.0, "resolved", 8_000, CodeQueryKind::Imports) > 0.0
    );
    assert!(
        import_resolution_confidence_bonus(2.0, "unresolved", 2_500, CodeQueryKind::Imports) < 0.0
    );
    assert_eq!(
        import_resolution_confidence_bonus(2.0, "resolved", 8_000, CodeQueryKind::Hybrid),
        0.0
    );
}

#[tokio::test]
async fn import_path_direct_rows_apply_inline_path_before_gate() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    store
            .run(|connection| {
                connection.execute(
                    "
                    INSERT INTO code_repository_files (
                        repository_id, source_scope, file_id, path, language_id, blob_hash,
                        byte_len, line_count, parse_status, is_generated, degraded_reason
                    )
                    VALUES
                        ('repo', ?1, 'api-file', 'src/api.rs', 'rust', 'api-hash', 1, 1, 'parsed', 0, NULL),
                        ('repo', ?1, 'storage-file', 'src/storage/use.rs', 'rust', 'storage-hash', 1, 1, 'parsed', 0, NULL)
                    ",
                    params![TEST_SCOPE],
                )?;
                connection.execute(
                    "
                    INSERT INTO code_repository_imports (
                        repository_id, source_scope, import_id, file_id, path, module,
                        target_hint, resolution_state, confidence_basis_points,
                        confidence_tier, line_start, line_end
                    )
                    VALUES
                        ('repo', ?1, 'api-import', 'api-file', 'src/api.rs', 'shared/module', NULL, 'unresolved', 5000, 'inferred', 1, 1),
                        ('repo', ?1, 'storage-import', 'storage-file', 'src/storage/use.rs', 'shared/module', NULL, 'unresolved', 5000, 'inferred', 1, 1)
                    ",
                    params![TEST_SCOPE],
                )?;
                Ok(())
            })
            .await
            .expect("fixture rows should insert");
    let request = CodeRetrievalRequest::new(
        "path:storage shared/module",
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate"),
        CodeQueryKind::Imports,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let status = CodeRepositoryStatus {
        repository_id: "repo".to_owned(),
        alias: "repo".to_owned(),
        root_path: "/tmp/repo".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        last_indexed_scope_id: Some(TEST_SCOPE.to_owned()),
        last_indexed_commit: Some("commit".to_owned()),
        tree_hash: Some("tree".to_owned()),
        state: "fresh".to_owned(),
        indexed_file_count: 2,
        symbol_count: 0,
        reference_count: 0,
        chunk_count: 0,
        stale: false,
        degraded_reason: None,
    };

    let paths = store
        .run(move |connection| {
            let rows = search_import_path_rows(connection, &status, &request)?;
            Ok(rows
                .rows
                .into_iter()
                .map(|row| row.path)
                .collect::<Vec<_>>())
        })
        .await
        .expect("direct import lookup should run");

    assert_eq!(paths, ["src/storage/use.rs"]);
}
