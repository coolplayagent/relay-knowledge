// Direct tests for repository-set refresh identities and state transitions.

use super::*;
use crate::domain::{
    CodeRepositorySet, CodeRepositorySetMember, CodeRepositorySetMemberStatus,
    CodeRepositorySetOverlayStatus,
};

#[test]
fn refresh_fingerprint_captures_set_and_member_snapshot_state() {
    let status = status_with_members(vec![
        member_status("app", "scope-app", 1),
        member_status("svc", "scope-svc", 0),
    ]);

    let fingerprint = repository_set_refresh_fingerprint(&status);

    assert!(fingerprint.contains("set-workspace"));
    assert!(fingerprint.contains("repo-app:scope-app:commit-scope-app:tree-scope-app:false"));
}

fn status_with_members(members: Vec<CodeRepositorySetMemberStatus>) -> CodeRepositorySetStatus {
    CodeRepositorySetStatus {
        repository_set: CodeRepositorySet {
            set_id: "set-workspace".to_owned(),
            alias: "workspace".to_owned(),
            description: None,
            default_ref_policy_json: "{\"default_ref\":\"HEAD\"}".to_owned(),
            created_at_ms: 1,
            updated_at_ms: 1,
        },
        members,
        overlay: CodeRepositorySetOverlayStatus {
            state: "fresh".to_owned(),
            stale: false,
            edge_count: 1,
            refreshed_at_ms: Some(10),
            degraded_reason: None,
        },
        freshness_state: "fresh".to_owned(),
        degraded_reason: None,
    }
}

fn member_status(
    repository_alias: &str,
    source_scope: &str,
    priority: i32,
) -> CodeRepositorySetMemberStatus {
    CodeRepositorySetMemberStatus {
        member: CodeRepositorySetMember {
            set_id: "set-workspace".to_owned(),
            repository_id: format!("repo-{repository_alias}"),
            repository_alias: repository_alias.to_owned(),
            ref_selector: "HEAD".to_owned(),
            resolved_commit_sha: format!("commit-{source_scope}"),
            source_scope: source_scope.to_owned(),
            path_filters: vec!["src".to_owned()],
            language_filters: vec!["rust".to_owned()],
            priority,
        },
        tree_hash: format!("tree-{source_scope}"),
        indexed_path_filters: vec!["src".to_owned()],
        indexed_language_filters: vec!["rust".to_owned()],
        freshness_state: "fresh".to_owned(),
        stale: false,
        indexed_file_count: 1,
        symbol_count: 1,
        reference_count: 0,
        chunk_count: 1,
        degraded_reason: None,
    }
}
