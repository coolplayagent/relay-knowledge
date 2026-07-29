use crate::domain::CodeRetrievalRequest;

pub(super) fn merged_filters(left: &[String], right: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    for value in left.iter().chain(right.iter()) {
        if !merged.contains(value) {
            merged.push(value.clone());
        }
    }

    merged
}

pub(super) fn query_language_filters(
    base_filters: Vec<String>,
    query_filters: &[String],
) -> Vec<String> {
    const NO_MATCHING_LANGUAGE_FILTER: &str = "__relay_no_matching_language__";
    const C_CPP_HEADER_LANGUAGE_FILTER: &str = "__relay_c_cpp_header_only__";

    if query_filters.is_empty() {
        return base_filters;
    }
    if base_filters.is_empty() {
        return query_filters.to_vec();
    }

    let mut intersection = Vec::new();
    for query_filter in query_filters {
        for base_filter in &base_filters {
            if base_filter == query_filter && !intersection.contains(query_filter) {
                intersection.push(query_filter.clone());
            } else if c_cpp_header_language_overlap(base_filter, query_filter)
                && !intersection
                    .iter()
                    .any(|filter| filter == C_CPP_HEADER_LANGUAGE_FILTER)
            {
                intersection.push(C_CPP_HEADER_LANGUAGE_FILTER.to_owned());
            }
        }
    }
    let mut intersection = merged_filters(&[], &intersection);
    if intersection.is_empty() {
        intersection.push(NO_MATCHING_LANGUAGE_FILTER.to_owned());
    }
    intersection
}

fn c_cpp_header_language_overlap(base_filter: &str, query_filter: &str) -> bool {
    matches!((base_filter, query_filter), ("c", "cpp") | ("cpp", "c"))
}

pub(super) fn query_field_filters_allow_match(
    request: &CodeRetrievalRequest,
    path: &str,
    excerpt: &str,
) -> bool {
    request.query_kind_filters.is_empty()
        && filters_match_text(&request.query_path_substrings, path)
        && source_fallback_name_filters_match(request, excerpt)
}

fn source_fallback_name_filters_match(request: &CodeRetrievalRequest, excerpt: &str) -> bool {
    request.query_name_substrings.is_empty()
        || request.query_name_substrings.iter().any(|filter| {
            text_matches_filter(&request.query, filter) || text_matches_filter(excerpt, filter)
        })
}

fn filters_match_text(filters: &[String], text: &str) -> bool {
    filters.is_empty()
        || filters
            .iter()
            .any(|filter| text_matches_filter(text, filter))
}

fn text_matches_filter(text: &str, filter: &str) -> bool {
    text.to_ascii_lowercase()
        .contains(&filter.to_ascii_lowercase())
}
