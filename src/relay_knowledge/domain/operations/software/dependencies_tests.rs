use super::*;

#[test]
fn component_identity_includes_scope_and_resolved_version() {
    let base = component_input("scope-a", Some("1.0.0"));
    let component = SoftwareComponent::new(base).expect("component should validate");
    let changed = SoftwareComponent::new(component_input("scope-b", Some("1.0.0")))
        .expect("component should validate");

    assert_ne!(component.component_id, changed.component_id);
}

#[test]
fn component_identity_preserves_duplicate_evidence_rows() {
    let first = SoftwareComponent::new(component_input("scope-a", Some("1.0.0")))
        .expect("component should validate");
    let mut second_input = component_input("scope-a", Some("1.0.0"));
    second_input.evidence_path = "crates/core/Cargo.toml".to_owned();
    second_input.evidence_line_range = RepositoryCodeRange { start: 9, end: 9 };
    let second = SoftwareComponent::new(second_input).expect("component should validate");

    assert_ne!(first.component_id, second.component_id);
}

#[test]
fn component_identity_includes_expanded_language_rows() {
    let rust = SoftwareComponent::new(component_input("scope-a", Some("1.0.0")))
        .expect("component should validate");
    let mut tsx_input = component_input("scope-a", Some("1.0.0"));
    tsx_input.language_id = "tsx".to_owned();
    let tsx = SoftwareComponent::new(tsx_input).expect("component should validate");

    assert_ne!(rust.component_id, tsx.component_id);
}

#[test]
fn component_rejects_empty_name_and_invalid_confidence() {
    let mut input = component_input("scope-a", None);
    input.name = " ".to_owned();
    assert_eq!(
        SoftwareComponent::new(input)
            .expect_err("empty name should fail")
            .field,
        "component_name"
    );

    let mut input = component_input("scope-a", None);
    input.confidence_basis_points = 10_001;
    assert_eq!(
        SoftwareComponent::new(input)
            .expect_err("bad confidence should fail")
            .field,
        "confidence"
    );
}

#[test]
fn sdk_usage_preserves_unresolved_target_hint() {
    let usage = SoftwareSdkUsage::new(SoftwareSdkUsageInput {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        language_id: "cpp".to_owned(),
        module: "#include <securec.h>".to_owned(),
        target_hint: Some("securec.h".to_owned()),
        resolution_state: "unresolved".to_owned(),
        evidence_path: "src/main.cc".to_owned(),
        evidence_line_range: RepositoryCodeRange { start: 3, end: 3 },
        confidence_basis_points: 2500,
        created_graph_version: GraphVersion::new(7),
    })
    .expect("usage should validate");

    assert_eq!(usage.target_hint.as_deref(), Some("securec.h"));
}

#[test]
fn sdk_usage_identity_preserves_repeated_evidence_rows() {
    let first = SoftwareSdkUsage::new(sdk_usage_input(3)).expect("usage should validate");
    let second = SoftwareSdkUsage::new(sdk_usage_input(9)).expect("usage should validate");

    assert_ne!(first.usage_id, second.usage_id);
}

#[test]
fn dependency_usage_identity_binds_component_and_import_evidence() {
    let first = SoftwareDependencyUsage::new(dependency_usage_input("component:serde", "serde", 3))
        .expect("usage should validate");
    let second =
        SoftwareDependencyUsage::new(dependency_usage_input("component:serde", "serde", 9))
            .expect("usage should validate");
    let other_component =
        SoftwareDependencyUsage::new(dependency_usage_input("component:tokio", "serde", 3))
            .expect("usage should validate");

    assert_ne!(first.usage_id, second.usage_id);
    assert_ne!(first.usage_id, other_component.usage_id);
}

fn component_input(scope: &str, version: Option<&str>) -> SoftwareComponentInput {
    SoftwareComponentInput {
        repository_id: "repo".to_owned(),
        source_scope: scope.to_owned(),
        ecosystem: "cargo".to_owned(),
        name: "serde".to_owned(),
        requirement: Some("1".to_owned()),
        resolved_version: version.map(str::to_owned),
        dependency_group: "normal".to_owned(),
        source_kind: "manifest".to_owned(),
        relationship_state: "declared".to_owned(),
        language_id: "rust".to_owned(),
        evidence_path: "Cargo.toml".to_owned(),
        evidence_line_range: RepositoryCodeRange { start: 1, end: 1 },
        confidence_basis_points: 10_000,
        created_graph_version: GraphVersion::new(1),
    }
}

fn sdk_usage_input(line: u32) -> SoftwareSdkUsageInput {
    SoftwareSdkUsageInput {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        language_id: "cpp".to_owned(),
        module: "#include <securec.h>".to_owned(),
        target_hint: Some("securec.h".to_owned()),
        resolution_state: "unresolved".to_owned(),
        evidence_path: "src/main.cc".to_owned(),
        evidence_line_range: RepositoryCodeRange {
            start: line,
            end: line,
        },
        confidence_basis_points: 2500,
        created_graph_version: GraphVersion::new(7),
    }
}

fn dependency_usage_input(
    component_id: &str,
    module: &str,
    line: u32,
) -> SoftwareDependencyUsageInput {
    SoftwareDependencyUsageInput {
        component_id: component_id.to_owned(),
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        ecosystem: "cargo".to_owned(),
        package_name: "serde".to_owned(),
        language_id: "rust".to_owned(),
        module: module.to_owned(),
        target_hint: Some(module.to_owned()),
        resolution_state: "unresolved".to_owned(),
        evidence_path: "src/lib.rs".to_owned(),
        evidence_line_range: RepositoryCodeRange {
            start: line,
            end: line,
        },
        confidence_basis_points: 9000,
        created_graph_version: GraphVersion::new(7),
    }
}
