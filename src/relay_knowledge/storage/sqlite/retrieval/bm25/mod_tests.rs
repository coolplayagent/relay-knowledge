//! Direct BM25 query, routing, and retry-classification invariants.

use std::collections::BTreeSet;

use rusqlite::{Connection, params};

use super::super::{
    bm25_routing::{Bm25RoutingPlan, Bm25RoutingText, plan_query, prepare_document},
    read_model::{Bm25WriteTarget, RetrievalWriteContext, insert_code_chunk_document},
};
use super::{
    BM25_SQL, RawBm25Row, bm25_candidate_rows, distinct_candidate_count,
    graph_bm25_query_error_is_retryable, graph_bm25_query_error_message_is_retryable,
    planned_match_query,
};
use crate::{
    domain::GraphVersion,
    storage::{GraphSearchRequest, StorageError},
};

const PRODUCTION_RECALL_SCOPE: &str = "bm25-production-recall";
const PRODUCTION_RECALL_GROUPS: usize = 64;
const PRODUCTION_RECALL_DOCUMENTS_PER_GROUP: usize = 64;

struct ProductionRecallCluster {
    seed: usize,
    source_path: String,
    group_token: String,
    needle_frequency: usize,
}

fn production_recall_content(seed: usize, needle_frequency: usize, padding: usize) -> String {
    let topics = [
        format!("topic{seed}alpha"),
        format!("topic{seed}bravo"),
        format!("topic{seed}charlie"),
        format!("topic{seed}delta"),
    ];
    let mut terms = Vec::with_capacity(topics.len() * 8 + needle_frequency + padding);
    for topic in &topics {
        for _ in 0..8 {
            terms.push(topic.as_str());
        }
    }
    terms.extend(std::iter::repeat_n("needle", needle_frequency));
    terms.extend(std::iter::repeat_n("padding", padding));
    terms.join(" ")
}

fn production_recall_clusters() -> Vec<ProductionRecallCluster> {
    let mut clusters = Vec::with_capacity(PRODUCTION_RECALL_GROUPS);
    let mut group_tokens = BTreeSet::new();
    let mut seed = 0_usize;
    while clusters.len() < PRODUCTION_RECALL_GROUPS {
        assert!(
            seed < 4_096,
            "production routing must yield {PRODUCTION_RECALL_GROUPS} bounded synthetic groups"
        );
        let cluster_index = clusters.len();
        let needle_frequency = match cluster_index {
            0..=6 => 4,
            7..=11 => 1,
            _ => 0,
        };
        let source_path = format!("src/synthetic/topic{seed}.rs");
        let content = production_recall_content(seed, needle_frequency, 8);
        let route = prepare_document(Bm25RoutingText {
            source_scope: PRODUCTION_RECALL_SCOPE,
            source_path: Some(&source_path),
            entity_labels: "",
            entity_aliases: "",
            content: &content,
            graph_version: 1,
        });
        if group_tokens.insert(route.group_token.clone()) {
            clusters.push(ProductionRecallCluster {
                seed,
                source_path,
                group_token: route.group_token,
                needle_frequency,
            });
        }
        seed += 1;
    }
    clusters
}

fn planned_match_document_count(
    connection: &Connection,
    request: &GraphSearchRequest,
    planned_match: &str,
) -> usize {
    connection
        .query_row(
            "SELECT COUNT(*)
             FROM graph_bm25
             WHERE graph_bm25 MATCH ?1
               AND (?2 IS NULL OR graph_bm25.source_scope = ?2)
               AND graph_bm25.created_graph_version <= ?3",
            params![
                planned_match,
                request.source_scope.as_deref(),
                request.graph_version.get()
            ],
            |row| row.get(0),
        )
        .expect("planned MATCH candidate-domain count should load")
}

#[test]
fn query_retry_is_limited_to_transient_query_errors() {
    assert!(graph_bm25_query_error_message_is_retryable(
        "vtable constructor failed: graph_bm25"
    ));
    assert!(graph_bm25_query_error_message_is_retryable(
        "database table is locked: graph_bm25"
    ));
    assert!(!graph_bm25_query_error_message_is_retryable(
        "no such table: graph_bm25"
    ));
    assert!(!graph_bm25_query_error_is_retryable(
        &StorageError::InvalidInput("database is locked".to_owned())
    ));
}

