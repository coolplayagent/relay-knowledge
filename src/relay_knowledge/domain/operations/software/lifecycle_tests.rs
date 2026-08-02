use super::*;

#[test]
fn lifecycle_projection_rows_use_distinct_domain_identities() {
    let build = SoftwareBuildTarget::new(SoftwareBuildTargetInput {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        ecosystem: "cargo".to_owned(),
        language_id: "rust".to_owned(),
        name: "relay-knowledge".to_owned(),
        kind: "binary".to_owned(),
        command: Some("cargo build".to_owned()),
        output_hint: Some("target/debug/relay-knowledge".to_owned()),
        source_kind: "manifest".to_owned(),
        evidence_path: "Cargo.toml".to_owned(),
        evidence_line_range: RepositoryCodeRange { start: 1, end: 8 },
        confidence_basis_points: 10_000,
        created_graph_version: GraphVersion::new(4),
    })
    .expect("build target should validate");
    let resource = SoftwareIacResource::new(SoftwareIacResourceInput {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        language_id: "yaml".to_owned(),
        provider: "kubernetes".to_owned(),
        resource_kind: "deployment".to_owned(),
        name: "relay-knowledge".to_owned(),
        scope_hint: Some("default".to_owned()),
        target_hint: None,
        resolution_state: "resolved".to_owned(),
        source_kind: "manifest".to_owned(),
        evidence_path: "deploy/app.yaml".to_owned(),
        evidence_line_range: RepositoryCodeRange { start: 2, end: 9 },
        confidence_basis_points: 9000,
        created_graph_version: GraphVersion::new(4),
    })
    .expect("IaC resource should validate");
    let design = SoftwareDesignElement::new(SoftwareDesignElementInput {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        language_id: "markdown".to_owned(),
        element_kind: "component".to_owned(),
        name: "storage".to_owned(),
        parent: Some("relay-knowledge".to_owned()),
        summary: Some("graph persistence".to_owned()),
        source_kind: "documentation".to_owned(),
        evidence_path: "docs/architecture.md".to_owned(),
        evidence_line_range: RepositoryCodeRange { start: 4, end: 12 },
        confidence_basis_points: 8000,
        created_graph_version: GraphVersion::new(4),
    })
    .expect("design element should validate");

    assert!(build.target_id.starts_with("build_target:"));
    assert!(resource.resource_id.starts_with("iac_resource:"));
    assert!(design.element_id.starts_with("design_element:"));
}

#[test]
fn lifecycle_projection_rejects_blank_optional_text() {
    let error = SoftwareBuildTarget::new(SoftwareBuildTargetInput {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        ecosystem: "cargo".to_owned(),
        language_id: "rust".to_owned(),
        name: "relay-knowledge".to_owned(),
        kind: "binary".to_owned(),
        command: Some(" ".to_owned()),
        output_hint: None,
        source_kind: "manifest".to_owned(),
        evidence_path: "Cargo.toml".to_owned(),
        evidence_line_range: RepositoryCodeRange { start: 1, end: 8 },
        confidence_basis_points: 10_000,
        created_graph_version: GraphVersion::new(4),
    })
    .expect_err("blank command should fail");

    assert_eq!(error.field, "command");
}
