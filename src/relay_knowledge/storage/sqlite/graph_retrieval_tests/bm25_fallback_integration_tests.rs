use super::*;

#[tokio::test]
async fn bm25_fallback_like_matches_when_fts_finds_nothing() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("repo").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-like",
        scope,
        "signInWithGoogle requires OAuth2 tokens",
        vec!["signInWithGoogle".to_owned()],
    )
    .expect("evidence should validate");
    store
        .commit_mutation_batch(GraphMutationBatch::new(vec![evidence]).expect("batch"))
        .await
        .expect("commit should succeed");

    let hits = store
        .search(GraphSearchRequest {
            query: "sign".to_owned(),
            source_scope: Some("repo".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].evidence_id, "ev-like");
}

#[tokio::test]
async fn bm25_fallback_exact_name_match_always_included() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("repo").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-exact",
        scope,
        "getBean returns the configured bean instance",
        vec!["getBean".to_owned()],
    )
    .expect("evidence should validate");
    store
        .commit_mutation_batch(GraphMutationBatch::new(vec![evidence]).expect("batch"))
        .await
        .expect("commit should succeed");

    let hits = store
        .search(GraphSearchRequest {
            query: "getBean".to_owned(),
            source_scope: Some("repo".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");

    assert!(!hits.is_empty(), "exact name should always be found");
    assert_eq!(hits[0].evidence_id, "ev-exact");
}

#[tokio::test]
async fn bm25_fallback_like_matches_source_path_substring() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("repo").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-path",
        scope,
        "Configuration module handles app settings",
        vec!["ConfigHandler".to_owned()],
    )
    .expect("evidence should validate");
    store
        .commit_mutation_batch(GraphMutationBatch::new(vec![evidence]).expect("batch"))
        .await
        .expect("commit should succeed");

    let hits = store
        .search(GraphSearchRequest {
            query: "confighandler".to_owned(),
            source_scope: Some("repo".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");

    assert!(!hits.is_empty(), "LIKE fallback should match via content");
}

#[tokio::test]
async fn bm25_fallback_skips_two_char_query_for_fuzzy() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("repo").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-short-fuzzy",
        scope,
        "XY coordinates transform module",
        vec!["XYTransform".to_owned()],
    )
    .expect("evidence should validate");
    store
        .commit_mutation_batch(GraphMutationBatch::new(vec![evidence]).expect("batch"))
        .await
        .expect("commit should succeed");

    let hits = store
        .search(GraphSearchRequest {
            query: "xz".to_owned(),
            source_scope: Some("repo".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");

    assert!(
        !hits.iter().any(|h| h.evidence_id == "ev-short-fuzzy"),
        "two-char query should not trigger fuzzy Levenshtein with dist>1"
    );
}

#[tokio::test]
async fn bm25_fallback_fuzzy_matches_typo_queries() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("repo").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-typo",
        scope,
        "getUser fetches the current user profile",
        vec!["getUser".to_owned()],
    )
    .expect("evidence should validate");
    store
        .commit_mutation_batch(GraphMutationBatch::new(vec![evidence]).expect("batch"))
        .await
        .expect("commit should succeed");

    let hits = store
        .search(GraphSearchRequest {
            query: "getUsr".to_owned(),
            source_scope: Some("repo".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");

    assert!(!hits.is_empty(), "fuzzy fallback should match typo query");
    assert_eq!(hits[0].evidence_id, "ev-typo");
}

#[tokio::test]
async fn bm25_returns_empty_for_single_char_query() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("repo").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-short",
        scope,
        "Short query boundary test content",
        vec!["boundaryTest".to_owned()],
    )
    .expect("evidence should validate");
    store
        .commit_mutation_batch(GraphMutationBatch::new(vec![evidence]).expect("batch"))
        .await
        .expect("commit should succeed");

    let hits = store
        .search(GraphSearchRequest {
            query: "x".to_owned(),
            source_scope: Some("repo".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");

    assert!(hits.is_empty(), "single char should not trigger fallback");
}
