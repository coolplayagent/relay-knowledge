use super::{compound_identifier_fts_terms, compound_identifier_source_term};

#[test]
fn compound_terms_generate_bounded_compact_and_snake_alternatives() {
    let alternatives = compound_identifier_fts_terms(&[
        "repository".to_owned(),
        "scope".to_owned(),
        "status".to_owned(),
    ]);

    assert!(alternatives.contains(&"repositoryscopestatus".to_owned()));
    assert!(alternatives.contains(&"repository_scope_status".to_owned()));
    assert!(alternatives.len() <= 24);
}

#[test]
fn compound_source_terms_reject_path_and_member_punctuation() {
    assert!(compound_identifier_source_term("repository_scope"));
    assert!(!compound_identifier_source_term("src/repository"));
    assert!(!compound_identifier_source_term("repository.scope"));
}
