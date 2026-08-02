// Direct tests for repository-set status and freshness aggregation.

use std::sync::Arc;

use super::*;
use crate::{
    api::ErrorKind,
    domain::{
        CodeRepositorySet, CodeRepositorySetMember, CodeRepositorySetMemberStatus,
        CodeRepositorySetOverlayStatus,
    },
    storage::SqliteGraphStore,
};

#[tokio::test]
async fn required_status_reports_missing_sets() {
    let store: Arc<dyn crate::storage::KnowledgeStore> =
        Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let error = required_set_status(&store, "missing")
        .await
        .expect_err("missing set should fail");

    assert_eq!(error.error_kind, ErrorKind::InvalidArgument);
    assert!(error.message.contains("is not registered"));
}

#[test]
fn pinned_commit_refs_do_not_track_repository_movement() {
    assert!(!member_ref_tracks_repository("abcdef1", "abcdef1234"));
    assert!(!member_ref_tracks_repository("abcdef1234", "abcdef1234"));
    assert!(member_ref_tracks_repository("HEAD", "abcdef1234"));
}

#[test]
fn freshness_aggregation_marks_member_and_overlay_stale() {
    let mut status = status_with_members(vec![member_status()], overlay(false));
    status.members[0].stale = true;
    status.members[0].degraded_reason = Some("member stale".to_owned());

    refresh_repository_set_freshness(&mut status);

    assert_eq!(status.freshness_state, "stale");
    assert_eq!(status.degraded_reason.as_deref(), Some("member stale"));
    assert!(status.overlay.stale);
    assert_eq!(status.overlay.state, "overlay_stale");
}

fn status_with_members(
    members: Vec<CodeRepositorySetMemberStatus>,
    overlay: CodeRepositorySetOverlayStatus,
) -> CodeRepositorySetStatus {
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
        overlay,
        freshness_state: "fresh".to_owned(),
        degraded_reason: None,
    }
}

fn member_status() -> CodeRepositorySetMemberStatus {
    CodeRepositorySetMemberStatus {
        member: CodeRepositorySetMember {
            set_id: "set-workspace".to_owned(),
            repository_id: "repo-app".to_owned(),
            repository_alias: "app".to_owned(),
            ref_selector: "HEAD".to_owned(),
            resolved_commit_sha: "abcdef1234".to_owned(),
            source_scope: "scope-app".to_owned(),
            path_filters: vec!["src".to_owned()],
            language_filters: vec!["rust".to_owned()],
            priority: 1,
        },
        tree_hash: "tree-app".to_owned(),
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

fn overlay(stale: bool) -> CodeRepositorySetOverlayStatus {
    CodeRepositorySetOverlayStatus {
        state: if stale { "overlay_stale" } else { "fresh" }.to_owned(),
        stale,
        edge_count: usize::from(!stale),
        refreshed_at_ms: (!stale).then_some(10),
        degraded_reason: None,
    }
}
