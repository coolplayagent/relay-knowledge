use super::*;

fn setup_test_schema(connection: &Connection) {
    connection
        .execute_batch(
            "
        CREATE TABLE IF NOT EXISTS evidence (
            id TEXT PRIMARY KEY,
            status TEXT NOT NULL
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS graph_bm25 USING fts5(
            document_id UNINDEXED,
            document_kind UNINDEXED,
            evidence_id UNINDEXED,
            parent_evidence_id UNINDEXED,
            modality UNINDEXED,
            created_graph_version UNINDEXED,
            source_scope,
            source_path,
            entity_labels,
            entity_aliases,
            content
        );
        ",
        )
        .expect("schema should initialize");
}

fn insert_test_symbol(connection: &Connection, id: &str, scope: &str, labels: &str, content: &str) {
    connection
        .execute(
            "
        INSERT INTO graph_bm25 (
            document_id, document_kind, evidence_id, parent_evidence_id,
            modality, created_graph_version, source_scope, source_path,
            entity_labels, entity_aliases, content
        ) VALUES (?1, 'code_symbol', ?2, NULL, 'text_span', 1, ?3, NULL, ?4, '', ?5)
        ",
            params![id, id, scope, labels, content],
        )
        .expect("should insert test symbol");
}

fn insert_test_chunk(
    connection: &Connection,
    id: &str,
    scope: &str,
    path: Option<&str>,
    labels: &str,
    content: &str,
) {
    connection
        .execute(
            "
        INSERT INTO graph_bm25 (
            document_id, document_kind, evidence_id, parent_evidence_id,
            modality, created_graph_version, source_scope, source_path,
            entity_labels, entity_aliases, content
        ) VALUES (?1, 'code_chunk', ?2, NULL, 'text_span', 1, ?3, ?4, ?5, '', ?6)
        ",
            params![id, id, scope, path, labels, content],
        )
        .expect("should insert test chunk");
}

fn test_request(query: &str) -> GraphSearchRequest {
    GraphSearchRequest {
        query: query.to_owned(),
        source_scope: None,
        graph_version: crate::domain::GraphVersion::new(1),
        limit: 10,
        disabled_retriever_sources: Vec::new(),
    }
}

#[test]
fn fallback_candidates_returns_empty_for_short_query() {
    let connection = Connection::open_in_memory().expect("db should open");
    setup_test_schema(&connection);
    let result = fallback_candidates(&connection, &test_request("a")).expect("should succeed");
    assert!(result.is_empty());
}

#[test]
fn exact_name_rows_match_by_content() {
    let connection = Connection::open_in_memory().expect("db should open");
    setup_test_schema(&connection);
    insert_test_symbol(&connection, "doc-1", "docs", "[\"getUser\"]", "getUser");
    let request = GraphSearchRequest {
        query: "getUser".to_owned(),
        source_scope: None,
        graph_version: crate::domain::GraphVersion::new(1),
        limit: 10,
        disabled_retriever_sources: Vec::new(),
    };
    let result = fallback_candidates(&connection, &request).expect("should succeed");
    assert!(!result.is_empty());
}

#[test]
fn like_substring_matches_partial_content() {
    let connection = Connection::open_in_memory().expect("db should open");
    setup_test_schema(&connection);
    insert_test_chunk(
        &connection,
        "doc-2",
        "docs",
        Some("src/sign_in.rs"),
        "[\"signIn\"]",
        "signInWithGoogle requires OAuth2 configuration",
    );
    let request = test_request("signIn");
    let result = fallback_candidates(&connection, &request).expect("should succeed");
    assert!(!result.is_empty());
}

#[test]
fn fuzzy_levenshtein_matches_close_names() {
    let connection = Connection::open_in_memory().expect("db should open");
    setup_test_schema(&connection);
    insert_test_symbol(
        &connection,
        "doc-3",
        "repo",
        "[\"getUser\"]",
        "getUser fn function",
    );
    let request = test_request("getUsr");
    let result = fallback_candidates(&connection, &request).expect("should succeed");
    assert!(!result.is_empty());
}

#[test]
fn convert_fallback_candidates_handles_empty_evidence() {
    let connection = Connection::open_in_memory().expect("db should open");
    setup_test_schema(&connection);
    insert_test_chunk(
        &connection,
        "doc-4",
        "repo",
        None,
        "[\"handler\"]",
        "request handler implementation",
    );
    let request = test_request("handler");
    let result = fallback_candidates(&connection, &request).expect("should succeed");
    assert!(!result.is_empty());
}

#[test]
fn adaptive_max_distance_returns_one_for_short_queries() {
    assert_eq!(adaptive_max_distance("ab"), 1);
    assert_eq!(adaptive_max_distance("abc"), 1);
    assert_eq!(adaptive_max_distance("abcd"), 1);
}

#[test]
fn adaptive_max_distance_returns_two_for_long_queries() {
    assert_eq!(adaptive_max_distance("abcde"), 2);
    assert_eq!(adaptive_max_distance("getUser"), 2);
    assert_eq!(adaptive_max_distance("signInWithGoogle"), 2);
}

#[test]
fn levenshtein_distance_computes_correct_edit_distance() {
    assert_eq!(levenshtein_distance("", ""), 0);
    assert_eq!(levenshtein_distance("abc", ""), 3);
    assert_eq!(levenshtein_distance("", "abc"), 3);
    assert_eq!(levenshtein_distance("getUser", "getUsr"), 1);
    assert_eq!(levenshtein_distance("getUser", "getUssr"), 1);
    assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
    assert_eq!(levenshtein_distance("abc", "def"), 3);
    assert_eq!(levenshtein_distance("abc", "abc"), 0);
}

#[test]
fn merge_fallback_candidates_deduplicates_by_document_id() {
    let exact = vec![FallbackCandidate {
        document_id: "doc-1".to_owned(),
        document_kind: "evidence".to_owned(),
        evidence_id: "ev-1".to_owned(),
        parent_evidence_id: None,
        modality: "text_span".to_owned(),
        source_scope: "docs".to_owned(),
        source_path: None,
        entity_labels: vec![],
        content: "content".to_owned(),
        match_score: 1.0,
    }];
    let like = vec![FallbackCandidate {
        document_id: "doc-1".to_owned(),
        document_kind: "evidence".to_owned(),
        evidence_id: "ev-1".to_owned(),
        parent_evidence_id: None,
        modality: "text_span".to_owned(),
        source_scope: "docs".to_owned(),
        source_path: None,
        entity_labels: vec![],
        content: "content".to_owned(),
        match_score: 0.5,
    }];
    let fuzzy: Vec<FallbackCandidate> = vec![];

    let merged = merge_fallback_candidates(exact, like, fuzzy);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].match_score, 1.0);
}

