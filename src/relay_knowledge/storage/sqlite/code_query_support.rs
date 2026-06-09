use rusqlite::types::Value;

use super::super::code_query_hits::chunk_layers;
use super::code_query_identifiers::identifier_terms_equivalent;
use crate::domain::{
    CodeQueryKind, CodeRepositoryStatus, CodeRetrievalLayer, CodeRetrievalRequest,
};

#[path = "code_query_conversion_scoring.rs"]
mod code_query_conversion_scoring;
#[path = "code_query_declaration_scoring.rs"]
mod code_query_declaration_scoring;
#[path = "code_query_filters.rs"]
mod code_query_filters;
#[path = "code_query_fts.rs"]
mod code_query_fts;

use code_query_conversion_scoring::conversion_symbol_bonus;
pub(super) use code_query_declaration_scoring::declaration_chunk_bonus;
pub(super) use code_query_filters::*;
pub(super) use code_query_fts::{
    compound_hybrid_chunk_fts_match_query, direct_hybrid_chunk_fts_match_query,
    focused_hybrid_chunk_fts_match_query, focused_symbol_fts_match_query, fts_match_query,
    hybrid_chunk_fts_match_query, lifecycle_hybrid_chunk_fts_match_query,
    strict_hybrid_chunk_fts_match_query, structured_hybrid_chunk_fts_match_query,
    symbol_fts_match_query,
};

#[cfg(test)]
use super::MAX_CANDIDATE_BIND_VALUES;

pub(super) struct ScoreQuery {
    tokens: Vec<String>,
}

pub(super) struct SymbolIdentityQuery {
    leaf_name: String,
    scoped_terms: Option<Vec<String>>,
}

struct ScoreField<'field> {
    original: &'field str,
    lower: Option<String>,
    identifier_terms: Option<Vec<String>>,
}

impl<'field> ScoreField<'field> {
    fn new(field: &'field str) -> Self {
        Self {
            original: field.trim(),
            lower: None,
            identifier_terms: None,
        }
    }

    fn lower(&mut self) -> &str {
        self.lower
            .get_or_insert_with(|| self.original.to_lowercase())
            .as_str()
    }

    fn matches_lower_token(&mut self, token: &str) -> bool {
        if self.original.is_ascii() && token.is_ascii() {
            self.original.eq_ignore_ascii_case(token)
        } else {
            self.lower() == token
        }
    }

    fn matches_identifier_token(&mut self, token: &str, cache_terms: bool) -> bool {
        if !cache_terms {
            return identifier_field_matches_token(self.original, token);
        }
        let terms = self
            .identifier_terms
            .get_or_insert_with(|| identifier_match_terms(self.original));
        terms
            .iter()
            .any(|term| identifier_terms_equivalent(term, token))
    }
}

impl ScoreQuery {
    pub(super) fn new(query: &str) -> Self {
        let tokens = score_query_tokens(query);

        Self { tokens }
    }

    pub(super) fn score<'field>(&self, fields: impl IntoIterator<Item = &'field str>) -> f64 {
        let mut fields = fields.into_iter().map(ScoreField::new).collect::<Vec<_>>();
        let cache_identifier_terms = self.tokens.len() > 1;
        let mut score = 0.0;
        for token in &self.tokens {
            let mut token_score = 0.0_f64;
            for field in &mut fields {
                if field.matches_lower_token(token) {
                    token_score = token_score.max(4.0);
                    break;
                } else if token_score < 2.0
                    && field.matches_identifier_token(token, cache_identifier_terms)
                {
                    token_score = token_score.max(2.0);
                } else if token_score < 0.5 && field.lower().contains(token) {
                    token_score = token_score.max(0.5);
                }
            }
            score += token_score;
        }

        score
    }
}

impl SymbolIdentityQuery {
    pub(super) fn from_query(query: &str) -> Option<Self> {
        for raw_token in query.split_whitespace().map(str::trim) {
            if raw_token.contains('/')
                || raw_token.contains('\\')
                || token_has_path_like_extension(raw_token)
            {
                continue;
            }
            if raw_token.contains("::") || raw_token.contains('.') {
                let terms = identity_terms(raw_token);
                if terms.len() >= 2 {
                    return Some(Self {
                        leaf_name: terms.last()?.clone(),
                        scoped_terms: Some(
                            terms
                                .into_iter()
                                .map(|term| term.to_ascii_lowercase())
                                .collect(),
                        ),
                    });
                }
            }
        }

        let mut tokens = query.split_whitespace().map(str::trim);
        let token = tokens.next()?;
        if tokens.next().is_some() || !simple_identity_token(token) {
            return None;
        }

        Some(Self {
            leaf_name: token.to_owned(),
            scoped_terms: None,
        })
    }

