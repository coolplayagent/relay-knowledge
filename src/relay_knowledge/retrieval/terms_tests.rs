use super::*;

#[test]
fn normalized_terms_split_mixed_identifiers_and_preserve_whole_tokens() {
    let terms = normalized_terms("GraphRAGContextPack retry_policy RESTClient W3Connector", 2);

    for term in [
        "graphragcontextpack",
        "graph",
        "rag",
        "context",
        "pack",
        "grcp",
        "retry_policy",
        "retry",
        "policy",
        "rest",
        "client",
        "w3connector",
        "connector",
        "w3c",
    ] {
        assert!(terms.contains(term), "missing term {term}");
    }
    assert!(!terms.contains("w"));
    assert!(!terms.contains("3"));
}

#[test]
fn normalized_terms_can_keep_single_character_terms_for_rerank() {
    let terms = normalized_terms("C API W3", 1);

    assert!(terms.contains("c"));
    assert!(terms.contains("api"));
    assert!(terms.contains("w"));
    assert!(terms.contains("3"));
}

#[test]
fn extend_normalized_terms_matches_owned_collection() {
    let mut extended = BTreeSet::from(["existing".to_owned()]);
    extend_normalized_terms("GraphRAGContextPack retry_policy", 2, &mut extended);

    let mut expected = normalized_terms("GraphRAGContextPack retry_policy", 2);
    expected.insert("existing".to_owned());

    assert_eq!(extended, expected);
}

#[test]
fn extract_identifiers_pascal_case() {
    let ids = extract_identifiers("UserService");
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0].original, "UserService");
    assert_eq!(ids[0].kind, IdentifierKind::PascalCase);
    assert!(ids[0].parts.contains(&"user".to_owned()));
    assert!(ids[0].parts.contains(&"service".to_owned()));
    assert!(ids[0].weight > 1.0);
}

#[test]
fn extract_identifiers_camel_case() {
    let ids = extract_identifiers("signInWithGoogle");
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0].original, "signInWithGoogle");
    assert_eq!(ids[0].kind, IdentifierKind::CamelCase);
    assert!(ids[0].parts.contains(&"sign".to_owned()));
    assert!(ids[0].parts.contains(&"google".to_owned()));
    assert!(!ids[0].parts.contains(&"in".to_owned()));
    assert!(!ids[0].parts.contains(&"with".to_owned()));
}

#[test]
fn extract_identifiers_snake_case() {
    let ids = extract_identifiers("max_retries");
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0].original, "max_retries");
    assert_eq!(ids[0].kind, IdentifierKind::SnakeCase);
    assert_eq!(ids[0].parts, vec!["max", "retries"]);
}

#[test]
fn extract_identifiers_screaming_snake_case() {
    let ids = extract_identifiers("MAX_RETRIES");
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0].original, "MAX_RETRIES");
    assert_eq!(ids[0].kind, IdentifierKind::ScreamingSnakeCase);
    assert_eq!(ids[0].parts, vec!["max", "retries"]);
}

#[test]
fn extract_identifiers_dot_notation() {
    let ids = extract_identifiers("app.isPackaged");
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0].original, "app.isPackaged");
    assert_eq!(ids[0].kind, IdentifierKind::DotNotation);
    assert!(ids[0].parts.contains(&"app".to_owned()));
    assert!(ids[0].parts.contains(&"ispackaged".to_owned()));
}

#[test]
fn extract_identifiers_all_caps() {
    let ids = extract_identifiers("HTTP");
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0].original, "HTTP");
    assert_eq!(ids[0].kind, IdentifierKind::AllCaps);
}

#[test]
fn extract_identifiers_lowercase() {
    let ids = extract_identifiers("render");
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0].original, "render");
    assert_eq!(ids[0].kind, IdentifierKind::Lowercase);
}

#[test]
fn extract_identifiers_natural_language_query() {
    let ids = extract_identifiers("how does UserService handle signInWithGoogle authentication");
    let originals: Vec<&str> = ids.iter().map(|id| id.original.as_str()).collect();
    assert!(originals.contains(&"UserService"));
    assert!(originals.contains(&"signInWithGoogle"));
    assert!(originals.contains(&"authentication"));
    assert!(!originals.contains(&"how"));
    assert!(!originals.contains(&"does"));
}

