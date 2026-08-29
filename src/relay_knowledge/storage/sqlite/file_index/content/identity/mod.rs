//! Stable content entry, chunk, cursor, and hash identities.

use crate::domain::IndexKind;
pub(super) use crate::identity::stable_hash64;

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

#[cfg(test)]
mod mod_tests;
