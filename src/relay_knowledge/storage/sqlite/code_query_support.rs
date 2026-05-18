use rusqlite::types::Value;

use super::code_query_identifiers::identifier_terms_equivalent;
use crate::domain::{CodeQueryKind, CodeRepositoryStatus, CodeRetrievalRequest};

#[cfg(test)]
use super::MAX_CANDIDATE_BIND_VALUES;

pub(super) struct ScoreQuery {
    tokens: Vec<String>,
}

struct ScoreField {
    original: String,
    lower: String,
    identifier_terms: Option<Vec<String>>,
}

impl ScoreField {
    fn new(field: impl AsRef<str>) -> Self {
        let original = field.as_ref().trim().to_owned();
        let lower = original.to_lowercase();

        Self {
            original,
            lower,
            identifier_terms: None,
        }
    }

    fn matches_identifier_token(&mut self, token: &str, cache_terms: bool) -> bool {
        if !cache_terms {
            return identifier_field_matches_token(&self.original, token);
        }
        let terms = self
            .identifier_terms
            .get_or_insert_with(|| identifier_match_terms(&self.original));
        terms
            .iter()
            .any(|term| identifier_terms_equivalent(term, token))
    }
}

impl ScoreQuery {
    pub(super) fn new(query: &str) -> Self {
        let tokens = query
            .split_whitespace()
            .map(str::to_lowercase)
            .filter(|token| !token.is_empty())
            .collect();

        Self { tokens }
    }

    pub(super) fn score(&self, fields: impl IntoIterator<Item = impl AsRef<str>>) -> f64 {
        let mut fields = fields.into_iter().map(ScoreField::new).collect::<Vec<_>>();
        let cache_identifier_terms = self.tokens.len() > 1;
        let mut score = 0.0;
        for token in &self.tokens {
            let mut token_score = 0.0_f64;
            for field in &mut fields {
                if field.lower == token.as_str() {
                    token_score = token_score.max(4.0);
                    break;
                } else if token_score < 2.0
                    && field.matches_identifier_token(token, cache_identifier_terms)
                {
                    token_score = token_score.max(2.0);
                } else if token_score < 0.5 && field.lower.contains(token) {
                    token_score = token_score.max(0.5);
                }
            }
            score += token_score;
        }

        score
    }
}

pub(super) fn score_text(query: &str, fields: impl IntoIterator<Item = impl AsRef<str>>) -> f64 {
    ScoreQuery::new(query).score(fields)
}

pub(super) fn declaration_chunk_bonus(terms: &[String], content: &str) -> f64 {
    let abstract_interface = terms.iter().any(|term| term == "interface")
        && content.contains("virtual ")
        && (content.contains("= 0;") || content.contains("=0;"));
    let relationship_declaration = terms.iter().any(|term| type_relationship_intent(term))
        && content_has_type_relationship_declaration(content);
    let declaration_lines = if abstract_interface {
        0
    } else {
        content
            .lines()
            .map(str::trim)
            .filter(|line| declaration_line_is_prototype(line))
            .take(2)
            .count()
    };
    if !abstract_interface && declaration_lines < 2 && !relationship_declaration {
        return 0.0;
    }

    let lower_content = content.to_lowercase();
    let matched_terms = terms
        .iter()
        .filter(|term| {
            term.len() >= 3
                && (identifier_field_matches_token(content, term)
                    || lower_content.contains(term.as_str()))
        })
        .count();
    if matched_terms < 3 {
        return 0.0;
    }

    if abstract_interface {
        3.0
    } else if relationship_declaration {
        2.75
    } else if declaration_lines >= 2 {
        2.0
    } else {
        0.0
    }
}

fn type_relationship_intent(term: &str) -> bool {
    matches!(
        term,
        "derive"
            | "derived"
            | "extend"
            | "extends"
            | "implement"
            | "implements"
            | "inherit"
            | "inheritance"
            | "inherited"
            | "inherits"
            | "override"
            | "overrides"
            | "overriding"
            | "subclass"
            | "subclasses"
    )
}

fn content_has_type_relationship_declaration(content: &str) -> bool {
    content.lines().map(str::trim).any(|line| {
        if line.starts_with("//") || line.starts_with('*') {
            return false;
        }
        let type_declaration = line
            .split(|character: char| character.is_whitespace() || matches!(character, '{' | '('))
            .filter(|word| !word.is_empty())
            .take(4)
            .any(|word| matches!(word, "class" | "struct" | "interface"));
        type_declaration
            && (line.contains(" : ")
                || line.contains(": public ")
                || line.contains(": protected ")
                || line.contains(": private ")
                || line.contains(" extends ")
                || line.contains(" implements "))
    })
}

