use super::*;
use crate::domain::SOFTWARE_ONTOLOGY_VERSION;

#[test]
fn empty_projection_preserves_freshness_status() {
    let projection = SoftwareGlobalProjection {
        status: SoftwareGlobalStatus {
            repository_id: "repo".to_owned(),
            source_scope: "scope".to_owned(),
            projected_graph_version: GraphVersion::new(9),
            stale: true,
            ontology_version: SOFTWARE_ONTOLOGY_VERSION.to_owned(),
            projection_schema_version: SOFTWARE_PROJECTION_SCHEMA_VERSION,
            source_coverage: SoftwareSourceCoverage::default(),
            completeness_basis_points: 0,
            freshness: SoftwareProjectionFreshness::Stale,
            conflict_count: 0,
            entity_count: 0,
            statement_count: 0,
            diagnostic_count: 0,
            component_count: 0,
            sdk_usage_count: 0,
            file_count: 0,
            topic_count: 0,
            relationship_count: 0,
            build_target_count: 0,
            iac_resource_count: 0,
            design_element_count: 0,
            last_error: Some("refresh pending".to_owned()),
        },
        components: Vec::new(),
        dependency_usages: Vec::new(),
        sdk_usages: Vec::new(),
        files: Vec::new(),
        topics: Vec::new(),
        relationships: Vec::new(),
        build_targets: Vec::new(),
        iac_resources: Vec::new(),
        design_elements: Vec::new(),
        entities: Vec::new(),
        statements: Vec::new(),
        diagnostics: Vec::new(),
    };

    assert!(projection.status.stale);
    assert_eq!(
        projection.status.projected_graph_version,
        GraphVersion::new(9)
    );
    assert!(projection.components.is_empty());
}
