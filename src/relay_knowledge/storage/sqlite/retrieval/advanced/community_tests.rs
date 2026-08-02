use super::*;

#[test]
fn community_intent_recognizes_summary_terms_without_false_prefixes() {
    assert!(wants_community_summary("global architecture overview"));
    assert!(wants_community_summary("COMMUNITY map"));
    assert!(!wants_community_summary("find a symbol"));
}

#[test]
fn fact_count_rejects_tables_outside_the_owned_allowlist() {
    let connection = Connection::open_in_memory().expect("database should open");

    let error = count_scoped_facts(&connection, "evidence", "repo", 1)
        .expect_err("unowned tables should be rejected");

    assert!(
        matches!(error, StorageError::InvalidInput(message) if message == "unsupported fact table")
    );
}
