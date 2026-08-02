use super::fts_query;

#[test]
fn fts_query_keeps_identifier_tokens_and_rejects_empty_input() {
    assert_eq!(
        fts_query("GraphVersion/source_scope"),
        Some("\"GraphVersion\" OR \"source_scope\"".to_owned())
    );
    assert_eq!(fts_query(" / "), None);
}