fn declaration_line_is_prototype(line: &str) -> bool {
    line.ends_with(';')
        && line.contains('(')
        && !line.contains("->")
        && !line.contains('.')
        && !line.starts_with("return ")
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

pub(super) fn symbol_query_bonus(
    query: &str,
    name: &str,
    qualified_name: &str,
    signature: &str,
    canonical_symbol_id: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    let name_bonus = symbol_name_query_bonus(query, name, request);
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
    if !(query.contains("::") || query.contains('.')) {
        return None;
    }
    let terms = scoped_terms(query);
    (terms.len() >= 2).then_some(terms)
}

fn contains_scoped_terms(field: &str, query_terms: &[String]) -> bool {
    let field_terms = scoped_terms(field);
    field_terms
        .windows(query_terms.len())
        .any(|window| window == query_terms)
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
        0.35 + (1.2 / callee_identifier_part_count(callee_name))
    } else {
        0.0
    }
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
        -2.0
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

pub(super) fn call_excerpt(caller_excerpt: Option<&str>, caller: &str, callee: &str) -> String {
    let summary = format!("{caller} calls {callee}");
    let Some(site) = caller_excerpt
        .map(str::trim)
        .filter(|excerpt| !excerpt.is_empty())
        .map(|excerpt| call_site_excerpt(excerpt, callee))
    else {
        return summary;
    };

    if site.is_empty() || site == summary {
        summary
    } else {
        format!("{summary}: {site}")
    }
}

fn call_site_excerpt(caller_excerpt: &str, callee: &str) -> String {
    let matching_line = caller_excerpt
        .lines()
        .find(|line| line_looks_like_call_to(line, callee))
        .or_else(|| caller_excerpt.lines().find(|line| line.contains(callee)));
    matching_line
        .map(compact_excerpt_line)
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| compact_excerpt_line(caller_excerpt))
}

fn line_looks_like_call_to(line: &str, callee: &str) -> bool {
    let Some((_, suffix)) = line.split_once(callee) else {
        return false;
    };
    let suffix = suffix.trim_start();
    suffix.starts_with('(') || (suffix.starts_with('<') && suffix.contains('('))
}

