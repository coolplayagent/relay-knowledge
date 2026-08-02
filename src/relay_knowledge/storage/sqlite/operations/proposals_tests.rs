use crate::{
    domain::{ProposalConflictSeverity, ProposalKind, ProposalProvenance, ProposalState},
    storage::{
        IndexStore, NewProposal, NewProposalConflict, ProposalDecision, ProposalListRequest,
        SqliteGraphStore,
    },
};

#[tokio::test]
async fn sqlite_proposals_and_conflicts_round_trip() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let proposal = store
        .insert_proposal(NewProposal {
            proposal_id: "proposal:fixture".to_owned(),
            source_scope: "docs".to_owned(),
            kind: ProposalKind::Evidence,
            title: "Derived evidence".to_owned(),
            summary: "OCR output".to_owned(),
            payload_json: "{\"source_scope\":\"docs\",\"evidence\":[]}".to_owned(),
            origin: "worker:ocr".to_owned(),
            provenance: ProposalProvenance {
                producer: "ocr_worker".to_owned(),
                provider: Some("fixture".to_owned()),
                model: Some("fixture-ocr".to_owned()),
                prompt_id: None,
                prompt_version: None,
                schema_version: Some("worker-proposal.v2".to_owned()),
                input_source_hash: Some("sha256:image".to_owned()),
                input_fact_ids: vec!["ev-1".to_owned()],
                stale_when: vec!["parent evidence changes".to_owned()],
                budget_notes: vec!["timeout_ms=30000".to_owned()],
            },
            confidence_basis_points: 7000,
            conflicts: vec![NewProposalConflict {
                conflict_id: "conflict:1".to_owned(),
                existing_fact_kind: "evidence".to_owned(),
                existing_fact_id: "ev-1".to_owned(),
                severity: ProposalConflictSeverity::Blocking,
                reason: "same parent evidence".to_owned(),
            }],
            now_ms: 10,
        })
        .await
        .expect("proposal should insert");

    assert_eq!(proposal.state, ProposalState::Proposed);
    assert_eq!(proposal.conflict_count, 1);
    assert_eq!(proposal.provenance.producer, "ocr_worker");
    assert_eq!(proposal.provenance.input_fact_ids, ["ev-1"]);
    assert_eq!(
        store
            .proposal_count(Some(ProposalState::Proposed))
            .await
            .expect("proposal count should load"),
        1
    );
    assert_eq!(
        store
            .proposal_count(Some(ProposalState::Rejected))
            .await
            .expect("rejected proposal count should load"),
        0
    );

    let listed = store
        .list_proposals(ProposalListRequest {
            state: Some(ProposalState::Proposed),
            limit: 10,
        })
        .await
        .expect("proposal list should load");
    let conflicts = store
        .proposal_conflicts("proposal:fixture".to_owned())
        .await
        .expect("conflicts should load");

    assert_eq!(listed.len(), 1);
    assert_eq!(conflicts[0].severity, ProposalConflictSeverity::Blocking);

    let decided = store
        .decide_proposal(ProposalDecision {
            proposal_id: "proposal:fixture".to_owned(),
            next_state: ProposalState::Rejected,
            actor: "reviewer".to_owned(),
            reason: Some("duplicate".to_owned()),
            now_ms: 20,
        })
        .await
        .expect("proposal should reject");

    assert_eq!(decided.state, ProposalState::Rejected);
    assert_eq!(decided.decided_by.as_deref(), Some("reviewer"));
    assert_eq!(
        store
            .proposal_count(Some(ProposalState::Proposed))
            .await
            .expect("updated proposal count should load"),
        0
    );
    assert_eq!(
        store
            .proposal_count(Some(ProposalState::Rejected))
            .await
            .expect("updated rejected count should load"),
        1
    );
}
