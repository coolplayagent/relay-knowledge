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
const MIN_API_DENSE_HIGH_SIGNAL_TERMS: usize = 3;
const MIN_HIGH_SIGNAL_TERM_PRIORITY: usize = 4;
const MAX_API_DENSE_UNSTRUCTURED_TERMS: usize = 1;
const STRICT_HYBRID_CHUNK_MIN_STRUCTURED_TERMS: usize = 2;
const STRICT_HYBRID_CHUNK_MAX_TERMS: usize = 3;
const FOCUSED_HYBRID_CHUNK_MAX_TERMS: usize = 8;
const FOCUSED_HYBRID_CHUNK_PAIR_DISTANCE: usize = 4;
const COMPOUND_HYBRID_CHUNK_MIN_TERM_LEN: usize = 4;
const COMPOUND_HYBRID_CHUNK_MAX_TERMS: usize = 8;
const COMPOUND_HYBRID_CHUNK_PAIR_DISTANCE: usize = 1;
const FOCUSED_SYMBOL_MAX_TERMS: usize = 3;
const FOCUSED_SYMBOL_MAX_WORKFLOW_TERMS: usize = 2;

pub(in crate::storage::sqlite::code::code_query) fn fts_match_query(query: &str) -> String {
    fts_match_query_with_operator(&super::fts_query_terms(query), " ", true)
}

pub(in crate::storage::sqlite::code::code_query) fn symbol_fts_match_query(query: &str) -> String {
    fts_match_query_with_operator(&super::fts_query_terms(query), " OR ", true)
}

pub(in crate::storage::sqlite::code::code_query) fn focused_symbol_fts_match_query(
    query: &str,
) -> Option<String> {
    let terms = dedupe_terms(super::fts_query_terms(query));
    if terms.len() <= MAX_HYBRID_CHUNK_SIMPLE_RECALL_TERMS {
        return None;
    }
    let mut ranked = terms
        .iter()
        .enumerate()
        .filter(|(_, term)| !focused_symbol_generic_term(term))
        .map(|(position, term)| {
            (
                identifier_term_has_structure(term),
                hybrid_chunk_term_priority(term),
                position,
                term,
            )
        })
        .filter(|(_, priority, _, _)| *priority >= 2)
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.1.cmp(&left.1))
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| left.3.cmp(right.3))
    });
    let mut recall_terms = ranked
        .into_iter()
        .map(|(_, _, _, term)| term.to_owned())
        .take(FOCUSED_SYMBOL_MAX_TERMS)
        .collect::<Vec<_>>();
    append_focused_symbol_workflow_terms(&terms, &mut recall_terms);
    append_type_surface_companion_terms(&terms, &mut recall_terms);

    (recall_terms.len() >= 2).then(|| fts_match_query_with_operator(&recall_terms, " OR ", false))
}

fn append_focused_symbol_workflow_terms(terms: &[String], recall_terms: &mut Vec<String>) {
    let mut appended = 0usize;
    for term in terms {
        if appended >= FOCUSED_SYMBOL_MAX_WORKFLOW_TERMS {
            break;
        }
        if focused_symbol_workflow_term(term) {
            let before = recall_terms.len();
            push_case_insensitive_unique_term(recall_terms, term);
            if recall_terms.len() > before {
                appended += 1;
            }
        }
    }
}

fn focused_symbol_workflow_term(term: &str) -> bool {
    matches!(
        term.to_ascii_lowercase().as_str(),
        "connect" | "connection" | "event" | "run" | "source" | "stream"
    )
}

fn focused_symbol_generic_term(term: &str) -> bool {
    matches!(
        term.to_ascii_lowercase().as_str(),
        "call"
            | "arrow"
            | "client"
            | "contract"
            | "flow"
            | "function"
            | "generic"
            | "handler"
            | "interface"
            | "literal"
            | "object"
            | "provider"
            | "record"
            | "request"
            | "response"
            | "service"
            | "typed"
            | "type"
    )
}

