use crate::domain::{
    CodeImportRecord, CodeRepositorySelector, CodebaseViewDependency, CodebaseViewFile,
    CodebaseViewKind, CodebaseViewRequest, CodebaseViewSnapshot, FreshnessPolicy,
    RepositoryCodeRange,
};

use super::derive_view;

#[test]
fn dependency_tour_reports_cycle_with_retained_edges() {
    let request = request(CodebaseViewKind::DependencyTour, 10);
    let snapshot = CodebaseViewSnapshot {
        files: vec![file("src/a/mod.rs", "rust"), file("src/b/mod.rs", "rust")],
        imports: vec![
            import("src/a/mod.rs", "crate::b", Some("src/b/mod.rs")),
            import("src/b/mod.rs", "crate::a", Some("src/a/mod.rs")),
        ],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);
    let section = derived
        .sections
        .iter()
        .find(|section| section.id == "section:dependency_tour")
        .unwrap();

    assert_eq!(section.diagnostics.len(), 1);
    assert!(section.diagnostics[0].contains("cycle"));
    assert_eq!(
        derived
            .edges
            .iter()
            .filter(|edge| edge.edge_kind == "depends_on")
            .count(),
        2
    );
}

#[test]
fn dependency_tour_inserts_nodes_in_tour_order_before_budget() {
    let request = request(CodebaseViewKind::DependencyTour, 1);
    let snapshot = CodebaseViewSnapshot {
        files: vec![
            file("src/a/mod.rs", "rust"),
            file("src/b/mod.rs", "rust"),
            file("src/c/mod.rs", "rust"),
        ],
        imports: vec![import("src/b/mod.rs", "crate::a", Some("src/a/mod.rs"))],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);
    let section = derived
        .sections
        .iter()
        .find(|section| section.id == "section:dependency_tour")
        .unwrap();

    assert_eq!(derived.nodes[0].id, "module:b");
    assert_eq!(section.node_ids, ["module:b"]);
    assert!(section.edge_ids.is_empty());
    assert!(section.edge_ids.iter().all(|edge_id| {
        derived
            .edges
            .iter()
            .find(|edge| edge.id == *edge_id)
            .is_some_and(|edge| {
                section.node_ids.contains(&edge.source_id)
                    && section.node_ids.contains(&edge.target_id)
            })
    }));
    assert!(
        section
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("truncated"))
    );
    assert_eq!(section.narrative, "Suggested tour order: b.");
}

#[test]
fn dependency_tour_includes_manifest_dependency_rows() {
    let request = request(CodebaseViewKind::DependencyTour, 10);
    let snapshot = CodebaseViewSnapshot {
        dependencies: vec![dependency("Cargo.toml", "cargo", "serde")],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);
    let section = derived
        .sections
        .iter()
        .find(|section| section.id == "section:dependency_tour")
        .unwrap();

    assert!(
        derived
            .nodes
            .iter()
            .any(|node| node.id == "package:cargo-serde"
                && node.node_kind == "package"
                && node.label == "serde")
    );
    assert!(
        derived
            .edges
            .iter()
            .any(|edge| edge.source_id == "module:root"
                && edge.target_id == "package:cargo-serde"
                && edge.edge_kind == "depends_on")
    );
    assert!(section.node_ids.contains(&"package:cargo-serde".to_owned()));
    assert!(
        derived
            .evidence
            .iter()
            .any(|evidence| evidence.evidence_kind == "dependency"
                && evidence.symbol.as_deref() == Some("serde"))
    );
}

#[test]
fn dependency_tour_deduplicates_package_slots_before_limit() {
    let request = request(CodebaseViewKind::DependencyTour, 2);
    let snapshot = CodebaseViewSnapshot {
        dependencies: vec![
            dependency("Cargo.toml", "cargo", "serde"),
            dependency("Cargo.lock", "cargo", "serde"),
            dependency("Cargo.toml", "cargo", "tokio"),
        ],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);
    let package_ids = derived
        .nodes
        .iter()
        .filter(|node| node.node_kind == "package")
        .map(|node| node.id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(package_ids, ["package:cargo-serde"]);
    let serde_node = derived
        .nodes
        .iter()
        .find(|node| node.id == "package:cargo-serde")
        .unwrap();
    assert_eq!(serde_node.evidence_ids.len(), 2);
}

#[test]
fn dependency_tour_reserves_slots_for_package_dependencies() {
    let request = request(CodebaseViewKind::DependencyTour, 4);
    let snapshot = CodebaseViewSnapshot {
        files: vec![
            file("src/a/mod.rs", "rust"),
            file("src/b/mod.rs", "rust"),
            file("src/c/mod.rs", "rust"),
            file("src/d/mod.rs", "rust"),
        ],
        dependencies: vec![dependency("Cargo.toml", "cargo", "serde")],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);
    let section = derived
        .sections
        .iter()
        .find(|section| section.id == "section:dependency_tour")
        .unwrap();

    assert_eq!(derived.nodes.len(), 4);
    assert!(section.node_ids.contains(&"module:root".to_owned()));
    assert!(section.node_ids.contains(&"package:cargo-serde".to_owned()));
    assert!(
        derived
            .edges
            .iter()
            .any(|edge| edge.source_id == "module:root"
                && edge.target_id == "package:cargo-serde"
                && edge.edge_kind == "depends_on")
    );
    assert!(
        section
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("truncated"))
    );
}

#[test]
fn dependency_tour_preserves_punctuation_distinct_package_keys() {
    let request = request(CodebaseViewKind::DependencyTour, 10);
    let snapshot = CodebaseViewSnapshot {
        dependencies: vec![
            dependency("package.json", "npm", "foo.bar"),
            dependency("package.json", "npm", "foo-bar"),
        ],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);

    assert!(
        derived
            .nodes
            .iter()
            .any(|node| node.id == "package:npm-foo~2Ebar" && node.label == "foo.bar")
    );
    assert!(
        derived
            .nodes
            .iter()
            .any(|node| node.id == "package:npm-foo-bar" && node.label == "foo-bar")
    );
}

fn request(view_kind: CodebaseViewKind, limit: usize) -> CodebaseViewRequest {
    CodebaseViewRequest::new(
        CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap(),
        view_kind,
        FreshnessPolicy::AllowStale,
        limit,
        Vec::new(),
    )
    .unwrap()
}

fn file(path: &str, language: &str) -> CodebaseViewFile {
    CodebaseViewFile {
        path: path.to_owned(),
        language_id: language.to_owned(),
        parse_status: "parsed".to_owned(),
        line_count: 10,
        is_generated: false,
    }
}

fn import(path: &str, module: &str, target_hint: Option<&str>) -> CodeImportRecord {
    CodeImportRecord {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        import_id: format!("import:{path}:{module}"),
        file_id: format!("file:{path}"),
        path: path.to_owned(),
        module: module.to_owned(),
        target_hint: target_hint.map(ToOwned::to_owned),
        resolution_state: "resolved".to_owned(),
        confidence_basis_points: 8000,
        confidence_tier: "extracted".to_owned(),
        line_range: range(2, 2),
    }
}

fn dependency(path: &str, ecosystem: &str, package_name: &str) -> CodebaseViewDependency {
    CodebaseViewDependency {
        dependency_id: format!("dependency:{ecosystem}:{package_name}"),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        ecosystem: ecosystem.to_owned(),
        package_name: package_name.to_owned(),
        requirement: Some("1".to_owned()),
        resolved_version: None,
        dependency_group: "runtime".to_owned(),
        source_kind: "manifest".to_owned(),
        line_range: range(4, 4),
    }
}

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
}
