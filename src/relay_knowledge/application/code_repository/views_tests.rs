use crate::domain::{
    CodeCallRecord, CodeFeatureFlagRecord, CodeImportRecord, CodeRepositorySelector,
    CodeRepositoryStatus, CodeRouteRecord, CodebaseViewCall, CodebaseViewFile, CodebaseViewKind,
    CodebaseViewRequest, CodebaseViewSnapshot, FreshnessPolicy, RepositoryCodeRange,
};

use super::{derive_view, served_scope_is_stale, view_source_read_paths};

#[test]
fn architecture_layers_include_import_and_call_edges() {
    let request = request(CodebaseViewKind::ArchitectureLayers, 10, Vec::new());
    let snapshot = CodebaseViewSnapshot {
        files: vec![
            file("src/interfaces/http.rs", "rust"),
            file("src/domain/model.rs", "rust"),
            file("src/storage/repository.rs", "rust"),
        ],
        imports: vec![import(
            "src/interfaces/http.rs",
            "crate::domain::model",
            Some("src/domain/model.rs"),
        )],
        calls: vec![call(
            "src/interfaces/http.rs",
            Some("handler"),
            "save_model",
            Some("src/storage/repository.rs"),
        )],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);

    assert!(
        derived
            .nodes
            .iter()
            .any(|node| node.id == "layer:interfaces")
    );
    assert!(
        derived
            .edges
            .iter()
            .any(|edge| edge.source_id == "layer:interfaces"
                && edge.target_id == "layer:domain"
                && edge.edge_kind == "imports")
    );
    assert!(
        derived
            .edges
            .iter()
            .any(|edge| edge.source_id == "layer:interfaces"
                && edge.target_id == "layer:storage"
                && edge.edge_kind == "calls")
    );
    assert!(
        derived
            .sections
            .iter()
            .any(|section| section.id == "section:layer:interfaces"
                && section.narrative.contains("indexed file"))
    );
}

#[test]
fn architecture_layers_ignore_unresolved_external_import_hints() {
    let request = request(CodebaseViewKind::ArchitectureLayers, 10, Vec::new());
    let mut unresolved = import("src/interfaces/http.rs", "serde", Some("serde"));
    unresolved.resolution_state = "unresolved".to_owned();
    let snapshot = CodebaseViewSnapshot {
        files: vec![file("src/interfaces/http.rs", "rust")],
        imports: vec![unresolved],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);

    assert!(
        !derived
            .edges
            .iter()
            .any(|edge| edge.edge_kind == "imports" && edge.target_id == "layer:source")
    );
}

#[test]
fn architecture_layers_rank_layer_candidates_before_node_budget() {
    let request = request(CodebaseViewKind::ArchitectureLayers, 1, Vec::new());
    let snapshot = CodebaseViewSnapshot {
        files: vec![
            file("docs/guide.md", "markdown"),
            file("src/domain/user.rs", "rust"),
            file("src/domain/order.rs", "rust"),
        ],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);

    assert_eq!(derived.nodes[0].id, "layer:domain");
}

#[test]
fn business_domains_aggregate_route_flag_and_path_signals() {
    let request = request(CodebaseViewKind::BusinessDomains, 10, Vec::new());
    let snapshot = CodebaseViewSnapshot {
        files: vec![file("src/orders/service.rs", "rust")],
        routes: vec![route(
            "route-billing",
            "src/interfaces/billing.rs",
            "GET",
            "/billing/invoices",
            "list_invoices",
            None,
        )],
        feature_flags: vec![feature_flag(
            "src/application/checkout.rs",
            "checkout.new_flow",
            "runtime",
        )],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);
    let domain_ids = derived
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    assert!(domain_ids.contains("domain:billing"));
    assert!(domain_ids.contains("domain:checkout"));
    assert!(domain_ids.contains("domain:orders"));
    assert!(
        derived
            .evidence
            .iter()
            .any(|evidence| evidence.evidence_kind == "feature_flag"
                && evidence.symbol.as_deref() == Some("checkout.new_flow"))
    );
}

#[test]
fn business_domains_mark_budget_when_domains_are_limited() {
    let request = request(CodebaseViewKind::BusinessDomains, 2, Vec::new());
    let snapshot = CodebaseViewSnapshot {
        files: vec![
            file("src/users.rs", "rust"),
            file("src/orders.rs", "rust"),
            file("src/billing.rs", "rust"),
        ],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);

    assert_eq!(derived.nodes.len(), 2);
    assert!(derived.budget.nodes_truncated);
    assert!(derived.budget.sections_truncated);
}