fn compact_excerpt_line(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
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

fn candidate_patterns(query: &str, max_patterns: usize) -> Vec<String> {
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

pub(super) fn candidate_limit(request: &CodeRetrievalRequest) -> usize {
    request.limit.saturating_mul(100).clamp(500, 2000)
}

pub(super) fn fts_match_query(query: &str) -> String {
    fts_match_query_with_operator(query, " ", true)
}

pub(super) fn symbol_fts_match_query(query: &str) -> String {
    fts_match_query_with_operator(query, " OR ", true)
}

fn fts_match_query_with_operator(
    query: &str,
    operator: &str,
    include_compound_identifiers: bool,
) -> String {
    let terms = fts_query_terms(query);

    if terms.is_empty() {
        return "relayknowledgeunlikelyemptyquerytoken".to_owned();
    }

    let primary = terms
        .iter()
        .map(|term| quote_fts_term(term))
        .collect::<Vec<_>>()
        .join(operator);
    let alternatives = if include_compound_identifiers {
        compound_identifier_fts_terms(&terms)
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

fn fts_query_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect()
}

fn quote_fts_term(term: &str) -> String {
    format!("\"{}\"", term.replace('"', "\"\""))
}

const MAX_COMPOUND_QUERY_TERMS: usize = 6;
const MAX_COMPOUND_IDENTIFIER_PARTS: usize = 8;
const MIN_COMPOUND_IDENTIFIER_LEN: usize = 6;
const MAX_COMPOUND_IDENTIFIER_LEN: usize = 80;
const MIN_SUBPHRASE_IDENTIFIER_PARTS: usize = 2;
const MAX_SUBPHRASE_IDENTIFIER_PARTS: usize = 4;
const MAX_COMPOUND_FTS_ALTERNATIVES: usize = 24;

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

pub(super) fn hybrid_chunk_fts_match_query(query: &str) -> String {
    let terms = fts_query_terms(query);

    if terms.is_empty() {
        "relayknowledgeunlikelyemptyquerytoken".to_owned()
    } else if terms.len() == 1 {
        quote_fts_term(&terms[0])
    } else {
        fts_match_query_with_operator(query, " OR ", true)
    }
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

pub(super) fn fts_values_for_limited_with_language_and_call_direction(
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
    push_call_direction_filter_values(&mut values, request);
    values.push(Value::Integer(fts_limit as i64));
    values.push(Value::Integer(limit as i64));

    values
}

pub(super) fn call_direction_fts_filter_sql(request: &CodeRetrievalRequest) -> String {
    let Some(column) = call_direction_filter_column(request.code_query_kind) else {
        return String::new();
    };
    let patterns = candidate_patterns(&request.query, 8);
    if patterns.is_empty() {
        return String::new();
    }
    let clauses = patterns
        .iter()
        .map(|_| format!("{column} LIKE ? ESCAPE '\\'"))
        .collect::<Vec<_>>()
        .join(" OR ");

    format!(
        "AND EXISTS (
            SELECT 1
            FROM code_repository_calls call_filter
            WHERE call_filter.source_scope = code_repository_search.source_scope
              AND call_filter.call_id = code_repository_search.record_id
              AND ({clauses})
        )"
    )
}

fn push_call_direction_filter_values(values: &mut Vec<Value>, request: &CodeRetrievalRequest) {
    if call_direction_filter_column(request.code_query_kind).is_none() {
        return;
    }
    values.extend(
        candidate_patterns(&request.query, 8)
            .into_iter()
            .map(Value::Text),
    );
}

fn call_direction_filter_column(kind: CodeQueryKind) -> Option<&'static str> {
    match kind {
        CodeQueryKind::Callers => Some("lower(call_filter.callee_name)"),
        CodeQueryKind::Callees => Some("lower(coalesce(call_filter.caller_name, ''))"),
        _ => None,
    }
}

pub(super) fn fts_path_and_language_filter_sql(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> String {
    let mut clauses = Vec::new();
    push_path_filter_sql(&mut clauses, "path", &status.path_filters);
    push_path_filter_sql(&mut clauses, "path", &request.repository.path_filters);
    push_language_filter_sql(&mut clauses, "language_id", &status.language_filters);
    push_language_filter_sql(
        &mut clauses,
        "language_id",
        &request.repository.language_filters,
    );
    if clauses.is_empty() {
        String::new()
    } else {
        format!("AND {}", clauses.join(" AND "))
    }
}

pub(super) fn path_filter_sql_for_column(
    column: &str,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> String {
    let mut clauses = Vec::new();
    push_path_filter_sql(&mut clauses, column, &status.path_filters);
    push_path_filter_sql(&mut clauses, column, &request.repository.path_filters);
    if clauses.is_empty() {
        String::new()
    } else {
        format!("AND {}", clauses.join(" AND "))
    }
}

pub(super) fn language_filter_sql_for_column(
    column: &str,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> String {
    let mut clauses = Vec::new();
    push_language_filter_sql(&mut clauses, column, &status.language_filters);
    push_language_filter_sql(&mut clauses, column, &request.repository.language_filters);
    if clauses.is_empty() {
        String::new()
    } else {
        format!("AND {}", clauses.join(" AND "))
    }
}

fn push_path_filter_sql(clauses: &mut Vec<String>, column: &str, filters: &[String]) {
    let clauses_for_filters = filters
        .iter()
        .filter_map(|filter| normalized_sql_path_filter(filter))
        .map(|_| format!("({column} = ? OR {column} LIKE ? ESCAPE '\\')"))
        .collect::<Vec<_>>();
    if !clauses_for_filters.is_empty() {
        clauses.push(format!("({})", clauses_for_filters.join(" OR ")));
    }
}

fn push_language_filter_sql(clauses: &mut Vec<String>, column: &str, filters: &[String]) {
    let clauses_for_filters = filters
        .iter()
        .map(|_| format!("{column} = ?"))
        .collect::<Vec<_>>();
    if !clauses_for_filters.is_empty() {
        clauses.push(format!("({})", clauses_for_filters.join(" OR ")));
    }
}

pub(super) fn push_path_filter_values(values: &mut Vec<Value>, filters: &[String]) {
    for filter in filters
        .iter()
        .filter_map(|filter| normalized_sql_path_filter(filter))
    {
        values.push(Value::Text(filter.clone()));
        values.push(Value::Text(format!("{}/%", escape_sql_like(&filter))));
    }
}

pub(super) fn push_language_filter_values(values: &mut Vec<Value>, filters: &[String]) {
    values.extend(filters.iter().cloned().map(Value::Text));
}

fn normalized_sql_path_filter(filter: &str) -> Option<String> {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }
    (!filter.is_empty() && filter != ".").then(|| filter.to_owned())
}

fn escape_sql_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(character);
    }

    escaped
}
