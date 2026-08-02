//! Storage contracts and SQLite-backed graph state.
//!
//! Storage owns persisted graph facts, mutation log entries, derived index
//! metadata, and health snapshots. Domain and interface modules must not depend
//! on SQL or concrete database types.

mod contracts;
mod partitioned;
mod sqlite;

pub use contracts::*;
pub use partitioned::PartitionedSqliteKnowledgeStore;
pub use sqlite::SqliteGraphStore;
