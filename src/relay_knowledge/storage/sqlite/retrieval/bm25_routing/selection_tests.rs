use super::*;
use crate::{domain::GraphVersion, storage::GraphSearchRequest};
use rusqlite::Connection;

fn balanced_groups() -> Vec<RoutingGroup> {
    (0..100)
        .map(|index| RoutingGroup {
            source_scope: "scope".to_owned(),
            token: format!("rkg{index:03}"),
            document_count: 50,
        })
        .collect()
}

#[test]
fn bm25_hierarchy_suite_does_not_swallow_cancellation_or_corruption() {
    let busy =
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY), None);
    let interrupted = rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_INTERRUPT),
        None,
    );
    let corrupt = rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
        None,
    );

    assert!(routing_state_is_temporarily_unavailable(&busy));
    assert!(!routing_state_is_temporarily_unavailable(&interrupted));
    assert!(!routing_state_is_temporarily_unavailable(&corrupt));
}

#[test]
fn bm25_hierarchy_suite_bounds_selected_document_fraction() {
    let groups = balanced_groups();
    let scores = (0..40)
        .map(|index| {
            (
                ("scope".to_owned(), format!("rkg{index:03}")),
                GroupScore {
                    score: f64::from((40 - index) * (40 - index)),
                    matched_terms: 1,
                },
            )
        })
        .collect();

    let plan = select_groups(5_000, &groups, scores).expect("balanced groups should route");
    let explanation = plan.explanation.expect("routing should be observable");
    assert!(explanation.contains("selected_groups=10/40"));
    assert!(explanation.contains("selected_documents=500/5000"));

    let broad_groups = (0..8)
        .map(|index| RoutingGroup {
            source_scope: "scope".to_owned(),
            token: format!("broad{index}"),
            document_count: 512,
        })
        .collect::<Vec<_>>();
    let broad_scores = (0..8)
        .map(|index| {
            (
                ("scope".to_owned(), format!("broad{index}")),
                GroupScore {
                    score: f64::from(8 - index),
                    matched_terms: 1,
                },
            )
        })
        .collect();
    assert!(population_is_routable(4_096, &broad_groups));
    assert_eq!(
        select_groups(4_096, &broad_groups, broad_scores)
            .expect_err("broad groups should exceed the candidate budget"),
        "candidate_budget"
    );
}

#[test]
fn bm25_hierarchy_suite_falls_back_for_skew_or_small_population() {
    let mut groups = balanced_groups();
    groups[0].document_count = 600;

    assert!(!population_is_routable(5_550, &groups));
    assert!(!population_is_routable(3_999, &balanced_groups()));
}

#[test]
fn bm25_hierarchy_suite_falls_back_when_coarse_scores_do_not_separate() {
    let groups = balanced_groups();
    let dispersed_singletons = (0..40)
        .map(|index| {
            (
                ("scope".to_owned(), format!("rkg{index:03}")),
                GroupScore {
                    score: 1.0,
                    matched_terms: 1,
                },
            )
        })
        .collect();

    assert_eq!(
        select_groups(5_000, &groups, dispersed_singletons)
            .expect_err("tied coarse scores should fall back"),
        "coarse_score_margin"
    );
}

#[test]
fn bm25_hierarchy_suite_skips_routes_that_cannot_reduce_candidates() {
    let groups = balanced_groups();
    let exact_scores = (0..4)
        .map(|index| {
            (
                ("scope".to_owned(), format!("rkg{index:03}")),
                GroupScore {
                    score: f64::from(4 - index),
                    matched_terms: 1,
                },
            )
        })
        .collect();

    assert_eq!(
        select_groups(5_000, &groups, exact_scores)
            .expect_err("routing every matching group cannot reduce candidates"),
        "no_candidate_reduction"
    );
}

#[test]
fn bm25_hierarchy_suite_enforces_the_five_percent_coarse_margin() {
    fn boundary_scores(
        cutoff_score: f64,
        cutoff_terms: usize,
    ) -> BTreeMap<(String, String), GroupScore> {
        (0..40)
            .map(|index| {
                let (score, matched_terms) = match index {
                    0..=8 => (200.0 - index as f64, 1),
                    9 => (cutoff_score, cutoff_terms),
                    10 => (100.0, 1),
                    _ => (90.0 - index as f64, 1),
                };
                (
                    ("scope".to_owned(), format!("rkg{index:03}")),
                    GroupScore {
                        score,
                        matched_terms,
                    },
                )
            })
            .collect()
    }

    let groups = balanced_groups();
    assert_eq!(
        select_groups(5_000, &groups, boundary_scores(104.9, 1))
            .expect_err("a sub-five-percent margin should fall back"),
        "coarse_score_margin"
    );
    assert!(select_groups(5_000, &groups, boundary_scores(105.0, 1)).is_ok());
    assert!(select_groups(5_000, &groups, boundary_scores(100.0, 2)).is_ok());
}

#[test]
fn bm25_hierarchy_suite_bounds_term_validation_postings() {
    assert_eq!(
        reserve_term_validation(0, MAX_TERM_VALIDATION_POSTINGS - 1),
        Some(MAX_TERM_VALIDATION_POSTINGS)
    );
    assert_eq!(
        reserve_term_validation(1, MAX_TERM_VALIDATION_POSTINGS - 1),
        None
    );
}

