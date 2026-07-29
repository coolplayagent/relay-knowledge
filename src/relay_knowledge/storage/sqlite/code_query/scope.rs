pub(in crate::storage::sqlite::code) fn selector_filters_fit_indexed_scope(
    indexed_path_filters: &[String],
    indexed_language_filters: &[String],
    selector_path_filters: &[String],
    selector_language_filters: &[String],
) -> bool {
    requested_paths_fit_indexed_scope(indexed_path_filters, selector_path_filters)
        && requested_languages_fit_indexed_scope(
            indexed_language_filters,
            selector_language_filters,
        )
}

fn requested_paths_fit_indexed_scope(
    indexed_filters: &[String],
    selector_filters: &[String],
) -> bool {
    selector_filters.is_empty()
        || indexed_filters.is_empty()
        || selector_filters.iter().all(|selector_filter| {
            indexed_filters
                .iter()
                .any(|indexed_filter| path_filter_covers(indexed_filter, selector_filter))
        })
}

fn requested_languages_fit_indexed_scope(
    indexed_filters: &[String],
    selector_filters: &[String],
) -> bool {
    selector_filters.is_empty()
        || indexed_filters.is_empty()
        || selector_filters
            .iter()
            .all(|selector_filter| indexed_filters.contains(selector_filter))
}

fn path_filter_covers(indexed_filter: &str, selector_filter: &str) -> bool {
    let indexed_filter = normalize_path_filter(indexed_filter);
    let selector_filter = normalize_path_filter(selector_filter);
    indexed_filter == "."
        || (!indexed_filter.is_empty()
            && !selector_filter.is_empty()
            && (selector_filter == indexed_filter
                || selector_filter.starts_with(&format!("{indexed_filter}/"))))
}

pub(in crate::storage::sqlite::code) fn path_filter_allows(path: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters
            .iter()
            .any(|filter| path_matches_filter(path, filter))
}

pub(in crate::storage::sqlite::code) fn language_filter_allows(
    language_id: &str,
    filters: &[String],
) -> bool {
    filters.is_empty() || filters.iter().any(|filter| filter == language_id)
}

pub(in crate::storage::sqlite::code) fn language_filter_allows_path(
    path: &str,
    language_id: &str,
    filters: &[String],
) -> bool {
    filters.is_empty()
        || filters.iter().any(|filter| {
            filter == language_id || cxx_header_filter_allows(path, language_id, filter)
        })
}

fn cxx_header_filter_allows(path: &str, language_id: &str, filter: &str) -> bool {
    filter == "cpp" && language_id == "c" && path.to_ascii_lowercase().ends_with(".h")
}

pub(in crate::storage::sqlite::code) fn path_matches_filter(path: &str, filter: &str) -> bool {
    let filter = normalize_path_filter(filter);
    if filter == "." {
        return true;
    }
    !filter.is_empty() && (path == filter || path.starts_with(&format!("{filter}/")))
}

fn normalize_path_filter(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}
