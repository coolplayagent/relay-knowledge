use super::*;

#[test]
fn conversion_expansion_intent_accepts_scored_conversion_verbs() {
    for verb in ["adapt", "map", "normalize"] {
        assert!(
            hybrid_query_has_conversion_expansion_intent(&format!(
                "provider response parts {verb} shared event"
            )),
            "{verb} should request conversion expansion"
        );
    }
}

#[test]
fn conversion_expansion_intent_accepts_cased_conversion_verbs() {
    for query in ["Convert Common Chunk", "Map Provider Response Parts"] {
        assert!(
            hybrid_query_has_conversion_expansion_intent(query),
            "{query} should request conversion expansion"
        );
    }
}
