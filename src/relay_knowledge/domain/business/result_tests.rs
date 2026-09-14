use super::*;
use crate::domain::GraphVersion;

#[test]
fn readiness_uses_scope_counts_without_hiding_staleness() {
    for (sources, terms, mappings, expected) in [
        (0, 0, 0, BusinessKnowledgeState::NoSources),
        (1, 0, 0, BusinessKnowledgeState::EmptyGlossary),
        (1, 2, 0, BusinessKnowledgeState::TermsOnly),
        (1, 2, 1, BusinessKnowledgeState::Mapped),
    ] {
        for stale in [false, true] {
            let knowledge = BusinessKnowledgeSummary::from_projection(BusinessKnowledgeStatus {
                repository_id: "repo".into(),
                source_scope: "scope".into(),
                resolved_commit_sha: "commit".into(),
                projected_graph_version: GraphVersion::new(1),
                stale,
                source_count: sources,
                domain_count: usize::from(terms > 0),
                term_count: terms,
                mapping_count: mappings,
                last_error: None,
            });
            assert_eq!(knowledge.state, expected);
            let json = serde_json::to_value(&knowledge).unwrap();
            assert_eq!(json["stale"], stale);
            assert_eq!(json["term_count"], terms);
            assert!(json.get("projection").is_none());
            assert_eq!(
                serde_json::from_value::<BusinessKnowledgeSummary>(json).unwrap(),
                knowledge
            );
        }
    }
}
