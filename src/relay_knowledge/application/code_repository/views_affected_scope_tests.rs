use crate::domain::{
    CodeCallRecord, CodeRepositorySelector, CodebaseViewCall, CodebaseViewFile, CodebaseViewKind,
    CodebaseViewRequest, CodebaseViewSnapshot, FreshnessPolicy, RepositoryCodeRange,
};

use super::{derive_view, view_source_read_paths};

#[test]
fn affected_scope_reports_missing_changes_and_verification_candidates() {
    let missing_request = request(10, Vec::new());
    let missing = derive_view(&missing_request, CodebaseViewSnapshot::default(), 20);

    assert!(missing.diagnostics[0].contains("--changed-path"));
    assert_eq!(
        missing.sections[0].id,
        "section:affected_scope:missing_changes"
    );

    let request = request(
        10,
        vec!["src/billing/service.rs".to_owned(), "src/lib.rs".to_owned()],
    );
    let snapshot = CodebaseViewSnapshot {
        files: vec![
            file("src/billing/service_test.rs", "rust"),
            file("src/billing/config.yaml", "yaml"),
            file("src/orders/service_test.rs", "rust"),
            file("src/lib_test.rs", "rust"),
        ],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);

    assert!(
        derived
            .nodes
            .iter()
            .any(|node| node.id == "candidate:src/billing/service_test.rs")
    );
    assert!(
        derived
            .nodes
            .iter()
            .any(|node| node.id == "candidate:src/billing/config.yaml")
    );
    assert!(
        derived
            .nodes
            .iter()
            .any(|node| node.id == "candidate:src/lib_test.rs")
    );
    assert!(
        !derived
            .nodes
            .iter()
            .any(|node| node.id == "candidate:src/orders/service_test.rs")
    );
}

#[test]
fn affected_scope_reserves_budget_for_derived_nodes() {
    let request = request(
        4,
        vec![
            "src/billing/service.rs".to_owned(),
            "src/billing/model.rs".to_owned(),
            "src/billing/repository.rs".to_owned(),
            "src/billing/routes.rs".to_owned(),
        ],
    );
    let snapshot = CodebaseViewSnapshot {
        files: vec![file("src/billing/service_test.rs", "rust")],
        calls: vec![call(
            "src/billing/service.rs",
            Some("run_billing"),
            "load_policy",
            Some("src/billing/policy.rs"),
        )],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);
    let section = derived
        .sections
        .iter()
        .find(|section| section.id == "section:affected_scope")
        .unwrap();

    assert_eq!(
        derived
            .nodes
            .iter()
            .filter(|node| node.node_kind == "changed_file")
            .count(),
        1
    );
    assert!(
        derived
            .nodes
            .iter()
            .any(|node| node.node_kind == "affected_module")
    );
    assert!(
        derived
            .nodes
            .iter()
            .any(|node| node.id == "candidate:src/billing/service_test.rs")
    );
    assert!(
        section
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("summarized"))
    );
}

#[test]
fn affected_scope_changed_caller_includes_callee_module() {
    let request = request(10, vec!["src/api/handler.rs".to_owned()]);
    let snapshot = CodebaseViewSnapshot {
        calls: vec![call(
            "src/api/handler.rs",
            Some("handler"),
            "apply_policy",
            Some("src/domain/policy.rs"),
        )],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);

    assert!(
        derived.nodes.iter().any(|node| node.id == "module:domain"
            && node.path.as_deref() == Some("src/domain/policy.rs"))
    );
    assert!(derived.edges.iter().any(|edge| {
        edge.edge_kind == "affected_call"
            && edge.source_id == "module:api"
            && edge.target_id == "module:domain"
    }));
}

