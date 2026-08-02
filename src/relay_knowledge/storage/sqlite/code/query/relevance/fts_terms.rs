//! FTS term normalization, quoting, type companions, and identifier structure.

pub(super) const MAX_HYBRID_CHUNK_RECALL_ANCHORS: usize = 3;
pub(super) const MIN_HIGH_SIGNAL_TERM_PRIORITY: usize = 4;

pub(super) fn quote_fts_term(term: &str) -> String {
    format!("\"{}\"", term.replace('"', "\"\""))
}

pub(super) fn dedupe_terms(terms: Vec<String>) -> Vec<String> {
    let mut deduped = Vec::new();
    for term in terms {
        if !deduped
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&term))
        {
            deduped.push(term);
        }
    }

    deduped
}

pub(super) fn append_type_surface_companion_terms(
    query_terms: &[String],
    recall_terms: &mut Vec<String>,
) {
    if recall_terms.is_empty()
        || !query_terms
            .iter()
            .any(|term| term.eq_ignore_ascii_case("type"))
    {
        return;
    }

    let mut appended = 0usize;
    for companion in ["component", "metadata"] {
        if appended >= 2 {
            break;
        }
        if query_terms
            .iter()
            .any(|term| term.eq_ignore_ascii_case(companion))
        {
            push_case_insensitive_unique_term(recall_terms, &format!("{companion} Type"));
            appended += 1;
        }
    }
}

pub(super) fn push_case_insensitive_unique_term(terms: &mut Vec<String>, term: &str) {
    if !terms
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(term))
    {
        terms.push(term.to_owned());
    }
}

pub(super) fn hybrid_chunk_term_priority(term: &str) -> usize {
    let length = term.chars().count();
    let length_score = if length >= 12 {
        6
    } else if length >= 10 {
        5
    } else if length >= 8 {
        4
    } else if length >= 5 {
        2
    } else {
        1
    };
    if identifier_term_has_structure(term) {
        length_score + 8
    } else {
        length_score
    }
}

pub(super) fn identifier_term_has_structure(term: &str) -> bool {
    identifier_term_structure_boundary_count(term) > 0
}

pub(super) fn identifier_term_has_recall_structure(term: &str) -> bool {
    term.contains('_') || identifier_term_structure_boundary_count(term) >= 2
}

fn identifier_term_structure_boundary_count(term: &str) -> usize {
    if term.contains('_') {
        return 1;
    }
    let mut previous: Option<char> = None;
    let chars = term.chars().collect::<Vec<_>>();
    let mut boundaries = 0usize;
    for (index, character) in chars.iter().enumerate() {
        let next = chars.get(index + 1).copied();
        let starts_upper_word = character.is_ascii_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || next.is_some_and(|next| next.is_ascii_lowercase())
            });
        if starts_upper_word {
            boundaries += 1;
        }
        previous = Some(*character);
    }

    boundaries
}

#[cfg(test)]
#[path = "fts_terms_tests.rs"]
mod tests;
