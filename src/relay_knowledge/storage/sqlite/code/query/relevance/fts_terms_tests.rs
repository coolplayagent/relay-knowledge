use super::{
    append_type_surface_companion_terms, dedupe_terms, identifier_term_has_recall_structure,
    quote_fts_term,
};

#[test]
fn terms_dedupe_case_insensitively_and_escape_fts_quotes() {
    assert_eq!(
        dedupe_terms(vec!["Symbol".to_owned(), "symbol".to_owned()]),
        ["Symbol"]
    );
    assert_eq!(quote_fts_term("say \"hello\""), "\"say \"\"hello\"\"\"");
}

#[test]
fn type_surface_companions_require_type_intent_and_structured_recall() {
    let query_terms = vec![
        "Type".to_owned(),
        "component".to_owned(),
        "metadata".to_owned(),
    ];
    let mut recall_terms = vec!["ComponentType".to_owned()];

    append_type_surface_companion_terms(&query_terms, &mut recall_terms);

    assert_eq!(
        recall_terms,
        ["ComponentType", "component Type", "metadata Type"]
    );
    assert!(identifier_term_has_recall_structure(
        "ComponentTypeMetadata"
    ));
    assert!(!identifier_term_has_recall_structure("Component"));
}
