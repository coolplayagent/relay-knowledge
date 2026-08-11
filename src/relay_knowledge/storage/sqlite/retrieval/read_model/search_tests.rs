use super::{
    bm25_source_is_temporarily_unavailable, fts_query, search_graph, validate_search_request,
};
use crate::{
    domain::GraphVersion,
    storage::{
        GraphSearchRequest, MAX_GRAPH_SEARCH_FTS_CODEPOINTS, MAX_GRAPH_SEARCH_FTS_TOKENS,
        MAX_GRAPH_SEARCH_LIMIT, MAX_GRAPH_SEARCH_QUERY_CHARS, MAX_GRAPH_SEARCH_TOKEN_BYTES,
        StorageError,
    },
};

#[test]
fn fts_query_keeps_identifier_tokens_and_rejects_empty_input() {
    assert_eq!(
        fts_query("GraphVersion/source_scope"),
        Some("\"GraphVersion\" OR \"source\" OR \"scope\"".to_owned())
    );
    assert_eq!(fts_query(" / "), None);
    assert_eq!(fts_query("向量检索"), Some("\"向量检索\"".to_owned()));
}

#[test]
fn search_request_bounds_query_work_before_fts_parsing() {
    let request = |query: String, limit| GraphSearchRequest {
        query,
        source_scope: None,
        graph_version: GraphVersion::new(1),
        limit,
        disabled_retriever_sources: Vec::new(),
    };

    assert!(
        validate_search_request(&request("bounded".to_owned(), MAX_GRAPH_SEARCH_LIMIT)).is_ok()
    );
    assert!(
        validate_search_request(&request("bounded".to_owned(), MAX_GRAPH_SEARCH_LIMIT + 1))
            .is_err()
    );
    assert!(
        validate_search_request(&request("x ".repeat(MAX_GRAPH_SEARCH_QUERY_CHARS + 1), 1))
            .is_err()
    );
    let too_many_terms = (0..=MAX_GRAPH_SEARCH_FTS_TOKENS)
        .map(|index| format!("term{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(validate_search_request(&request(too_many_terms, 1)).is_err());
    assert!(
        validate_search_request(&request("x".repeat(MAX_GRAPH_SEARCH_TOKEN_BYTES + 1), 1)).is_err()
    );
    let underscore_separated_terms = (0..=MAX_GRAPH_SEARCH_FTS_TOKENS)
        .map(|index| format!("term{index}"))
        .collect::<Vec<_>>()
        .join("_");
    assert!(validate_search_request(&request(underscore_separated_terms, 1)).is_err());
    let unicode61_separator_stress = (0..MAX_GRAPH_SEARCH_FTS_TOKENS)
        .map(|_| "a\u{0345}a\u{0345}a\u{0345}a\u{0345}a")
        .collect::<Vec<_>>()
        .join(" ");
    assert!(unicode61_separator_stress.chars().count() > MAX_GRAPH_SEARCH_FTS_CODEPOINTS);
    assert!(validate_search_request(&request(unicode61_separator_stress, 1)).is_err());
}

#[test]
fn hybrid_search_only_degrades_transient_bm25_failures() {
    let sqlite_error = |code| {
        StorageError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(code),
            None,
        ))
    };

    assert!(bm25_source_is_temporarily_unavailable(&sqlite_error(
        rusqlite::ffi::SQLITE_BUSY
    )));
    assert!(!bm25_source_is_temporarily_unavailable(&sqlite_error(
        rusqlite::ffi::SQLITE_INTERRUPT
    )));
    assert!(!bm25_source_is_temporarily_unavailable(&sqlite_error(
        rusqlite::ffi::SQLITE_CORRUPT
    )));
}

#[test]
fn bm25_hierarchy_suite_pauses_live_companion_indexes_during_rebuild() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let mut connection = store.connection.lock().expect("connection should lock");
    connection
        .execute(
            "UPDATE graph_bm25_route_state SET state = 'building' WHERE id = 1",
            [],
        )
        .expect("rebuild state should install");

    let outcome = search_graph(
        &mut connection,
        GraphSearchRequest {
            query: "no-match".to_owned(),
            source_scope: None,
            graph_version: GraphVersion::new(0),
            limit: 10,
            disabled_retriever_sources: Vec::new(),
        },
    )
    .expect("search should remain available during rebuild");

    assert!(outcome.hits.iter().all(|hit| {
        !hit.retriever_sources
            .contains(&crate::domain::RetrieverSource::Semantic)
            && !hit
                .retriever_sources
                .contains(&crate::domain::RetrieverSource::Vector)
    }));
    assert!(
        outcome
            .trace
            .degraded_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("retrievers paused"))
    );
}

#[test]
fn disabled_retriever_sources_are_not_executed_or_reported_as_degraded() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let mut connection = store.connection.lock().expect("connection should lock");
    connection
        .execute(
            "UPDATE graph_bm25_route_state SET state = 'building' WHERE id = 1",
            [],
        )
        .expect("rebuild state should install");

    let outcome = search_graph(
        &mut connection,
        GraphSearchRequest {
            query: "disabled".to_owned(),
            source_scope: None,
            graph_version: GraphVersion::new(0),
            limit: 10,
            disabled_retriever_sources: vec![
                crate::domain::RetrieverSource::Bm25,
                crate::domain::RetrieverSource::GraphEvidence,
                crate::domain::RetrieverSource::CodeGraph,
                crate::domain::RetrieverSource::Semantic,
                crate::domain::RetrieverSource::Vector,
                crate::domain::RetrieverSource::GraphPath,
                crate::domain::RetrieverSource::Temporal,
                crate::domain::RetrieverSource::CommunitySummary,
            ],
        },
    )
    .expect("fully disabled search should remain available");

    assert!(outcome.hits.is_empty());
    assert!(outcome.trace.degraded_reason.is_none());
}