    pub(super) fn leaf_name(&self) -> &str {
        &self.leaf_name
    }

    pub(super) fn is_scoped(&self) -> bool {
        self.scoped_terms.is_some()
    }

    pub(super) fn scoped_like_pattern(&self) -> Option<String> {
        let scoped_terms = self.scoped_terms.as_ref()?;
        let mut pattern = String::from("%");
        for term in scoped_terms {
            pattern.push_str(&escape_sql_like(term));
            pattern.push('%');
        }

        Some(pattern)
    }

    pub(super) fn matches_symbol(
        &self,
        name: &str,
        qualified_name: &str,
        signature: &str,
        canonical_symbol_id: &str,
    ) -> bool {
        if name != self.leaf_name {
            return false;
        }
        let Some(scoped_terms) = &self.scoped_terms else {
            return true;
        };

        [qualified_name, signature, canonical_symbol_id]
            .iter()
            .any(|field| contains_scoped_terms(field, scoped_terms))
    }
}

pub(super) fn query_is_single_symbol_identity(query: &str) -> bool {
    let mut tokens = query.split_whitespace();
    let Some(token) = tokens.next() else {
        return false;
    };

    tokens.next().is_none() && SymbolIdentityQuery::from_query(token).is_some()
}

fn identity_terms(token: &str) -> Vec<String> {
    token
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect()
}

fn simple_identity_token(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

const MIN_DECOMPOSED_SCORE_TERM_LEN: usize = 2;

fn score_query_tokens(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for raw_token in query.split_whitespace().map(str::trim) {
        if raw_token.is_empty() {
            continue;
        }
        push_score_query_token(&mut tokens, raw_token.to_ascii_lowercase());
        if !raw_score_token_allows_decomposition(raw_token) {
            continue;
        }
        for term in raw_token
            .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .filter(|term| term.len() >= MIN_DECOMPOSED_SCORE_TERM_LEN)
        {
            push_score_query_token(&mut tokens, term.to_ascii_lowercase());
        }
    }

    tokens
}

fn raw_score_token_allows_decomposition(token: &str) -> bool {
    !(token.contains('/') || token.contains('\\') || token_has_path_like_extension(token))
}

fn token_has_path_like_extension(token: &str) -> bool {
    let token = token.trim_matches(|character: char| {
        !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
    });
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

fn push_score_query_token(tokens: &mut Vec<String>, token: String) {
    if !token.is_empty() && !tokens.contains(&token) {
        tokens.push(token);
    }
}

pub(super) fn score_text<'field>(
    query: &str,
    fields: impl IntoIterator<Item = &'field str>,
) -> f64 {
    ScoreQuery::new(query).score(fields)
}

pub(super) fn score_exact_path(query: &str, path: &str) -> f64 {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return 0.0;
    }
    let path = path.trim().to_lowercase();
    if path == query {
        return 4.0;
    }
    if path.rsplit('/').next().is_some_and(|name| name == query) {
        return 2.0;
    }

    0.0
}

