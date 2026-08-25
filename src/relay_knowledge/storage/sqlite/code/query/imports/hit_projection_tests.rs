//! Direct hit-projection contract for excerpts and resolution scoring.

use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn grouped_go_import_excerpts_include_source_like_siblings() {
    let excerpt = import_excerpt(
        "go",
        "ctxalias context",
        None,
        &[
            ". strings".to_owned(),
            "_ embed".to_owned(),
            "ctxalias context".to_owned(),
        ],
    );

    assert!(excerpt.contains("ctxalias \"context\""), "{excerpt}");
    assert!(excerpt.contains("import ctxalias \"context\""), "{excerpt}");
    assert!(excerpt.contains(". \"strings\""), "{excerpt}");
    assert!(excerpt.contains("_ \"embed\""), "{excerpt}");
}

#[test]
fn import_excerpts_keep_target_symbol_context() {
    let excerpt = import_excerpt(
        "cpp",
        "#include \"leveldb/filter_policy.h\"",
        Some("FilterPolicy"),
        &[],
    );

    assert!(excerpt.contains("leveldb/filter_policy.h"));
    assert!(excerpt.contains("FilterPolicy"));
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

#[test]
fn contextual_import_queries_keep_importer_identity_terms_for_scoring() {
    let request = CodeRetrievalRequest::new(
        "controller clientset example.org/runtime/client",
        CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        CodeQueryKind::Imports,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    assert_eq!(import_scoring_query(&request), request.query);
}

#[test]
fn grouped_import_lookup_uses_row_value_instead_of_or_expansion() {
    assert_eq!(import_group_key_rows(3), "(?, ?, ?), (?, ?, ?), (?, ?, ?)");
}

#[test]
fn grouped_import_lookup_executes_values_row_set_and_returns_siblings() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_imports (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                module TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL
            );
            INSERT INTO code_repository_imports VALUES
                ('scope', 'src/main.go', 'context', 3, 7),
                ('scope', 'src/main.go', 'strings', 3, 7),
                ('scope', 'src/other.go', 'io', 4, 4);
            ",
        )
        .expect("grouped-import fixture should initialize");
    let rows = vec![ImportRow {
        file_id: "file-1".to_owned(),
        path: "src/main.go".to_owned(),
        language_id: "go".to_owned(),
        module: "context".to_owned(),
        matched_symbol_name: None,
        target_symbol_names: None,
        same_file_query_usage_count: 0,
        line_range: RepositoryCodeRange { start: 3, end: 7 },
        target_hint: None,
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 5_000,
        confidence_tier: "inferred".to_owned(),
        is_generated: false,
        source_line_count: 10,
    }];
    let status = CodeRepositoryStatus {
        repository_id: "repo".to_owned(),
        alias: "repo".to_owned(),
        root_path: "/repo".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        last_indexed_scope_id: Some("scope".to_owned()),
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

    let groups = import_group_modules(&connection, &status, &rows)
        .expect("SQLite should execute the bounded VALUES row set");

    let modules = groups
        .get(&ImportGroupKey {
            path: "src/main.go".to_owned(),
            line_start: 3,
            line_end: 7,
        })
        .expect("the requested import group should exist");
    assert_eq!(modules, &["context".to_owned(), "strings".to_owned()]);
}
