const EMPTY_FTS_QUERY: &str = "relayknowledgeunlikelyemptyquerytoken";
const MAX_COMPOUND_QUERY_TERMS: usize = 6;
const MAX_COMPOUND_IDENTIFIER_PARTS: usize = 8;
const MIN_COMPOUND_IDENTIFIER_LEN: usize = 6;
const MAX_COMPOUND_IDENTIFIER_LEN: usize = 80;
const MIN_SUBPHRASE_IDENTIFIER_PARTS: usize = 2;
const MAX_SUBPHRASE_IDENTIFIER_PARTS: usize = 4;
const MAX_COMPOUND_FTS_ALTERNATIVES: usize = 24;
const MAX_HYBRID_CHUNK_SIMPLE_RECALL_TERMS: usize = 4;
const MAX_HYBRID_CHUNK_RECALL_TERMS: usize = 6;
const MAX_HYBRID_CHUNK_RECALL_ANCHORS: usize = 3;

pub(in crate::storage::sqlite::code::code_query) fn fts_match_query(query: &str) -> String {
    fts_match_query_with_operator(&super::fts_query_terms(query), " ", true)
}

pub(in crate::storage::sqlite::code::code_query) fn symbol_fts_match_query(query: &str) -> String {
    fts_match_query_with_operator(&super::fts_query_terms(query), " OR ", true)
}

pub(in crate::storage::sqlite::code::code_query) fn hybrid_chunk_fts_match_query(
    query: &str,
) -> String {
    let terms = dedupe_terms(super::fts_query_terms(query));
    if terms.len() <= MAX_HYBRID_CHUNK_SIMPLE_RECALL_TERMS {
        return fts_match_query_with_operator(&terms, " OR ", true);
    }

    let recall_terms = hybrid_chunk_recall_terms(&terms);
    fts_match_query_with_operator(&recall_terms, " OR ", true)
}

fn fts_match_query_with_operator(
    terms: &[String],
    operator: &str,
    include_compound_identifiers: bool,
) -> String {
    if terms.is_empty() {
        return EMPTY_FTS_QUERY.to_owned();
    }

    let primary = terms
        .iter()
        .map(|term| quote_fts_term(term))
        .collect::<Vec<_>>()
        .join(operator);
    let alternatives = if include_compound_identifiers {
        compound_identifier_fts_terms(terms)
    } else {
        Vec::new()
    };
    if alternatives.is_empty() {
        primary
    } else {
        format!(
            "({}) OR {}",
            primary,
            alternatives
                .iter()
                .map(|term| quote_fts_term(term))
                .collect::<Vec<_>>()
                .join(" OR ")
        )
    }
}

fn quote_fts_term(term: &str) -> String {
    format!("\"{}\"", term.replace('"', "\"\""))
}

fn dedupe_terms(terms: Vec<String>) -> Vec<String> {
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

fn hybrid_chunk_recall_terms(terms: &[String]) -> Vec<String> {
    let mut recall_terms = leading_hybrid_chunk_recall_anchors(terms);
    let mut ranked = terms
        .iter()
        .enumerate()
        .map(|(position, term)| (hybrid_chunk_term_priority(term), position, term))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(right.2))
    });
    for (priority, _, term) in ranked {
        if recall_terms.len() >= MAX_HYBRID_CHUNK_RECALL_TERMS {
            break;
        }
        if priority < 2 {
            continue;
        }
        push_case_insensitive_unique_term(&mut recall_terms, term);
    }

    recall_terms
}

fn leading_hybrid_chunk_recall_anchors(terms: &[String]) -> Vec<String> {
    let mut anchors = Vec::new();
    for term in terms {
        if anchors.len() >= MAX_HYBRID_CHUNK_RECALL_ANCHORS {
            break;
        }
        if leading_hybrid_chunk_anchor(term) {
            push_case_insensitive_unique_term(&mut anchors, term);
        }
    }

    anchors
}

fn leading_hybrid_chunk_anchor(term: &str) -> bool {
    let length = term.chars().count();
    (4..=16).contains(&length)
        && term
            .chars()
            .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
}

fn push_case_insensitive_unique_term(terms: &mut Vec<String>, term: &str) {
    if !terms
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(term))
    {
        terms.push(term.to_owned());
    }
}

fn hybrid_chunk_term_priority(term: &str) -> usize {
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

fn identifier_term_has_structure(term: &str) -> bool {
    if term.contains('_') {
        return true;
    }
    let mut previous: Option<char> = None;
    let chars = term.chars().collect::<Vec<_>>();
    for (index, character) in chars.iter().enumerate() {
        let next = chars.get(index + 1).copied();
        let starts_upper_word = character.is_ascii_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || next.is_some_and(|next| next.is_ascii_lowercase())
            });
        if starts_upper_word {
            return true;
        }
        previous = Some(*character);
    }

    false
}

fn compound_identifier_fts_terms(terms: &[String]) -> Vec<String> {
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

fn push_compound_identifier_window(
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
mod tests {
    use super::*;

    #[test]
    fn hybrid_chunk_fts_query_uses_bounded_identifier_anchors() {
        let query = "client.Open LoadDefaultOptions workflow client retry timeout";
        let fts_query = hybrid_chunk_fts_match_query(query);

        assert!(fts_query.contains("\"LoadDefaultOptions\""));
        assert!(fts_query.contains("\"workflow\""));
        assert!(!fts_query.contains("\"Open\""));
        assert_eq!(fts_query.matches("\"client\"").count(), 1);
        assert!(
            fts_query.matches(" OR ").count()
                <= MAX_COMPOUND_FTS_ALTERNATIVES + MAX_HYBRID_CHUNK_RECALL_TERMS
        );
    }

    #[test]
    fn fts_query_terms_are_deduplicated_before_planning() {
        assert_eq!(
            hybrid_chunk_fts_match_query("cache cache Lookup Insert"),
            "(\"cache\" OR \"Lookup\" OR \"Insert\") OR \"cachelookupinsert\" OR \"cache_lookup_insert\""
        );
    }

    #[test]
    fn hybrid_chunk_fts_query_keeps_leading_lowercase_intent_terms() {
        let fts_query = hybrid_chunk_fts_match_query(
            "operation table read callback dispatch designated initializer",
        );

        for term in ["operation", "table", "read", "designated", "initializer"] {
            assert!(fts_query.contains(&format!("\"{term}\"")));
        }
    }
}