pub(in crate::storage::sqlite::code::code_query) fn hybrid_chunk_fts_match_query(
    query: &str,
) -> String {
    hybrid_chunk_fts_match_query_with_compound(query, true)
}

pub(in crate::storage::sqlite::code::code_query) fn direct_hybrid_chunk_fts_match_query(
    query: &str,
) -> String {
    hybrid_chunk_fts_match_query_with_compound(query, false)
}

pub(in crate::storage::sqlite::code::code_query) fn focused_hybrid_chunk_fts_match_query(
    query: &str,
) -> Option<String> {
    let terms = dedupe_terms(super::fts_query_terms(query));
    if terms.len() <= MAX_HYBRID_CHUNK_SIMPLE_RECALL_TERMS {
        return None;
    }
    if terms.iter().any(|term| identifier_term_has_structure(term)) {
        return None;
    }
    let terms = terms
        .into_iter()
        .filter(|term| term.len() >= MIN_HIGH_SIGNAL_TERM_PRIORITY)
        .take(FOCUSED_HYBRID_CHUNK_MAX_TERMS)
        .collect::<Vec<_>>();
    if terms.len() < 3 {
        return None;
    }
    let mut groups = Vec::new();
    for (index, left) in terms.iter().enumerate() {
        for right in terms
            .iter()
            .skip(index + 1)
            .take(FOCUSED_HYBRID_CHUNK_PAIR_DISTANCE)
        {
            groups.push(format!(
                "({} {})",
                quote_fts_term(left),
                quote_fts_term(right)
            ));
        }
    }

    (!groups.is_empty()).then(|| groups.join(" OR "))
}

pub(in crate::storage::sqlite::code::code_query) fn lifecycle_hybrid_chunk_fts_match_query(
    query: &str,
) -> Option<String> {
    let terms = dedupe_terms(
        super::fts_query_terms(query)
            .into_iter()
            .map(|term| term.to_ascii_lowercase())
            .collect(),
    );
    if terms.len() <= MAX_HYBRID_CHUNK_SIMPLE_RECALL_TERMS {
        return None;
    }
    let finalization_terms = lifecycle_finalization_recall_terms(&terms);
    if finalization_terms.is_empty() {
        return None;
    }
    let has_tool_call_intent = terms
        .iter()
        .any(|term| matches!(term.as_str(), "tool" | "tools"))
        && terms
            .iter()
            .any(|term| matches!(term.as_str(), "call" | "calls"));
    let has_lifecycle_intent = terms.iter().any(|term| {
        matches!(
            term.as_str(),
            "delta" | "event" | "events" | "lifecycle" | "stream"
        )
    });
    if !has_tool_call_intent || !has_lifecycle_intent {
        return None;
    }

    let anchor = if terms.iter().any(|term| term == "delta") {
        "delta"
    } else if terms.iter().any(|term| term == "tool") {
        "tool"
    } else {
        "lifecycle"
    };
    Some(lifecycle_recall_match_query(anchor, &finalization_terms))
}

fn lifecycle_finalization_recall_terms(terms: &[String]) -> Vec<String> {
    let mut recall_terms = Vec::new();
    for term in terms {
        match term.as_str() {
            "finish" | "finalize" | "finalized" => recall_terms.push(term.clone()),
            "finished" => {
                recall_terms.push("finish".to_owned());
                recall_terms.push("finished".to_owned());
            }
            _ => {}
        }
    }
    recall_terms.sort();
    recall_terms.dedup();

    recall_terms
}

fn lifecycle_recall_match_query(anchor: &str, finalization_terms: &[String]) -> String {
    finalization_terms
        .iter()
        .map(|term| format!("{} {}", quote_fts_term(anchor), quote_fts_term(term)))
        .collect::<Vec<_>>()
        .join(" OR ")
}

