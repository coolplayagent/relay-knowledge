use super::{
    BoundedLogValue, MAX_LOG_IDENTITY_CHARS, REBUILD_BATCH_BUDGET, RebuildWorkload, bounded_page,
    code_chunk_rebuild_page, code_symbol_rebuild_page, evidence_rebuild_page,
};

#[test]
fn bm25_hierarchy_suite_stops_before_each_cumulative_rebuild_work_budget() {
    let boundary_pairs = [
        (
            RebuildWorkload {
                source_bytes: REBUILD_BATCH_BUDGET.source_bytes / 2 + 1,
                labels: 0,
                links: 0,
            },
            "source bytes",
        ),
        (
            RebuildWorkload {
                source_bytes: 0,
                labels: REBUILD_BATCH_BUDGET.labels / 2 + 1,
                links: 0,
            },
            "labels",
        ),
        (
            RebuildWorkload {
                source_bytes: 0,
                labels: 0,
                links: REBUILD_BATCH_BUDGET.links / 2 + 1,
            },
            "links",
        ),
    ];

    for (workload, boundary) in boundary_pairs {
        let (page, oversized) = bounded_page(
            vec![("first", workload), ("second", workload)],
            REBUILD_BATCH_BUDGET,
        );
        assert_eq!(page.keys, ["first"], "{boundary} must bound the page");
        assert!(
            !page.page_is_complete,
            "{boundary} must preserve the cursor"
        );
        assert_eq!(oversized, None, "each individual item fits {boundary}");
    }
}

#[test]
fn bm25_hierarchy_suite_isolates_oversized_rebuild_work_and_advances_after_it() {
    let oversized_workload = RebuildWorkload {
        source_bytes: REBUILD_BATCH_BUDGET.source_bytes + 1,
        labels: REBUILD_BATCH_BUDGET.labels + 1,
        links: REBUILD_BATCH_BUDGET.links + 1,
    };
    let small_workload = RebuildWorkload {
        source_bytes: 1,
        labels: 1,
        links: 1,
    };

    let (oversized_page, oversized) = bounded_page(
        vec![("oversized", oversized_workload), ("next", small_workload)],
        REBUILD_BATCH_BUDGET,
    );
    assert_eq!(oversized_page.keys, ["oversized"]);
    assert!(!oversized_page.page_is_complete);
    assert_eq!(oversized, Some(oversized_workload));

    let (resumed_page, resumed_oversized) =
        bounded_page(vec![("next", small_workload)], REBUILD_BATCH_BUDGET);
    assert_eq!(resumed_page.keys, ["next"]);
    assert!(resumed_page.page_is_complete);
    assert_eq!(resumed_oversized, None);
}

#[test]
fn bm25_hierarchy_suite_bounds_oversized_document_identity_in_warning_fields() {
    let identity = "界".repeat(MAX_LOG_IDENTITY_CHARS + 1);
    let rendered = BoundedLogValue(&identity).to_string();

    assert_eq!(rendered.chars().count(), MAX_LOG_IDENTITY_CHARS + 1);
    assert!(rendered.ends_with('…'));
    assert!(!rendered.contains(&"界".repeat(MAX_LOG_IDENTITY_CHARS + 1)));
}

#[test]
fn empty_bootstrap_sources_do_not_require_the_full_authoritative_schema() {
    let connection = rusqlite::Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch("CREATE TABLE evidence (id TEXT PRIMARY KEY, status TEXT NOT NULL);")
        .expect("minimal evidence schema should create");

    let evidence = evidence_rebuild_page(&connection, None).expect("empty evidence should page");
    let symbols =
        code_symbol_rebuild_page(&connection, None).expect("missing symbol source should page");
    let chunks =
        code_chunk_rebuild_page(&connection, None).expect("missing chunk source should page");

    assert!(evidence.keys.is_empty() && evidence.page_is_complete);
    assert!(symbols.keys.is_empty() && symbols.page_is_complete);
    assert!(chunks.keys.is_empty() && chunks.page_is_complete);
}
