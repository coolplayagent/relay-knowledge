use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, RepositoryCodeRange,
    },
    storage::StorageError,
};

use super::{
    HitParts,
    code_query_path_ranking::{
        path_looks_like_test_or_benchmark, query_mentions_test_or_benchmark,
    },
    code_query_rows::CallRow,
    dedupe_sort_truncate, escape_sql_like, hit_from_parts, prepare_code_search_statement,
    required_scope, selected_row,
};

struct AmbiguousCalleeContext {
    callee_name: String,
    path: String,
    language_id: String,
    line_range: RepositoryCodeRange,
    target_hint: Option<String>,
    caller_name: Option<String>,
    caller_signature: Option<String>,
    caller_excerpt: Option<String>,
    caller_canonical_symbol_id: Option<String>,
}

struct CalleeImplementationCandidate {
    file_id: String,
    path: String,
    language_id: String,
    symbol_snapshot_id: String,
    canonical_symbol_id: String,
    name: String,
    signature: String,
    byte_range: RepositoryCodeRange,
    line_range: RepositoryCodeRange,
    body_excerpt: Option<String>,
    parse_status: String,
    degraded_reason: Option<String>,
}

const AMBIGUOUS_CALLEE_IMPLEMENTATION_LIMIT: usize = 120;
const AMBIGUOUS_CALLEE_CONTEXT_LIMIT: usize = 32;
const AMBIGUOUS_CALLEE_CONTEXT_MIN_SCORE: f64 = 1.5;
const AMBIGUOUS_CALLEE_IMPLEMENTATION_BASE_SCORE: f64 = 1.65;
const AMBIGUOUS_CALLEE_IMPLEMENTATION_MAX_SCORE: f64 = 5.25;
const AMBIGUOUS_CALLEE_TARGET_HINT_TERM_LIMIT: usize = 8;

pub(super) fn search_ambiguous_callee_implementation_hits(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    rows: &[CallRow],
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    if request.code_query_kind != CodeQueryKind::Callees {
        return Ok(Vec::new());
    }
    let contexts = ambiguous_callee_contexts(rows);
    if contexts.is_empty() {
        return Ok(Vec::new());
    }

    let candidates = search_callee_implementation_candidates(
        connection,
        status,
        &contexts,
        AMBIGUOUS_CALLEE_IMPLEMENTATION_LIMIT,
    )?;
    let query_has_test_intent = query_mentions_test_or_benchmark(&request.query);
    let mut hits = candidates
        .into_iter()
        .filter(|candidate| {
            selected_row(&candidate.path, &candidate.language_id, status, request)
                && (query_has_test_intent || !path_looks_like_test_or_benchmark(&candidate.path))
        })
        .filter_map(|candidate| {
            let (context, context_score) = best_ambiguous_callee_context(&candidate, &contexts)?;
            let caller = context.caller_name.as_deref().unwrap_or("<module>");
            let score = ambiguous_callee_implementation_score(
                &candidate,
                query_has_test_intent,
                context_score,
            );
            (score > 0.0).then(|| {
                let edge_target_hint = context
                    .target_hint
                    .clone()
                    .filter(|target_hint| !target_hint.trim().is_empty())
                    .unwrap_or_else(|| candidate.name.clone());
                hit_from_parts(
                    status,
                    HitParts {
                        path: candidate.path,
                        language_id: candidate.language_id,
                        byte_range: candidate.byte_range,
                        line_range: candidate.line_range,
                        symbol_snapshot_id: Some(candidate.symbol_snapshot_id),
                        canonical_symbol_id: Some(candidate.canonical_symbol_id),
                        file_id: Some(candidate.file_id),
                        retrieval_layers: vec![
                            CodeRetrievalLayer::CallGraph,
                            CodeRetrievalLayer::Definition,
                        ],
                        score,
                        excerpt: ambiguous_callee_implementation_excerpt(
                            caller,
                            &candidate.name,
                            &candidate.signature,
                            candidate.body_excerpt.as_deref(),
                        ),
                        degraded_reason: candidate.degraded_reason,
                        edge_kind: Some("call".to_owned()),
                        edge_resolution_state: Some("inferred".to_owned()),
                        edge_target_hint: Some(edge_target_hint),
                        edge_confidence_basis_points: Some(5_500),
                        edge_confidence_tier: Some("inferred".to_owned()),
                    },
                )
            })
        })
        .collect::<Vec<_>>();
    dedupe_sort_truncate(&mut hits, request.limit);

    Ok(hits)
}