pub(in crate::storage::sqlite::code::code_query) fn structured_hybrid_chunk_fts_match_query(
    query: &str,
) -> Option<String> {
    let query_terms = dedupe_terms(super::fts_query_terms(query));
    let mut terms = query_terms
        .into_iter()
        .filter(|term| identifier_term_has_recall_structure(term))
        .take(MAX_HYBRID_CHUNK_RECALL_ANCHORS)
        .collect::<Vec<_>>();
    append_type_surface_companion_terms(&dedupe_terms(super::fts_query_terms(query)), &mut terms);

    (!terms.is_empty()).then(|| fts_match_query_with_operator(&terms, " OR ", false))
}

fn append_type_surface_companion_terms(query_terms: &[String], recall_terms: &mut Vec<String>) {
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

pub(in crate::storage::sqlite::code::code_query) fn compound_hybrid_chunk_fts_match_query(
    query: &str,
) -> Option<String> {
    let terms = dedupe_terms(super::fts_query_terms(query))
        .into_iter()
        .filter(|term| term.len() >= COMPOUND_HYBRID_CHUNK_MIN_TERM_LEN)
        .take(COMPOUND_HYBRID_CHUNK_MAX_TERMS)
        .collect::<Vec<_>>();
    if terms.len() < 2 {
        return None;
    }

    let mut alternatives = Vec::new();
    for (index, left) in terms.iter().enumerate() {
        for right in terms
            .iter()
            .skip(index + 1)
            .take(COMPOUND_HYBRID_CHUNK_PAIR_DISTANCE)
        {
            push_compound_identifier_window(
                &mut alternatives,
                &terms,
                &[left.clone(), right.clone()],
            );
        }
    }

    (!alternatives.is_empty()).then(|| {
        alternatives
            .iter()
            .map(|term| quote_fts_term(term))
            .collect::<Vec<_>>()
            .join(" OR ")
    })
}

fn hybrid_chunk_fts_match_query_with_compound(
    query: &str,
    include_compound_identifiers: bool,
) -> String {
    let mut terms = dedupe_terms(super::fts_query_terms(query));
    if terms.len() <= MAX_HYBRID_CHUNK_SIMPLE_RECALL_TERMS {
        let query_terms = terms.clone();
        append_type_surface_companion_terms(&query_terms, &mut terms);
        return fts_match_query_with_operator(&terms, " OR ", include_compound_identifiers);
    }

    let recall_terms = hybrid_chunk_recall_terms(&terms);
    fts_match_query_with_operator(&recall_terms, " OR ", include_compound_identifiers)
}

pub(in crate::storage::sqlite::code::code_query) fn strict_hybrid_chunk_fts_match_query(
    query: &str,
) -> Option<String> {
    let terms = dedupe_terms(super::fts_query_terms(query));
    if terms.len() <= MAX_HYBRID_CHUNK_SIMPLE_RECALL_TERMS {
        return None;
    }
    let strict_terms = strict_hybrid_chunk_recall_terms(query, &terms);
    if !api_dense_hybrid_query(&terms) && !strict_member_access_recall_allowed(query, &strict_terms)
    {
        return None;
    }
    (strict_terms.len() >= STRICT_HYBRID_CHUNK_MIN_STRUCTURED_TERMS)
        .then(|| fts_match_query_with_operator(&strict_terms, " ", false))
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
    if api_dense_hybrid_query(terms) {
        let mut recall_terms = high_signal_hybrid_chunk_recall_terms(terms);
        append_type_surface_companion_terms(terms, &mut recall_terms);
        return recall_terms;
    }

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
    append_type_surface_companion_terms(terms, &mut recall_terms);

    recall_terms
}

fn api_dense_hybrid_query(terms: &[String]) -> bool {
    let mut high_signal_terms = 0usize;
    let mut has_structured_term = false;
    for term in terms {
        let structured = identifier_term_has_structure(term);
        has_structured_term |= structured;
        if hybrid_chunk_term_priority(term) >= MIN_HIGH_SIGNAL_TERM_PRIORITY {
            high_signal_terms += 1;
        }
    }

    has_structured_term && high_signal_terms >= MIN_API_DENSE_HIGH_SIGNAL_TERMS
}

fn high_signal_hybrid_chunk_recall_terms(terms: &[String]) -> Vec<String> {
    let mut ranked = terms
        .iter()
        .enumerate()
        .map(|(position, term)| {
            (
                identifier_term_has_structure(term),
                hybrid_chunk_term_priority(term),
                position,
                term,
            )
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.1.cmp(&left.1))
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| left.3.cmp(right.3))
    });

    let mut recall_terms = Vec::new();
    let mut unstructured_terms = 0usize;
    for (structured, priority, _, term) in ranked {
        if recall_terms.len() >= MAX_HYBRID_CHUNK_RECALL_TERMS {
            break;
        }
        if priority < MIN_HIGH_SIGNAL_TERM_PRIORITY {
            continue;
        }
        if !structured {
            if unstructured_terms >= MAX_API_DENSE_UNSTRUCTURED_TERMS {
                continue;
            }
            unstructured_terms += 1;
        }
        push_case_insensitive_unique_term(&mut recall_terms, term);
    }

    recall_terms
}

