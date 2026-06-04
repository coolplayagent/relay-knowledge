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
    dedupe_sort_truncate, hit_from_parts, prepare_code_search_statement, required_scope,
    selected_row,
};

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

pub(super) fn search_ambiguous_callee_implementation_hits(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    rows: &[CallRow],
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    if request.code_query_kind != CodeQueryKind::Callees {
        return Ok(Vec::new());
    }
    let callee_names = ambiguous_callee_names(rows);
    if callee_names.is_empty() {
        return Ok(Vec::new());
    }

    let candidates = search_callee_implementation_candidates(
        connection,
        status,
        &callee_names,
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
            let caller = caller_for_callee(rows, &candidate.name).unwrap_or("<module>");
            let score = ambiguous_callee_implementation_score(&candidate, query_has_test_intent);
            (score > 0.0).then(|| {
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
                        edge_target_hint: Some(candidate.name),
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

fn ambiguous_callee_names(rows: &[CallRow]) -> Vec<String> {
    let mut names = Vec::new();
    for row in rows {
        if row.resolution_state == "ambiguous" && !names.contains(&row.callee_name) {
            names.push(row.callee_name.clone());
        }
    }

    names
}

fn caller_for_callee<'row>(rows: &'row [CallRow], callee_name: &str) -> Option<&'row str> {
    rows.iter()
        .find(|row| row.callee_name == callee_name)
        .and_then(|row| row.caller_name.as_deref())
}

fn search_callee_implementation_candidates(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    callee_names: &[String],
    limit: usize,
) -> Result<Vec<CalleeImplementationCandidate>, StorageError> {
    let placeholders = std::iter::repeat_n("?", callee_names.len())
        .collect::<Vec<_>>()
        .join(", ");
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
          AND s.kind IN ('function', 'method')
        ORDER BY s.path ASC, s.line_start ASC
        LIMIT ?
        "
    );
    let mut values = vec![Value::Text(required_scope(status)?.to_owned())];
    values.extend(callee_names.iter().cloned().map(Value::Text));
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

fn ambiguous_callee_implementation_score(
    candidate: &CalleeImplementationCandidate,
    query_has_test_intent: bool,
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

    8.0 + source_bonus + parse_bonus + body_bonus
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
            ambiguous_callee_implementation_score(&source, false)
                > ambiguous_callee_implementation_score(&interface, false)
        );
        assert!(
            ambiguous_callee_implementation_score(&source, false)
                > ambiguous_callee_implementation_score(&fake, false)
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
