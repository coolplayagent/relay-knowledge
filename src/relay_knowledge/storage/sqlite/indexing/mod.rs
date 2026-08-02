mod cursor_metadata;
mod diagnostics;
mod metadata;
mod schema;
mod status;
mod task_queue;

use crate::{domain::IndexModality, storage::DEFAULT_INDEX_SOURCE_SCOPE};

pub(super) use diagnostics::diagnostics;
use diagnostics::{unfinished_task_count, unfinished_task_for_kind_count};
use metadata::{
    append_hash_part, invalid_index_metadata, invalid_to_sqlite, parse_index_kind,
    parse_index_modality, parse_index_state, parse_task_state, stable_hash64,
    validate_required_index_statuses,
};
pub(super) use metadata::{json_array, parse_json_array, source_hash};
pub(super) use schema::initialize_schema;
use status::{
    current_graph_version, ensure_cursor, mark_cursor_complete, mark_cursor_stale_at,
    read_index_status, recompute_aggregate_status,
};
pub(super) use status::{
    index_cursors, index_statuses, mark_mutation_cursors_stale, mark_refresh_complete,
    missing_cursor_scopes,
};
pub(super) use task_queue::{
    claim_index_refresh_task, complete_index_refresh_task, fail_index_refresh_task,
    queue_index_refreshes,
};

pub(super) const DEFAULT_SCOPE: &str = DEFAULT_INDEX_SOURCE_SCOPE;
const TEXT_MODALITY: IndexModality = IndexModality::TEXT;
