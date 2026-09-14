//! Owns stable storage identities and derived evidence source hashes.

use std::io::{self, Write};

use crate::{
    domain::EvidenceExtractionMetadata,
    identity::{StableHasher64, stable_hash64},
};

use super::indexing;

pub(super) fn source_hash_for_evidence(
    extraction: &EvidenceExtractionMetadata,
    source_scope: &str,
    source_path: Option<&str>,
    content: &str,
) -> String {
    extraction
        .source_hash
        .clone()
        .unwrap_or_else(|| indexing::source_hash(source_scope, source_path, content))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;

pub(super) fn stable_id(prefix: &str, value: &str) -> String {
    let normalized = value.to_lowercase();

    stable_bytes_id(prefix, normalized.as_bytes())
}

pub(super) fn stable_bytes_id(prefix: &str, value: &[u8]) -> String {
    format!("{prefix}:{:016x}", stable_hash64(value))
}

/// Incrementally hashes serialized evidence without materializing an unbounded buffer.
pub(super) struct StableIdWriter {
    hasher: StableHasher64,
}

impl StableIdWriter {
    pub(super) const fn new() -> Self {
        Self {
            hasher: StableHasher64::new(),
        }
    }

    pub(super) fn finish(&self, prefix: &str) -> String {
        format!("{prefix}:{:016x}", self.hasher.finish())
    }

    pub(super) fn finish_hex(&self) -> String {
        format!("{:016x}", self.hasher.finish())
    }
}

impl Write for StableIdWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.hasher.update(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
