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

/// Current QoS usage plus cumulative admission and overload counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QosDiagnosticsSnapshot {
    pub usage: QosSnapshot,
    pub admitted_total: u64,
    pub queued_total: u64,
    pub rejected_total: u64,
    pub timed_out_total: u64,
    pub cancelled_total: u64,
    pub dropped_total: u64,
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

impl RejectReason {
    /// Stable low-cardinality reason label for diagnostics and metrics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConnectionBudgetExceeded => "connection_budget_exceeded",
            Self::RequestBudgetExceeded => "request_budget_exceeded",
            Self::QueueBudgetExceeded => "queue_budget_exceeded",
        }
    }
}

/// Runtime QoS counters used by inbound protocol adapters.
#[derive(Debug, Clone, Default)]
pub struct QosRuntime {
    state: Arc<Mutex<QosState>>,
}

#[derive(Debug, Clone, Copy, Default)]
struct QosState {
    usage: QosSnapshot,
    counters: QosCounters,
}

#[derive(Debug, Clone, Copy, Default)]
struct QosCounters {
    admitted_total: u64,
    queued_total: u64,
    rejected_total: u64,
    timed_out_total: u64,
    cancelled_total: u64,
    dropped_total: u64,
}

impl QosRuntime {
    /// Reserves space in the bounded request queue before active admission.
    pub fn reserve_queue(&self, policy: &QosPolicy) -> Result<QosPermit, RejectReason> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.usage.queued_requests >= policy.max_queue_depth {
            state.record_rejection();
            return Err(RejectReason::QueueBudgetExceeded);
        }

        state.usage.queued_requests += 1;
        state.counters.queued_total = state.counters.queued_total.saturating_add(1);
        Ok(QosPermit {
            runtime: self.clone(),
            kind: QosPermitKind::Queue,
            released: false,
        })
    }

    /// Applies queue and active request budgets atomically for immediate admission.
    pub fn admit_queued_request(&self, policy: &QosPolicy) -> Result<QosPermit, RejectReason> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.usage.queued_requests >= policy.max_queue_depth {
            state.record_rejection();
            return Err(RejectReason::QueueBudgetExceeded);
        }

        if state.usage.in_flight_requests >= policy.max_in_flight_requests {
            state.record_rejection();
            return Err(RejectReason::RequestBudgetExceeded);
        }

        state.usage.in_flight_requests += 1;
        state.counters.queued_total = state.counters.queued_total.saturating_add(1);
        state.counters.admitted_total = state.counters.admitted_total.saturating_add(1);
        Ok(QosPermit {
            runtime: self.clone(),
            kind: QosPermitKind::Request,
            released: false,
        })
    }

    /// Attempts to admit one inbound request and returns a release-on-drop permit.
    pub fn admit_request(&self, policy: &QosPolicy) -> Result<QosPermit, RejectReason> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.usage.in_flight_requests >= policy.max_in_flight_requests {
            state.record_rejection();
            return Err(RejectReason::RequestBudgetExceeded);
        }

        state.usage.in_flight_requests += 1;
        state.counters.admitted_total = state.counters.admitted_total.saturating_add(1);
        Ok(QosPermit {
            runtime: self.clone(),
            kind: QosPermitKind::Request,
            released: false,
        })
    }

    /// Attempts to admit one open network connection.
    pub fn admit_connection(&self, policy: &QosPolicy) -> Result<QosPermit, RejectReason> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.usage.connections >= policy.max_connections {
            state.record_rejection();
            return Err(RejectReason::ConnectionBudgetExceeded);
        }

        state.usage.connections += 1;
        state.counters.admitted_total = state.counters.admitted_total.saturating_add(1);
        Ok(QosPermit {
            runtime: self.clone(),
            kind: QosPermitKind::Connection,
            released: false,
        })
    }

    /// Returns the current request budget snapshot for diagnostics and tests.
    pub fn snapshot(&self) -> QosSnapshot {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .usage
    }

    /// Returns current usage and cumulative overload counters.
    pub fn diagnostics_snapshot(&self) -> QosDiagnosticsSnapshot {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        QosDiagnosticsSnapshot {
            usage: state.usage,
            admitted_total: state.counters.admitted_total,
            queued_total: state.counters.queued_total,
            rejected_total: state.counters.rejected_total,
            timed_out_total: state.counters.timed_out_total,
            cancelled_total: state.counters.cancelled_total,
            dropped_total: state.counters.dropped_total,
        }
    }

    /// Records an operation that exceeded its runtime timeout after admission.
    pub fn record_timed_out(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.counters.timed_out_total = state.counters.timed_out_total.saturating_add(1);
    }

    /// Records an admitted operation cancelled by the caller or peer.
    pub fn record_cancelled(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.counters.cancelled_total = state.counters.cancelled_total.saturating_add(1);
    }

    /// Records work dropped before application handling could start.
    pub fn record_dropped(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.counters.dropped_total = state.counters.dropped_total.saturating_add(1);
    }

    fn release(&self, kind: QosPermitKind) {
        if matches!(kind, QosPermitKind::Noop) {
            return;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match kind {
            QosPermitKind::Connection => {
                state.usage.connections = state.usage.connections.saturating_sub(1);
            }
            QosPermitKind::Request => {
                state.usage.in_flight_requests = state.usage.in_flight_requests.saturating_sub(1);
            }
            QosPermitKind::Queue => {
                state.usage.queued_requests = state.usage.queued_requests.saturating_sub(1);
            }
            QosPermitKind::Noop => {}
        }
    }
}

impl QosState {
    fn record_rejection(&mut self) {
        self.counters.rejected_total = self.counters.rejected_total.saturating_add(1);
    }
}

#[derive(Debug, Clone, Copy)]
enum QosPermitKind {
    Connection,
    Request,
    Queue,
    Noop,
}

/// Admission permit that releases its QoS budget on drop.
#[derive(Debug)]
pub struct QosPermit {
    runtime: QosRuntime,
    kind: QosPermitKind,
    released: bool,
}

impl QosPermit {
    /// Creates a permit for nested work already charged to an outer QoS boundary.
    pub(crate) fn already_admitted(runtime: QosRuntime) -> Self {
        Self {
            runtime,
            kind: QosPermitKind::Noop,
            released: false,
        }
    }
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
#[path = "qos_tests.rs"]
mod tests;
