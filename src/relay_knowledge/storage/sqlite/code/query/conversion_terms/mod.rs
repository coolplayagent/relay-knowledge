//! Conversion-action vocabulary shared by query ranking owners.

pub(in crate::storage::sqlite::code::query) fn conversion_action_term(term: &str) -> bool {
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
