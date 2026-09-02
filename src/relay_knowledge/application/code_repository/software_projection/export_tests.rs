use super::*;
use std::collections::BTreeMap;

#[test]
fn utc_timestamp_formatter_covers_epoch_and_leap_year() {
    assert_eq!(format_utc_timestamp(0), "1970-01-01T00:00:00Z");
    assert_eq!(format_utc_timestamp(1_709_251_200), "2024-03-01T00:00:00Z");
}

#[test]
fn standard_documents_expose_required_profile_roots() {
    let response = empty_response();
    let spdx = spdx_document(&response, "2026-01-01T00:00:00Z");
    assert_eq!(spdx["@context"], SPDX_CONTEXT);
    assert_eq!(spdx["type"], "SpdxDocument");
    assert_eq!(spdx["creationInfo"]["type"], "CreationInfo");
    assert_eq!(spdx["creationInfo"]["specVersion"], "3.0.1");

    let cyclonedx = cyclonedx_document(&response, "2026-01-01T00:00:00Z");
    assert_eq!(cyclonedx["bomFormat"], "CycloneDX");
    assert_eq!(cyclonedx["specVersion"], "1.7");

    let prov = prov_document(&response);
    assert_eq!(prov["@context"]["prov"], PROV_NAMESPACE);
    assert_eq!(
        prov["@context"]["rko"],
        crate::domain::SOFTWARE_ONTOLOGY_NAMESPACE
    );
    assert!(
        prov["@graph"]
            .as_array()
            .is_some_and(|graph| !graph.is_empty())
    );
}

#[test]
fn prov_export_uses_the_executable_owl_vocabulary() {
    use crate::domain::{
        GraphVersion, RepositoryCodeRange, SoftwareAssertionMode, SoftwareEntityInput,
        SoftwareEvidenceRef, SoftwareFactState, SoftwareSourceKind, SoftwareStatementInput,
        SoftwareStatementResolution,
    };

    let mut response = empty_response();
    let evidence = SoftwareEvidenceRef::new(
        "scope",
        "Cargo.toml",
        RepositoryCodeRange { start: 1, end: 1 },
    )
    .expect("evidence");
    let component = SoftwareEntity::new(SoftwareEntityInput {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        entity_kind: SoftwareEntityKind::Component,
        name: "relay-knowledge".to_owned(),
        namespace: None,
        source_kind: SoftwareSourceKind::Manifest,
        evidence_refs: vec![evidence.clone()],
        attributes: BTreeMap::new(),
        created_graph_version: GraphVersion::new(1),
    })
    .expect("component");
    let package = SoftwareEntity::new(SoftwareEntityInput {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        entity_kind: SoftwareEntityKind::PackageComponent,
        name: "serde".to_owned(),
        namespace: None,
        source_kind: SoftwareSourceKind::Manifest,
        evidence_refs: vec![evidence.clone()],
        attributes: BTreeMap::new(),
        created_graph_version: GraphVersion::new(1),
    })
    .expect("package");
    let statement = SoftwareStatement::candidate(SoftwareStatementInput {
        subject_id: component.entity_key.clone(),
        predicate: SoftwarePredicate::DependsOn,
        object_id: Some(package.entity_key.clone()),
        object_value: None,
        source_scope: "scope".to_owned(),
        source_kind: SoftwareSourceKind::Manifest,
        evidence_refs: vec![evidence],
        assertion_mode: SoftwareAssertionMode::Declared,
        resolution_state: SoftwareStatementResolution::Resolved,
        valid_from: None,
        valid_to: None,
        observed_at: None,
        extractor_id: "relay-knowledge/software-ontology".to_owned(),
        extractor_version: crate::domain::SOFTWARE_ONTOLOGY_VERSION.to_owned(),
        confidence_basis_points: 10_000,
        fact_state: SoftwareFactState::Active,
    });
    response.entities = vec![component, package];
    response.statements = vec![statement];

    let document = prov_document(&response);
    let graph = document["@graph"].as_array().expect("PROV graph");

    assert!(graph.iter().any(|node| {
        node["@type"]
            .as_array()
            .is_some_and(|types| types.iter().any(|kind| kind == "rko:Component"))
    }));
    assert!(
        graph
            .iter()
            .any(|node| { node["rko:ontologyProperty"]["@id"] == "rko:dependsOn" })
    );
}

fn empty_response() -> SoftwareGlobalResponse {
    use crate::{
        api::{ApiMetadata, CodeRepositoryScopeMetadata},
        domain::{
            CodeRepositorySelector, FreshnessPolicy, GraphVersion, SoftwareGlobalKind,
            SoftwareGlobalRequest, SoftwareGlobalStatus, SoftwareProjectionFreshness,
            SoftwareSourceCoverage,
        },
    };

    SoftwareGlobalResponse {
        metadata: ApiMetadata {
            trace_id: "trace".to_owned(),
            request_id: "request".to_owned(),
            graph_version: 1,
            index_version: None,
            indexed_graph_version: Some(1),
            stale: false,
        },
        scope: CodeRepositoryScopeMetadata {
            scope_id: "scope".to_owned(),
            repository_id: "repo".to_owned(),
            alias: "repo".to_owned(),
            requested_ref: "HEAD".to_owned(),
            resolved_commit_sha: "abc".to_owned(),
            tree_hash: "tree".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            indexed_file_count: 0,
            index_versions: Vec::new(),
            stale: false,
        },
        request: SoftwareGlobalRequest::new(
            CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).expect("selector"),
            SoftwareGlobalKind::All,
            FreshnessPolicy::AllowStale,
            10,
        )
        .expect("request"),
        status: SoftwareGlobalStatus {
            repository_id: "repo".to_owned(),
            source_scope: "scope".to_owned(),
            projected_graph_version: GraphVersion::new(1),
            stale: false,
            ontology_version: crate::domain::SOFTWARE_ONTOLOGY_VERSION.to_owned(),
            projection_schema_version: crate::domain::SOFTWARE_PROJECTION_SCHEMA_VERSION,
            source_coverage: SoftwareSourceCoverage::default(),
            completeness_basis_points: 10_000,
            freshness: SoftwareProjectionFreshness::Fresh,
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
            last_error: None,
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
    }
}