fn ambiguous_callee_contexts(rows: &[CallRow]) -> Vec<AmbiguousCalleeContext> {
    let mut contexts = Vec::new();
    for row in rows {
        if row.resolution_state != "ambiguous" || row.callee_name.trim().is_empty() {
            continue;
        }
        let duplicate = contexts.iter().any(|context: &AmbiguousCalleeContext| {
            context.callee_name == row.callee_name
                && context.path == row.path
                && context.caller_name == row.caller_name
                && context.target_hint == row.target_hint
                && context.line_range.start == row.line_range.start
                && context.line_range.end == row.line_range.end
        });
        if !duplicate {
            contexts.push(AmbiguousCalleeContext {
                callee_name: row.callee_name.clone(),
                path: row.path.clone(),
                language_id: row.language_id.clone(),
                line_range: row.line_range.clone(),
                target_hint: row.target_hint.clone(),
                caller_name: row.caller_name.clone(),
                caller_signature: row.caller_signature.clone(),
                caller_excerpt: row.caller_excerpt.clone(),
                caller_canonical_symbol_id: row.caller_canonical_symbol_id.clone(),
            });
        }
        if contexts.len() >= AMBIGUOUS_CALLEE_CONTEXT_LIMIT {
            break;
        }
    }

    contexts
}

