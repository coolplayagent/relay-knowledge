use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, RepositoryCodeRange,
    },
    storage::StorageError,
};

use super::repository_status;

pub(super) fn search_code(
    connection: &mut Connection,
    request: CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let status = required_repository(
        connection,
        &request.repository.repository,
        &request.repository.ref_selector,
    )?;
    if request.code_query_kind == CodeQueryKind::Impact {
        return Err(StorageError::InvalidInput(
            "impact query kind requires repo impact with base/head refs".to_owned(),
        ));
    }
    let mut hits = Vec::new();
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Hybrid | CodeQueryKind::Symbol | CodeQueryKind::Definition
    ) {
        hits.extend(search_symbols(connection, &status, &request)?);
    }
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Hybrid | CodeQueryKind::References
    ) {
        hits.extend(search_references(connection, &status, &request)?);
    }
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Hybrid | CodeQueryKind::Callers | CodeQueryKind::Callees
    ) {
        hits.extend(search_calls(connection, &status, &request)?);
    }
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Hybrid | CodeQueryKind::Imports
    ) {
        hits.extend(search_imports(connection, &status, &request)?);
    }
    if matches!(request.code_query_kind, CodeQueryKind::Hybrid) {
        hits.extend(search_chunks(connection, &status, &request)?);
    }
    dedupe_sort_truncate(&mut hits, request.limit);

    Ok(hits)
}

fn search_symbols(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT symbol_snapshot_id, file_id, path, language_id, signature, doc_comment,
               byte_start, byte_end, line_start, line_end, name, qualified_name
        FROM code_repository_symbols
        WHERE repository_id = ?1
          AND (
            lower(name) LIKE ?2 OR lower(qualified_name) LIKE ?2
            OR lower(signature) LIKE ?2 OR lower(coalesce(doc_comment, '')) LIKE ?2
            OR lower(path) LIKE ?2
          )
        ORDER BY path ASC, line_start ASC
        LIMIT ?3
        ",
    )?;
    let rows = statement.query_map(
        params![
            status.repository_id,
            candidate_like(&request.query),
            candidate_limit(request)
        ],
        |row| {
            Ok(SymbolRow {
                symbol_snapshot_id: row.get(0)?,
                file_id: row.get(1)?,
                path: row.get(2)?,
                language_id: row.get(3)?,
                signature: row.get(4)?,
                doc_comment: row.get(5)?,
                byte_range: RepositoryCodeRange {
                    start: row.get(6)?,
                    end: row.get(7)?,
                },
                line_range: RepositoryCodeRange {
                    start: row.get(8)?,
                    end: row.get(9)?,
                },
                name: row.get(10)?,
                qualified_name: row.get(11)?,
            })
        },
    )?;
    let query = request.query.to_lowercase();
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, status, request))
        .filter_map(|row| {
            let score = score_text(
                &query,
                [
                    row.name.as_str(),
                    row.qualified_name.as_str(),
                    row.signature.as_str(),
                    row.doc_comment.as_deref().unwrap_or_default(),
                    row.path.as_str(),
                ],
            );
            (score > 0.0).then(|| {
                let mut excerpt = row.signature.clone();
                if let Some(doc) = &row.doc_comment {
                    excerpt = format!("{doc}\n{}", row.signature);
                }
                hit_from_parts(
                    status,
                    HitParts {
                        path: row.path,
                        language_id: row.language_id,
                        byte_range: row.byte_range,
                        line_range: row.line_range,
                        symbol_snapshot_id: Some(row.symbol_snapshot_id),
                        file_id: Some(row.file_id),
                        retrieval_layers: vec![
                            CodeRetrievalLayer::Symbol,
                            CodeRetrievalLayer::Definition,
                        ],
                        score: score + 2.0,
                        excerpt,
                        degraded_reason: None,
                    },
                )
            })
        })
        .collect())
}

