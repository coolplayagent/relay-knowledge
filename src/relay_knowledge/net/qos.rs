//! QoS admission policy for inbound and outbound network work.
//!
//! The policy keeps HTTP clients, HTTP servers, and background network tasks
//! inside explicit resource budgets before they can allocate unbounded work.

use std::{
    error::Error,
    fmt,
    sync::{Arc, Mutex},
};

use crate::env::NetworkEnvOverrides;

pub const DEFAULT_MAX_CONNECTIONS: usize = 1024;
pub const DEFAULT_MAX_IN_FLIGHT_REQUESTS: usize = 256;
pub const DEFAULT_MAX_QUEUE_DEPTH: usize = 512;

/// Bounded resource policy for network admission decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QosPolicy {
    pub max_connections: usize,
    pub max_in_flight_requests: usize,
    pub max_queue_depth: usize,
}

impl QosPolicy {
    /// Creates a policy and rejects zero-sized budgets.
    pub fn new(
        max_connections: usize,
        max_in_flight_requests: usize,
        max_queue_depth: usize,
    ) -> Result<Self, QosPolicyError> {
        ensure_positive("max_connections", max_connections)?;
        ensure_positive("max_in_flight_requests", max_in_flight_requests)?;
        ensure_positive("max_queue_depth", max_queue_depth)?;

        Ok(Self {
            max_connections,
            max_in_flight_requests,
            max_queue_depth,
        })
    }

    /// Applies environment overrides to the default interactive QoS budgets.
    pub fn from_overrides(overrides: &NetworkEnvOverrides) -> Result<Self, QosPolicyError> {
        Self::new(
            overrides
                .qos_max_connections
                .unwrap_or(DEFAULT_MAX_CONNECTIONS),
            overrides
                .qos_max_in_flight_requests
                .unwrap_or(DEFAULT_MAX_IN_FLIGHT_REQUESTS),
            overrides
                .qos_max_queue_depth
                .unwrap_or(DEFAULT_MAX_QUEUE_DEPTH),
        )
    }

    /// Evaluates current resource usage before admitting network work.
    pub fn evaluate(&self, snapshot: QosSnapshot) -> AdmissionDecision {
        if snapshot.connections >= self.max_connections {
            return AdmissionDecision::Reject(RejectReason::ConnectionBudgetExceeded);
        }

        if snapshot.in_flight_requests >= self.max_in_flight_requests {
            return AdmissionDecision::Reject(RejectReason::RequestBudgetExceeded);
        }

        if snapshot.queued_requests >= self.max_queue_depth {
            return AdmissionDecision::Reject(RejectReason::QueueBudgetExceeded);
        }

        AdmissionDecision::Admit
    }
}

/// Point-in-time network resource usage for QoS decisions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QosSnapshot {
    pub connections: usize,
    pub in_flight_requests: usize,
    pub queued_requests: usize,
}

/// Result of a QoS admission check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionDecision {
    Admit,
    Reject(RejectReason),
}

/// Reason attached to a rejected network operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    ConnectionBudgetExceeded,
    RequestBudgetExceeded,
    QueueBudgetExceeded,
}

/// Runtime QoS counters used by inbound protocol adapters.
#[derive(Debug, Clone, Default)]
pub struct QosRuntime {
    usage: Arc<Mutex<QosSnapshot>>,
}

impl QosRuntime {
    /// Reserves space in the bounded request queue before active admission.
    pub fn reserve_queue(&self, policy: &QosPolicy) -> Result<QosPermit, RejectReason> {
        let mut usage = self
            .usage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if usage.queued_requests >= policy.max_queue_depth {
            return Err(RejectReason::QueueBudgetExceeded);
        }

        usage.queued_requests += 1;
        Ok(QosPermit {
            runtime: self.clone(),
            kind: QosPermitKind::Queue,
            released: false,
        })
    }

    /// Applies queue and active request budgets atomically for immediate admission.
    pub fn admit_queued_request(&self, policy: &QosPolicy) -> Result<QosPermit, RejectReason> {
        let mut usage = self
            .usage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if usage.queued_requests >= policy.max_queue_depth {
            return Err(RejectReason::QueueBudgetExceeded);
        }

        if usage.in_flight_requests >= policy.max_in_flight_requests {
            return Err(RejectReason::RequestBudgetExceeded);
        }

        usage.in_flight_requests += 1;
        Ok(QosPermit {
            runtime: self.clone(),
            kind: QosPermitKind::Request,
            released: false,
        })
    }

    /// Attempts to admit one inbound request and returns a release-on-drop permit.
    pub fn admit_request(&self, policy: &QosPolicy) -> Result<QosPermit, RejectReason> {
        let mut usage = self
            .usage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if usage.in_flight_requests >= policy.max_in_flight_requests {
            return Err(RejectReason::RequestBudgetExceeded);
        }

        usage.in_flight_requests += 1;
        Ok(QosPermit {
            runtime: self.clone(),
            kind: QosPermitKind::Request,
            released: false,
        })
    }

    /// Attempts to admit one open network connection.
    pub fn admit_connection(&self, policy: &QosPolicy) -> Result<QosPermit, RejectReason> {
        let mut usage = self
            .usage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if usage.connections >= policy.max_connections {
            return Err(RejectReason::ConnectionBudgetExceeded);
        }

        usage.connections += 1;
        Ok(QosPermit {
            runtime: self.clone(),
            kind: QosPermitKind::Connection,
            released: false,
        })
    }

    /// Returns the current request budget snapshot for diagnostics and tests.
    pub fn snapshot(&self) -> QosSnapshot {
        *self
            .usage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn release(&self, kind: QosPermitKind) {
        let mut usage = self
            .usage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match kind {
            QosPermitKind::Connection => {
                usage.connections = usage.connections.saturating_sub(1);
            }
            QosPermitKind::Request => {
                usage.in_flight_requests = usage.in_flight_requests.saturating_sub(1);
            }
            QosPermitKind::Queue => {
                usage.queued_requests = usage.queued_requests.saturating_sub(1);
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum QosPermitKind {
    Connection,
    Request,
    Queue,
}

/// Admission permit that releases its QoS budget on drop.
#[derive(Debug)]
pub struct QosPermit {
    runtime: QosRuntime,
    kind: QosPermitKind,
    released: bool,
}

impl Drop for QosPermit {
    fn drop(&mut self) {
        if !self.released {
            self.runtime.release(self.kind);
            self.released = true;
        }
    }
}

/// QoS policy validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QosPolicyError {
    pub field: &'static str,
}

impl fmt::Display for QosPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} must be greater than zero", self.field)
    }
}

impl Error for QosPolicyError {}

fn ensure_positive(field: &'static str, value: usize) -> Result<(), QosPolicyError> {
    if value == 0 {
        return Err(QosPolicyError { field });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
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
}
