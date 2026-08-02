// Direct tests for repository-set member freshness reconciliation.

use super::*;
use crate::domain::{
    CodeRepositorySetMember, CodeRepositorySetMemberStatus, code_snapshot_scope_id,
};

#[test]
fn member_scopes_distinguish_current_legacy_and_custom_fact_versions() {
    let mut current = member_status();
    current.member.source_scope = code_snapshot_scope_id(
        &current.member.repository_id,
        &current.tree_hash,
        &current.member.path_filters,
        &current.member.language_filters,
    );
    let mut legacy = current.clone();
    legacy.member.source_scope = "git_snapshot:0000000000000000".to_owned();
    let mut custom = current.clone();
    custom.member.source_scope = "git_snapshot:fixture".to_owned();

    assert!(member_scope_matches_current_fact_version(&current));
    assert!(member_scope_matches_current_fact_version(&custom));
    assert!(
        fact_version_scope_mismatch_reason(&legacy)
            .is_some_and(|reason| reason.contains("code fact version"))
    );
}

fn member_status() -> CodeRepositorySetMemberStatus {
    CodeRepositorySetMemberStatus {
        member: CodeRepositorySetMember {
            set_id: "set-workspace".to_owned(),
            repository_id: "repo-app".to_owned(),
            repository_alias: "app".to_owned(),
            ref_selector: "HEAD".to_owned(),
            resolved_commit_sha: "abcdef1234".to_owned(),
            source_scope: "placeholder".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            priority: 1,
        },
        tree_hash: "tree-current".to_owned(),
        indexed_path_filters: Vec::new(),
        indexed_language_filters: Vec::new(),
        freshness_state: "fresh".to_owned(),
        stale: false,
        indexed_file_count: 1,
        symbol_count: 1,
        reference_count: 0,
        chunk_count: 1,
        degraded_reason: None,
    }
}
