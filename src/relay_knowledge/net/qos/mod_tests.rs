//! Unit contract for bounded QoS policy and runtime admission.

use super::*;

#[test]
fn admits_work_inside_all_budgets() {
    let policy = QosPolicy::new(2, 2, 2).expect("policy should build");

    let decision = policy.evaluate(QosSnapshot {
        connections: 1,
        in_flight_requests: 1,
        queued_requests: 1,
    });

    assert_eq!(decision, AdmissionDecision::Admit);
}

#[test]
fn rejects_when_connection_budget_is_exhausted() {
    let policy = QosPolicy::new(2, 2, 2).expect("policy should build");

    let decision = policy.evaluate(QosSnapshot {
        connections: 2,
        in_flight_requests: 1,
        queued_requests: 1,
    });

    assert_eq!(
        decision,
        AdmissionDecision::Reject(RejectReason::ConnectionBudgetExceeded)
    );
}

#[test]
fn rejects_when_request_budget_is_exhausted() {
    let policy = QosPolicy::new(2, 2, 2).expect("policy should build");

    let decision = policy.evaluate(QosSnapshot {
        connections: 1,
        in_flight_requests: 2,
        queued_requests: 1,
    });

    assert_eq!(
        decision,
        AdmissionDecision::Reject(RejectReason::RequestBudgetExceeded)
    );
}

#[test]
fn rejects_when_queue_budget_is_exhausted() {
    let policy = QosPolicy::new(2, 2, 2).expect("policy should build");

    let decision = policy.evaluate(QosSnapshot {
        connections: 1,
        in_flight_requests: 1,
        queued_requests: 2,
    });

    assert_eq!(
        decision,
        AdmissionDecision::Reject(RejectReason::QueueBudgetExceeded)
    );
}

#[test]
fn rejects_zero_sized_budgets() {
    let error = QosPolicy::new(0, 1, 1).expect_err("zero budget should fail");

    assert_eq!(
        error,
        QosPolicyError {
            field: "max_connections"
        }
    );
}

#[test]
fn permit_releases_in_flight_budget_on_drop() {
    let runtime = QosRuntime::default();
    let policy = QosPolicy::new(2, 1, 1).expect("policy should build");
    let permit = runtime
        .admit_request(&policy)
        .expect("first request should enter");

    assert_eq!(runtime.snapshot().in_flight_requests, 1);
    assert_eq!(
        runtime
            .admit_request(&policy)
            .expect_err("second request should fail"),
        RejectReason::RequestBudgetExceeded
    );

    drop(permit);

    assert_eq!(runtime.snapshot().in_flight_requests, 0);
    assert!(runtime.admit_request(&policy).is_ok());
}

#[test]
fn connection_and_request_permits_update_independent_budgets() {
    let runtime = QosRuntime::default();
    let policy = QosPolicy::new(1, 1, 1).expect("policy should build");
    let connection = runtime
        .admit_connection(&policy)
        .expect("connection should enter");

    assert_eq!(runtime.snapshot().connections, 1);
    assert_eq!(runtime.snapshot().in_flight_requests, 0);
    assert_eq!(
        runtime
            .admit_connection(&policy)
            .expect_err("second connection should fail"),
        RejectReason::ConnectionBudgetExceeded
    );

    let request = runtime
        .admit_request(&policy)
        .expect("request on existing connection should enter");

    assert_eq!(runtime.snapshot().connections, 1);
    assert_eq!(runtime.snapshot().in_flight_requests, 1);

    drop(request);
    assert_eq!(runtime.snapshot().connections, 1);
    assert_eq!(runtime.snapshot().in_flight_requests, 0);

    drop(connection);
    assert_eq!(runtime.snapshot().connections, 0);
}

#[test]
fn queue_permit_tracks_waiting_budget_independently() {
    let runtime = QosRuntime::default();
    let policy = QosPolicy::new(1, 1, 1).expect("policy should build");
    let queued = runtime
        .reserve_queue(&policy)
        .expect("first queued request should enter");

    assert_eq!(runtime.snapshot().queued_requests, 1);
    assert_eq!(
        runtime
            .reserve_queue(&policy)
            .expect_err("second queued request should fail"),
        RejectReason::QueueBudgetExceeded
    );

    drop(queued);
    assert_eq!(runtime.snapshot().queued_requests, 0);
}

#[test]
fn queued_request_admission_preserves_budget_invariants() {
    let runtime = QosRuntime::default();
    let policy = QosPolicy::new(1, 1, 1).expect("policy should build");
    let queued = runtime
        .reserve_queue(&policy)
        .expect("queued request should reserve budget");

    assert_eq!(
        runtime
            .admit_queued_request(&policy)
            .expect_err("full queue should reject admission"),
        RejectReason::QueueBudgetExceeded
    );
    drop(queued);

    let active = runtime
        .admit_queued_request(&policy)
        .expect("request should enter through queued admission");

    assert_eq!(runtime.snapshot().queued_requests, 0);
    assert_eq!(runtime.snapshot().in_flight_requests, 1);
    assert_eq!(
        runtime
            .admit_queued_request(&policy)
            .expect_err("active request budget should reject admission"),
        RejectReason::RequestBudgetExceeded
    );
    assert_eq!(runtime.snapshot().queued_requests, 0);

    drop(active);
    assert_eq!(runtime.snapshot().in_flight_requests, 0);
}

#[test]
fn diagnostics_snapshot_records_admission_and_overload_outcomes() {
    let runtime = QosRuntime::default();
    let policy = QosPolicy::new(1, 1, 1).expect("policy should build");
    let permit = runtime
        .admit_request(&policy)
        .expect("first request should enter");

    assert_eq!(
        runtime
            .admit_request(&policy)
            .expect_err("second request should exceed active budget"),
        RejectReason::RequestBudgetExceeded
    );
    runtime.record_timed_out();
    runtime.record_cancelled();
    runtime.record_dropped();

    let diagnostics = runtime.diagnostics_snapshot();
    assert_eq!(diagnostics.usage.in_flight_requests, 1);
    assert_eq!(diagnostics.admitted_total, 1);
    assert_eq!(diagnostics.rejected_total, 1);
    assert_eq!(diagnostics.timed_out_total, 1);
    assert_eq!(diagnostics.cancelled_total, 1);
    assert_eq!(diagnostics.dropped_total, 1);

    drop(permit);
    assert_eq!(runtime.diagnostics_snapshot().usage.in_flight_requests, 0);
}
