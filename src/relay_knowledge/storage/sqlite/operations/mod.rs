//! Stable facade for durable operational persistence owners.

mod audit_events;
mod proposals;
mod schema;
mod service_operator;
mod worker_tasks;

pub(super) use audit_events::{audit_event_count, insert_audit_event, query_audit_events};
pub(super) use proposals::{
    decide_proposal, insert_proposal, list_proposals, proposal_by_id, proposal_conflicts,
    proposal_count,
};
pub(super) use schema::initialize_schema;
pub(super) use service_operator::{service_operator_status, update_service_operator};
pub(super) use worker_tasks::{
    claim_worker_task, complete_worker_task, fail_worker_task, queue_worker_tasks, worker_statuses,
};
