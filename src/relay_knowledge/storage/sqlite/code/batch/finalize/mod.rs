//! Stable internal facade for checkpointed code-index finalization stages.

mod call_targets;
mod calls;
mod files;
mod imported_references;
mod imports;
mod pages;
pub(in crate::storage::sqlite::code) mod phases;
pub(in crate::storage::sqlite::code::batch) mod references;
pub(super) mod search_documents;
mod symbols;

#[cfg(test)]
#[path = "tests/typescript.rs"]
mod typescript_integration_tests;