fn strict_hybrid_chunk_recall_terms(query: &str, terms: &[String]) -> Vec<String> {
    let mut ranked = terms
        .iter()
        .enumerate()
        .filter(|(_, term)| identifier_term_has_structure(term))
        .filter(|(_, term)| hybrid_chunk_term_priority(term) >= MIN_HIGH_SIGNAL_TERM_PRIORITY)
        .map(|(position, term)| (hybrid_chunk_term_priority(term), position, term))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(right.2))
    });

    let mut recall_terms = Vec::new();
    for (_, _, term) in ranked {
        if recall_terms.len() >= STRICT_HYBRID_CHUNK_MAX_TERMS {
            break;
        }
        push_case_insensitive_unique_term(&mut recall_terms, term);
    }
    if recall_terms.len() < STRICT_HYBRID_CHUNK_MIN_STRUCTURED_TERMS {
        for term in member_access_leaf_terms(query) {
            if recall_terms.len() >= STRICT_HYBRID_CHUNK_MIN_STRUCTURED_TERMS {
                break;
            }
            push_case_insensitive_unique_term(&mut recall_terms, &term);
        }
    }

    recall_terms
}

fn strict_member_access_recall_allowed(query: &str, recall_terms: &[String]) -> bool {
    let member_leaves = member_access_leaf_terms(query);
    !member_leaves.is_empty()
        && recall_terms.iter().any(|term| {
            identifier_term_has_structure(term)
                && hybrid_chunk_term_priority(term) >= MIN_HIGH_SIGNAL_TERM_PRIORITY
        })
        && member_leaves.iter().any(|leaf| {
            recall_terms
                .iter()
                .any(|term| term.eq_ignore_ascii_case(leaf))
        })
}

fn member_access_leaf_terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for raw_token in query.split_whitespace().map(str::trim) {
        let token = raw_token.trim_matches(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | ':'))
        });
        if token.is_empty()
            || token.contains('/')
            || token.contains('\\')
            || token_has_path_like_extension(token)
            || !(token.contains('.') || token.contains("::"))
        {
            continue;
        }
        let Some(leaf) = token
            .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .find(|term| !term.is_empty())
        else {
            continue;
        };
        if leaf.len() >= 4
            && leaf
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
            && !terms
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(leaf))
        {
            terms.push(leaf.to_owned());
        }
    }

    terms
}

fn token_has_path_like_extension(token: &str) -> bool {
    let Some((stem, extension)) = token.rsplit_once('.') else {
        return false;
    };

    !stem.is_empty() && file_extension_is_path_like(extension)
}

