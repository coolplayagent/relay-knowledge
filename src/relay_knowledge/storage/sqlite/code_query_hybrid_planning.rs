use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

use super::{
    code_query_api_identities,
    code_query_conversion_terms::conversion_action_term,
    code_query_support::{query_is_single_symbol_identity, query_terms},
};

const API_CHUNK_FIRST_MIN_TERMS: usize = 4;
const STRUCTURED_SEQUENCE_MIN_TERMS: usize = 4;
const STRUCTURED_SEQUENCE_MIN_STRUCTURED_TERMS: usize = 3;
const PROCEDURAL_LANGUAGE_MIN_TERMS: usize = 6;
const PROCEDURAL_LANGUAGE_MIN_HIGH_SIGNAL_TERMS: usize = 5;
const WORKFLOW_SURFACE_MIN_TERMS: usize = 5;
const WORKFLOW_SURFACE_MIN_HIGH_SIGNAL_TERMS: usize = 4;
const WORKFLOW_SURFACE_MIN_DATAFLOW_TERMS: usize = 2;
const CONTEXTUAL_API_SURFACE_MIN_TERMS: usize = 5;
const CONTEXTUAL_API_SURFACE_MIN_CONTEXT_TERMS: usize = 3;
const TYPED_DATAFLOW_SURFACE_MIN_TERMS: usize = 6;
const TYPED_DATAFLOW_SURFACE_MIN_HIGH_SIGNAL_TERMS: usize = 5;
const TYPED_DATAFLOW_SURFACE_MIN_DATAFLOW_TERMS: usize = 3;
const HIGH_SIGNAL_TERM_LEN: usize = 5;

enum HybridChunkFirstPlan {
    ApiIdentities,
    ContextualSurface,
    StructuredSequence,
    FilteredProceduralSurface,
    WorkflowSurface {
        query_language_scopes: Vec<&'static str>,
    },
}

pub(super) fn hybrid_query_prefers_chunk_first(request: &CodeRetrievalRequest) -> bool {
    hybrid_chunk_first_plan(request).is_some()
}

pub(super) fn hybrid_query_requires_chunk_first_before_symbols(
    request: &CodeRetrievalRequest,
) -> bool {
    matches!(
        hybrid_chunk_first_plan(request),
        Some(
            HybridChunkFirstPlan::ApiIdentities
                | HybridChunkFirstPlan::StructuredSequence
                | HybridChunkFirstPlan::FilteredProceduralSurface
                | HybridChunkFirstPlan::WorkflowSurface { .. }
        )
    )
}

pub(super) fn query_language_scoped_workflow_surface_scopes(
    request: &CodeRetrievalRequest,
) -> Vec<&'static str> {
    match hybrid_chunk_first_plan(request) {
        Some(HybridChunkFirstPlan::WorkflowSurface {
            query_language_scopes,
        }) => query_language_scopes,
        Some(
            HybridChunkFirstPlan::ApiIdentities
            | HybridChunkFirstPlan::ContextualSurface
            | HybridChunkFirstPlan::StructuredSequence
            | HybridChunkFirstPlan::FilteredProceduralSurface,
        )
        | None => Vec::new(),
    }
}

pub(super) fn workflow_language_scope_language_ids(scope: &str) -> &'static [&'static str] {
    match scope {
        "csharp" => &["cs", "csharp"],
        "go" => &["go"],
        "java" => &["java"],
        "javascript" => &["javascript", "js", "jsx"],
        "kotlin" => &["kotlin", "kt"],
        "php" => &["php"],
        "python" => &["py", "python"],
        "ruby" => &["rb", "ruby"],
        "rust" => &["rs", "rust"],
        "scala" => &["scala"],
        "swift" => &["swift"],
        "typescript" => &["ts", "tsx", "typescript"],
        _ => &[],
    }
}