#[test]
fn bm25_hierarchy_suite_preserves_global_score_in_single_fts_intersection() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (id TEXT PRIMARY KEY, status TEXT NOT NULL);")
        .expect("evidence table should exist");
    crate::storage::sqlite::retrieval::read_model::initialize_schema(&connection)
        .expect("retrieval schema should initialize");
    let scope_token = super::super::bm25_routing::scope_token("scope");
    let other_scope_token = super::super::bm25_routing::scope_token("other-scope");
    let first_routing_key = format!("{scope_token} rkg001");
    let second_routing_key = format!("{scope_token} rkg002");
    let other_routing_key = format!("{other_scope_token} rkg001");
    connection
        .execute(
            "INSERT INTO graph_bm25 (
                document_id, document_kind, evidence_id, parent_evidence_id, modality,
                created_graph_version, routing_key, source_scope, source_path, entity_labels,
                entity_aliases, content
             ) VALUES ('doc-a', 'code_chunk', 'a', NULL, 'text_span', 1,
                       ?1, 'scope', 'a.rs', '[]', '', 'alpha lexical ranking')",
            params![first_routing_key],
        )
        .expect("first document should insert");
    connection
        .execute(
            "INSERT INTO graph_bm25 (
                document_id, document_kind, evidence_id, parent_evidence_id, modality,
                created_graph_version, routing_key, source_scope, source_path, entity_labels,
                entity_aliases, content
             ) VALUES ('doc-b', 'code_chunk', 'b', NULL, 'text_span', 1,
                       ?1, 'scope', 'b.rs', '[]', '', 'alpha unrelated filler filler')",
            params![second_routing_key],
        )
        .expect("second document should insert");
    connection
        .execute(
            "INSERT INTO graph_bm25 (
                document_id, document_kind, evidence_id, parent_evidence_id, modality,
                created_graph_version, routing_key, source_scope, source_path, entity_labels,
                entity_aliases, content
             ) VALUES ('doc-c', 'code_chunk', 'c', NULL, 'text_span', 1,
                       ?1, 'other-scope', 'c.rs', '[]', '', 'alpha cross scope')",
            params![other_routing_key],
        )
        .expect("cross-scope document should insert");

    let business_match =
        "{source_scope source_path entity_labels entity_aliases content} : (\"alpha\")";
    let scoped_match = format!("({business_match}) AND (routing_key : {scope_token})");
    let routed_match = format!("({scoped_match}) AND (routing_key : rkg001)");
    let mut baseline_statement = connection
        .prepare(BM25_SQL)
        .expect("business-only query should prepare");
    let baseline_rows = baseline_statement
        .query_map(
            params![business_match, Some("scope"), 1_u64, 10_usize],
            |row| Ok((row.get::<_, String>(1)?, row.get::<_, f64>(5)?)),
        )
        .expect("business-only query should run")
        .collect::<Result<Vec<_>, _>>()
        .expect("business-only rows should load");
    let mut flat_statement = connection
        .prepare(BM25_SQL)
        .expect("flat query should prepare");
    let flat_rows = flat_statement
        .query_map(
            params![scoped_match, Some("scope"), 1_u64, 10_usize],
            |row| Ok((row.get::<_, String>(1)?, row.get::<_, f64>(5)?)),
        )
        .expect("flat query should run")
        .collect::<Result<Vec<_>, _>>()
        .expect("flat rows should load");
    let mut routed_statement = connection
        .prepare(BM25_SQL)
        .expect("routed query should prepare");
    let routed_rows = routed_statement
        .query_map(
            params![routed_match, Some("scope"), 1_u64, 10_usize],
            |row| Ok((row.get::<_, String>(1)?, row.get::<_, f64>(5)?)),
        )
        .expect("routed query should run")
        .collect::<Result<Vec<_>, _>>()
        .expect("routed rows should load");
    assert_eq!(flat_rows.len(), 2);
    assert_eq!(routed_rows.len(), 1);
    assert_eq!(routed_rows[0].0, "doc-a");
    let flat_rank = flat_rows
        .iter()
        .find(|(document_id, _)| document_id == "doc-a")
        .expect("flat result should contain routed document")
        .1;
    let baseline_rank = baseline_rows
        .iter()
        .find(|(document_id, _)| document_id == "doc-a")
        .expect("business-only result should contain routed document")
        .1;
    let routed_rank = routed_rows[0].1;
    assert_eq!(baseline_rank.to_bits(), flat_rank.to_bits());
    assert_eq!(flat_rank.to_bits(), routed_rank.to_bits());

    let route_like_business_rows = connection
        .prepare(BM25_SQL)
        .expect("route-like business query should prepare")
        .query_map(
            params![
                "{source_scope source_path entity_labels entity_aliases content} : (rkg001)",
                Some("scope"),
                1_u64,
                10_usize
            ],
            |row| row.get::<_, String>(1),
        )
        .expect("route-like business query should run")
        .collect::<Result<Vec<_>, _>>()
        .expect("route-like business rows should load");
    assert!(route_like_business_rows.is_empty());

    let explain_sql = format!("EXPLAIN QUERY PLAN {BM25_SQL}");
    let mut statement = connection
        .prepare(&explain_sql)
        .expect("query plan should prepare");
    let plan = statement
        .query_map(
            params![routed_match, Some("scope"), 1_u64, 10_usize],
            |row| row.get::<_, String>(3),
        )
        .expect("query plan should run")
        .collect::<Result<Vec<_>, _>>()
        .expect("query plan rows should load")
        .join("\n");
    assert_eq!(
        plan.matches("graph_bm25 VIRTUAL TABLE").count(),
        1,
        "{plan}"
    );
    assert!(
        plan.contains("rM12"),
        "rank-ordered FTS MATCH missing: {plan}"
    );
    assert!(
        !plan.contains("USE TEMP B-TREE"),
        "rank window must not sort the full posting list: {plan}"
    );
}

