//! Stable internal facade for checkpointed code-index finalization stages.

mod call_targets;
mod calls;
mod files;
mod imported_references;
mod imports;
pub(super) mod phases;
mod references;
mod search_documents;
mod symbols;

#[cfg(test)]
#[path = "tests/typescript.rs"]
mod typescript_integration_tests;