fn search_callee_implementation_candidates(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    contexts: &[AmbiguousCalleeContext],
    limit: usize,
) -> Result<Vec<CalleeImplementationCandidate>, StorageError> {
    let callee_names = ambiguous_context_callee_lookup_names(contexts);
    let language_ids = unique_context_values(contexts, |context| context.language_id.as_str());
    let exact_paths = unique_context_values(contexts, |context| context.path.as_str());
    let path_prefixes = ambiguous_context_path_prefixes(contexts);
    let target_hint_term_sets = ambiguous_context_target_hint_term_sets(contexts);
    if callee_names.is_empty() || language_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat_n("?", callee_names.len())
        .collect::<Vec<_>>()
        .join(", ");
    let language_placeholders = std::iter::repeat_n("?", language_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let candidate_scope_predicate =
        callee_candidate_scope_predicate(&exact_paths, &path_prefixes, &target_hint_term_sets);
    let target_hint_order_expression =
        callee_candidate_target_hint_order_expression(&target_hint_term_sets);
    let sql = format!(
        "
        SELECT s.file_id, s.path, s.language_id, s.symbol_snapshot_id,
               s.canonical_symbol_id, s.name, s.signature, s.byte_start, s.byte_end,
               s.line_start, s.line_end, f.parse_status, f.degraded_reason,
               (
                   SELECT chunk.content
                   FROM code_repository_chunks chunk
                   WHERE chunk.source_scope = s.source_scope
                     AND chunk.symbol_snapshot_id = s.symbol_snapshot_id
                   ORDER BY (chunk.line_end - chunk.line_start) DESC,
                            chunk.line_start ASC,
                            chunk.chunk_id ASC
                   LIMIT 1
               ) AS body_excerpt
        FROM code_repository_symbols s
        INNER JOIN code_repository_files f
            ON f.source_scope = s.source_scope AND f.path = s.path
        WHERE s.source_scope = ?
          AND s.name IN ({placeholders})
          AND s.language_id IN ({language_placeholders})
          AND ({candidate_scope_predicate})
          AND s.kind IN ('function', 'method')
        ORDER BY {target_hint_order_expression} ASC, s.path ASC, s.line_start ASC
        LIMIT ?
        "
    );
    let mut values = vec![Value::Text(required_scope(status)?.to_owned())];
    values.extend(callee_names.into_iter().map(Value::Text));
    values.extend(language_ids.into_iter().map(Value::Text));
    values.extend(exact_paths.into_iter().map(Value::Text));
    values.extend(path_prefixes.into_iter().map(Value::Text));
    push_target_hint_term_values(&mut values, &target_hint_term_sets);
    push_target_hint_term_values(&mut values, &target_hint_term_sets);
    values.push(Value::Integer(limit as i64));

    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        Ok(CalleeImplementationCandidate {
            file_id: row.get(0)?,
            path: row.get(1)?,
            language_id: row.get(2)?,
            symbol_snapshot_id: row.get(3)?,
            canonical_symbol_id: row.get(4)?,
            name: row.get(5)?,
            signature: row.get(6)?,
            byte_range: RepositoryCodeRange {
                start: row.get(7)?,
                end: row.get(8)?,
            },
            line_range: RepositoryCodeRange {
                start: row.get(9)?,
                end: row.get(10)?,
            },
            parse_status: row.get(11)?,
            degraded_reason: row.get(12)?,
            body_excerpt: row.get(13)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn unique_context_values<F>(contexts: &[AmbiguousCalleeContext], value: F) -> Vec<String>
where
    F: Fn(&AmbiguousCalleeContext) -> &str,
{
    let mut values = Vec::new();
    for context in contexts {
        let value = value(context).trim();
        if !value.is_empty() && !values.iter().any(|existing| existing == value) {
            values.push(value.to_owned());
        }
    }

    values
}

fn ambiguous_context_callee_lookup_names(contexts: &[AmbiguousCalleeContext]) -> Vec<String> {
    let mut names = Vec::new();
    for context in contexts {
        let Some(name) = callable_leaf_name(&context.callee_name) else {
            continue;
        };
        if !names.iter().any(|existing| existing == &name) {
            names.push(name);
        }
    }

    names
}

fn ambiguous_context_path_prefixes(contexts: &[AmbiguousCalleeContext]) -> Vec<String> {
    let mut prefixes = Vec::new();
    for context in contexts {
        let Some(parent) = parent_path(&context.path) else {
            continue;
        };
        let prefix = format!("{}/%", escape_sql_like(parent));
        if !prefixes.contains(&prefix) {
            prefixes.push(prefix);
        }
    }

    prefixes
}

fn ambiguous_context_target_hint_term_sets(
    contexts: &[AmbiguousCalleeContext],
) -> Vec<Vec<String>> {
    let mut sets = Vec::new();
    for context in contexts {
        let callee_leaf = callable_leaf_name(&context.callee_name).unwrap_or_default();
        let terms = specific_target_hint_terms(context.target_hint.as_deref(), &callee_leaf)
            .into_iter()
            .take(AMBIGUOUS_CALLEE_TARGET_HINT_TERM_LIMIT)
            .collect::<Vec<_>>();
        if !terms.is_empty() && !sets.contains(&terms) {
            sets.push(terms);
        }
    }

    sets
}

fn callee_candidate_scope_predicate(
    exact_paths: &[String],
    path_prefixes: &[String],
    target_hint_term_sets: &[Vec<String>],
) -> String {
    let mut predicates = Vec::new();
    if !exact_paths.is_empty() {
        predicates.push(format!(
            "s.path IN ({})",
            std::iter::repeat_n("?", exact_paths.len())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !path_prefixes.is_empty() {
        predicates.push(format!(
            "({})",
            std::iter::repeat_n("s.path LIKE ? ESCAPE '\\'", path_prefixes.len())
                .collect::<Vec<_>>()
                .join(" OR ")
        ));
    }
    for term_set in target_hint_term_sets {
        predicates.push(callee_candidate_target_hint_match_predicate(term_set));
    }

    if predicates.is_empty() {
        "0 = 1".to_owned()
    } else {
        predicates.join(" OR ")
    }
}

fn callee_candidate_target_hint_order_expression(target_hint_term_sets: &[Vec<String>]) -> String {
    let predicates = target_hint_term_sets
        .iter()
        .map(|term_set| callee_candidate_target_hint_match_predicate(term_set))
        .collect::<Vec<_>>();
    if predicates.is_empty() {
        "1".to_owned()
    } else {
        format!("CASE WHEN ({}) THEN 0 ELSE 1 END", predicates.join(" OR "))
    }
}

fn callee_candidate_target_hint_match_predicate(term_set: &[String]) -> String {
    format!(
        "({})",
        std::iter::repeat_n(
            "lower(coalesce(s.canonical_symbol_id, '') || ' ' || coalesce(s.signature, '') || ' ' || s.path) LIKE ? ESCAPE '\\'",
            term_set.len(),
        )
        .collect::<Vec<_>>()
        .join(" AND ")
    )
}

fn push_target_hint_term_values(values: &mut Vec<Value>, target_hint_term_sets: &[Vec<String>]) {
    for term_set in target_hint_term_sets {
        values.extend(
            term_set
                .iter()
                .map(|term| Value::Text(format!("%{}%", escape_sql_like(term)))),
        );
    }
}

fn best_ambiguous_callee_context<'context>(
    candidate: &CalleeImplementationCandidate,
    contexts: &'context [AmbiguousCalleeContext],
) -> Option<(&'context AmbiguousCalleeContext, f64)> {
    contexts
        .iter()
        .filter_map(|context| {
            let score = ambiguous_callee_context_score(candidate, context);
            (score > 0.0).then_some((context, score))
        })
        .max_by(|(_, left), (_, right)| left.total_cmp(right))
}

fn ambiguous_callee_context_score(
    candidate: &CalleeImplementationCandidate,
    context: &AmbiguousCalleeContext,
) -> f64 {
    if callable_leaf_name(&context.callee_name).as_deref() != Some(candidate.name.as_str())
        || candidate.language_id != context.language_id
    {
        return 0.0;
    }

    let mut score = 0.6;
    if candidate.path == context.path {
        score += 2.4;
    } else if same_parent_path(&candidate.path, &context.path) {
        score += 1.5;
    }
    if specific_target_hint_matches_candidate(context.target_hint.as_deref(), candidate) {
        score += 2.2;
    }
    if caller_context_mentions_candidate(context, candidate) {
        score += 1.0;
    }

    if score >= AMBIGUOUS_CALLEE_CONTEXT_MIN_SCORE {
        score
    } else {
        0.0
    }
}

fn ambiguous_callee_implementation_score(
    candidate: &CalleeImplementationCandidate,
    query_has_test_intent: bool,
    context_score: f64,
) -> f64 {
    let body = candidate.body_excerpt.as_deref().unwrap_or_default();
    let concrete_body = body_contains_executable_implementation(body, &candidate.name);
    let source_bonus = if !path_looks_like_test_or_benchmark(&candidate.path) {
        1.0
    } else if query_has_test_intent {
        0.0
    } else {
        -4.0
    };
    let parse_bonus = (candidate.parse_status == "parsed") as u8 as f64 * 0.25;
    let body_bonus = if concrete_body { 2.2 } else { 0.0 };

    (AMBIGUOUS_CALLEE_IMPLEMENTATION_BASE_SCORE
        + context_score
        + source_bonus
        + parse_bonus
        + body_bonus)
        .min(AMBIGUOUS_CALLEE_IMPLEMENTATION_MAX_SCORE)
}

fn body_contains_executable_implementation(body: &str, name: &str) -> bool {
    body.contains('{')
        && body.contains('}')
        && body.contains(name)
        && (body.contains("return ") || body.contains("=>") || body.contains("->"))
}

fn ambiguous_callee_implementation_excerpt(
    caller: &str,
    callee: &str,
    signature: &str,
    body_excerpt: Option<&str>,
) -> String {
    let body = body_excerpt
        .map(str::trim)
        .filter(|body| !body.is_empty())
        .unwrap_or(signature);
    format!("{caller} calls {callee}: {}", compact_excerpt(body))
}

fn compact_excerpt(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(12)
        .collect::<Vec<_>>()
        .join(" ")
}

fn parent_path(path: &str) -> Option<&str> {
    path.rsplit_once('/')
        .map(|(parent, _)| parent)
        .filter(|parent| !parent.is_empty())
}

fn same_parent_path(left: &str, right: &str) -> bool {
    parent_path(left)
        .zip(parent_path(right))
        .is_some_and(|(left, right)| left == right)
}

fn callable_leaf_name(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let mut end = trimmed.len();
    while end > 0 {
        let Some((index, character)) = trimmed[..end].char_indices().next_back() else {
            break;
        };
        if callable_identifier_character(character) {
            break;
        }
        end = index;
    }
    let mut start = end;
    while start > 0 {
        let Some((index, character)) = trimmed[..start].char_indices().next_back() else {
            break;
        };
        if !callable_identifier_character(character) {
            break;
        }
        start = index;
    }

    (start < end).then(|| trimmed[start..end].to_owned())
}

fn callable_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '$')
}

fn specific_target_hint_matches_candidate(
    target_hint: Option<&str>,
    candidate: &CalleeImplementationCandidate,
) -> bool {
    let specific_terms = specific_target_hint_terms(target_hint, &candidate.name);
    if specific_terms.is_empty() {
        return false;
    }
    let candidate_terms = identifier_terms(&format!(
        "{} {} {}",
        candidate.canonical_symbol_id, candidate.signature, candidate.path
    ));

    specific_terms
        .iter()
        .all(|term| candidate_terms.iter().any(|candidate| candidate == term))
}

fn specific_target_hint_terms(target_hint: Option<&str>, callee_name: &str) -> Vec<String> {
    let Some(target_hint) = target_hint.map(str::trim).filter(|hint| !hint.is_empty()) else {
        return Vec::new();
    };
    let name_terms = identifier_terms(callee_name);
    identifier_terms(target_hint)
        .into_iter()
        .filter(|term| !name_terms.iter().any(|name_term| name_term == term))
        .collect()
}

fn caller_context_mentions_candidate(
    context: &AmbiguousCalleeContext,
    candidate: &CalleeImplementationCandidate,
) -> bool {
    let context_text = [
        context.caller_name.as_deref().unwrap_or_default(),
        context.caller_signature.as_deref().unwrap_or_default(),
        context.caller_excerpt.as_deref().unwrap_or_default(),
        context
            .caller_canonical_symbol_id
            .as_deref()
            .unwrap_or_default(),
    ]
    .join(" ");
    if context_text.trim().is_empty() {
        return false;
    }
    if path_stem(&candidate.path)
        .is_some_and(|stem| stem.len() >= 4 && contains_identifier_surface(&context_text, stem))
    {
        return true;
    }

    let context_terms = identifier_terms(&context_text);
    let candidate_terms = candidate_identity_terms(candidate);
    let matched_terms = candidate_terms
        .iter()
        .filter(|term| {
            context_terms
                .iter()
                .any(|context_term| context_term == *term)
        })
        .take(2)
        .count();

    matched_terms >= 2
}

fn candidate_identity_terms(candidate: &CalleeImplementationCandidate) -> Vec<String> {
    let name_terms = identifier_terms(&candidate.name);
    identifier_terms(&format!(
        "{} {} {}",
        candidate.canonical_symbol_id, candidate.signature, candidate.path
    ))
    .into_iter()
    .filter(|term| term.len() >= 4)
    .filter(|term| !name_terms.iter().any(|name_term| name_term == term))
    .filter(|term| {
        !matches!(
            term.as_str(),
            "java" | "main" | "src" | "source" | "function" | "method" | "repo"
        )
    })
    .collect()
}

fn path_stem(path: &str) -> Option<&str> {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    file_name
        .rsplit_once('.')
        .map_or(Some(file_name), |(stem, _)| {
            (!stem.is_empty()).then_some(stem)
        })
}

fn contains_identifier_surface(haystack: &str, needle: &str) -> bool {
    let haystack = haystack.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    haystack.contains(&needle)
}

fn identifier_terms(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            current.push(character);
        } else {
            push_identifier_token(&current, &mut terms);
            current.clear();
        }
    }
    push_identifier_token(&current, &mut terms);

    terms
}

fn push_identifier_token(token: &str, terms: &mut Vec<String>) {
    if token.is_empty() {
        return;
    }
    let normalized = token.to_ascii_lowercase();
    push_unique_term(&normalized, terms);
    push_camel_terms(token, terms);
}

fn push_camel_terms(token: &str, terms: &mut Vec<String>) {
    let mut current = String::new();
    for character in token.chars() {
        if character.is_ascii_uppercase() && !current.is_empty() {
            push_unique_term(&current.to_ascii_lowercase(), terms);
            current.clear();
        }
        current.push(character);
    }
    push_unique_term(&current.to_ascii_lowercase(), terms);
}

fn push_unique_term(term: &str, terms: &mut Vec<String>) {
    if term.len() >= 2 && !terms.iter().any(|existing| existing == term) {
        terms.push(term.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambiguous_callee_score_prefers_concrete_source_body() {
        let source = candidate(
            "src/main/java/example/AnnotatedService.java",
            "public String handle(String value) { return normalize(value).trim(); }",
        );
        let interface = candidate(
            "src/main/java/example/ServiceContract.java",
            "T handle(T value);",
        );
        let fake = candidate(
            "src/test/java/example/FakeService.java",
            "String handle(String value) { return value; }",
        );

        assert!(
            ambiguous_callee_implementation_score(&source, false, 2.0)
                > ambiguous_callee_implementation_score(&interface, false, 2.0)
        );
        assert!(
            ambiguous_callee_implementation_score(&source, false, 2.0)
                > ambiguous_callee_implementation_score(&fake, false, 2.0)
        );
        assert_eq!(
            ambiguous_callee_implementation_score(&source, false, 4.0),
            AMBIGUOUS_CALLEE_IMPLEMENTATION_MAX_SCORE
        );
    }

    #[test]
    fn ambiguous_callee_excerpt_uses_body_when_available() {
        let excerpt = ambiguous_callee_implementation_excerpt(
            "dispatch",
            "handle",
            "public String handle(String value)",
            Some("public String handle(String value) {\n return normalize(value).trim();\n}"),
        );

        assert!(excerpt.contains("normalize(value).trim()"));
    }

    #[test]
    fn ambiguous_callee_context_accepts_same_directory_implementation() {
        let context = context("src/main/java/example/ServiceFactory.java", None);
        let candidate = candidate(
            "src/main/java/example/AnnotatedService.java",
            "public String handle(String value) { return normalize(value).trim(); }",
        );

        assert!(ambiguous_callee_context_score(&candidate, &context) > 0.0);
    }

    #[test]
    fn ambiguous_callee_context_rejects_same_name_without_local_evidence() {
        let context = context("src/main/java/example/ServiceFactory.java", None);
        let candidate = candidate(
            "src/main/java/other/RemoteHandler.java",
            "public String handle(String value) { return value; }",
        );

        assert_eq!(ambiguous_callee_context_score(&candidate, &context), 0.0);
    }

    #[test]
    fn ambiguous_callee_context_accepts_specific_target_hint() {
        let context = context(
            "src/main/java/example/ServiceFactory.java",
            Some("com.acme.worker.AnnotatedService.handle"),
        );
        let mut candidate = candidate(
            "src/main/java/worker/AnnotatedService.java",
            "public String handle(String value) { return normalize(value).trim(); }",
        );
        candidate.canonical_symbol_id =
            "repo://repo/com::acme::worker::AnnotatedService.handle".to_owned();

        assert!(ambiguous_callee_context_score(&candidate, &context) > 0.0);
    }

    #[test]
    fn ambiguous_callee_candidate_scope_includes_target_hint_identity_terms() {
        let context = context(
            "src/main/java/example/ServiceFactory.java",
            Some("com.acme.worker.AnnotatedService.handle"),
        );
        let target_hint_terms = ambiguous_context_target_hint_term_sets(&[context]);
        let predicate = callee_candidate_scope_predicate(
            &["src/main/java/example/ServiceFactory.java".to_owned()],
            &[],
            &target_hint_terms,
        );

        assert!(target_hint_terms[0].contains(&"worker".to_owned()));
        assert!(target_hint_terms[0].contains(&"annotatedservice".to_owned()));
        assert!(!target_hint_terms[0].contains(&"handle".to_owned()));
        assert!(predicate.contains("canonical_symbol_id"), "{predicate}");
        assert!(predicate.contains("s.path IN (?)"), "{predicate}");
    }

    #[test]
    fn ambiguous_callee_order_prioritizes_target_hint_identity_terms() {
        let context = context(
            "src/main/java/example/ServiceFactory.java",
            Some("com.acme.worker.AnnotatedService.handle"),
        );
        let target_hint_terms = ambiguous_context_target_hint_term_sets(&[context]);
        let expression = callee_candidate_target_hint_order_expression(&target_hint_terms);

        assert!(expression.starts_with("CASE WHEN"), "{expression}");
        assert!(expression.contains("canonical_symbol_id"), "{expression}");
        assert!(expression.contains("THEN 0 ELSE 1"), "{expression}");
    }

    #[test]
    fn ambiguous_callee_lookup_uses_leaf_but_keeps_qualified_hint_terms() {
        let mut call_context = context(
            "src/main/java/example/ConnectorFactory.java",
            Some("net::C.connect"),
        );
        call_context.callee_name = "C.connect".to_owned();
        let lookup_names = ambiguous_context_callee_lookup_names(&[call_context]);

        assert_eq!(lookup_names, vec!["connect".to_owned()]);

        let mut call_context = context(
            "src/main/java/example/ConnectorFactory.java",
            Some("net::C.connect"),
        );
        call_context.callee_name = "C.connect".to_owned();
        let hint_terms = ambiguous_context_target_hint_term_sets(&[call_context]);

        assert!(hint_terms[0].contains(&"net".to_owned()));
        assert!(!hint_terms[0].contains(&"connect".to_owned()));
    }

    #[test]
    fn ambiguous_callee_context_accepts_qualified_member_leaf() {
        let mut context = context(
            "src/main/java/example/ConnectorFactory.java",
            Some("net::C.connect"),
        );
        context.callee_name = "C.connect".to_owned();
        let mut candidate = candidate(
            "src/main/java/net/C.java",
            "public Connection connect(Target target) { return target.open(); }",
        );
        candidate.name = "connect".to_owned();
        candidate.signature = "public Connection connect(Target target)".to_owned();
        candidate.canonical_symbol_id = "repo://repo/net::C.connect".to_owned();

        assert!(ambiguous_callee_context_score(&candidate, &context) > 0.0);
    }

    #[test]
    fn ambiguous_callee_contexts_keep_distinct_target_hints() {
        let contexts = ambiguous_callee_contexts(&[
            call_row("handle", Some("primary.Service.handle"), 10),
            call_row("handle", Some("fallback.Service.handle"), 11),
            call_row("handle", Some("primary.Service.handle"), 10),
        ]);

        assert_eq!(contexts.len(), 2);
        assert!(
            contexts
                .iter()
                .any(|context| context.target_hint.as_deref() == Some("primary.Service.handle"))
        );
        assert!(
            contexts
                .iter()
                .any(|context| context.target_hint.as_deref() == Some("fallback.Service.handle"))
        );
    }

    fn context(path: &str, target_hint: Option<&str>) -> AmbiguousCalleeContext {
        AmbiguousCalleeContext {
            callee_name: "handle".to_owned(),
            path: path.to_owned(),
            language_id: "java".to_owned(),
            line_range: range(10, 10),
            target_hint: target_hint.map(str::to_owned),
            caller_name: Some("dispatch".to_owned()),
            caller_signature: Some("void dispatch(Service service)".to_owned()),
            caller_excerpt: Some("return service.handle(payload);".to_owned()),
            caller_canonical_symbol_id: Some("repo://repo/ServiceFactory.dispatch".to_owned()),
        }
    }

    fn call_row(callee_name: &str, target_hint: Option<&str>, line: u32) -> CallRow {
        CallRow {
            file_id: "file".to_owned(),
            path: "src/main/java/example/ServiceFactory.java".to_owned(),
            language_id: "java".to_owned(),
            caller_symbol_snapshot_id: Some("caller".to_owned()),
            caller_name: Some("dispatch".to_owned()),
            callee_symbol_snapshot_id: None,
            callee_name: callee_name.to_owned(),
            line_range: range(line, line),
            caller_line_range: Some(range(1, 20)),
            target_hint: target_hint.map(str::to_owned),
            resolution_state: "ambiguous".to_owned(),
            confidence_basis_points: 5_000,
            confidence_tier: "ambiguous".to_owned(),
            caller_canonical_symbol_id: Some("repo://repo/ServiceFactory.dispatch".to_owned()),
            callee_canonical_symbol_id: None,
            caller_signature: Some("void dispatch(Service primary, Service fallback)".to_owned()),
            callee_signature: None,
            caller_excerpt: Some("primary.handle(payload); fallback.handle(payload);".to_owned()),
            callee_excerpt: None,
        }
    }

    fn range(start: u32, end: u32) -> RepositoryCodeRange {
        RepositoryCodeRange { start, end }
    }

    fn candidate(path: &str, body: &str) -> CalleeImplementationCandidate {
        CalleeImplementationCandidate {
            file_id: "file".to_owned(),
            path: path.to_owned(),
            language_id: "java".to_owned(),
            symbol_snapshot_id: "symbol".to_owned(),
            canonical_symbol_id: "repo://repo/handle".to_owned(),
            name: "handle".to_owned(),
            signature: "public String handle(String value)".to_owned(),
            byte_range: RepositoryCodeRange { start: 0, end: 0 },
            line_range: RepositoryCodeRange { start: 1, end: 3 },
            body_excerpt: Some(body.to_owned()),
            parse_status: "parsed".to_owned(),
            degraded_reason: None,
        }
    }
}
