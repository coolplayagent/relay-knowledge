use super::*;
use crate::domain::feedback::fixtures::report;

fn run() -> FeedbackValidation {
    let reproduction = report().reproduction.unwrap();
    FeedbackValidation {
        runner: "fixture-runner".into(),
        scenario_digest: reproduction.digest(),
        actual: reproduction.expected,
        version: "1.1.19".into(),
        environment: "fixed fixture; local retrieval".into(),
        run_id: "ci-run-42".into(),
        elapsed_ms: Some(10),
        steps: Some(1),
    }
}

fn fix() -> FeedbackFix {
    FeedbackFix {
        reference: "owner/repo#42".into(),
        target_version: "1.1.19".into(),
    }
}

#[test]
fn compares_original_criterion_instead_of_a_claimed_pass_boolean() {
    let result = validate_regression(&report(), &fix(), run(), 1234).unwrap();
    assert!(result.passed);
    assert_eq!(result.recorded_at_ms, 1234);
    let mut regression = run();
    regression.actual = "map version increases by ten".into();
    assert!(
        !validate_regression(&report(), &fix(), regression, 1235)
            .unwrap()
            .passed
    );
    let mut untrusted = serde_json::to_value(run()).unwrap();
    untrusted["passed"] = true.into();
    assert!(serde_json::from_value::<FeedbackValidation>(untrusted).is_err());
}

#[test]
fn rejects_validation_without_original_scenario_fix_or_matching_snapshot() {
    let mut source = report();
    source.reproduction = None;
    assert!(
        validate_regression(&source, &fix(), run(), 0)
            .unwrap_err()
            .to_string()
            .contains("awaiting-validation")
    );
    let mut wrong = run();
    wrong.scenario_digest = "unrelated".into();
    assert!(validate_regression(&report(), &fix(), wrong, 0).is_err());
    wrong = run();
    wrong.version = "1.1.18".into();
    assert!(validate_regression(&report(), &fix(), wrong, 0).is_err());
    source = report();
    source.intent.clear();
    assert!(validate_regression(&source, &fix(), run(), 0).is_err());
    let mut invalid_fix = fix();
    invalid_fix.reference.clear();
    assert!(validate_regression(&report(), &invalid_fix, run(), 0).is_err());
}

#[test]
fn requires_bounded_result_and_environment_provenance() {
    let mut missing = run();
    missing.actual.clear();
    assert!(validate_regression(&report(), &fix(), missing, 0).is_err());
    missing = run();
    missing.environment.clear();
    assert!(validate_regression(&report(), &fix(), missing, 0).is_err());
    missing = run();
    missing.run_id = "r".repeat(257);
    assert!(validate_regression(&report(), &fix(), missing, 0).is_err());
}

#[test]
fn scenario_identity_is_framed_and_includes_expected_result() {
    let first = FeedbackReproduction {
        scenario: "ab".into(),
        expected: "c".into(),
    };
    let second = FeedbackReproduction {
        scenario: "a".into(),
        expected: "bc".into(),
    };
    assert_ne!(first.digest(), second.digest());
    assert_eq!(first.digest().len(), 64);
}