#[test]
fn bm25_hierarchy_suite_production_routes_preserve_recall_and_reduce_candidate_domain() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (id TEXT PRIMARY KEY, status TEXT NOT NULL);")
        .expect("evidence table should exist");
    crate::storage::sqlite::retrieval::read_model::initialize_schema(&connection)
        .expect("retrieval schema should initialize");
    connection
        .execute_batch(
            "CREATE TABLE graph_state (id INTEGER PRIMARY KEY, graph_version INTEGER NOT NULL);
             INSERT INTO graph_state VALUES (1, 1);",
        )
        .expect("graph version should initialize");

    let clusters = production_recall_clusters();
    let write = RetrievalWriteContext {
        graph_version: 1,
        bm25_target: Bm25WriteTarget::Live,
        refresh_labels: false,
        refresh_semantic: false,
        refresh_vector: false,
    };
    let transaction = connection
        .unchecked_transaction()
        .expect("production recall fixture transaction should begin");
    for (cluster_index, cluster) in clusters.iter().enumerate() {
        for document_index in 0..PRODUCTION_RECALL_DOCUMENTS_PER_GROUP {
            let global_index =
                cluster_index * PRODUCTION_RECALL_DOCUMENTS_PER_GROUP + document_index;
            let padding = if cluster.needle_frequency == 0 {
                8
            } else {
                8 + global_index
            };
            let content =
                production_recall_content(cluster.seed, cluster.needle_frequency, padding);
            insert_code_chunk_document(
                &transaction,
                PRODUCTION_RECALL_SCOPE,
                &cluster.source_path,
                &format!("chunk-{cluster_index:02}-{document_index:02}"),
                &[],
                &content,
                write,
            )
            .expect("production code-chunk persistence should maintain every BM25 layer");
        }
    }
    transaction
        .commit()
        .expect("production recall fixture should commit atomically");

    let route_document_count = connection
        .query_row(
            "SELECT COUNT(*) FROM graph_bm25_route_documents",
            [],
            |row| row.get::<_, usize>(0),
        )
        .expect("route-document count should load");
    let (group_count, minimum_group_size, maximum_group_size) = connection
        .query_row(
            "SELECT COUNT(*), MIN(document_count), MAX(document_count)
             FROM graph_bm25_route_groups",
            [],
            |row| {
                Ok((
                    row.get::<_, usize>(0)?,
                    row.get::<_, usize>(1)?,
                    row.get::<_, usize>(2)?,
                ))
            },
        )
        .expect("production route-group distribution should load");
    let persisted_group_tokens = connection
        .prepare("SELECT group_token FROM graph_bm25_route_groups ORDER BY group_token")
        .expect("production group-token query should prepare")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("production group-token query should run")
        .collect::<Result<BTreeSet<_>, _>>()
        .expect("production group tokens should load");
    let needle_document_frequency = connection
        .query_row(
            "SELECT document_frequency
             FROM graph_bm25_route_term_totals
             WHERE term = 'needle'",
            [],
            |row| row.get::<_, usize>(0),
        )
        .expect("production route-term frequency should load");
    assert_eq!(
        route_document_count,
        PRODUCTION_RECALL_GROUPS * PRODUCTION_RECALL_DOCUMENTS_PER_GROUP
    );
    assert_eq!(
        (group_count, minimum_group_size, maximum_group_size),
        (PRODUCTION_RECALL_GROUPS, 64, 64)
    );
    assert_eq!(needle_document_frequency, 12 * 64);
    let prepared_group_tokens = clusters
        .iter()
        .map(|cluster| cluster.group_token.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(persisted_group_tokens, prepared_group_tokens);

    let request = GraphSearchRequest {
        query: "needle".to_owned(),
        source_scope: Some(PRODUCTION_RECALL_SCOPE.to_owned()),
        graph_version: GraphVersion::new(1),
        limit: 10,
        disabled_retriever_sources: Vec::new(),
    };
    let routed_plan = plan_query(&connection, &request).expect("production route should plan");
    assert!(routed_plan.route_match.is_some());
    assert!(
        routed_plan
            .explanation
            .as_deref()
            .is_some_and(|explanation| {
                explanation.contains("selected_groups=7/12")
                    && explanation.contains("selected_documents=448/4096")
                    && !explanation.contains("fallback=")
            }),
        "production fixture must exercise the admitted hierarchical plan: {routed_plan:?}"
    );
    let flat_plan = Bm25RoutingPlan::flat("production_recall_oracle");
    let routed_match = planned_match_query(&request, "\"needle\"", &routed_plan);
    let flat_match = planned_match_query(&request, "\"needle\"", &flat_plan);
    let routed_match_rows = planned_match_document_count(&connection, &request, &routed_match);
    let flat_match_rows = planned_match_document_count(&connection, &request, &flat_match);
    assert_eq!(routed_match_rows, 7 * 64);
    assert_eq!(flat_match_rows, 12 * 64);
    assert!(
        routed_match_rows < flat_match_rows,
        "routed MATCH must reduce the deterministic candidate domain"
    );
    eprintln!(
        "BM25_WORK population={} routed_match_rows={routed_match_rows} flat_match_rows={flat_match_rows}",
        route_document_count
    );

    let flat_boundary_ranks = connection
        .prepare(BM25_SQL)
        .expect("flat boundary query should prepare")
        .query_map(
            params![
                flat_match.as_str(),
                request.source_scope.as_deref(),
                request.graph_version.get(),
                request.limit + 1
            ],
            |row| row.get::<_, f64>(5),
        )
        .expect("flat boundary query should run")
        .collect::<Result<Vec<_>, _>>()
        .expect("flat boundary ranks should load");
    assert_eq!(flat_boundary_ranks.len(), request.limit + 1);
    assert_ne!(
        flat_boundary_ranks[request.limit - 1].to_bits(),
        flat_boundary_ranks[request.limit].to_bits(),
        "Recall oracle must not depend on an equal-rank Top10 cutoff"
    );

    let routed = bm25_candidate_rows(&connection, &request, "\"needle\"")
        .expect("routed production candidates should load");
    assert_eq!(routed.len(), request.limit);
    assert!(routed.iter().all(|row| {
        row.explanation.as_deref().is_some_and(|explanation| {
            explanation.contains("hierarchical_bm25 algorithm=")
                && !explanation.contains("fallback=")
        })
    }));

    connection
        .execute(
            "UPDATE graph_bm25_route_state SET indexed_graph_version = 0 WHERE id = 1",
            [],
        )
        .expect("flat oracle should disable only routing eligibility");
    let flat = bm25_candidate_rows(&connection, &request, "\"needle\"")
        .expect("flat production candidates should load");
    let flat_top_k = flat
        .iter()
        .map(|row| row.document_id.as_str())
        .collect::<BTreeSet<_>>();
    let recalled = routed
        .iter()
        .filter(|row| flat_top_k.contains(row.document_id.as_str()))
        .count();
    assert!(
        recalled * 10 >= request.limit * 9,
        "production-path routed Recall@{} was {recalled}/{}",
        request.limit,
        request.limit
    );
}

