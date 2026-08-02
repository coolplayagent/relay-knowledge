use super::super::super::GraphVersion;
use super::*;

#[test]
fn graph_path_preserves_fact_provenance() {
    let fact = ContextGraphFact {
        fact_id: "fact-1".to_owned(),
        kind: ContextGraphFactKind::Relation,
        subject: "service".to_owned(),
        predicate: "depends_on".to_owned(),
        object: Some("database".to_owned()),
        evidence_ids: vec!["evidence-1".to_owned()],
        confidence: ConfidenceScore {
            basis_points: 9_000,
        },
        status: FactStatus::Accepted,
        version_range: GraphVersionRange::open_from(GraphVersion::new(1)),
    };

    let path = ContextGraphPath::from_fact(&fact);

    assert_eq!(path.path_id, "path:fact-1");
    assert_eq!(path.nodes, ["service", "database"]);
    assert_eq!(path.edges.len(), 1);
    assert_eq!(path.edges[0].fact_id, "fact-1");
    assert_eq!(path.edges[0].evidence_ids, ["evidence-1"]);
}
