//! SQLite local-file metadata, content, lifecycle, search, and diagnostics owners.

pub(super) mod content;
mod diagnostics;
mod retirement;
mod root_update;
mod schema;
mod search;

pub(super) use diagnostics::diagnostics;
pub(super) use retirement::mark_unconfigured_roots;
pub(super) use root_update::replace_root;
pub(super) use schema::initialize_schema;
pub(super) use search::search;

#[cfg(test)]
mod tests;