fn hybrid_chunk_first_plan(request: &CodeRetrievalRequest) -> Option<HybridChunkFirstPlan> {
    if request.code_query_kind != CodeQueryKind::Hybrid
        || query_is_single_symbol_identity(&request.query)
    {
        return None;
    }

    let raw_terms = query_terms(&request.query);
    let terms = hybrid_sequence_terms_from_raw(raw_terms.iter().map(String::as_str));
    if terms.len() < API_CHUNK_FIRST_MIN_TERMS {
        return None;
    }
    if !code_query_api_identities::hybrid_api_symbol_identities(&request.query, request).is_empty()
    {
        return Some(HybridChunkFirstPlan::ApiIdentities);
    }

    if hybrid_query_has_structured_sequence(&raw_terms, &terms) {
        return Some(HybridChunkFirstPlan::StructuredSequence);
    }
    if hybrid_query_has_filtered_procedural_surface(request, &terms) {
        return Some(HybridChunkFirstPlan::FilteredProceduralSurface);
    }
    if let Some(plan) = workflow_surface_chunk_first_plan(request, &raw_terms, &terms) {
        return Some(plan);
    }
    if hybrid_query_has_contextual_api_surface(&raw_terms, &terms)
        || hybrid_query_has_typed_dataflow_surface(request, &raw_terms, &terms)
    {
        return Some(HybridChunkFirstPlan::ContextualSurface);
    }

    None
}

pub(super) fn hybrid_sequence_terms(query: &str) -> Vec<String> {
    let raw_terms = query_terms(query);
    hybrid_sequence_terms_from_raw(raw_terms.iter().map(String::as_str))
}

pub(super) fn hybrid_query_has_declaration_expansion_intent(query: &str) -> bool {
    hybrid_sequence_terms(query).iter().any(|term| {
        matches!(
            term.as_str(),
            "class"
                | "classes"
                | "exception"
                | "exceptions"
                | "extends"
                | "implement"
                | "implements"
                | "inherit"
                | "inheritance"
                | "inherits"
                | "interface"
                | "interfaces"
                | "mixin"
                | "mixins"
                | "module"
                | "modules"
                | "protocol"
                | "protocols"
                | "subclass"
                | "subclasses"
                | "trait"
                | "traits"
        )
    })
}

pub(super) fn hybrid_query_has_conversion_expansion_intent(query: &str) -> bool {
    let raw_terms = query_terms(query);
    let terms = hybrid_sequence_terms_from_raw(raw_terms.iter().map(String::as_str));
    let has_conversion = raw_terms.iter().any(|term| conversion_action_term(term));
    let has_chunk_or_common_surface = terms.iter().any(|term| {
        matches!(
            term.as_str(),
            "chunk"
                | "chunks"
                | "common"
                | "event"
                | "events"
                | "part"
                | "parts"
                | "provider"
                | "providers"
                | "response"
                | "responses"
        )
    });

    has_conversion && has_chunk_or_common_surface
}

pub(super) fn hybrid_query_has_inline_expansion_intent(query: &str) -> bool {
    hybrid_sequence_terms(query).iter().any(|term| {
        matches!(
            term.as_str(),
            "callback" | "callbacks" | "closure" | "closures" | "lambda" | "lambdas"
        )
    })
}

pub(super) fn workflow_language_scope_matches(language_id: &str, scope: &str) -> bool {
    workflow_language_family(language_id).is_some_and(|language_scope| language_scope == scope)
}

fn hybrid_sequence_terms_from_raw<'a>(raw_terms: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut terms = Vec::new();
    for term in raw_terms {
        if term.len() < API_CHUNK_FIRST_MIN_TERMS {
            continue;
        }
        let term = term.to_ascii_lowercase();
        if !terms.contains(&term) {
            terms.push(term);
        }
    }

    terms
}

fn hybrid_query_has_structured_sequence(raw_terms: &[String], terms: &[String]) -> bool {
    terms.len() >= STRUCTURED_SEQUENCE_MIN_TERMS
        && raw_terms
            .iter()
            .filter(|term| term.len() >= API_CHUNK_FIRST_MIN_TERMS)
            .filter(|term| hybrid_term_has_structure(term))
            .count()
            >= STRUCTURED_SEQUENCE_MIN_STRUCTURED_TERMS
}