#[test]
fn bm25_hierarchy_suite_partitions_scoped_common_terms_and_keeps_sql_authority() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (id TEXT PRIMARY KEY, status TEXT NOT NULL);")
        .expect("evidence table should exist");
    crate::storage::sqlite::retrieval::read_model::initialize_schema(&connection)
        .expect("retrieval schema should initialize");
    connection
        .execute_batch(
            "CREATE TABLE graph_state (id INTEGER PRIMARY KEY, graph_version INTEGER NOT NULL);
             INSERT INTO graph_state VALUES (1, 1);",
        )
        .expect("graph version should initialize");

    let allowed_scope = "tenant-allowed";
    let allowed_scope_token = super::super::bm25_routing::scope_token(allowed_scope);
    let allowed_routing_key = format!("{allowed_scope_token} rkgallowed");
    connection
        .execute(
            "INSERT INTO graph_bm25 (
                 document_id, document_kind, evidence_id, parent_evidence_id, modality,
                 created_graph_version, routing_key, source_scope, source_path,
                 entity_labels, entity_aliases, content
             ) VALUES ('allowed', 'code_chunk', 'allowed', NULL, 'text_span', 1,
                       ?1, ?2, 'src/allowed.rs', '[]', '', 'commonterm authorized')",
            params![allowed_routing_key, allowed_scope],
        )
        .expect("authorized document should insert");
    for index in 0..128 {
        let denied_scope = format!("tenant-denied-{index:03}");
        let denied_token = super::super::bm25_routing::scope_token(&denied_scope);
        let routing_key = format!("{denied_token} rkgdenied{index:03}");
        connection
            .execute(
                "INSERT INTO graph_bm25 (
                     document_id, document_kind, evidence_id, parent_evidence_id, modality,
                     created_graph_version, routing_key, source_scope, source_path,
                     entity_labels, entity_aliases, content
                 ) VALUES (?1, 'code_chunk', ?1, NULL, 'text_span', 1,
                           ?2, ?3, 'src/denied.rs', '[]', '',
                           'commonterm commonterm commonterm')",
                params![format!("denied-{index:03}"), routing_key, denied_scope],
            )
            .expect("cross-scope document should insert");
    }
    connection
        .execute(
            "INSERT INTO graph_bm25 (
                 document_id, document_kind, evidence_id, parent_evidence_id, modality,
                 created_graph_version, routing_key, source_scope, source_path,
                 entity_labels, entity_aliases, content
             ) VALUES ('spoofed-token', 'code_chunk', 'spoofed-token', NULL, 'text_span', 1,
                       ?1, 'tenant-denied-spoof', 'src/spoof.rs', '[]', '',
                       'commonterm commonterm commonterm commonterm')",
            params![format!("{allowed_scope_token} rkgspoof")],
        )
        .expect("scope-token collision fixture should insert");

    let request = GraphSearchRequest {
        query: "commonterm".to_owned(),
        source_scope: Some(allowed_scope.to_owned()),
        graph_version: GraphVersion::new(1),
        limit: 5,
        disabled_retriever_sources: Vec::new(),
    };
    let flat_plan = super::super::bm25_routing::Bm25RoutingPlan::flat("test");
    let planned_match = planned_match_query(&request, "\"commonterm\"", &flat_plan);
    assert!(planned_match.contains(&format!("routing_key : {allowed_scope_token}")));
    let scoped_postings = connection
        .query_row(
            "SELECT COUNT(*) FROM graph_bm25 WHERE graph_bm25 MATCH ?1",
            params![planned_match.as_str()],
            |row| row.get::<_, usize>(0),
        )
        .expect("scoped posting count should load");
    assert_eq!(
        scoped_postings, 2,
        "scope routing should prune unrelated scopes"
    );
    let explain_sql = format!("EXPLAIN QUERY PLAN {BM25_SQL}");
    let mut query_plan_statement = connection
        .prepare(&explain_sql)
        .expect("scoped query plan should prepare");
    let query_plan = query_plan_statement
        .query_map(
            params![planned_match.as_str(), Some(allowed_scope), 1_u64, 5_usize],
            |row| row.get::<_, String>(3),
        )
        .expect("scoped query plan should run")
        .collect::<Result<Vec<_>, _>>()
        .expect("scoped query plan should load")
        .join("\n");
    assert_eq!(
        query_plan.matches("graph_bm25 VIRTUAL TABLE").count(),
        1,
        "{query_plan}"
    );
    assert!(query_plan.contains("rM12"), "{query_plan}");

    let rows = bm25_candidate_rows(&connection, &request, "\"commonterm\"")
        .expect("scoped flat BM25 should load");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].document_id, "allowed");
    assert_eq!(rows[0].source_scope, allowed_scope);

    let unscoped_request = GraphSearchRequest {
        source_scope: None,
        ..request
    };
    let unscoped_match = planned_match_query(&unscoped_request, "\"commonterm\"", &flat_plan);
    assert_eq!(
        unscoped_match,
        "{source_scope source_path entity_labels entity_aliases content} : (\"commonterm\")"
    );
    let unscoped_postings = connection
        .query_row(
            "SELECT COUNT(*) FROM graph_bm25 WHERE graph_bm25 MATCH ?1",
            params![unscoped_match],
            |row| row.get::<_, usize>(0),
        )
        .expect("unscoped posting count should load");
    assert_eq!(unscoped_postings, 130);
}

