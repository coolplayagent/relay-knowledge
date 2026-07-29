//! Scores symbol names, kinds, signatures, and scoped identities.

use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

use super::super::code_query_identifiers::identifier_terms_equivalent;
use super::{
    conversion_scoring::conversion_symbol_bonus,
    symbol_identity::{contains_scoped_terms, scoped_query_terms},
    tokens::identifier_search_tokens,
};

pub(in crate::storage::sqlite::code::code_query) fn symbol_kind_bonus(
    kind: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if !matches!(
        request.code_query_kind,
        CodeQueryKind::Definition | CodeQueryKind::Symbol | CodeQueryKind::Hybrid
    ) {
        return 0.0;
    }
    match kind {
        "macro" => 0.35,
        "function" | "method" => 0.25,
        "function_declaration" => 0.0,
        _ => 0.1,
    }
}

pub(in crate::storage::sqlite::code::code_query) fn symbol_name_query_bonus(
    query: &str,
    name: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if !matches!(
        request.code_query_kind,
        CodeQueryKind::Definition | CodeQueryKind::Symbol | CodeQueryKind::Hybrid
    ) {
        return 0.0;
    }
    let query_terms = identifier_search_tokens(query);
    if query_terms.is_empty() {
        return 0.0;
    }
    let name_tokens = identifier_search_tokens(name);
    if query_terms.iter().all(|term| {
        name_tokens
            .iter()
            .any(|token| identifier_terms_equivalent(token, term))
    }) {
        2.0
    } else {
        partial_symbol_name_query_bonus(&query_terms, &name_tokens)
    }
}

fn partial_symbol_name_query_bonus(query_terms: &[String], name_tokens: &[String]) -> f64 {
    let matched_terms = query_terms
        .iter()
        .filter(|term| {
            term.len() >= 3
                && name_tokens.iter().any(|token| {
                    identifier_terms_equivalent(token, term)
                        || (token.len() >= 3
                            && (term.starts_with(token) || token.starts_with(term.as_str())))
                })
        })
        .count();
    if matched_terms >= 3 {
        (matched_terms as f64 * 0.75).min(2.0)
    } else if matched_terms == 2 {
        1.1
    } else {
        0.0
    }
}

pub(in crate::storage::sqlite::code::code_query) fn symbol_query_bonus(
    query: &str,
    name: &str,
    qualified_name: &str,
    signature: &str,
    canonical_symbol_id: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    let name_bonus = symbol_name_query_bonus(query, name, request)
        + workflow_connection_lifecycle_symbol_bonus(query, name, signature, request)
        + conversion_symbol_bonus(query, name, signature, request);
    if !matches!(
        request.code_query_kind,
        CodeQueryKind::Definition | CodeQueryKind::Symbol | CodeQueryKind::Hybrid
    ) {
        return name_bonus;
    }
    let Some(scoped_terms) = scoped_query_terms(query) else {
        return name_bonus;
    };
    let has_scoped_match = [qualified_name, signature, canonical_symbol_id]
        .iter()
        .any(|field| contains_scoped_terms(field, &scoped_terms));
    if has_scoped_match {
        name_bonus + 3.0
    } else {
        name_bonus
    }
}

fn workflow_connection_lifecycle_symbol_bonus(
    query: &str,
    name: &str,
    signature: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if request.code_query_kind != CodeQueryKind::Hybrid {
        return 0.0;
    }
    let query_terms = identifier_search_tokens(query);
    let has_stream_lifecycle_intent = query_terms.iter().any(|term| {
        matches!(
            term.as_str(),
            "connect" | "connection" | "event" | "reconnect" | "run" | "source" | "stream"
        )
    }) && query_terms
        .iter()
        .filter(|term| matches!(term.as_str(), "event" | "run" | "source" | "stream"))
        .count()
        >= 2;
    if !has_stream_lifecycle_intent {
        return 0.0;
    }

    let mut symbol_terms = identifier_search_tokens(name);
    symbol_terms.extend(identifier_search_tokens(signature));
    symbol_terms.sort();
    symbol_terms.dedup();
    let has_lifecycle_opener = symbol_terms
        .iter()
        .any(|term| matches!(term.as_str(), "connect" | "open" | "reconnect"));
    if !has_lifecycle_opener {
        return 0.0;
    }

    let matched_workflow_terms = ["connection", "event", "run", "source", "stream"]
        .iter()
        .filter(|term| {
            query_terms.iter().any(|query_term| query_term == **term)
                && symbol_terms.iter().any(|symbol_term| symbol_term == **term)
        })
        .count();
    if matched_workflow_terms >= 2 {
        3.25
    } else {
        0.0
    }
}

pub(in crate::storage::sqlite::code::code_query) fn scoped_identity_query_bonus(
    query: &str,
    fields: impl IntoIterator<Item = impl AsRef<str>>,
) -> f64 {
    let Some(scoped_terms) = scoped_query_terms(query) else {
        return 0.0;
    };
    if fields
        .into_iter()
        .any(|field| contains_scoped_terms(field.as_ref(), &scoped_terms))
    {
        2.0
    } else {
        0.0
    }
}

pub(in crate::storage::sqlite::code::code_query) fn symbol_excerpt(
    name: &str,
    qualified_name: &str,
    signature: &str,
    doc_comment: Option<&str>,
) -> String {
    let body = if let Some(doc) = doc_comment {
        format!("{doc}\n{signature}")
    } else {
        signature.to_owned()
    };
    let Some(display_name) = class_member_display_name(name, qualified_name) else {
        return body;
    };
    if body.contains(&display_name) {
        body
    } else {
        format!("{display_name}: {body}")
    }
}

fn class_member_display_name(name: &str, qualified_name: &str) -> Option<String> {
    let name = name.trim();
    let qualified_name = qualified_name.trim();
    if name.is_empty() || qualified_name == name {
        return None;
    }

    let raw_prefix = qualified_name.strip_suffix(name)?;
    if !(raw_prefix.ends_with('.') || raw_prefix.ends_with("::")) {
        return None;
    }
    let prefix = raw_prefix.trim_end_matches(['.', ':']);
    if prefix.is_empty() {
        return None;
    }
    let owner = prefix
        .rsplit(['.', ':'])
        .find(|segment| !segment.is_empty())?;
    if owner
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_uppercase())
    {
        Some(format!("{owner}.{name}"))
    } else {
        None
    }
}
