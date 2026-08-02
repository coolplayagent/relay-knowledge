//! Software file, topic, and relationship projection persistence.

mod file_role;
mod files;
mod relationships;
mod topics;

pub(super) use files::{files_for_scope, materialize_files};
pub(super) use relationships::{materialize_relationships, relationships_for_scope};
pub(super) use topics::{materialize_topics, topics_for_scope};
