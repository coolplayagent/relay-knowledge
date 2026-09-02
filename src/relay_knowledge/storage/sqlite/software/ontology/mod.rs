//! Versioned software ontology occurrences, statements, validation, and queries.

mod materialize;
mod query;
mod schema;

pub(super) use materialize::refresh_projection;
pub(super) use query::{
    diagnostics_for_scope, entities_by_keys_for_scope, entities_for_scope, statements_for_scope,
};
pub(super) use schema::{delete_scope, initialize_schema};