fn search_references(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT r.file_id, r.path, f.language_id, r.name, r.kind,
               r.target_symbol_snapshot_id, r.byte_start, r.byte_end,
               r.line_start, r.line_end
        FROM code_repository_references r
        INNER JOIN code_repository_files f
            ON f.repository_id = r.repository_id AND f.path = r.path
        WHERE r.repository_id = ?1
          AND (lower(r.name) LIKE ?2 OR lower(r.kind) LIKE ?2 OR lower(r.path) LIKE ?2)
        ORDER BY r.path ASC, r.line_start ASC
        LIMIT ?3
        ",
    )?;
    let rows = statement.query_map(
        params![
            status.repository_id,
            candidate_like(&request.query),
            candidate_limit(request)
        ],
        |row| {
            Ok(ReferenceRow {
                file_id: row.get(0)?,
                path: row.get(1)?,
                language_id: row.get(2)?,
                name: row.get(3)?,
                kind: row.get(4)?,
                target_symbol_snapshot_id: row.get(5)?,
                byte_range: RepositoryCodeRange {
                    start: row.get(6)?,
                    end: row.get(7)?,
                },
                line_range: RepositoryCodeRange {
                    start: row.get(8)?,
                    end: row.get(9)?,
                },
            })
        },
    )?;
    let query = request.query.to_lowercase();
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, status, request))
        .filter_map(|row| {
            let score = score_text(&query, [&row.name, &row.kind]);
            (score > 0.0).then(|| {
                hit_from_parts(
                    status,
                    HitParts {
                        path: row.path,
                        language_id: row.language_id,
                        byte_range: row.byte_range,
                        line_range: row.line_range,
                        symbol_snapshot_id: row.target_symbol_snapshot_id,
                        file_id: Some(row.file_id),
                        retrieval_layers: vec![CodeRetrievalLayer::Reference],
                        score: score + 1.5,
                        excerpt: format!("{} reference to {}", row.kind, row.name),
                        degraded_reason: None,
                    },
                )
            })
        })
        .collect())
}

fn search_calls(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT c.file_id, c.path, f.language_id, c.caller_symbol_snapshot_id,
               c.caller_name, c.callee_symbol_snapshot_id, c.callee_name,
               c.line_start, c.line_end
        FROM code_repository_calls c
        INNER JOIN code_repository_files f
            ON f.repository_id = c.repository_id AND f.path = c.path
        WHERE c.repository_id = ?1
          AND (
            lower(coalesce(c.caller_name, '')) LIKE ?2
            OR lower(c.callee_name) LIKE ?2
            OR lower(c.path) LIKE ?2
          )
        ORDER BY c.path ASC, c.line_start ASC
        LIMIT ?3
        ",
    )?;
    let rows = statement.query_map(
        params![
            status.repository_id,
            candidate_like(&request.query),
            candidate_limit(request)
        ],
        |row| {
            Ok(CallRow {
                file_id: row.get(0)?,
                path: row.get(1)?,
                language_id: row.get(2)?,
                caller_symbol_snapshot_id: row.get(3)?,
                caller_name: row.get(4)?,
                callee_symbol_snapshot_id: row.get(5)?,
                callee_name: row.get(6)?,
                line_range: RepositoryCodeRange {
                    start: row.get(7)?,
                    end: row.get(8)?,
                },
            })
        },
    )?;
    let query = request.query.to_lowercase();
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, status, request))
        .filter_map(|row| {
            let search_fields = match request.code_query_kind {
                CodeQueryKind::Callees => [row.caller_name.as_deref().unwrap_or(""), ""],
                CodeQueryKind::Callers => [&row.callee_name, ""],
                _ => [row.caller_name.as_deref().unwrap_or(""), &row.callee_name],
            };
            let score = score_text(&query, search_fields);
            (score > 0.0).then(|| {
                let caller = row.caller_name.unwrap_or_else(|| "<module>".to_owned());
                let symbol_snapshot_id = if request.code_query_kind == CodeQueryKind::Callees {
                    row.callee_symbol_snapshot_id
                } else {
                    row.caller_symbol_snapshot_id
                };
                hit_from_parts(
                    status,
                    HitParts {
                        path: row.path,
                        language_id: row.language_id,
                        byte_range: RepositoryCodeRange { start: 0, end: 0 },
                        line_range: row.line_range,
                        symbol_snapshot_id,
                        file_id: Some(row.file_id),
                        retrieval_layers: vec![CodeRetrievalLayer::CallGraph],
                        score: score + 1.25,
                        excerpt: format!("{caller} calls {}", row.callee_name),
                        degraded_reason: None,
                    },
                )
            })
        })
        .collect())
}

