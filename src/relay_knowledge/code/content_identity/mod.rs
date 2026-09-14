//! Derives stable content hashes and scoped identifiers for indexed records.

pub(in crate::code) use crate::identity::stable_hash64;

pub(in crate::code) fn stable_content_hash(bytes: &[u8]) -> String {
    format!("{:016x}", stable_hash64(bytes))
}

pub(in crate::code) fn stable_id<'a>(
    prefix: &str,
    parts: impl IntoIterator<Item = &'a str>,
) -> String {
    let mut bytes = Vec::new();
    for part in parts {
        bytes.extend_from_slice(&(part.len() as u64).to_le_bytes());
        bytes.extend_from_slice(part.as_bytes());
    }

    format!("{prefix}:{:016x}", stable_hash64(&bytes))
}