fn hybrid_query_has_filtered_procedural_surface(
    request: &CodeRetrievalRequest,
    terms: &[String],
) -> bool {
    request
        .repository
        .language_filters
        .iter()
        .any(|language| procedural_chunk_first_language(language))
        && terms.len() >= PROCEDURAL_LANGUAGE_MIN_TERMS
        && terms
            .iter()
            .filter(|term| hybrid_sequence_term_has_high_signal(term))
            .count()
            >= PROCEDURAL_LANGUAGE_MIN_HIGH_SIGNAL_TERMS
}

fn hybrid_query_has_contextual_api_surface(raw_terms: &[String], terms: &[String]) -> bool {
    if terms.len() < CONTEXTUAL_API_SURFACE_MIN_TERMS {
        return false;
    }

    let contextual_api_terms = raw_terms
        .iter()
        .filter(|term| contextual_api_surface_term(term))
        .count();
    contextual_api_terms > 0
        && terms.len().saturating_sub(contextual_api_terms)
            >= CONTEXTUAL_API_SURFACE_MIN_CONTEXT_TERMS
        && terms
            .iter()
            .filter(|term| hybrid_sequence_term_has_high_signal(term))
            .count()
            >= WORKFLOW_SURFACE_MIN_HIGH_SIGNAL_TERMS
}

fn hybrid_query_has_typed_dataflow_surface(
    request: &CodeRetrievalRequest,
    raw_terms: &[String],
    terms: &[String],
) -> bool {
    let signal_terms = workflow_surface_signal_terms(raw_terms);
    request
        .repository
        .language_filters
        .iter()
        .any(|language| workflow_chunk_first_language(language))
        && terms.len() >= TYPED_DATAFLOW_SURFACE_MIN_TERMS
        && terms
            .iter()
            .filter(|term| hybrid_sequence_term_has_high_signal(term))
            .count()
            >= TYPED_DATAFLOW_SURFACE_MIN_HIGH_SIGNAL_TERMS
        && signal_terms
            .iter()
            .any(|term| typed_function_surface_term(term))
        && signal_terms
            .iter()
            .filter(|term| dataflow_surface_term(term))
            .count()
            >= TYPED_DATAFLOW_SURFACE_MIN_DATAFLOW_TERMS
}

fn workflow_surface_chunk_first_plan(
    request: &CodeRetrievalRequest,
    raw_terms: &[String],
    terms: &[String],
) -> Option<HybridChunkFirstPlan> {
    if terms.len() < WORKFLOW_SURFACE_MIN_TERMS
        || terms
            .iter()
            .filter(|term| hybrid_sequence_term_has_high_signal(term))
            .count()
            < WORKFLOW_SURFACE_MIN_HIGH_SIGNAL_TERMS
    {
        return None;
    }
    let query_language_scopes = query_workflow_language_scopes(request, raw_terms);
    let has_explicit_workflow_language = request
        .repository
        .language_filters
        .iter()
        .any(|language| workflow_chunk_first_language(language));
    if !has_explicit_workflow_language && query_language_scopes.is_empty() {
        return None;
    }

    let signal_terms = workflow_surface_signal_terms(raw_terms);
    let workflow_terms = signal_terms
        .iter()
        .filter(|term| workflow_surface_term(term))
        .count();
    let workflow_surface_matches = workflow_terms >= 2
        || workflow_terms == 1
            && signal_terms
                .iter()
                .filter(|term| dataflow_surface_term(term))
                .count()
                >= WORKFLOW_SURFACE_MIN_DATAFLOW_TERMS;

    workflow_surface_matches.then_some(HybridChunkFirstPlan::WorkflowSurface {
        query_language_scopes,
    })
}

fn hybrid_sequence_term_has_high_signal(term: &str) -> bool {
    term.chars().count() >= HIGH_SIGNAL_TERM_LEN
        || term.contains('_')
        || term_has_alpha_digit_mix(term)
}

fn hybrid_term_has_structure(term: &str) -> bool {
    term.contains('_') || term_has_alpha_digit_mix(term) || term_has_case_boundary(term)
}

fn contextual_api_surface_term(term: &str) -> bool {
    term.len() >= HIGH_SIGNAL_TERM_LEN && hybrid_term_has_structure(term)
}

