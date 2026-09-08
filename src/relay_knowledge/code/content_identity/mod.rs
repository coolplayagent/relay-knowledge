//! Derives stable content hashes and scoped identifiers for indexed records.

pub(in crate::code) use crate::identity::stable_hash64;

pub(in crate::code) fn stable_content_hash(bytes: &[u8]) -> String {
    format!("{:016x}", stable_hash64(bytes))
}

pub(in crate::code) use crate::identity::stable_id;
