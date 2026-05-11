//! QoS admission policy for inbound and outbound network work.
//!
//! The policy keeps future HTTP, indexing, and background network tasks inside
//! explicit resource budgets before they can allocate unbounded work.

use std::{error::Error, fmt};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}
