use super::*;

#[test]
fn retriever_source_labels_match_wire_values() {
    assert_eq!(RetrieverSource::Bm25.as_str(), "bm25");
    assert_eq!(RetrieverSource::GraphEvidence.as_str(), "graph_evidence");
    assert_eq!(RetrieverSource::CodeGraph.as_str(), "code_graph");
    assert_eq!(RetrieverSource::Semantic.as_str(), "semantic");
    assert_eq!(RetrieverSource::Vector.as_str(), "vector");
    assert_eq!(RetrieverSource::GraphPath.as_str(), "graph_path");
    assert_eq!(RetrieverSource::Temporal.as_str(), "temporal");
    assert_eq!(
        RetrieverSource::CommunitySummary.as_str(),
        "community_summary"
    );
}