#[test]
fn affected_scope_normalizes_changed_paths_for_call_matching() {
    let request = request(10, vec![".\\src\\api\\handler.rs".to_owned()]);
    let snapshot = CodebaseViewSnapshot {
        calls: vec![call(
            "src/api/handler.rs",
            Some("handler"),
            "apply_policy",
            Some("src/domain/policy.rs"),
        )],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);
    let paths = view_source_read_paths(&request, &derived);

    assert!(
        derived
            .nodes
            .iter()
            .any(|node| node.id == "file:src/api/handler.rs")
    );
    assert!(
        !derived
            .nodes
            .iter()
            .any(|node| node.id == "file:./src/api/handler.rs")
    );
    assert!(derived.edges.iter().any(|edge| {
        edge.edge_kind == "affected_call"
            && edge.source_id == "module:api"
            && edge.target_id == "module:domain"
    }));
    assert!(paths.contains(&"src/api/handler.rs".to_owned()));
    assert!(!paths.contains(&".\\src\\api\\handler.rs".to_owned()));
}

#[test]
fn affected_scope_matches_changed_directory_prefixes_for_calls() {
    let request = request(10, vec!["src/domain".to_owned()]);
    let snapshot = CodebaseViewSnapshot {
        calls: vec![call(
            "src/api/handler.rs",
            Some("handler"),
            "apply_policy",
            Some("src/domain/policy.rs"),
        )],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);

    assert!(derived.edges.iter().any(|edge| {
        edge.edge_kind == "affected_call"
            && edge.source_id == "module:api"
            && edge.target_id == "module:domain"
    }));
}

#[test]
fn affected_scope_changed_callee_includes_callee_module() {
    let request = request(10, vec!["src/domain/policy.rs".to_owned()]);
    let snapshot = CodebaseViewSnapshot {
        calls: vec![call(
            "src/api/handler.rs",
            Some("handler"),
            "apply_policy",
            Some("src/domain/policy.rs"),
        )],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);

    assert!(
        derived.nodes.iter().any(|node| node.id == "module:domain"
            && node.path.as_deref() == Some("src/domain/policy.rs"))
    );
    assert!(derived.edges.iter().any(|edge| {
        edge.edge_kind == "affected_call"
            && edge.source_id == "module:api"
            && edge.target_id == "module:domain"
    }));
}

#[test]
fn source_read_paths_come_from_returned_evidence_and_nodes() {
    let request = request(10, vec!["src/api/handler.rs".to_owned()]);
    let snapshot = CodebaseViewSnapshot {
        calls: vec![call(
            "src/api/handler.rs",
            Some("handler"),
            "apply_policy",
            Some("src/domain/policy.rs"),
        )],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);
    let paths = view_source_read_paths(&request, &derived);

    assert!(paths.contains(&"src/api/handler.rs".to_owned()));
    assert!(paths.contains(&"src/domain/policy.rs".to_owned()));
}

fn request(limit: usize, changed_paths: Vec<String>) -> CodebaseViewRequest {
    CodebaseViewRequest::new(
        CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap(),
        CodebaseViewKind::AffectedScope,
        FreshnessPolicy::AllowStale,
        limit,
        changed_paths,
    )
    .unwrap()
}

fn file(path: &str, language_id: &str) -> CodebaseViewFile {
    CodebaseViewFile {
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        parse_status: "parsed".to_owned(),
        line_count: 10,
        is_generated: false,
    }
}

fn call(
    path: &str,
    caller_name: Option<&str>,
    callee_name: &str,
    callee_path: Option<&str>,
) -> CodebaseViewCall {
    CodebaseViewCall {
        call: CodeCallRecord {
            repository_id: "repo".to_owned(),
            source_scope: "scope".to_owned(),
            call_id: format!("call:{path}:{callee_name}"),
            file_id: format!("file:{path}"),
            path: path.to_owned(),
            caller_symbol_snapshot_id: caller_name.map(|name| format!("symbol:{name}")),
            caller_name: caller_name.map(ToOwned::to_owned),
            callee_symbol_snapshot_id: Some(format!("symbol:{callee_name}")),
            callee_name: callee_name.to_owned(),
            target_hint: callee_path.map(ToOwned::to_owned),
            resolution_state: "resolved".to_owned(),
            confidence_basis_points: 8000,
            confidence_tier: "extracted".to_owned(),
            line_range: range(12, 12),
        },
        callee_path: callee_path.map(ToOwned::to_owned),
    }
}

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
}
