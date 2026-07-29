use super::{CodeRepositoryReport, CodeRepositoryTotals};

#[test]
fn generated_repository_counts_default_when_deserializing_older_responses() {
    let mut totals_json =
        serde_json::to_value(CodeRepositoryTotals::default()).expect("totals should serialize");
    let totals_object = totals_json
        .as_object_mut()
        .expect("totals json should be an object");
    totals_object.remove("handwritten_symbol_count");
    totals_object.remove("generated_symbol_count");
    let totals = serde_json::from_value::<CodeRepositoryTotals>(totals_json)
        .expect("older totals response should deserialize");

    assert_eq!(totals.handwritten_symbol_count, 0);
    assert_eq!(totals.generated_symbol_count, 0);

    let mut report_json = serde_json::to_value(CodeRepositoryReport {
        repository_id: "repo".to_owned(),
        alias: "fixture".to_owned(),
        root_path: "/tmp/repo".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        resolved_commit_sha: Some("commit".to_owned()),
        tree_hash: Some("tree".to_owned()),
        indexed_file_count: 1,
        symbol_count: 2,
        handwritten_symbol_count: 1,
        generated_symbol_count: 1,
        reference_count: 0,
        chunk_count: 0,
        degraded_file_count: 0,
        resolved_edge_count: 0,
        ambiguous_edge_count: 0,
        unresolved_edge_count: 0,
        degradation_summary: Vec::new(),
        representative_queries: Vec::new(),
        latency_samples: Vec::new(),
        freshness_state: "fresh".to_owned(),
    })
    .expect("report should serialize");
    let report_object = report_json
        .as_object_mut()
        .expect("report json should be an object");
    report_object.remove("handwritten_symbol_count");
    report_object.remove("generated_symbol_count");
    let report = serde_json::from_value::<CodeRepositoryReport>(report_json)
        .expect("older report response should deserialize");

    assert_eq!(report.handwritten_symbol_count, 0);
    assert_eq!(report.generated_symbol_count, 0);
}