fn search_imports(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT i.file_id, i.path, f.language_id, i.module, i.line_start, i.line_end
        FROM code_repository_imports i
        INNER JOIN code_repository_files f
            ON f.repository_id = i.repository_id AND f.path = i.path
        WHERE i.repository_id = ?1
          AND (lower(i.module) LIKE ?2 OR lower(i.path) LIKE ?2)
        ORDER BY i.path ASC, i.line_start ASC
        LIMIT ?3
        ",
    )?;
    let rows = statement.query_map(
        params![
            status.repository_id,
            candidate_like(&request.query),
            candidate_limit(request)
        ],
        |row| {
            Ok(ImportRow {
                file_id: row.get(0)?,
                path: row.get(1)?,
                language_id: row.get(2)?,
                module: row.get(3)?,
                line_range: RepositoryCodeRange {
                    start: row.get(4)?,
                    end: row.get(5)?,
                },
            })
        },
    )?;
    let query = request.query.to_lowercase();
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, status, request))
        .filter_map(|row| {
            let score = score_text(&query, [&row.module, &row.path]);
            (score > 0.0).then(|| {
                hit_from_parts(
                    status,
                    HitParts {
                        path: row.path,
                        language_id: row.language_id,
                        byte_range: RepositoryCodeRange { start: 0, end: 0 },
                        line_range: row.line_range,
                        symbol_snapshot_id: None,
                        file_id: Some(row.file_id),
                        retrieval_layers: vec![CodeRetrievalLayer::ImportGraph],
                        score: score + 1.0,
                        excerpt: row.module,
                        degraded_reason: None,
                    },
                )
            })
        })
        .collect())
}

fn search_chunks(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT c.file_id, c.path, c.language_id, c.content, c.byte_start, c.byte_end,
               c.line_start, c.line_end, c.symbol_snapshot_id, f.parse_status,
               f.degraded_reason
        FROM code_repository_chunks c
        INNER JOIN code_repository_files f
            ON f.repository_id = c.repository_id AND f.path = c.path
        WHERE c.repository_id = ?1
          AND (lower(c.content) LIKE ?2 OR lower(c.path) LIKE ?2)
        ORDER BY c.path ASC, c.line_start ASC
        LIMIT ?3
        ",
    )?;
    let rows = statement.query_map(
        params![
            status.repository_id,
            candidate_like(&request.query),
            candidate_limit(request)
        ],
        |row| {
            Ok(ChunkRow {
                file_id: row.get(0)?,
                path: row.get(1)?,
                language_id: row.get(2)?,
                content: row.get(3)?,
                byte_range: RepositoryCodeRange {
                    start: row.get(4)?,
                    end: row.get(5)?,
                },
                line_range: RepositoryCodeRange {
                    start: row.get(6)?,
                    end: row.get(7)?,
                },
                symbol_snapshot_id: row.get(8)?,
                parse_status: row.get(9)?,
                degraded_reason: row.get(10)?,
            })
        },
    )?;
    let query = request.query.to_lowercase();
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, status, request))
        .filter_map(|row| {
            let score = score_text(&query, [&row.content, &row.path]);
            (score > 0.0).then(|| {
                hit_from_parts(
                    status,
                    HitParts {
                        path: row.path,
                        language_id: row.language_id,
                        byte_range: row.byte_range,
                        line_range: row.line_range,
                        symbol_snapshot_id: row.symbol_snapshot_id,
                        file_id: Some(row.file_id),
                        retrieval_layers: chunk_layers(&row.parse_status),
                        score,
                        excerpt: row.content,
                        degraded_reason: row.degraded_reason,
                    },
                )
            })
        })
        .collect())
}

