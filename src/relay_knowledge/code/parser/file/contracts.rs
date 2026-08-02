//! Shared file-parse inputs, output buffers, and reference deduplication identity.

use std::collections::HashSet;

use crate::{
    code::{SnapshotBuild, languages::LanguageSpec},
    domain::{RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord},
};

pub(in crate::code::parser) struct SyntaxFileInput<'a> {
    pub(in crate::code::parser) path: &'a str,
    pub(in crate::code::parser) file_id: &'a str,
    pub(in crate::code::parser) language: LanguageSpec,
    pub(in crate::code::parser) blob_hash: &'a str,
    pub(in crate::code::parser) byte_len: usize,
    pub(in crate::code::parser) line_count: usize,
    pub(in crate::code::parser) is_generated: bool,
    pub(in crate::code::parser) content: &'a str,
}

pub(in crate::code::parser) struct FileParseContext<'a> {
    pub(in crate::code::parser) build: &'a SnapshotBuild,
    pub(in crate::code::parser) path: &'a str,
    pub(in crate::code::parser) file_id: &'a str,
    pub(in crate::code::parser) language_id: &'a str,
    pub(in crate::code::parser) content: &'a str,
}

pub(in crate::code::parser) struct FileParseOutput {
    pub(in crate::code::parser) symbols: Vec<RepositoryCodeSymbolRecord>,
    pub(in crate::code::parser) references: Vec<RepositoryCodeReferenceRecord>,
    pub(in crate::code::parser) reference_keys: HashSet<ReferenceDedupKey>,
}

pub(in crate::code::parser) type ReferenceDedupKey = (String, String, u32, u32, u32);

impl FileParseOutput {
    pub(in crate::code::parser) fn new() -> Self {
        Self {
            symbols: Vec::new(),
            references: Vec::new(),
            reference_keys: HashSet::new(),
        }
    }
}

#[cfg(test)]
#[path = "contracts_tests.rs"]
mod tests;
