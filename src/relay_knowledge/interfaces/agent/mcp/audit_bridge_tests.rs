use serde_json::json;

use super::{
    audit_freshness, audit_graph_version, audit_limit, audit_result_count, audit_source_scope,
    audit_truncated,
};

#[test]
fn audit_graph_version_reads_common_response_shapes() {
    assert_eq!(
        audit_graph_version(&json!({"metadata": {"graph_version": 7}})),
        7
    );
    assert_eq!(audit_graph_version(&json!({"graph_version": 8})), 8);
    assert_eq!(
        audit_graph_version(&json!({"graph": {"graph_version": 9}})),
        9
    );
    assert_eq!(audit_graph_version(&json!({"error_kind": "timeout"})), 0);
}

#[test]
fn audit_source_scope_reads_repository_set_query_response() {
    assert_eq!(
        audit_source_scope(&json!({"request": {"set_alias": "workspace"}})).as_deref(),
        Some("workspace")
    );
    assert_eq!(
        audit_source_scope(&json!({
            "status": {"repository_set": {"alias": "workspace"}}
        }))
        .as_deref(),
        Some("workspace")
    );
}

#[test]
fn audit_result_count_reads_software_projection_response() {
    assert_eq!(
        audit_result_count(&json!({
            "components": [{"name": "serde"}],
            "relationships": [{"relationship_kind": "configures"}]
        })),
        Some(2)
    );
}

#[test]
fn audit_budget_reads_software_projection_request_shape() {
    let structured = json!({
        "request": {
            "freshness_policy": "wait_until_fresh",
            "limit": 13
        }
    });

    assert_eq!(
        audit_freshness(&structured).as_deref(),
        Some("wait-until-fresh")
    );
    assert_eq!(audit_limit(&structured), Some(13));
}

#[test]
fn audit_reads_codebase_view_budget_shape() {
    let structured = json!({
        "sections": [{"id": "section:one"}, {"id": "section:two"}],
        "budget": {
            "requested_limit": 2,
            "snapshot_row_limit": 40,
            "snapshot_truncated": false,
            "nodes_truncated": true,
            "edges_truncated": false,
            "sections_truncated": false,
            "evidence_truncated": false
        }
    });

    assert_eq!(audit_result_count(&structured), Some(2));
    assert!(audit_truncated(&structured));
}