pub(super) fn required_repository(
    connection: &mut Connection,
    repository: &str,
    ref_selector: &str,
) -> Result<CodeRepositoryStatus, StorageError> {
    let status = repository_status(connection, repository)?.ok_or_else(|| {
        StorageError::InvalidInput(format!("code repository '{repository}' is not registered"))
    })?;
    let indexed_commit = status.last_indexed_commit.as_deref().ok_or_else(|| {
        StorageError::InvalidInput(format!("code repository '{repository}' is not indexed"))
    })?;
    if indexed_commit != ref_selector {
        return Err(StorageError::InvalidInput(format!(
            "code repository '{repository}' is indexed at {indexed_commit}, not requested ref {ref_selector}"
        )));
    }

    Ok(status)
}

fn selected_row(
    path: &str,
    language_id: &str,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> bool {
    path_filter_allows(path, &status.path_filters)
        && path_filter_allows(path, &request.repository.path_filters)
        && language_filter_allows(language_id, &status.language_filters)
        && language_filter_allows(language_id, &request.repository.language_filters)
}

pub(super) fn path_filter_allows(path: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters
            .iter()
            .any(|filter| path_matches_filter(path, filter))
}

pub(super) fn language_filter_allows(language_id: &str, filters: &[String]) -> bool {
    filters.is_empty() || filters.iter().any(|filter| filter == language_id)
}

fn path_matches_filter(path: &str, filter: &str) -> bool {
    let filter = normalize_path_filter(filter);
    if filter == "." {
        return true;
    }
    !filter.is_empty() && (path == filter || path.starts_with(&format!("{filter}/")))
}

fn normalize_path_filter(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}

pub(super) fn chunk_layers(parse_status: &str) -> Vec<CodeRetrievalLayer> {
    let mut layers = vec![CodeRetrievalLayer::Lexical];
    if parse_status != "parsed" {
        layers.push(CodeRetrievalLayer::TextFallback);
    }

    layers
}

pub(super) struct HitParts {
    pub(super) path: String,
    pub(super) language_id: String,
    pub(super) byte_range: RepositoryCodeRange,
    pub(super) line_range: RepositoryCodeRange,
    pub(super) symbol_snapshot_id: Option<String>,
    pub(super) file_id: Option<String>,
    pub(super) retrieval_layers: Vec<CodeRetrievalLayer>,
    pub(super) score: f64,
    pub(super) excerpt: String,
    pub(super) degraded_reason: Option<String>,
}

pub(super) fn hit_from_parts(status: &CodeRepositoryStatus, parts: HitParts) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: status.repository_id.clone(),
        scope_id: status.alias.clone(),
        resolved_commit_sha: status.last_indexed_commit.clone().unwrap_or_default(),
        tree_hash: status.tree_hash.clone().unwrap_or_default(),
        path: parts.path,
        language_id: parts.language_id,
        byte_range: parts.byte_range,
        line_range: parts.line_range,
        symbol_snapshot_id: parts.symbol_snapshot_id,
        file_id: parts.file_id,
        retrieval_layers: parts.retrieval_layers,
        index_versions: vec![format!(
            "code:{}",
            status.tree_hash.as_deref().unwrap_or("unindexed")
        )],
        stale: status.stale,
        degraded_reason: parts
            .degraded_reason
            .or_else(|| status.degraded_reason.clone()),
        score: parts.score,
        excerpt: parts.excerpt,
    }
}

