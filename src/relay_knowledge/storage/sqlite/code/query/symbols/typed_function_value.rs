use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

use super::super::{rows::SymbolRow, scoring::path_ranking::path_looks_like_test_or_benchmark};

const MIN_QUERY_TERMS: usize = 4;
const MIN_MATCHED_TERMS: usize = 3;
const BASE_BONUS: f64 = 1.25;
const EXPORTED_BONUS: f64 = 0.35;
const MAX_BONUS: f64 = 2.35;

struct TypedFunctionValueSurface {
    name_terms: Vec<String>,
    declared_type_terms: Vec<String>,
    signature_terms: Vec<String>,
    exported: bool,
}

pub(super) struct TypedFunctionValueQuery {
    terms: Vec<String>,
    mentions_surface_intent: bool,
}

impl TypedFunctionValueQuery {
    pub(super) fn from_request(query: &str, request: &CodeRetrievalRequest) -> Option<Self> {
        if request.code_query_kind != CodeQueryKind::Hybrid {
            return None;
        }
        let terms = symbol_surface_terms(query);
        if terms.len() < MIN_QUERY_TERMS {
            return None;
        }

        Some(Self {
            mentions_surface_intent: query_mentions_typed_function_value(query, &terms),
            terms,
        })
    }
}

pub(super) fn typed_function_value_surface_bonus(
    row: &SymbolRow,
    query: Option<&TypedFunctionValueQuery>,
    query_has_test_intent: bool,
) -> f64 {
    let Some(query) = query else {
        return 0.0;
    };
    if !matches!(row.kind.as_str(), "constant" | "function" | "variable")
        || (path_looks_like_test_or_benchmark(&row.path) && !query_has_test_intent)
    {
        return 0.0;
    }
    let Some(surface) = typed_function_value_surface(&row.signature) else {
        return 0.0;
    };
    if !terms_overlap(&query.terms, &surface.name_terms)
        || !terms_overlap(&query.terms, &surface.declared_type_terms)
    {
        return 0.0;
    }
    let matched_terms = query
        .terms
        .iter()
        .filter(|query_term| {
            surface
                .signature_terms
                .iter()
                .any(|surface_term| related_surface_terms(surface_term, query_term))
        })
        .count();
    if matched_terms < MIN_MATCHED_TERMS
        || (!query.mentions_surface_intent && matched_terms < MIN_MATCHED_TERMS + 1)
    {
        return 0.0;
    }

    let coverage = matched_terms as f64 / query.terms.len() as f64;
    (BASE_BONUS
        + coverage
        + if surface.exported {
            EXPORTED_BONUS
        } else {
            0.0
        })
    .min(MAX_BONUS)
}

fn typed_function_value_surface(signature: &str) -> Option<TypedFunctionValueSurface> {
    let signature = signature.trim();
    if !(signature.contains("=>") && signature.contains('=')) {
        return None;
    }
    let (left_side, _) = signature.split_once('=')?;
    let (before_type, declared_type) = left_side.rsplit_once(':')?;
    let name = before_type
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|part| !part.is_empty())?;
    let name_terms = symbol_surface_terms(name);
    let declared_type_terms = symbol_surface_terms(declared_type);
    if name_terms.is_empty() || declared_type_terms.is_empty() {
        return None;
    }

    Some(TypedFunctionValueSurface {
        name_terms,
        declared_type_terms,
        signature_terms: symbol_surface_terms(signature),
        exported: signature.starts_with("export ") || signature.starts_with("pub "),
    })
}

fn query_mentions_typed_function_value(query: &str, query_terms: &[String]) -> bool {
    query.contains("=>")
        || query_terms.iter().any(|term| {
            matches!(
                term.as_str(),
                "arrow" | "callback" | "closure" | "function" | "inline" | "lambda" | "typed"
            )
        })
}

fn terms_overlap(left: &[String], right: &[String]) -> bool {
    left.iter()
        .any(|left| right.iter().any(|right| related_surface_terms(left, right)))
}

fn related_surface_terms(left: &str, right: &str) -> bool {
    left == right
        || (left.len() >= 4
            && right.len() >= 4
            && (left.starts_with(right) || right.starts_with(left)))
}

fn symbol_surface_terms(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for token in value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
    {
        terms.push(token.to_ascii_lowercase());
        terms.extend(
            token
                .split('_')
                .filter(|part| !part.is_empty())
                .map(str::to_ascii_lowercase),
        );
        push_camel_surface_terms(token, &mut terms);
    }
    terms.sort();
    terms.dedup();

    terms
}

fn push_camel_surface_terms(token: &str, terms: &mut Vec<String>) {
    let chars = token.char_indices().collect::<Vec<_>>();
    if chars.is_empty() {
        return;
    }

    let mut start = 0usize;
    for index in 1..chars.len() {
        let previous = chars[index - 1].1;
        let current = chars[index].1;
        let next = chars.get(index + 1).map(|(_, character)| *character);
        let starts_word = previous.is_ascii_lowercase() && current.is_ascii_uppercase();
        let ends_acronym = previous.is_ascii_uppercase()
            && current.is_ascii_uppercase()
            && next.is_some_and(|character| character.is_ascii_lowercase());
        let changes_kind = previous.is_ascii_alphabetic() != current.is_ascii_alphabetic();
        if starts_word || ends_acronym || changes_kind {
            terms.push(token[start..chars[index].0].to_ascii_lowercase());
            start = chars[index].0;
        }
    }
    terms.push(token[start..].to_ascii_lowercase());
}

#[cfg(test)]
#[path = "typed_function_value_tests.rs"]
mod tests;
