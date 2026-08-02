//! Direct tests for runtime operation contracts.

use super::*;

#[test]
fn operational_enums_have_stable_storage_values() {
    for (kind, value) in [
        (WorkerKind::Embedding, "embedding"),
        (WorkerKind::Ocr, "ocr"),
        (WorkerKind::Vision, "vision"),
        (WorkerKind::Extractor, "extractor"),
    ] {
        assert_eq!(kind.as_str(), value);
        assert_eq!(WorkerKind::parse(value).expect("worker kind"), kind);
    }
    for (state, value) in [
        (WorkerTaskState::Queued, "queued"),
        (WorkerTaskState::Running, "running"),
        (WorkerTaskState::Succeeded, "succeeded"),
        (WorkerTaskState::Retrying, "retrying"),
        (WorkerTaskState::Failed, "failed"),
        (WorkerTaskState::DeadLetter, "dead_letter"),
    ] {
        assert_eq!(state.as_str(), value);
        assert_eq!(WorkerTaskState::parse(value).expect("task state"), state);
    }
    for (state, value) in [
        (WorkerBackendState::Fallback, "fallback"),
        (WorkerBackendState::Configured, "configured"),
        (WorkerBackendState::Degraded, "degraded"),
        (WorkerBackendState::Unavailable, "unavailable"),
    ] {
        assert_eq!(state.as_str(), value);
    }
}

#[test]
fn proposal_audit_and_operator_enums_have_stable_values() {
    for (kind, value) in [
        (ProposalKind::Evidence, "evidence"),
        (ProposalKind::Relation, "relation"),
        (ProposalKind::Claim, "claim"),
        (ProposalKind::Event, "event"),
    ] {
        assert_eq!(kind.as_str(), value);
        assert_eq!(ProposalKind::parse(value).expect("proposal kind"), kind);
    }
    for (state, value) in [
        (ProposalState::Proposed, "proposed"),
        (ProposalState::Accepted, "accepted"),
        (ProposalState::Rejected, "rejected"),
        (ProposalState::Superseded, "superseded"),
    ] {
        assert_eq!(state.as_str(), value);
        assert_eq!(ProposalState::parse(value).expect("proposal state"), state);
    }
    for (severity, value) in [
        (ProposalConflictSeverity::Info, "info"),
        (ProposalConflictSeverity::Warning, "warning"),
        (ProposalConflictSeverity::Blocking, "blocking"),
    ] {
        assert_eq!(severity.as_str(), value);
        assert_eq!(
            ProposalConflictSeverity::parse(value).expect("conflict severity"),
            severity
        );
    }
    for (status, value) in [
        (AuditStatus::Started, "started"),
        (AuditStatus::Completed, "completed"),
        (AuditStatus::Failed, "failed"),
        (AuditStatus::Cancelled, "cancelled"),
    ] {
        assert_eq!(status.as_str(), value);
        assert_eq!(AuditStatus::parse(value).expect("audit status"), status);
    }
    for (state, value) in [
        (ServiceOperatorState::Disabled, "disabled"),
        (ServiceOperatorState::Enabled, "enabled"),
        (ServiceOperatorState::Paused, "paused"),
        (ServiceOperatorState::Degraded, "degraded"),
        (ServiceOperatorState::Failed, "failed"),
    ] {
        assert_eq!(state.as_str(), value);
        assert_eq!(
            ServiceOperatorState::parse(value).expect("operator state"),
            state
        );
    }
    for (action, value) in [
        (ServiceManagerAction::Install, "install"),
        (ServiceManagerAction::Upgrade, "upgrade"),
        (ServiceManagerAction::Rollback, "rollback"),
        (ServiceManagerAction::Uninstall, "uninstall"),
    ] {
        assert_eq!(action.as_str(), value);
        assert_eq!(
            ServiceManagerAction::parse(value).expect("service action"),
            action
        );
    }
}

#[test]
fn invalid_operational_values_are_rejected_or_redacted() {
    assert!(WorkerKind::parse("gpu").is_err());
    assert!(ProposalState::parse("merged").is_err());
    assert!(AuditStatus::parse("pending").is_err());
    assert!(ServiceManagerAction::parse("restart").is_err());
    assert!(normalize_actor("  ").is_err());

    let proposal = ProposalRecord {
        proposal_id: "proposal:test".to_owned(),
        source_scope: "docs".to_owned(),
        kind: ProposalKind::Evidence,
        state: ProposalState::Proposed,
        title: "title".to_owned(),
        summary: "summary".to_owned(),
        payload_json: "{".to_owned(),
        origin: "test".to_owned(),
        provenance: ProposalProvenance::new("test"),
        confidence_basis_points: 1,
        conflict_count: 0,
        decided_by: None,
        decision_reason: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    };

    assert!(proposal.payload_value().is_null());
}

#[test]
fn proposal_provenance_normalizes_and_validates_lineage() {
    let provenance = ProposalProvenance {
        producer: " llm_spo_extraction ".to_owned(),
        provider: Some(" openai-compatible ".to_owned()),
        model: Some(" graph-extractor ".to_owned()),
        prompt_id: Some(" relay.extract.spo ".to_owned()),
        prompt_version: Some(" 1 ".to_owned()),
        schema_version: Some(" worker-proposal.v2 ".to_owned()),
        input_source_hash: Some(" sha256:source ".to_owned()),
        input_fact_ids: vec![" ev-1 ".to_owned(), "ev-1".to_owned()],
        stale_when: vec![" graph_version_advances ".to_owned()],
        budget_notes: vec![" timeout_ms=30000 ".to_owned()],
    }
    .validate()
    .expect("provenance should validate");

    assert_eq!(provenance.producer, "llm_spo_extraction");
    assert_eq!(provenance.input_fact_ids, ["ev-1"]);
    assert_eq!(
        ProposalProvenance::from_json(&provenance.to_json())
            .expect("stored provenance should parse"),
        provenance
    );
    assert_eq!(
        ProposalProvenance::from_json("{}")
            .expect("legacy provenance should default")
            .producer,
        "unspecified"
    );
    assert_eq!(
        ProposalProvenance::new(" ")
            .validate()
            .expect_err("empty producer should fail")
            .field,
        "proposal_producer"
    );
}