#[test]
fn bm25_hierarchy_suite_counts_collapsed_evidence_as_one_candidate() {
    let rows = (0..8)
        .map(|index| RawBm25Row {
            document_id: format!("child-{index}"),
            document_kind: "evidence".to_owned(),
            evidence_id: format!("child-evidence-{index}"),
            parent_evidence_id: Some("parent".to_owned()),
            modality: "text_span".to_owned(),
            source_scope: "scope".to_owned(),
            source_path: None,
            entity_labels: Vec::new(),
            content: "child".to_owned(),
            rank: -1.0,
            explanation: None,
        })
        .collect::<Vec<_>>();

    assert_eq!(distinct_candidate_count(&rows), 1);
}

#[test]
fn bm25_hierarchy_suite_expands_flat_window_after_parent_collapse() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (id TEXT PRIMARY KEY, status TEXT NOT NULL);")
        .expect("evidence table should exist");
    crate::storage::sqlite::retrieval::read_model::initialize_schema(&connection)
        .expect("retrieval schema should initialize");
    connection
        .execute_batch(
            "CREATE TABLE graph_state (id INTEGER PRIMARY KEY, graph_version INTEGER NOT NULL);
             INSERT INTO graph_state VALUES (1, 1);",
        )
        .expect("graph version should initialize");
    let routing_scope_token = super::super::bm25_routing::scope_token("scope");
    let shared_routing_key = format!("{routing_scope_token} rkg001");
    let unique_routing_key = format!("{routing_scope_token} rkg002");
    for index in 0..40 {
        let evidence_id = format!("child-{index:03}");
        connection
            .execute(
                "INSERT INTO evidence (id, status) VALUES (?1, 'accepted')",
                params![evidence_id],
            )
            .expect("child evidence should insert");
        connection
            .execute(
                "INSERT INTO graph_bm25 (
                     document_id, document_kind, evidence_id, parent_evidence_id, modality,
                     created_graph_version, routing_key, source_scope, source_path,
                     entity_labels, entity_aliases, content
                 ) VALUES (?1, 'evidence', ?1, 'shared-parent', 'text_span', 1,
                           ?2, 'scope', NULL, '[]', '', 'needle')",
                params![evidence_id, shared_routing_key],
            )
            .expect("child bm25 row should insert");
    }
    for index in 0..10 {
        let evidence_id = format!("unique-{index:03}");
        connection
            .execute(
                "INSERT INTO evidence (id, status) VALUES (?1, 'accepted')",
                params![evidence_id],
            )
            .expect("unique evidence should insert");
        connection
            .execute(
                "INSERT INTO graph_bm25 (
                     document_id, document_kind, evidence_id, parent_evidence_id, modality,
                     created_graph_version, routing_key, source_scope, source_path,
                     entity_labels, entity_aliases, content
                 ) VALUES (?1, 'evidence', ?1, NULL, 'text_span', 1,
                           ?2, 'scope', NULL, '[]', '', 'needle')",
                params![evidence_id, unique_routing_key],
            )
            .expect("unique bm25 row should insert");
    }
    let request = GraphSearchRequest {
        query: "needle".to_owned(),
        source_scope: Some("scope".to_owned()),
        graph_version: GraphVersion::new(1),
        limit: 10,
        disabled_retriever_sources: Vec::new(),
    };

    let rows = bm25_candidate_rows(&connection, &request, "\"needle\"")
        .expect("flat window should expand");

    assert_eq!(distinct_candidate_count(&rows), request.limit);
    assert_eq!(rows.len(), request.limit);
}
