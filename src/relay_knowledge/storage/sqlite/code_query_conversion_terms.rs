pub(in crate::storage::sqlite::code::code_query) fn conversion_action_term(term: &str) -> bool {
    matches!(
        term,
        "adapt"
            | "adapts"
            | "convert"
            | "conversion"
            | "format"
            | "formats"
            | "map"
            | "maps"
            | "normalize"
            | "normalized"
            | "transform"
            | "translate"
    )
}
