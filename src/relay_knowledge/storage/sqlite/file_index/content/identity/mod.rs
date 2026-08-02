//! Stable content entry, chunk, cursor, and hash identities.

use crate::domain::IndexKind;

pub(super) fn chunk_id(entry_key: &str, chunk_index: usize) -> String {
    format!(
        "file-content-chunk:{:016x}:{chunk_index}",
        stable_hash64(entry_key.as_bytes())
    )
}

pub(super) fn cursor_key(kind: IndexKind, scope_id: &str, root_id: &str, path: &str) -> String {
    format!("{}\n{scope_id}\n{root_id}\n{path}", kind.as_str())
}

pub(super) fn entry_key(scope_id: &str, root_id: &str, path: &str) -> String {
    format!("{scope_id}\n{root_id}\n{path}")
}

pub(super) fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}

#[cfg(test)]
mod mod_tests;