fn file_extension_is_path_like(extension: &str) -> bool {
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "c" | "cc"
            | "cpp"
            | "cs"
            | "go"
            | "gradle"
            | "h"
            | "hh"
            | "hpp"
            | "hxx"
            | "java"
            | "js"
            | "json"
            | "jsx"
            | "kt"
            | "md"
            | "php"
            | "py"
            | "rb"
            | "rs"
            | "scala"
            | "sh"
            | "swift"
            | "ts"
            | "tsx"
            | "txt"
            | "xml"
            | "yaml"
            | "yml"
    )
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
    identifier_term_structure_boundary_count(term) > 0
}

fn identifier_term_has_recall_structure(term: &str) -> bool {
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

    #[test]
    fn hybrid_chunk_fts_query_uses_high_signal_terms_for_api_dense_queries() {
        let fts_query = hybrid_chunk_fts_match_query(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
        );

        for term in ["RegisterWorkflow", "RegisterActivity", "InterruptCh"] {
            assert!(fts_query.contains(&format!("\"{term}\"")));
        }
        for term in ["worker", "task", "queue"] {
            assert!(!fts_query.contains(&format!("\"{term}\"")));
        }
    }

    #[test]
    fn hybrid_chunk_fts_query_limits_broad_context_terms_for_api_dense_queries() {
        let fts_query = hybrid_chunk_fts_match_query(
            "client.Dial envconfig MustLoadDefaultClientOptions workflow client",
        );

        assert!(fts_query.contains("\"MustLoadDefaultClientOptions\""));
        assert!(fts_query.contains("\"envconfig\""));
        assert!(!fts_query.contains("\"workflow\""));
        assert!(!fts_query.contains("\"client\""));
    }

    #[test]
    fn direct_hybrid_chunk_fts_query_omits_compound_alternatives() {
        assert_eq!(
            direct_hybrid_chunk_fts_match_query("cache cache Lookup Insert"),
            "\"cache\" OR \"Lookup\" OR \"Insert\""
        );
        assert!(
            !direct_hybrid_chunk_fts_match_query("checkpoint metadata version constant")
                .contains("\"checkpointmetadataversionconstant\"")
        );
    }

    #[test]
    fn focused_symbol_fts_query_uses_bounded_high_signal_terms() {
        assert_eq!(
            focused_symbol_fts_match_query(
                "NoDestructor variadic constructor template instance type"
            )
            .as_deref(),
            Some("\"NoDestructor\" OR \"constructor\" OR \"variadic\"")
        );
        assert!(focused_symbol_fts_match_query("NoDestructor constructor").is_none());
    }

    #[test]
    fn focused_symbol_fts_query_keeps_workflow_identity_terms() {
        let fts_query = focused_symbol_fts_match_query(
            "background stream discovery reconcile multiplex run event source reconnect",
        )
        .expect("focused symbol query should be planned");

        assert!(fts_query.contains("\"stream\""));
        assert!(fts_query.contains("\"run\""));
    }

    #[test]
    fn focused_hybrid_chunk_fts_query_uses_bounded_neighbor_pairs() {
        let fts_query = focused_hybrid_chunk_fts_match_query(
            "typed arrow payload projector trim provider record",
        )
        .expect("focused hybrid query should be planned");

        assert!(fts_query.contains("(\"payload\" \"projector\")"));
        assert!(fts_query.contains("(\"payload\" \"trim\")"));
        assert!(fts_query.contains("(\"payload\" \"provider\")"));
        assert!(fts_query.contains("(\"provider\" \"record\")"));
        assert!(!fts_query.contains("\"payload\" OR \"projector\""));
    }

    #[test]
    fn focused_hybrid_chunk_fts_query_skips_structured_identifier_terms() {
        assert!(
            focused_hybrid_chunk_fts_match_query(
                "EvalCheckpointStore signature mismatch append result"
            )
            .is_none()
        );
        assert!(
            focused_hybrid_chunk_fts_match_query(
                "external session workflow TypeScript client openExternalSession"
            )
            .is_none()
        );
    }

    #[test]
    fn lifecycle_hybrid_chunk_fts_query_recalls_tool_finalization_flow() {
        assert_eq!(
            lifecycle_hybrid_chunk_fts_match_query(
                "OpenAI Chat protocol sse tool call delta lifecycle finish events"
            )
            .as_deref(),
            Some("\"delta\" \"finish\"")
        );
        assert_eq!(
            lifecycle_hybrid_chunk_fts_match_query(
                "OpenAI Chat protocol SSE Tool Call Delta Lifecycle Finish Events"
            )
            .as_deref(),
            Some("\"delta\" \"finish\"")
        );
        assert_eq!(
            lifecycle_hybrid_chunk_fts_match_query(
                "OpenAI Chat protocol sse tool call delta lifecycle finalize events"
            )
            .as_deref(),
            Some("\"delta\" \"finalize\"")
        );
        assert_eq!(
            lifecycle_hybrid_chunk_fts_match_query(
                "OpenAI Chat protocol sse tool call delta lifecycle finalized events"
            )
            .as_deref(),
            Some("\"delta\" \"finalized\"")
        );
        assert_eq!(
            lifecycle_hybrid_chunk_fts_match_query(
                "OpenAI Chat protocol sse tool call delta lifecycle finished events"
            )
            .as_deref(),
            Some("\"delta\" \"finish\" OR \"delta\" \"finished\"")
        );
        assert!(lifecycle_hybrid_chunk_fts_match_query("protocol lifecycle events").is_none());
        assert!(lifecycle_hybrid_chunk_fts_match_query("tool call setup delta events").is_none());
    }

    #[test]
    fn structured_hybrid_chunk_fts_query_uses_identifier_terms_only() {
        assert_eq!(
            structured_hybrid_chunk_fts_match_query(
                "external session workflow TypeScript client openExternalSession"
            )
            .as_deref(),
            Some("\"openExternalSession\"")
        );
        assert!(structured_hybrid_chunk_fts_match_query("plain workflow query").is_none());
    }

    #[test]
    fn structured_hybrid_chunk_fts_query_keeps_type_surface_companions() {
        assert_eq!(
            structured_hybrid_chunk_fts_match_query(
                "metricsink plugin component Type MustNewType metric_sink"
            )
            .as_deref(),
            Some("\"MustNewType\" OR \"metric_sink\" OR \"component Type\"")
        );
    }

    #[test]
    fn direct_hybrid_chunk_fts_query_keeps_type_surface_companions() {
        assert_eq!(
            direct_hybrid_chunk_fts_match_query(
                "metricsink plugin component Type MustNewType metric_sink"
            ),
            "\"MustNewType\" OR \"metric_sink\" OR \"metricsink\" OR \"component Type\""
        );
    }

    #[test]
    fn direct_hybrid_chunk_fts_query_keeps_type_surface_companions_for_short_queries() {
        assert_eq!(
            direct_hybrid_chunk_fts_match_query("metricsink component Type MustNewType"),
            "\"metricsink\" OR \"component\" OR \"Type\" OR \"MustNewType\" OR \"component Type\""
        );
    }

    #[test]
    fn compound_hybrid_chunk_fts_query_uses_bounded_adjacent_identifier_pairs() {
        let fts_query = compound_hybrid_chunk_fts_match_query(
            "tsx provider panel effect run provider envelope payload",
        )
        .expect("compound hybrid query should be planned");

        assert!(fts_query.contains("\"providerpanel\""));
        assert!(fts_query.contains("\"provider_panel\""));
        assert!(fts_query.contains("\"envelopepayload\""));
        assert!(!fts_query.contains("\"providerpaneleffect\""));
    }

    #[test]
    fn compound_hybrid_chunk_fts_query_recalls_type_identifier_pairs() {
        let fts_query = compound_hybrid_chunk_fts_match_query(
            "typed arrow payload projector trim provider record",
        )
        .expect("compound hybrid query should be planned");

        assert!(fts_query.contains("\"payloadprojector\""));
        assert!(fts_query.contains("\"payload_projector\""));
    }
}
