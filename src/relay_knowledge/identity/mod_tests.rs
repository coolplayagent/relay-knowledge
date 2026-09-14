use super::*;

#[test]
fn stable_hash_matches_the_persisted_fnv1a_contract() {
    assert_eq!(stable_hash64(b""), 0xcbf29ce484222325);
    assert_eq!(stable_hash64(b"hello"), 0xa430d84680aabd0b);
}

#[test]
fn incremental_hashing_matches_single_buffer_hashing() {
    let mut hasher = StableHasher64::new();
    hasher.update(b"relay-");
    hasher.update(b"knowledge");

    assert_eq!(hasher.finish(), stable_hash64(b"relay-knowledge"));
}
