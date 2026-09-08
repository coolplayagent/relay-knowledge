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

#[test]
fn scoped_ids_preserve_length_prefix_encoding_after_owner_move() {
    assert_eq!(
        stable_id("feature_flag", ["repo", "scope", "env_var", "FEATURE_X"]),
        "feature_flag:0bbcb881859c495a"
    );
    assert_ne!(stable_id("key", ["ab", "c"]), stable_id("key", ["a", "bc"]));
}
