//! Owns stable storage identities and derived evidence source hashes.

use crate::domain::EvidenceExtractionMetadata;

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

pub(super) fn stable_id(prefix: &str, value: &str) -> String {
    let normalized = value.to_lowercase();

    format!("{prefix}:{:016x}", stable_hash64(normalized.as_bytes()))
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}