#[test]
fn stop_word_filtering() {
    assert!(is_stop_word("the"));
    assert!(is_stop_word("and"));
    assert!(is_stop_word("for"));
    assert!(is_stop_word("with"));
    assert!(is_stop_word("how"));
    assert!(is_stop_word("what"));
    assert!(!is_stop_word("handle"));
    assert!(!is_stop_word("service"));
}

#[test]
fn stop_word_count_covers_at_least_80() {
    assert!(STOP_WORDS.len() >= 80);
}

#[test]
fn stem_variants_connecting() {
    let variants = stem_variants("connecting");
    assert!(variants.contains(&"connect".to_owned()));
    assert!(!variants.contains(&"connecte".to_owned()));
}

#[test]
fn stem_variants_connected() {
    let variants = stem_variants("connected");
    assert!(variants.contains(&"connect".to_owned()));
}

#[test]
fn stem_variants_renderer() {
    let variants = stem_variants("renderer");
    assert!(variants.contains(&"render".to_owned()));
    assert!(variants.contains(&"rendere".to_owned()));
}

#[test]
fn stem_variants_running() {
    let variants = stem_variants("running");
    assert!(variants.contains(&"run".to_owned()));
    assert!(variants.contains(&"runn".to_owned()));
}

#[test]
fn stem_variants_parsed() {
    let variants = stem_variants("parsed");
    assert!(variants.contains(&"parse".to_owned()));
    assert!(variants.contains(&"pars".to_owned()));
}

#[test]
fn identifier_parts_handles_embedded_acronyms() {
    let ids = extract_identifiers("parseXMLFile");
    assert_eq!(ids.len(), 1);
    assert!(ids[0].parts.contains(&"parse".to_owned()));
    assert!(ids[0].parts.contains(&"xml".to_owned()));
    assert!(ids[0].parts.contains(&"file".to_owned()));
}

#[test]
fn stem_variants_collections() {
    let variants = stem_variants("collections");
    assert!(variants.contains(&"collection".to_owned()));
}

#[test]
fn stem_variants_short_words_ignored() {
    assert!(stem_variants("go").is_empty());
    assert!(stem_variants("do").is_empty());
}

#[test]
fn identifier_weight_pascal_higher_than_lowercase() {
    let pascal = extract_identifiers("UserService");
    let lower = extract_identifiers("service");
    assert!(!pascal.is_empty());
    assert!(!lower.is_empty());
    assert!(pascal[0].weight > lower[0].weight);
}

#[test]
fn extract_query_identifiers_includes_stem_variants() {
    let ids = extract_query_identifiers("connecting renderer");
    let connecting = ids.iter().find(|id| id.original == "connecting").unwrap();
    assert!(connecting.parts.contains(&"connect".to_owned()));
    assert!(!connecting.parts.contains(&"connecte".to_owned()));
    let renderer = ids.iter().find(|id| id.original == "renderer").unwrap();
    assert!(renderer.parts.contains(&"render".to_owned()));
}

#[test]
fn classify_token_patterns() {
    assert_eq!(
        classify_token("UserService"),
        Some(IdentifierKind::PascalCase)
    );
    assert_eq!(classify_token("signIn"), Some(IdentifierKind::CamelCase));
    assert_eq!(
        classify_token("max_retries"),
        Some(IdentifierKind::SnakeCase)
    );
    assert_eq!(
        classify_token("MAX_RETRIES"),
        Some(IdentifierKind::ScreamingSnakeCase)
    );
    assert_eq!(classify_token("REST"), Some(IdentifierKind::AllCaps));
    assert_eq!(
        classify_token("app.init"),
        Some(IdentifierKind::DotNotation)
    );
    assert_eq!(classify_token("render"), Some(IdentifierKind::Lowercase));
    assert_eq!(classify_token(""), None);
}

#[test]
fn extract_identifiers_mixed_query() {
    let ids = extract_identifiers(
        "UserService retry_policy REST MAX_RETRIES API_KEY app.isPackaged render parse",
    );
    let originals: Vec<&str> = ids.iter().map(|id| id.original.as_str()).collect();
    assert!(originals.contains(&"UserService"));
    assert!(originals.contains(&"retry_policy"));
    assert!(originals.contains(&"REST"));
    assert!(originals.contains(&"MAX_RETRIES"));
    assert!(originals.contains(&"API_KEY"));
    assert!(originals.contains(&"app.isPackaged"));
    assert!(originals.contains(&"render"));
    assert!(originals.contains(&"parse"));
    assert_eq!(ids.len(), 8);
}