fn procedural_chunk_first_language(language: &str) -> bool {
    matches!(
        language.to_ascii_lowercase().as_str(),
        "c" | "cc" | "cpp" | "c++" | "cxx" | "h" | "hh" | "hpp" | "hxx"
    )
}

fn query_workflow_language_scopes(
    request: &CodeRetrievalRequest,
    raw_terms: &[String],
) -> Vec<&'static str> {
    if !request.repository.language_filters.is_empty() {
        return Vec::new();
    }

    let mut scopes = Vec::new();
    for term in raw_terms {
        if let Some(scope) = workflow_chunk_first_query_scope(term) {
            if !scopes.contains(&scope) {
                scopes.push(scope);
            }
        }
    }

    scopes
}

fn workflow_chunk_first_query_scope(term: &str) -> Option<&'static str> {
    let term = term.to_ascii_lowercase();
    if matches!(
        term.as_str(),
        "cs" | "go" | "js" | "kt" | "py" | "rb" | "rs" | "ts"
    ) {
        return None;
    }

    workflow_language_family(&term)
}

fn workflow_chunk_first_language(language: &str) -> bool {
    workflow_language_family(language).is_some()
}

fn workflow_language_family(language: &str) -> Option<&'static str> {
    let language = language.to_ascii_lowercase();
    [
        "csharp",
        "go",
        "java",
        "javascript",
        "kotlin",
        "php",
        "python",
        "ruby",
        "rust",
        "scala",
        "swift",
        "typescript",
    ]
    .into_iter()
    .find(|scope| workflow_language_scope_language_ids(scope).contains(&language.as_str()))
}

fn workflow_surface_term(term: &str) -> bool {
    matches!(
        term.to_ascii_lowercase().as_str(),
        "async"
            | "await"
            | "callback"
            | "channel"
            | "closure"
            | "defer"
            | "dispatch"
            | "effect"
            | "event"
            | "goroutine"
            | "handler"
            | "lambda"
            | "listener"
            | "pipeline"
            | "promise"
            | "queue"
            | "stream"
            | "task"
            | "worker"
            | "workflow"
    )
}

fn workflow_surface_signal_terms(raw_terms: &[String]) -> Vec<String> {
    let mut terms = Vec::new();
    for raw in raw_terms {
        push_unique_signal_term(&mut terms, &raw.to_ascii_lowercase());
        for part in raw.split('_').filter(|part| !part.is_empty()) {
            push_unique_signal_term(&mut terms, &part.to_ascii_lowercase());
        }
    }

    terms
}

fn push_unique_signal_term(terms: &mut Vec<String>, term: &str) {
    if !terms.iter().any(|existing| existing == term) {
        terms.push(term.to_owned());
    }
}

fn dataflow_surface_term(term: &str) -> bool {
    matches!(
        term.to_ascii_lowercase().as_str(),
        "envelope"
            | "filter"
            | "message"
            | "normalize"
            | "payload"
            | "projector"
            | "provider"
            | "registry"
            | "request"
            | "response"
            | "transport"
    )
}

fn typed_function_surface_term(term: &str) -> bool {
    matches!(
        term.to_ascii_lowercase().as_str(),
        "arrow" | "callback" | "closure" | "function" | "lambda" | "typed"
    )
}

fn term_has_case_boundary(term: &str) -> bool {
    let mut previous = None;
    term.chars().any(|character| {
        let boundary = character.is_ascii_uppercase()
            && previous.is_some_and(|previous: char| previous.is_ascii_lowercase());
        previous = Some(character);
        boundary
    })
}

fn term_has_alpha_digit_mix(term: &str) -> bool {
    let mut has_alpha = false;
    let mut has_digit = false;
    for character in term.chars() {
        has_alpha |= character.is_ascii_alphabetic();
        has_digit |= character.is_ascii_digit();
    }

    has_alpha && has_digit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_expansion_intent_accepts_scored_conversion_verbs() {
        for verb in ["adapt", "map", "normalize"] {
            assert!(
                hybrid_query_has_conversion_expansion_intent(&format!(
                    "provider response parts {verb} shared event"
                )),
                "{verb} should request conversion expansion"
            );
        }
    }
}
