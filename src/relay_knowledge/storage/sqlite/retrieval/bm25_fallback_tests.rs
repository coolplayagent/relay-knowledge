use super::*;

fn setup_test_schema(connection: &Connection) {
    connection
        .execute_batch(
            "
        CREATE TABLE IF NOT EXISTS evidence (
            id TEXT PRIMARY KEY,
            source_scope TEXT NOT NULL DEFAULT '',
            source_path TEXT,
            span_start_byte INTEGER,
            span_end_byte INTEGER,
            span_start_line INTEGER,
            span_end_line INTEGER,
            created_graph_version INTEGER NOT NULL DEFAULT 1,
            status TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS entities (
            id TEXT PRIMARY KEY,
            label TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS evidence_entities (
            evidence_id TEXT NOT NULL,
            entity_id TEXT NOT NULL,
            PRIMARY KEY (evidence_id, entity_id)
        );
        CREATE TABLE IF NOT EXISTS graph_fact_evidence (
            fact_kind TEXT NOT NULL,
            fact_id TEXT NOT NULL,
            evidence_id TEXT NOT NULL,
            PRIMARY KEY (fact_kind, fact_id, evidence_id)
        );
        CREATE TABLE IF NOT EXISTS graph_relations (
            id TEXT PRIMARY KEY,
            source_entity_id TEXT NOT NULL,
            relation_type TEXT NOT NULL,
            target_entity_id TEXT NOT NULL,
            evidence_ids_json TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            status TEXT NOT NULL,
            valid_from_graph_version INTEGER NOT NULL,
            valid_until_graph_version INTEGER,
            created_graph_version INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS graph_claims (
            id TEXT PRIMARY KEY,
            subject_entity_id TEXT NOT NULL,
            predicate TEXT NOT NULL,
            object TEXT NOT NULL,
            evidence_ids_json TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            status TEXT NOT NULL,
            valid_from_graph_version INTEGER NOT NULL,
            valid_until_graph_version INTEGER,
            created_graph_version INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS graph_events (
            id TEXT PRIMARY KEY,
            event_type TEXT NOT NULL,
            occurred_at TEXT,
            evidence_ids_json TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            status TEXT NOT NULL,
            valid_from_graph_version INTEGER NOT NULL,
            valid_until_graph_version INTEGER,
            created_graph_version INTEGER NOT NULL
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
    label_trigrams::initialize_schema(connection).expect("label gram schema should initialize");
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
    index_test_labels(connection, id, "code_symbol", scope, labels);
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
    index_test_labels(connection, id, "code_chunk", scope, labels);
}

fn insert_test_evidence(
    connection: &Connection,
    id: &str,
    scope: &str,
    labels: &str,
    content: &str,
) {
    connection
        .execute(
            "INSERT INTO evidence (id, status) VALUES (?1, 'accepted')",
            params![id],
        )
        .expect("should insert accepted evidence");
    connection
        .execute(
            "
        INSERT INTO graph_bm25 (
            document_id, document_kind, evidence_id, parent_evidence_id,
            modality, created_graph_version, source_scope, source_path,
            entity_labels, entity_aliases, content
        ) VALUES (?1, 'evidence', ?1, NULL, 'text_span', 1, ?2, NULL, ?3, '', ?4)
        ",
            params![id, scope, labels, content],
        )
        .expect("should insert test evidence");
    index_test_labels(connection, id, "evidence", scope, labels);
}

fn index_test_labels(
    connection: &Connection,
    document_id: &str,
    document_kind: &str,
    source_scope: &str,
    labels: &str,
) {
    let labels = split_labels(labels.to_owned());
    label_trigrams::replace_document(
        connection,
        label_trigrams::LabelGramDocument {
            document_id,
            document_kind,
            source_scope,
            graph_version: 1,
            labels: &labels,
        },
    )
    .expect("label grams should index");
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
fn fuzzy_levenshtein_matches_evidence_labels() {
    let connection = Connection::open_in_memory().expect("db should open");
    setup_test_schema(&connection);
    insert_test_evidence(
        &connection,
        "ev-fuzzy-label",
        "repo",
        "[\"getUser\"]",
        "profile retrieval behavior",
    );

    let result = fallback_candidates(&connection, &test_request("getUsr")).expect("should succeed");

    assert!(
        result
            .iter()
            .any(|hit| hit.hit.evidence_id == "ev-fuzzy-label"),
        "evidence labels should participate in fuzzy fallback"
    );
}

#[test]
fn fuzzy_levenshtein_matches_name_after_many_nonmatching_labels() {
    let connection = Connection::open_in_memory().expect("db should open");
    setup_test_schema(&connection);
    for index in 0..(FALLBACK_CANDIDATE_LIMIT * 5 + 20) {
        let label = format!("aaaNoiseSymbol{index:04}");
        let labels_json =
            serde_json::to_string(&vec![label.clone()]).expect("labels should encode");
        insert_test_symbol(
            &connection,
            &format!("doc-noise-{index:04}"),
            "repo",
            &labels_json,
            &format!("{label} fn function"),
        );
    }
    insert_test_symbol(
        &connection,
        "doc-fuzzy-tail",
        "repo",
        "[\"zzTailSymbol\"]",
        "zzTailSymbol fn function",
    );

    let request = test_request("zzTailSymbl");
    let result = fallback_candidates(&connection, &request).expect("should succeed");

    assert!(
        result
            .iter()
            .any(|hit| hit.hit.evidence_id == "doc-fuzzy-tail"),
        "fuzzy fallback should rank by edit distance before applying matched-name caps"
    );
}

#[test]
fn fuzzy_levenshtein_orders_closest_match_before_document_id() {
    let connection = Connection::open_in_memory().expect("db should open");
    setup_test_schema(&connection);
    insert_test_symbol(
        &connection,
        "doc-a-less-close",
        "repo",
        "[\"getUxx\"]",
        "getUxx fn function",
    );
    insert_test_symbol(
        &connection,
        "doc-z-closest",
        "repo",
        "[\"getUser\"]",
        "getUser fn function",
    );

    let result = fallback_candidates(&connection, &test_request("getUsr")).expect("should succeed");

    assert_eq!(result[0].hit.evidence_id, "doc-z-closest");
    assert!(
        result[0].source_score > result[1].source_score,
        "lower edit distance should get a higher fuzzy score"
    );
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
fn sort_fallback_candidates_orders_only_materialized_rows() {
    let mut candidates = vec![
        fallback_candidate("doc-c"),
        fallback_candidate("doc-a"),
        fallback_candidate("doc-b"),
    ];

    sort_fallback_candidates(&mut candidates);

    assert_eq!(
        candidates
            .into_iter()
            .map(|candidate| candidate.document_id)
            .collect::<Vec<_>>(),
        ["doc-a", "doc-b", "doc-c"]
    );
}

#[test]
fn sort_fuzzy_candidates_prefers_best_distance_score() {
    let mut candidates = vec![
        fallback_candidate_with_score("doc-a", 0.25),
        fallback_candidate_with_score("doc-z", 0.26),
    ];

    sort_fuzzy_candidates(&mut candidates);

    assert_eq!(candidates[0].document_id, "doc-z");
    assert!(candidates[0].match_score > candidates[1].match_score);
}

#[test]
fn fuzzy_label_candidates_rank_by_overlap_before_candidate_cap() {
    let connection = Connection::open_in_memory().expect("db should open");
    setup_test_schema(&connection);
    for index in 0..(FUZZY_LABEL_CANDIDATE_LIMIT + 20) {
        let label = format!("alpha{}gamma", four_character_noise(index));
        let labels_json =
            serde_json::to_string(&vec![label.clone()]).expect("labels should encode");
        insert_test_symbol(
            &connection,
            &format!("doc-noise-overlap-{index:04}"),
            "repo",
            &labels_json,
            &format!("{label} fn function"),
        );
    }
    insert_test_symbol(
        &connection,
        "doc-ranked-target",
        "repo",
        "[\"alphaBetaGamma\"]",
        "alphaBetaGamma fn function",
    );

    let result =
        fallback_candidates(&connection, &test_request("alphaBetoGamma")).expect("should succeed");

    assert!(
        result
            .iter()
            .any(|hit| hit.hit.evidence_id == "doc-ranked-target"),
        "trigram overlap should keep the closest label before applying the candidate cap"
    );
}

fn four_character_noise(index: usize) -> String {
    const ALPHABET: &[u8] = b"cdefghijklmnopqrstuvwxyz0123456789";
    (0..4)
        .map(|offset| {
            let divisor = ALPHABET.len().pow(offset as u32);
            char::from(ALPHABET[(index / divisor) % ALPHABET.len()])
        })
        .collect()
}

fn fallback_candidate(document_id: &str) -> FallbackCandidate {
    FallbackCandidate {
        document_id: document_id.to_owned(),
        document_kind: "evidence".to_owned(),
        evidence_id: document_id.to_owned(),
        parent_evidence_id: None,
        modality: "text_span".to_owned(),
        source_scope: "docs".to_owned(),
        source_path: None,
        entity_labels: vec![],
        content: "content".to_owned(),
        match_score: 1.0,
    }
}

fn fallback_candidate_with_score(document_id: &str, match_score: f64) -> FallbackCandidate {
    let mut candidate = fallback_candidate(document_id);
    candidate.match_score = match_score;
    candidate
}

#[test]
fn like_substring_query_escapes_special_characters() {
    assert_eq!(contains_like_pattern(r"path\name%_"), r"%path\\name\%\_%");
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
fn exact_name_rows_matches_json_escaped_labels() {
    let connection = Connection::open_in_memory().expect("db should open");
    setup_test_schema(&connection);
    let label = r#"get\user"name"#;
    let labels_json = serde_json::to_string(&vec![label.to_owned()]).expect("labels should encode");
    insert_test_symbol(
        &connection,
        "doc-json-escaped",
        "repo",
        &labels_json,
        "symbol metadata without literal label",
    );
    let request = test_request(label);
    let result = fallback_candidates(&connection, &request).expect("should succeed");
    assert!(
        result
            .iter()
            .any(|hit| hit.hit.evidence_id == "doc-json-escaped"),
        "exact match should use JSON-safe LIKE pattern"
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

#[test]
fn matching_fuzzy_names_orders_by_distance_and_caps_sql_terms() {
    let mut names = (0..(FUZZY_MATCHED_NAME_LIMIT + 20))
        .map(near_query_name)
        .collect::<Vec<_>>();
    names.push("aaaaa".to_owned());

    let matches = matching_fuzzy_names(names, "aaaaa", FUZZY_LONG_QUERY_MAX_DISTANCE);

    assert_eq!(matches.len(), FUZZY_MATCHED_NAME_LIMIT);
    assert_eq!(matches[0].name, "aaaaa");
    assert_eq!(matches[0].distance, 0);
    assert!(matches.windows(2).all(|window| {
        let left = &window[0];
        let right = &window[1];
        (left.distance, left.name.as_str()) <= (right.distance, right.name.as_str())
    }));
}

fn near_query_name(index: usize) -> String {
    const ALPHABET: &[u8] = b"bcdefghijklmnopqrstuvwxyz0123456789";
    let first = char::from(ALPHABET[index % ALPHABET.len()]);
    let second = char::from(ALPHABET[(index / ALPHABET.len()) % ALPHABET.len()]);
    format!("{first}{second}aaa")
}
