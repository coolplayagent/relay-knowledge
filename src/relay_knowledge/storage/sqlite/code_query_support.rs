use rusqlite::types::Value;

use crate::domain::{CodeQueryKind, CodeRepositoryStatus, CodeRetrievalRequest};

#[cfg(test)]
use super::MAX_CANDIDATE_BIND_VALUES;

pub(super) fn score_text(query: &str, fields: impl IntoIterator<Item = impl AsRef<str>>) -> f64 {
    let fields = fields
        .into_iter()
        .map(|field| {
            let original = field.as_ref().trim().to_owned();
            let lower = original.to_lowercase();
            (original, lower)
        })
        .collect::<Vec<_>>();
    let mut score = 0.0;
    for token in query.split_whitespace() {
        let token = token.to_lowercase();
        if token.is_empty() {
            continue;
        }
        let mut token_score = 0.0_f64;
        for (field, lower_field) in &fields {
            if lower_field == &token {
                token_score = token_score.max(4.0);
            } else if identifier_field_matches_token(field, &token) {
                token_score = token_score.max(2.0);
            } else if lower_field.contains(&token) {
                token_score = token_score.max(0.5);
            }
        }
        score += token_score;
    }

    score
}

pub(super) fn declaration_chunk_bonus(terms: &[String], content: &str) -> f64 {
    let abstract_interface = terms.iter().any(|term| term == "interface")
        && content.contains("virtual ")
        && (content.contains("= 0;") || content.contains("=0;"));
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
    if !abstract_interface && declaration_lines < 2 {
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
    } else if declaration_lines >= 2 {
        2.0
    } else {
        0.0
    }
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
        candidate.eq_ignore_ascii_case(token)
            || candidate
                .split('_')
                .filter(|part| !part.is_empty())
                .any(|part| part.eq_ignore_ascii_case(token))
            || camel_case_terms(candidate)
                .iter()
                .any(|part| part.eq_ignore_ascii_case(token))
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
    if query_terms
        .iter()
        .all(|term| name_tokens.iter().any(|token| token == term))
    {
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
                    token == *term
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
    query: &str,
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
        CodeQueryKind::Callers => 0.35 * score_text(query, [caller_name.unwrap_or_default(), path]),
        CodeQueryKind::Callees => 0.35 * score_text(query, [callee_name, path]),
        _ => 0.0,
    }
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
    caller_excerpt
        .lines()
        .find(|line| line.contains(callee))
        .map(compact_excerpt_line)
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| compact_excerpt_line(caller_excerpt))
}

fn compact_excerpt_line(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn import_line_priority(base_score: f64, line_start: u32) -> f64 {
    if base_score <= 0.0 {
        return 0.0;
    }

    1.0 / f64::from(line_start.clamp(1, 1_000))
}

pub(super) fn import_surface_bonus(base_score: f64, path: &str) -> f64 {
    if base_score <= 0.0 {
        return 0.0;
    }
    if path
        .split('/')
        .any(|segment| matches!(segment, "test" | "tests" | "__tests__"))
    {
        return 0.0;
    }
    match path.rsplit('/').next().unwrap_or(path) {
        "__init__.py" | "mod.rs" | "lib.rs" | "index.js" | "index.jsx" | "index.ts"
        | "index.tsx" => 0.2,
        _ => 0.0,
    }
}

pub(super) fn import_target_symbol_bonus(query: &str, matched_symbol_name: Option<&str>) -> f64 {
    let Some(matched_symbol_name) = matched_symbol_name else {
        return 0.0;
    };
    let terms = query_terms(query);
    let Some(term) = terms.last() else {
        return 0.0;
    };
    if term.len() >= 3
        && matched_symbol_name
            .split_whitespace()
            .any(|name| name.eq_ignore_ascii_case(term))
    {
        2.0
    } else {
        0.0
    }
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
    tokens.sort();
    tokens.dedup();

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

#[cfg(test)]
fn candidate_patterns(query: &str, max_patterns: usize) -> Vec<String> {
    let mut patterns = Vec::new();
    for token in query.to_lowercase().split_whitespace() {
        let token = token.chars().filter(|ch| *ch != '%').collect::<String>();
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
    fts_match_query_with_operator(query, " ")
}

pub(super) fn symbol_fts_match_query(query: &str) -> String {
    fts_match_query_with_operator(query, " OR ")
}

fn fts_match_query_with_operator(query: &str, operator: &str) -> String {
    let terms = query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>();

    if terms.is_empty() {
        "relayknowledgeunlikelyemptyquerytoken".to_owned()
    } else {
        terms.join(operator)
    }
}

pub(super) fn hybrid_chunk_fts_match_query(query: &str) -> String {
    let terms = query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>();

    if terms.is_empty() {
        "relayknowledgeunlikelyemptyquerytoken".to_owned()
    } else if terms.len() == 1 {
        terms[0].clone()
    } else {
        terms.join(" OR ")
    }
}

pub(super) fn fts_values_for_limited(
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
    push_fts_path_filter_values(&mut values, &status.path_filters);
    push_fts_path_filter_values(&mut values, &request.repository.path_filters);
    values.push(Value::Integer(fts_limit as i64));
    values.push(Value::Integer(limit as i64));

    values
}

pub(super) fn fts_path_filter_sql(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> String {
    let mut clauses = Vec::new();
    push_fts_path_filter_sql(&mut clauses, &status.path_filters);
    push_fts_path_filter_sql(&mut clauses, &request.repository.path_filters);
    if clauses.is_empty() {
        String::new()
    } else {
        format!("AND {}", clauses.join(" AND "))
    }
}

fn push_fts_path_filter_sql(clauses: &mut Vec<String>, filters: &[String]) {
    let clauses_for_filters = filters
        .iter()
        .filter_map(|filter| normalized_sql_path_filter(filter))
        .map(|_| "(path = ? OR path LIKE ? ESCAPE '\\')".to_owned())
        .collect::<Vec<_>>();
    if !clauses_for_filters.is_empty() {
        clauses.push(format!("({})", clauses_for_filters.join(" OR ")));
    }
}

fn push_fts_path_filter_values(values: &mut Vec<Value>, filters: &[String]) {
    for filter in filters
        .iter()
        .filter_map(|filter| normalized_sql_path_filter(filter))
    {
        values.push(Value::Text(filter.clone()));
        values.push(Value::Text(format!("{}/%", escape_sql_like(&filter))));
    }
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