pub(super) fn symbol_kind_bonus(kind: &str, request: &CodeRetrievalRequest) -> f64 {
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

pub(super) fn symbol_name_query_bonus(
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

fn identifier_field_matches_token(field: &str, token: &str) -> bool {
    identifier_tokens(field).any(|candidate| {
        identifier_terms_equivalent(candidate, token)
            || candidate
                .split('_')
                .filter(|part| !part.is_empty())
                .any(|part| identifier_terms_equivalent(part, token))
            || camel_case_terms(candidate)
                .iter()
                .any(|part| identifier_terms_equivalent(part, token))
    })
}

pub(super) fn symbol_query_bonus(
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

pub(super) fn scoped_identity_query_bonus(
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

pub(super) fn symbol_excerpt(
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

fn scoped_query_terms(query: &str) -> Option<Vec<String>> {
    let scoped_token = query
        .split_whitespace()
        .find(|token| token.contains("::") || token.contains('.'))?;
    let terms = scoped_terms(scoped_token);
    (terms.len() >= 2).then_some(terms)
}

fn contains_scoped_terms(field: &str, query_terms: &[String]) -> bool {
    if query_terms.is_empty() {
        return false;
    }
    let field_terms = scoped_terms(field);
    field_terms
        .windows(query_terms.len())
        .any(|window| window == query_terms)
        || contains_constructor_nested_scoped_terms(&field_terms, query_terms)
}

fn contains_constructor_nested_scoped_terms(
    field_terms: &[String],
    query_terms: &[String],
) -> bool {
    if query_terms.len() != 2 {
        return false;
    }
    for start in 0..field_terms.len().saturating_sub(2) {
        if field_terms[start] != query_terms[0] || field_terms[start + 1] != query_terms[0] {
            continue;
        }
        let tail = &field_terms[start + 2..];
        let Some(leaf_index) = tail.iter().position(|term| term == &query_terms[1]) else {
            continue;
        };
        if tail[..leaf_index]
            .iter()
            .all(|term| matches!(term.as_str(), "constructor" | "init" | "new"))
        {
            return true;
        }
    }

    false
}

fn scoped_terms(value: &str) -> Vec<String> {
    value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

pub(super) fn call_edge_confidence_bonus(confidence_basis_points: u16) -> f64 {
    f64::from(confidence_basis_points) / 10_000.0
}

pub(super) fn repeated_call_site_bonus(
    base_score: f64,
    call_site_count: usize,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0
        || request.code_query_kind != CodeQueryKind::Callers
        || call_site_count <= 1
    {
        return 0.0;
    }

    (call_site_count.saturating_sub(1).min(3) as f64) * 0.25
}

pub(super) fn callee_related_name_bonus(
    query: &str,
    callee_name: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if request.code_query_kind != CodeQueryKind::Callees {
        return 0.0;
    }
    let query_tokens = identifier_search_tokens(query);
    if query_tokens.is_empty() {
        return 0.0;
    }
    let callee_tokens = identifier_search_tokens(callee_name);
    if query_tokens.iter().any(|query_token| {
        query_token.len() > 2
            && callee_tokens
                .iter()
                .any(|callee_token| callee_token == query_token)
    }) {
        if callee_name_is_query_fragment(&query_tokens, &callee_tokens) {
            0.15
        } else {
            0.35 + (1.2 / callee_identifier_part_count(callee_name))
        }
    } else {
        0.0
    }
}

fn callee_name_is_query_fragment(query_tokens: &[String], callee_tokens: &[String]) -> bool {
    !callee_tokens.is_empty()
        && query_tokens.len() > callee_tokens.len()
        && callee_tokens
            .iter()
            .all(|callee| query_tokens.iter().any(|query| query == callee))
}

pub(super) fn directional_call_context_bonus(
    query: &ScoreQuery,
    base_score: f64,
    caller_name: Option<&str>,
    callee_name: &str,
    path: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0 {
        return 0.0;
    }
    match request.code_query_kind {
        CodeQueryKind::Callers => 0.35 * query.score([caller_name.unwrap_or_default(), path]),
        CodeQueryKind::Callees => 0.35 * query.score([callee_name, path]),
        _ => 0.0,
    }
}

pub(super) fn same_named_caller_penalty(
    caller_name: Option<&str>,
    callee_name: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if request.code_query_kind != CodeQueryKind::Callers {
        return 0.0;
    }
    let Some(caller_leaf) = caller_name.and_then(leaf_identifier) else {
        return 0.0;
    };
    let Some(callee_leaf) = leaf_identifier(callee_name) else {
        return 0.0;
    };
    let caller = compact_identifier(&caller_leaf);
    let callee = compact_identifier(&callee_leaf);
    if !caller.is_empty() && caller == callee {
        -2.5
    } else {
        0.0
    }
}

fn leaf_identifier(value: &str) -> Option<String> {
    value
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|term| !term.is_empty())
        .map(str::to_owned)
}

fn compact_identifier(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

fn callee_identifier_part_count(callee_name: &str) -> f64 {
    let part_count = identifier_tokens(callee_name)
        .flat_map(|token| token.split('_'))
        .filter(|part| !part.is_empty())
        .count()
        .max(1);

    part_count as f64
}

fn identifier_tokens(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
}

pub(super) fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect()
}

fn identifier_search_tokens(value: &str) -> Vec<String> {
    let mut tokens = identifier_match_terms(value);
    tokens.sort();
    tokens.dedup();

    tokens
}

fn identifier_match_terms(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for token in identifier_tokens(value) {
        tokens.push(token.to_ascii_lowercase());
        tokens.extend(
            token
                .split('_')
                .filter(|part| !part.is_empty())
                .map(str::to_ascii_lowercase),
        );
        tokens.extend(camel_case_terms(token));
    }

    tokens
}

fn camel_case_terms(token: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut start = 0;
    let mut previous: Option<char> = None;
    let chars = token.char_indices().collect::<Vec<_>>();
    for (index, (byte_index, character)) in chars.iter().enumerate() {
        let next = chars.get(index + 1).map(|(_, next)| *next);
        let starts_upper_word = character.is_ascii_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || next.is_some_and(|next| next.is_ascii_lowercase())
            });
        if *byte_index > start && starts_upper_word {
            terms.push(token[start..*byte_index].to_ascii_lowercase());
            start = *byte_index;
        }
        previous = Some(*character);
    }
    if start < token.len() {
        terms.push(token[start..].to_ascii_lowercase());
    }

    terms
}

#[cfg(test)]
pub(super) fn candidate_condition(fields: &[&str], query: &str) -> (String, Vec<Value>) {
    let max_patterns = (MAX_CANDIDATE_BIND_VALUES / fields.len().max(1)).max(1);
    let patterns = candidate_patterns(query, max_patterns);
    if patterns.is_empty() {
        return ("1 = 1".to_owned(), Vec::new());
    }

    let mut values = Vec::new();
    let groups = patterns
        .into_iter()
        .map(|pattern| {
            let clauses = fields
                .iter()
                .map(|field| {
                    values.push(Value::Text(pattern.clone()));
                    format!("{field} LIKE ?")
                })
                .collect::<Vec<_>>();
            format!("({})", clauses.join(" OR "))
        })
        .collect::<Vec<_>>();

    (groups.join(" OR "), values)
}

pub(super) fn candidate_patterns(query: &str, max_patterns: usize) -> Vec<String> {
    let mut patterns = Vec::new();
    for token in fts_query_terms(query) {
        let token = escape_sql_like(&token.to_lowercase());
        if token.is_empty() {
            continue;
        }
        let pattern = format!("%{token}%");
        if !patterns.contains(&pattern) {
            patterns.push(pattern);
        }
        if patterns.len() >= max_patterns {
            break;
        }
    }

    patterns
}

fn fts_query_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect()
}

#[derive(Clone, Copy)]
pub(super) enum CandidateLayer {
    Symbol,
    Reference,
    Call,
    Import,
    Sbom,
    Chunk,
}

pub(super) fn candidate_limit(request: &CodeRetrievalRequest, layer: CandidateLayer) -> usize {
    let requested = request.limit.max(1);
    let (multiplier, minimum, maximum) = match layer {
        CandidateLayer::Symbol => (40usize, 200usize, 800usize),
        CandidateLayer::Reference => (35, 200, 700),
        CandidateLayer::Call
            if matches!(
                request.code_query_kind,
                CodeQueryKind::Callers | CodeQueryKind::Callees
            ) =>
        {
            (100, 500, 1000)
        }
        CandidateLayer::Call => (40, 250, 800),
        CandidateLayer::Import => (35, 200, 700),
        CandidateLayer::Sbom => (35, 200, 700),
        CandidateLayer::Chunk => (45, 300, 900),
    };

    requested.saturating_mul(multiplier).clamp(minimum, maximum)
}

pub(super) fn chunk_layers_for_request(
    request: &CodeRetrievalRequest,
    parse_status: &str,
) -> Vec<CodeRetrievalLayer> {
    let mut layers = chunk_layers(parse_status);
    if request.code_query_kind == CodeQueryKind::References
        && SymbolIdentityQuery::from_query(&request.query).is_some()
        && !layers.contains(&CodeRetrievalLayer::TextFallback)
    {
        layers.push(CodeRetrievalLayer::TextFallback);
    }

    layers
}

pub(super) fn fts_values_for_limited_with_language(
    source_scope: &str,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    fts_query: &str,
    fts_limit: usize,
    limit: usize,
) -> Vec<Value> {
    let mut values = vec![
        Value::Text(source_scope.to_owned()),
        Value::Text(fts_query.to_owned()),
        Value::Text(source_scope.to_owned()),
    ];
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(fts_limit as i64));
    values.push(Value::Integer(limit as i64));

    values
}

pub(super) fn escape_sql_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(character);
    }

    escaped
}
