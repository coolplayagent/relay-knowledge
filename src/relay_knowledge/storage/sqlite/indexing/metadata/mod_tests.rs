//! Direct persistent metadata codec and identity invariants.

use super::*;

#[test]
fn persistent_enum_parsers_accept_stable_values_and_reject_unknown_values() {
    assert_eq!(parse_index_kind("bm25").expect("kind"), IndexKind::Bm25);
    assert_eq!(
        parse_index_modality("layout").expect("modality"),
        IndexModality::Layout
    );
    assert_eq!(
        parse_index_state("paused").expect("state"),
        IndexState::Paused
    );
    assert_eq!(
        parse_task_state("dead_letter").expect("task state"),
        IndexRefreshTaskState::DeadLetter
    );
    assert!(matches!(
        parse_index_kind("unknown"),
        Err(StorageError::InvalidInput(message)) if message.contains("storage metadata")
    ));
}

#[test]
fn json_and_source_hash_identity_are_deterministic_and_boundary_safe() {
    let encoded = json_array(["beta".to_owned(), "alpha".to_owned(), "beta".to_owned()])
        .expect("array should encode");

    assert_eq!(encoded, r#"["alpha","beta"]"#);
    assert_eq!(
        parse_json_array(encoded).expect("array should decode"),
        ["alpha", "beta"]
    );
    assert_eq!(
        source_hash("scope", Some("path"), "body"),
        source_hash("scope", Some("path"), "body")
    );
    assert_ne!(
        source_hash("ab", Some("c"), ""),
        source_hash("a", Some("bc"), "")
    );
}