#[test]
fn evidence_is_pruned_to_bounded_returned_references() {
    let request = request(CodebaseViewKind::ArchitectureLayers, 2, Vec::new());
    let snapshot = CodebaseViewSnapshot {
        files: (0..20)
            .map(|index| file(&format!("src/domain/module_{index}.rs"), "rust"))
            .collect(),
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 40);
    let evidence_ids = derived
        .evidence
        .iter()
        .map(|evidence| evidence.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    assert!(derived.budget.evidence_truncated);
    assert!(derived.evidence.len() <= request.limit * 4);
    assert!(
        derived
            .sections
            .iter()
            .flat_map(|section| section.evidence_ids.iter())
            .all(|evidence_id| evidence_ids.contains(evidence_id.as_str()))
    );
}

#[test]
fn affected_scope_reports_missing_changes_and_verification_candidates() {
    let missing_request = request(CodebaseViewKind::AffectedScope, 10, Vec::new());
    let missing = derive_view(&missing_request, CodebaseViewSnapshot::default(), 20);

    assert!(missing.diagnostics[0].contains("--changed-path"));
    assert_eq!(
        missing.sections[0].id,
        "section:affected_scope:missing_changes"
    );

    let request = request(
        CodebaseViewKind::AffectedScope,
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
fn truncated_nodes_are_not_returned_as_section_refs() {
    let request = request(CodebaseViewKind::ArchitectureLayers, 1, Vec::new());
    let snapshot = CodebaseViewSnapshot {
        files: vec![
            file("src/domain/model.rs", "rust"),
            file("src/interface/http.rs", "rust"),
        ],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);
    let node_ids = derived
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    assert!(derived.budget.nodes_truncated);
    assert!(
        derived
            .sections
            .iter()
            .flat_map(|section| section.node_ids.iter())
            .all(|node_id| node_ids.contains(node_id.as_str()))
    );
}

#[test]
fn dependency_tour_ignores_unresolved_external_import_hints() {
    let request = request(CodebaseViewKind::DependencyTour, 10, Vec::new());
    let mut unresolved = import("src/api/users.rs", "serde", Some("serde"));
    unresolved.resolution_state = "unresolved".to_owned();
    let snapshot = CodebaseViewSnapshot {
        files: vec![
            file("src/api/users.rs", "rust"),
            file("src/domain/users.rs", "rust"),
        ],
        imports: vec![
            unresolved,
            import(
                "src/api/users.rs",
                "crate::domain::users",
                Some("src/domain/users.rs"),
            ),
        ],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);
    let node_ids = derived
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    assert!(!node_ids.contains("module:serde"));
    assert!(node_ids.contains("module:api"));
    assert!(node_ids.contains("module:domain"));
}

#[test]
fn affected_scope_changed_caller_includes_callee_module() {
    let request = request(
        CodebaseViewKind::AffectedScope,
        10,
        vec!["src/api/handler.rs".to_owned()],
    );
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
    let request = request(
        CodebaseViewKind::AffectedScope,
        10,
        vec![".\\src\\api\\handler.rs".to_owned()],
    );
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
    let request = request(
        CodebaseViewKind::AffectedScope,
        10,
        vec!["src/domain".to_owned()],
    );
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
    let request = request(
        CodebaseViewKind::AffectedScope,
        10,
        vec!["src/domain/policy.rs".to_owned()],
    );
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
fn process_flow_ignores_calls_without_matching_caller() {
    let request = request(CodebaseViewKind::ProcessFlow, 10, Vec::new());
    let snapshot = CodebaseViewSnapshot {
        routes: vec![route(
            "route-1",
            "src/api/users.rs",
            "GET",
            "/users",
            "index",
            None,
        )],
        calls: vec![call(
            "src/api/users.rs",
            None,
            "load_users",
            Some("src/domain/users.rs"),
        )],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);

    assert!(!derived.edges.iter().any(|edge| edge.edge_kind == "calls"));
}

#[test]
fn process_flow_does_not_attach_substring_caller_names() {
    let request = request(CodebaseViewKind::ProcessFlow, 10, Vec::new());
    let snapshot = CodebaseViewSnapshot {
        routes: vec![route(
            "route-1",
            "src/api/users.rs",
            "GET",
            "/users",
            "index",
            Some("symbol:index"),
        )],
        calls: vec![call(
            "src/api/users.rs",
            Some("reindex"),
            "load_users",
            Some("src/domain/users.rs"),
        )],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);

    assert!(!derived.edges.iter().any(|edge| edge.edge_kind == "calls"));
}

#[test]
fn process_flow_matches_route_calls_by_symbol_id() {
    let request = request(CodebaseViewKind::ProcessFlow, 10, Vec::new());
    let mut handler_call = call(
        "src/api/users.rs",
        Some("renamed_index"),
        "load_users",
        Some("src/domain/users.rs"),
    );
    handler_call.call.caller_symbol_snapshot_id = Some("symbol:index".to_owned());
    let snapshot = CodebaseViewSnapshot {
        routes: vec![route(
            "route-1",
            "src/api/users.rs",
            "GET",
            "/users",
            "index",
            Some("symbol:index"),
        )],
        calls: vec![handler_call],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);

    assert!(derived.edges.iter().any(|edge| edge.edge_kind == "calls"));
}

#[test]
fn process_flow_handler_ids_include_route_identity() {
    let request = request(CodebaseViewKind::ProcessFlow, 10, Vec::new());
    let snapshot = CodebaseViewSnapshot {
        routes: vec![
            route(
                "route-1",
                "src/api/users.rs",
                "GET",
                "/users",
                "index",
                None,
            ),
            route(
                "route-2",
                "src/admin/users.rs",
                "GET",
                "/admin/users",
                "index",
                None,
            ),
        ],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);
    let handler_ids = derived
        .nodes
        .iter()
        .filter(|node| node.node_kind == "handler")
        .map(|node| node.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(handler_ids.len(), 2);
    assert!(handler_ids.contains("handler:src/api/users.rs:route-1"));
    assert!(handler_ids.contains("handler:src/admin/users.rs:route-2"));
}

#[test]
fn process_flow_sections_reference_call_target_nodes() {
    let request = request(CodebaseViewKind::ProcessFlow, 10, Vec::new());
    let snapshot = CodebaseViewSnapshot {
        routes: vec![route(
            "route-1",
            "src/api/users.rs",
            "GET",
            "/users",
            "index",
            None,
        )],
        calls: vec![call(
            "src/api/users.rs",
            Some("index"),
            "load_users",
            Some("src/domain/users.rs"),
        )],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);
    let section = derived
        .sections
        .iter()
        .find(|section| section.id == "section:route:route-1")
        .unwrap();

    assert!(
        section
            .node_ids
            .contains(&"call_target:symbol:symbol:load_users".to_owned())
    );
    assert!(
        section
            .edge_ids
            .iter()
            .any(|edge_id| edge_id.contains("load_users"))
    );
}

#[test]
fn process_flow_call_targets_include_resolved_identity() {
    let request = request(CodebaseViewKind::ProcessFlow, 10, Vec::new());
    let mut api_call = call(
        "src/api/users.rs",
        Some("index"),
        "load",
        Some("src/domain/users.rs"),
    );
    api_call.call.callee_symbol_snapshot_id = Some("symbol:domain:load".to_owned());
    let mut admin_call = call(
        "src/admin/users.rs",
        Some("index"),
        "load",
        Some("src/domain/admin_users.rs"),
    );
    admin_call.call.callee_symbol_snapshot_id = Some("symbol:admin:load".to_owned());
    let snapshot = CodebaseViewSnapshot {
        routes: vec![
            route(
                "route-api",
                "src/api/users.rs",
                "GET",
                "/users",
                "index",
                None,
            ),
            route(
                "route-admin",
                "src/admin/users.rs",
                "GET",
                "/admin/users",
                "index",
                None,
            ),
        ],
        calls: vec![api_call, admin_call],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);
    let target_ids = derived
        .nodes
        .iter()
        .filter(|node| node.node_kind == "call_target")
        .map(|node| node.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(target_ids.len(), 2);
    assert!(target_ids.contains("call_target:symbol:symbol:domain:load"));
    assert!(target_ids.contains("call_target:symbol:symbol:admin:load"));
    assert!(
        derived
            .edges
            .iter()
            .filter(|edge| edge.edge_kind == "calls")
            .count()
            >= 2
    );
}

#[test]
fn process_flow_reports_truncated_handler_calls() {
    let request = request(CodebaseViewKind::ProcessFlow, 20, Vec::new());
    let calls = (0..9)
        .map(|index| {
            call(
                "src/api/users.rs",
                Some("handler"),
                &format!("load_{index}"),
                None,
            )
        })
        .collect::<Vec<_>>();
    let snapshot = CodebaseViewSnapshot {
        routes: vec![route(
            "route-1",
            "src/api/users.rs",
            "GET",
            "/users",
            "handler",
            None,
        )],
        calls,
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 40);
    let section = derived
        .sections
        .iter()
        .find(|section| section.id == "section:route:route-1")
        .unwrap();

    assert!(derived.budget.nodes_truncated);
    assert!(derived.budget.edges_truncated);
    assert!(
        section
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("truncated"))
    );
}

#[test]
fn process_flow_stops_sections_when_route_node_budget_is_exhausted() {
    let request = request(CodebaseViewKind::ProcessFlow, 3, Vec::new());
    let snapshot = CodebaseViewSnapshot {
        routes: vec![
            route("route-1", "src/api/one.rs", "GET", "/one", "one", None),
            route("route-2", "src/api/two.rs", "GET", "/two", "two", None),
        ],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);

    assert_eq!(derived.sections.len(), 1);
    assert!(derived.budget.nodes_truncated);
    assert!(derived.budget.sections_truncated);
    assert!(
        derived
            .sections
            .iter()
            .all(|section| !section.node_ids.is_empty())
    );
}

#[test]
fn process_flow_allows_shared_handler_routes_to_use_remaining_slots() {
    let request = request(CodebaseViewKind::ProcessFlow, 3, Vec::new());
    let snapshot = CodebaseViewSnapshot {
        routes: vec![
            route("r1", "a.rs", "GET", "/one", "h", Some("symbol:h")),
            route("r2", "b.rs", "GET", "/two", "h", Some("symbol:h")),
        ],
        ..CodebaseViewSnapshot::default()
    };

    let derived = derive_view(&request, snapshot, 20);

    assert_eq!(
        derived
            .sections
            .iter()
            .filter(|section| section.id.starts_with("section:route:"))
            .count(),
        2
    );
    assert!(!derived.budget.nodes_truncated);
}

#[test]
fn source_read_paths_come_from_returned_evidence_and_nodes() {
    let request = request(
        CodebaseViewKind::AffectedScope,
        10,
        vec!["src/api/handler.rs".to_owned()],
    );
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

#[test]
fn served_stale_reason_marks_scope_stale() {
    let mut status = status();

    assert!(!served_scope_is_stale(&status, &None));
    assert!(served_scope_is_stale(
        &status,
        &Some("served last completed code index".to_owned())
    ));

    status.stale = true;
    assert!(served_scope_is_stale(&status, &None));
}

fn status() -> CodeRepositoryStatus {
    CodeRepositoryStatus {
        repository_id: "repo".to_owned(),
        alias: "repo".to_owned(),
        root_path: "/tmp/repo".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        last_indexed_scope_id: Some("scope".to_owned()),
        last_indexed_commit: Some("commit".to_owned()),
        tree_hash: Some("tree".to_owned()),
        state: "fresh".to_owned(),
        indexed_file_count: 1,
        symbol_count: 1,
        reference_count: 0,
        chunk_count: 1,
        stale: false,
        degraded_reason: None,
    }
}

fn request(
    view_kind: CodebaseViewKind,
    limit: usize,
    changed_paths: Vec<String>,
) -> CodebaseViewRequest {
    CodebaseViewRequest::new(
        CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap(),
        view_kind,
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

fn feature_flag(path: &str, name: &str, source_key: &str) -> CodeFeatureFlagRecord {
    CodeFeatureFlagRecord {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        feature_flag_id: format!("flag:{name}"),
        usage_id: format!("usage:{path}:{name}"),
        file_id: format!("file:{path}"),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        name: name.to_owned(),
        source_kind: "runtime_config".to_owned(),
        source_key: source_key.to_owned(),
        edge_kind: "reads".to_owned(),
        confidence_basis_points: 7600,
        confidence_tier: "extracted".to_owned(),
        byte_range: range(30, 50),
        line_range: range(6, 6),
        excerpt: name.to_owned(),
    }
}

fn route(
    route_id: &str,
    path: &str,
    method: &str,
    url: &str,
    handler_name: &str,
    handler_symbol_snapshot_id: Option<&str>,
) -> CodeRouteRecord {
    CodeRouteRecord {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        route_id: route_id.to_owned(),
        file_id: format!("file:{path}"),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        url: url.to_owned(),
        http_method: method.to_owned(),
        handler_name: handler_name.to_owned(),
        handler_symbol_snapshot_id: handler_symbol_snapshot_id.map(ToOwned::to_owned),
        framework: "fixture".to_owned(),
        line_range: range(4, 4),
    }
}

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
}
