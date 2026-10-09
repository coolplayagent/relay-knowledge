use super::*;
use crate::domain::feedback::{
    FeedbackEvidence, FeedbackKind, FeedbackObservation, FeedbackObservationOrigin,
    fixtures::report,
};

#[test]
fn publication_requires_explicit_repository_and_allowed_kind_scope() {
    let mut policy = FeedbackPolicy::default();
    assert_eq!(policy.mode, FeedbackMode::LocalOnly);
    assert!(policy.validate().is_ok());
    policy.mode = FeedbackMode::AutoSubmit;
    assert!(policy.validate().is_err());
    policy.target_repository = Some("owner/repository".into());
    assert!(policy.validate().is_err());
    policy.allowed_kinds = vec![FeedbackKind::Bug];
    assert!(policy.validate().is_ok());
    for repository in [
        "owner",
        "/repo",
        "owner/",
        "../repo",
        "owner/..",
        "owner/.",
        "owner/a/b",
        "owner/repo?x",
        "owner/repo@evil",
    ] {
        policy.target_repository = Some(repository.into());
        assert!(policy.validate().is_err(), "{repository}");
    }
    policy.target_repository = Some(format!("{}/repo", "x".repeat(101)));
    assert!(policy.validate().is_err());
    policy.target_repository = Some("Owner_1/repo-name.2".into());
    assert!(policy.validate().is_ok());
}

#[test]
fn policy_rejects_unbounded_or_ambiguous_authorization() {
    let mut policy = FeedbackPolicy {
        validation_runner: Some(" ".into()),
        ..FeedbackPolicy::default()
    };
    assert!(policy.validate().is_err());
    policy.validation_runner = Some("fixture-runner".into());
    policy.schema_version = 2;
    assert!(policy.validate().is_err());
    policy.schema_version = 1;
    for quota in [0, 101] {
        policy.daily_quota = quota;
        assert!(policy.validate().is_err());
    }
    policy.daily_quota = 100;
    policy.allowed_kinds = vec![FeedbackKind::Bug; 7];
    assert!(policy.validate().is_err());
    policy.allowed_kinds.truncate(2);
    assert!(policy.validate().is_err());
}

#[test]
fn report_bounds_every_public_field_and_local_evidence() {
    assert!(report().validate().is_ok());
    let mut invalid = report();
    invalid.schema_version = 0;
    assert!(invalid.validate().is_err());
    invalid = report();
    invalid.intent.clear();
    assert!(invalid.validate().is_err());
    invalid = report();
    invalid.expected = " ".into();
    assert!(invalid.validate().is_err());
    invalid = report();
    invalid.actual = "a".repeat(4097);
    assert!(invalid.validate().is_err());
    invalid = report();
    invalid.impact = "contains\0NUL".into();
    assert!(invalid.validate().is_err());
    invalid = report();
    invalid.observations = vec![
        FeedbackObservation {
            origin: FeedbackObservationOrigin::Hypothesis,
            text: "Maybe".into()
        };
        17
    ];
    assert!(invalid.validate().is_err());
    invalid.observations.truncate(1);
    invalid.observations[0].text.clear();
    assert!(invalid.validate().is_err());
    invalid = report();
    invalid.evidence = vec![
        FeedbackEvidence {
            label: "raw".into(),
            content: "log".into()
        };
        17
    ];
    assert!(invalid.validate().is_err());
    invalid.evidence.truncate(1);
    invalid.evidence[0].label.clear();
    assert!(invalid.validate().is_err());
    invalid.evidence[0].label = "raw".into();
    invalid.evidence[0].content = "a".repeat(16385);
    assert!(invalid.validate().is_err());
    invalid.evidence[0].content = "a".repeat(16384);
    invalid.evidence = vec![invalid.evidence[0].clone(); 5];
    assert!(invalid.validate().is_err());
}

#[test]
fn report_checks_optional_trace_and_reproduction_bounds() {
    for set_field in [true, false] {
        let mut invalid = report();
        if set_field {
            invalid.trace_id = Some("x".repeat(257));
        } else {
            invalid.request_id = Some(String::new());
        }
        assert!(invalid.validate().is_err());
    }
    let mut invalid = report();
    invalid.reproduction.as_mut().unwrap().scenario.clear();
    assert!(invalid.validate().is_err());
    invalid = report();
    invalid.reproduction.as_mut().unwrap().expected.clear();
    assert!(invalid.validate().is_err());
}

#[test]
fn occurrence_metadata_does_not_defeat_scenario_deduplication() {
    let original = report();
    let mut repeated = original.clone();
    repeated.trace_id = Some("trace-next".into());
    repeated.request_id = Some("request-next".into());
    repeated.evidence.push(FeedbackEvidence {
        label: "retry log".into(),
        content: "Attempt two".into(),
    });
    assert_eq!(
        original.fingerprint("1.1.18").unwrap(),
        repeated.fingerprint("1.1.18").unwrap()
    );
    assert_ne!(
        original.fingerprint("1.1.18").unwrap(),
        original.fingerprint("1.1.19").unwrap()
    );
    repeated.actual = "No map publication".into();
    assert_ne!(
        original.fingerprint("1.1.18").unwrap(),
        repeated.fingerprint("1.1.18").unwrap()
    );
    repeated.intent.clear();
    assert!(repeated.fingerprint("1.1.18").is_err());
}

#[test]
fn corrected_public_observations_and_impact_get_new_feedback_identity() {
    let mut source = report();
    source.observations.push(FeedbackObservation {
        origin: FeedbackObservationOrigin::AgentObservation,
        text: "Private user person@example.test encountered the problem".into(),
    });
    let previous = source.fingerprint("1.1.18").unwrap();
    source.observations[0].text = "An isolated test user encountered the problem".into();
    assert_ne!(previous, source.fingerprint("1.1.18").unwrap());
    let corrected = source.fingerprint("1.1.18").unwrap();
    source.impact = "The next batch cannot proceed".into();
    assert_ne!(corrected, source.fingerprint("1.1.18").unwrap());
}

#[test]
fn unknown_input_fields_cannot_override_policy_or_claim_validation_pass() {
    let mut value = serde_json::to_value(report()).unwrap();
    value["target_repository"] = "attacker/repository".into();
    assert!(serde_json::from_value::<FeedbackReport>(value).is_err());
    let mut policy = serde_json::to_value(FeedbackPolicy::default()).unwrap();
    policy["execute"] = "run arbitrary text".into();
    assert!(serde_json::from_value::<FeedbackPolicy>(policy).is_err());
}

#[test]
fn fix_requires_reference_and_target_version() {
    let mut fix = FeedbackFix {
        reference: String::new(),
        target_version: "1.2.0".into(),
    };
    assert!(fix.validate().is_err());
    fix.reference = "owner/repository#42".into();
    fix.target_version.clear();
    assert!(fix.validate().is_err());
    fix.target_version = "1.2.0".into();
    assert!(fix.validate().is_ok());
}

#[test]
fn packaged_feedback_example_uses_the_production_contract() {
    let source = include_str!(
        "../../../../../skills/relay-knowledge-cli/references/feedback-report.example.json"
    );
    let report: FeedbackReport = serde_json::from_str(source).unwrap();
    assert_eq!(report.kind, FeedbackKind::WorkflowFriction);
    assert!(report.validate().is_ok());
    assert!(
        super::super::prepare_payload(&report, "1.1.18", "linux", super::super::fixtures::MARKER)
            .is_ok()
    );
}
