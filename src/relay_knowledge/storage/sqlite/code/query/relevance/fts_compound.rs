//! Bounded compound-identifier alternatives for FTS recall.

const MAX_COMPOUND_QUERY_TERMS: usize = 6;
const MAX_COMPOUND_IDENTIFIER_PARTS: usize = 8;
const MIN_COMPOUND_IDENTIFIER_LEN: usize = 6;
const MAX_COMPOUND_IDENTIFIER_LEN: usize = 80;
const MIN_SUBPHRASE_IDENTIFIER_PARTS: usize = 2;
const MAX_SUBPHRASE_IDENTIFIER_PARTS: usize = 4;
pub(super) const MAX_COMPOUND_FTS_ALTERNATIVES: usize = 24;

pub(super) fn compound_identifier_fts_terms(terms: &[String]) -> Vec<String> {
    if terms.len() < 2 {
        return Vec::new();
    }
    let Some(parts) = compound_identifier_parts(terms) else {
        return Vec::new();
    };

    let mut alternatives = Vec::new();
    if terms.len() <= MAX_COMPOUND_QUERY_TERMS && parts.len() <= MAX_COMPOUND_IDENTIFIER_PARTS {
        push_compound_identifier_window(&mut alternatives, terms, &parts);
    }
    if parts.len() > MAX_SUBPHRASE_IDENTIFIER_PARTS - 1 {
        for window_len in (MIN_SUBPHRASE_IDENTIFIER_PARTS..=MAX_SUBPHRASE_IDENTIFIER_PARTS).rev() {
            for window in parts.windows(window_len) {
                push_compound_identifier_window(&mut alternatives, terms, window);
                if alternatives.len() >= MAX_COMPOUND_FTS_ALTERNATIVES {
                    return alternatives;
                }
            }
        }
    }

    alternatives
}

pub(super) fn compound_identifier_source_term(term: &str) -> bool {
    term.chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

pub(super) fn push_compound_identifier_window(
    alternatives: &mut Vec<String>,
    original_terms: &[String],
    parts: &[String],
) {
    let compact = parts.join("");
    if !(MIN_COMPOUND_IDENTIFIER_LEN..=MAX_COMPOUND_IDENTIFIER_LEN).contains(&compact.len()) {
        return;
    }

    let snake = parts.join("_");
    push_compound_identifier_alternative(alternatives, original_terms, compact);
    push_compound_identifier_alternative(alternatives, original_terms, snake);
}

fn compound_identifier_parts(terms: &[String]) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    for term in terms {
        for part in term.split('_').filter(|part| !part.is_empty()) {
            if part.len() < 2
                || !part
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
            {
                return None;
            }
            parts.push(part.to_ascii_lowercase());
        }
    }

    (parts.len() >= 2).then_some(parts)
}

fn push_compound_identifier_alternative(
    alternatives: &mut Vec<String>,
    original_terms: &[String],
    candidate: String,
) {
    if !original_terms
        .iter()
        .any(|term| term.eq_ignore_ascii_case(&candidate))
        && !alternatives.contains(&candidate)
    {
        alternatives.push(candidate);
    }
}

#[cfg(test)]
#[path = "fts_compound_tests.rs"]
mod tests;
