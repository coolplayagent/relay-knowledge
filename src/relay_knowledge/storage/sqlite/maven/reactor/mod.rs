//! Snapshot-bound Maven reactor facts and bounded downstream traversal.

mod build;
mod persistence;
mod query;
mod traversal;

pub(in crate::storage::sqlite) use persistence::{initialize_schema, refresh, require_complete};
pub(in crate::storage::sqlite) use query::{includes_language, read_page};
pub(in crate::storage::sqlite) use traversal::downstream;

use crate::domain::{SoftwareBuildTarget, SoftwareRelationship};

const MAX_MODULES: usize = 8_192;
const MAX_EDGES: usize = 131_072;

pub(in crate::storage::sqlite) struct ModuleImpact {
    pub path: String,
    pub line: u32,
    pub chain: String,
}

struct Module {
    target: SoftwareBuildTarget,
    directory: String,
}

struct Edge {
    relationship: SoftwareRelationship,
    dependency_scope: String,
    profile: Option<String>,
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