#[test]
fn bm25_hierarchy_suite_activates_only_for_complete_current_routes() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (id TEXT PRIMARY KEY, status TEXT NOT NULL);")
        .expect("evidence test schema should initialize");
    crate::storage::sqlite::retrieval::read_model::initialize_schema(&connection)
        .expect("retrieval schema should initialize");
    connection
        .execute_batch(
            "CREATE TABLE graph_state (id INTEGER PRIMARY KEY, graph_version INTEGER NOT NULL);
             INSERT INTO graph_state VALUES (1, 1);",
        )
        .expect("graph version should initialize");
    let routing_scope_token = super::super::scope_token("scope");
    connection
        .execute_batch(&format!(
            "
            WITH RECURSIVE sequence(value) AS (
                SELECT 1 UNION ALL SELECT value + 1 FROM sequence WHERE value < 4096
            )
            INSERT INTO graph_bm25 (
                rowid, document_id, document_kind, evidence_id, parent_evidence_id,
                modality, created_graph_version, routing_key, source_scope, source_path,
                entity_labels, entity_aliases, content
            )
            SELECT value, printf('doc-%04d', value), 'code_chunk', printf('chunk-%04d', value),
                   NULL, 'text_span', 1,
                   printf('{routing_scope_token} rkg%03d', value % 100),
                   'scope', printf('src/%04d.rs', value), '[]', '',
                   CASE
                       WHEN value % 100 < 10 THEN 'needle needle needle topic'
                       WHEN value <= 100 AND value % 100 BETWEEN 10 AND 39
                           THEN 'needle topic'
                       ELSE 'topic'
                   END
            FROM sequence;

            INSERT INTO graph_bm25_route_documents (
                document_id, fts_rowid, document_kind, created_graph_version,
                source_scope, source_path, label_gram_state, group_token,
                term_counts_json
            )
            SELECT document_id, rowid, document_kind, created_graph_version,
                   source_scope, source_path, 'indexed',
                   printf('rkg%03d', rowid % 100), '[]'
            FROM graph_bm25;

            INSERT INTO graph_bm25_route_groups (
                source_scope, group_token, document_count
            )
            SELECT 'scope', group_token, COUNT(*)
            FROM graph_bm25_route_documents
            GROUP BY group_token;

            INSERT INTO graph_bm25_route_terms (
                term, source_scope, group_token, collection_frequency
            )
            SELECT 'needle', 'scope', printf('rkg%03d', rowid % 100),
                   SUM(CASE
                       WHEN content LIKE '%needle needle needle%' THEN 3
                       ELSE 1
                   END)
            FROM graph_bm25
            WHERE content LIKE '%needle%'
            GROUP BY rowid % 100;

            INSERT INTO graph_bm25_route_term_totals (
                term, document_frequency
            )
            SELECT 'needle', COUNT(*)
            FROM graph_bm25
            WHERE content LIKE '%needle%';

            INSERT INTO graph_bm25_route_terms (
                term, source_scope, group_token, collection_frequency
            )
            SELECT 'topic', 'scope', printf('rkg%03d', rowid % 100), COUNT(*)
            FROM graph_bm25
            GROUP BY rowid % 100;

            INSERT INTO graph_bm25_route_term_totals (
                term, document_frequency
            ) VALUES ('topic', 4096);

            UPDATE graph_bm25_route_state
            SET indexed_graph_version = 1, document_count = 4096,
                algorithm_version = '{ROUTING_ALGORITHM_VERSION}';
            ",
        ))
        .expect("balanced routing fixture should build");
    let request = GraphSearchRequest {
        query: "needle".to_owned(),
        source_scope: Some("scope".to_owned()),
        graph_version: GraphVersion::new(1),
        limit: 10,
        disabled_retriever_sources: Vec::new(),
    };

    let active = plan_query(&connection, &request).expect("active route should plan");
    assert!(active.route_match.is_some());

    let mixed_request = GraphSearchRequest {
        query: "needle topic".to_owned(),
        ..request.clone()
    };
    let mixed = plan_query(&connection, &mixed_request).expect("mixed route should plan");
    assert!(mixed.route_match.is_none());
    assert_eq!(
        mixed.explanation.as_deref(),
        Some("hierarchical_bm25 fallback=low_selectivity")
    );

    connection
        .execute(
            "UPDATE graph_bm25_route_state SET state = 'building' WHERE id = 1",
            [],
        )
        .expect("route should enter building state");
    let building = plan_query(&connection, &request).expect("building route should plan");
    assert!(building.route_match.is_none());
    assert_eq!(
        building.explanation.as_deref(),
        Some("hierarchical_bm25 fallback=stale_route_generation")
    );
    connection
        .execute(
            "UPDATE graph_bm25_route_state SET state = 'fresh' WHERE id = 1",
            [],
        )
        .expect("route should return to fresh state");

    connection
        .execute(
            "UPDATE graph_bm25_route_terms
             SET group_token = 'orphan-group'
             WHERE term = 'needle' AND group_token = 'rkg039'",
            [],
        )
        .expect("orphan group statistic should be simulated");
    let orphaned = plan_query(&connection, &request).expect("orphaned route should plan");
    assert!(orphaned.route_match.is_none());
    assert_eq!(
        orphaned.explanation.as_deref(),
        Some("hierarchical_bm25 fallback=incomplete_group_statistics")
    );
    connection
        .execute(
            "UPDATE graph_bm25_route_terms
             SET group_token = 'rkg039'
             WHERE term = 'needle' AND group_token = 'orphan-group'",
            [],
        )
        .expect("group statistic should restore");
}