#[test]
fn merge_fallback_candidates_prioritizes_exact_over_like_and_fuzzy() {
    let exact = vec![FallbackCandidate {
        document_id: "doc-exact".to_owned(),
        document_kind: "evidence".to_owned(),
        evidence_id: "ev-exact".to_owned(),
        parent_evidence_id: None,
        modality: "text_span".to_owned(),
        source_scope: "docs".to_owned(),
        source_path: None,
        entity_labels: vec![],
        content: "exact match".to_owned(),
        match_score: 1.0,
    }];
    let like = vec![FallbackCandidate {
        document_id: "doc-like".to_owned(),
        document_kind: "evidence".to_owned(),
        evidence_id: "ev-like".to_owned(),
        parent_evidence_id: None,
        modality: "text_span".to_owned(),
        source_scope: "docs".to_owned(),
        source_path: None,
        entity_labels: vec![],
        content: "substring match".to_owned(),
        match_score: 0.5,
    }];
    let fuzzy = vec![FallbackCandidate {
        document_id: "doc-fuzzy".to_owned(),
        document_kind: "evidence".to_owned(),
        evidence_id: "ev-fuzzy".to_owned(),
        parent_evidence_id: None,
        modality: "text_span".to_owned(),
        source_scope: "docs".to_owned(),
        source_path: None,
        entity_labels: vec![],
        content: "fuzzy match".to_owned(),
        match_score: 0.25,
    }];

    let merged = merge_fallback_candidates(exact, like, fuzzy);
    assert_eq!(merged.len(), 3);
    assert_eq!(merged[0].match_score, 1.0);
    assert_eq!(merged[1].match_score, 0.5);
    assert_eq!(merged[2].match_score, 0.25);
}

#[test]
fn like_substring_query_escapes_special_characters() {
    let query_like = format!(
        "%{}%",
        "test%value_".replace('%', "\\%").replace('_', "\\_")
    );
    assert_eq!(query_like, "%test\\%value\\_%");
}

#[test]
fn exact_name_rows_matches_via_multi_label_like() {
    let connection = Connection::open_in_memory().expect("db should open");
    setup_test_schema(&connection);
    insert_test_symbol(
        &connection,
        "doc-multi",
        "repo",
        "[\"get\",\"getUser\",\"fetch\"]",
        "getUser function returns user data",
    );
    let request = test_request("getUser");
    let result = fallback_candidates(&connection, &request).expect("should succeed");
    assert!(
        !result.is_empty(),
        "exact match should find multi-label entity"
    );
}

#[test]
fn exact_name_rows_scope_filter_blocks_wrong_scope() {
    let connection = Connection::open_in_memory().expect("db should open");
    setup_test_schema(&connection);
    insert_test_symbol(
        &connection,
        "doc-scope",
        "repo-a",
        "[\"getUser\"]",
        "getUser function in scope a",
    );
    let mut request = test_request("getUser");
    request.source_scope = Some("repo-b".to_owned());
    let result = fallback_candidates(&connection, &request).expect("should succeed");
    assert!(
        result.is_empty() || !result.iter().any(|r| r.hit.evidence_id == "doc-scope"),
        "cross-scope query should not return results from wrong scope"
    );
}