pub(super) fn dedupe_sort_truncate(hits: &mut Vec<CodeRetrievalHit>, limit: usize) {
    let mut best = BTreeMap::<(String, u32, String), CodeRetrievalHit>::new();
    for hit in hits.drain(..) {
        let key = (hit.path.clone(), hit.line_range.start, hit.excerpt.clone());
        match best.get(&key) {
            Some(existing) if existing.score >= hit.score => {
                let existing = best.get_mut(&key).expect("checked entry should exist");
                merge_hit_provenance(existing, &hit);
            }
            Some(_) => {
                let mut hit = hit;
                if let Some(existing) = best.get(&key) {
                    merge_hit_provenance(&mut hit, existing);
                }
                best.insert(key, hit);
            }
            _ => {
                best.insert(key, hit);
            }
        }
    }
    hits.extend(best.into_values());
    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.line_range.start.cmp(&right.line_range.start))
    });
    hits.truncate(limit);
}

fn merge_hit_provenance(target: &mut CodeRetrievalHit, source: &CodeRetrievalHit) {
    for layer in &source.retrieval_layers {
        if !target.retrieval_layers.contains(layer) {
            target.retrieval_layers.push(*layer);
        }
    }
    for version in &source.index_versions {
        if !target.index_versions.contains(version) {
            target.index_versions.push(version.clone());
        }
    }
    if target.degraded_reason.is_none() {
        target.degraded_reason = source.degraded_reason.clone();
    }
    if target.symbol_snapshot_id.is_none() {
        target.symbol_snapshot_id = source.symbol_snapshot_id.clone();
    }
    if target.file_id.is_none() {
        target.file_id = source.file_id.clone();
    }
}

fn score_text(query: &str, fields: impl IntoIterator<Item = impl AsRef<str>>) -> f64 {
    let haystack = fields
        .into_iter()
        .map(|field| field.as_ref().to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let mut score = 0.0;
    for token in query.split_whitespace() {
        if haystack.contains(token) {
            score += 1.0;
        }
    }

    score
}

fn candidate_like(query: &str) -> String {
    let token = query
        .to_lowercase()
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|ch| *ch != '%')
        .collect::<String>();

    format!("%{token}%")
}

fn candidate_limit(request: &CodeRetrievalRequest) -> usize {
    request.limit.saturating_mul(40).max(100)
}

struct SymbolRow {
    symbol_snapshot_id: String,
    file_id: String,
    path: String,
    language_id: String,
    signature: String,
    doc_comment: Option<String>,
    byte_range: RepositoryCodeRange,
    line_range: RepositoryCodeRange,
    name: String,
    qualified_name: String,
}

struct ReferenceRow {
    file_id: String,
    path: String,
    language_id: String,
    name: String,
    kind: String,
    target_symbol_snapshot_id: Option<String>,
    byte_range: RepositoryCodeRange,
    line_range: RepositoryCodeRange,
}

struct CallRow {
    file_id: String,
    path: String,
    language_id: String,
    caller_symbol_snapshot_id: Option<String>,
    caller_name: Option<String>,
    callee_symbol_snapshot_id: Option<String>,
    callee_name: String,
    line_range: RepositoryCodeRange,
}

struct ImportRow {
    file_id: String,
    path: String,
    language_id: String,
    module: String,
    line_range: RepositoryCodeRange,
}

struct ChunkRow {
    file_id: String,
    path: String,
    language_id: String,
    content: String,
    byte_range: RepositoryCodeRange,
    line_range: RepositoryCodeRange,
    symbol_snapshot_id: Option<String>,
    parse_status: String,
    degraded_reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_filters_accept_trailing_slashes() {
        assert!(path_matches_filter("src/lib.rs", "src/"));
        assert!(path_matches_filter("src/lib.rs", "src"));
        assert!(path_matches_filter("src/lib.rs", "."));
        assert!(path_matches_filter("src/lib.rs", "./"));
        assert!(path_matches_filter("src/lib.rs", "./src"));
        assert!(!path_matches_filter("src-other/lib.rs", "src/"));
    }
}
