//! Repository-scoped business knowledge persistence facade.

mod projection;
mod read_model;
mod resolution;
mod row_mapping;
mod schema;

const PROJECTION_SCHEMA_VERSION: i64 = 1;
const AUTHORED_CONFIDENCE: u16 = 10_000;

pub(in crate::storage::sqlite) use projection::{mark_published, replace_projection};
pub(in crate::storage::sqlite) use read_model::{projection_for_scope, status_for_scope};
pub(in crate::storage::sqlite) use resolution::refresh_mapping_resolutions;
pub(in crate::storage::sqlite) use schema::initialize_schema;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
