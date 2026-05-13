use std::collections::BTreeMap;

use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, RepositoryCodeRange,
    },
    storage::StorageError,
};

const MAX_CANDIDATE_BIND_VALUES: usize = 900;

use super::{repository_scope_status, repository_status};

pub(super) fn search_code(
    connection: &mut Connection,
    request: CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let status = required_repository(connection, &request.repository)?;
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
    let (candidate_condition, candidate_values) = candidate_condition(
        &[
            "lower(name)",
            "lower(qualified_name)",
            "lower(signature)",
            "lower(coalesce(doc_comment, ''))",
            "lower(path)",
        ],
        &request.query,
    );
    let sql = format!(
        "
        SELECT symbol_snapshot_id, canonical_symbol_id, file_id, path, language_id, signature, doc_comment,
               byte_start, byte_end, line_start, line_end, name, qualified_name
        FROM code_repository_symbols
        WHERE source_scope = ?
          AND ({candidate_condition})
        ORDER BY path ASC, line_start ASC
        ",
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(candidate_values_for(
            required_scope(status)?,
            candidate_values,
        )),
        |row| {
            Ok(SymbolRow {
                symbol_snapshot_id: row.get(0)?,
                canonical_symbol_id: row.get(1)?,
                file_id: row.get(2)?,
                path: row.get(3)?,
                language_id: row.get(4)?,
                signature: row.get(5)?,
                doc_comment: row.get(6)?,
                byte_range: RepositoryCodeRange {
                    start: row.get(7)?,
                    end: row.get(8)?,
                },
                line_range: RepositoryCodeRange {
                    start: row.get(9)?,
                    end: row.get(10)?,
                },
                name: row.get(11)?,
                qualified_name: row.get(12)?,
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
                        canonical_symbol_id: Some(row.canonical_symbol_id),
                        file_id: Some(row.file_id),
                        retrieval_layers: vec![
                            CodeRetrievalLayer::Symbol,
                            CodeRetrievalLayer::Definition,
                        ],
                        score: score + 2.0,
                        excerpt,
                        degraded_reason: None,
                        edge_kind: None,
                        edge_resolution_state: None,
                        edge_target_hint: None,
                        edge_confidence_basis_points: None,
                        edge_confidence_tier: None,
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
    let (candidate_condition, candidate_values) = candidate_condition(
        &["lower(r.name)", "lower(r.kind)", "lower(r.path)"],
        &request.query,
    );
    let sql = format!(
        "
        SELECT r.file_id, r.path, f.language_id, r.name, r.kind,
               r.target_symbol_snapshot_id, r.byte_start, r.byte_end,
               r.line_start, r.line_end, r.target_hint, r.resolution_state,
               r.confidence_basis_points, r.confidence_tier
        FROM code_repository_references r
        INNER JOIN code_repository_files f
            ON f.source_scope = r.source_scope AND f.path = r.path
        WHERE r.source_scope = ?
          AND ({candidate_condition})
        ORDER BY r.path ASC, r.line_start ASC
        ",
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(candidate_values_for(
            required_scope(status)?,
            candidate_values,
        )),
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
                target_hint: row.get(10)?,
                resolution_state: row.get(11)?,
                confidence_basis_points: row.get(12)?,
                confidence_tier: row.get(13)?,
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
                        canonical_symbol_id: None,
                        file_id: Some(row.file_id),
                        retrieval_layers: vec![CodeRetrievalLayer::Reference],
                        score: score + 1.5,
                        excerpt: format!("{} reference to {}", row.kind, row.name),
                        degraded_reason: None,
                        edge_kind: Some(row.kind),
                        edge_resolution_state: Some(row.resolution_state),
                        edge_target_hint: row.target_hint,
                        edge_confidence_basis_points: Some(row.confidence_basis_points),
                        edge_confidence_tier: Some(row.confidence_tier),
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
    let (candidate_condition, candidate_values) = candidate_condition(
        &[
            "lower(coalesce(c.caller_name, ''))",
            "lower(c.callee_name)",
            "lower(c.path)",
        ],
        &request.query,
    );
    let sql = format!(
        "
        SELECT c.file_id, c.path, f.language_id, c.caller_symbol_snapshot_id,
               c.caller_name, c.callee_symbol_snapshot_id, c.callee_name,
               c.line_start, c.line_end, c.target_hint, c.resolution_state,
               c.confidence_basis_points, c.confidence_tier
        FROM code_repository_calls c
        INNER JOIN code_repository_files f
            ON f.source_scope = c.source_scope AND f.path = c.path
        WHERE c.source_scope = ?
          AND ({candidate_condition})
        ORDER BY c.path ASC, c.line_start ASC
        ",
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(candidate_values_for(
            required_scope(status)?,
            candidate_values,
        )),
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
                target_hint: row.get(9)?,
                resolution_state: row.get(10)?,
                confidence_basis_points: row.get(11)?,
                confidence_tier: row.get(12)?,
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
                        canonical_symbol_id: None,
                        file_id: Some(row.file_id),
                        retrieval_layers: vec![CodeRetrievalLayer::CallGraph],
                        score: score + 1.25,
                        excerpt: format!("{caller} calls {}", row.callee_name),
                        degraded_reason: None,
                        edge_kind: Some("call".to_owned()),
                        edge_resolution_state: Some(row.resolution_state),
                        edge_target_hint: row.target_hint,
                        edge_confidence_basis_points: Some(row.confidence_basis_points),
                        edge_confidence_tier: Some(row.confidence_tier),
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
    let (candidate_condition, candidate_values) =
        candidate_condition(&["lower(i.module)", "lower(i.path)"], &request.query);
    let sql = format!(
        "
        SELECT i.file_id, i.path, f.language_id, i.module, i.line_start, i.line_end,
               i.target_hint, i.resolution_state, i.confidence_basis_points, i.confidence_tier
        FROM code_repository_imports i
        INNER JOIN code_repository_files f
            ON f.source_scope = i.source_scope AND f.path = i.path
        WHERE i.source_scope = ?
          AND ({candidate_condition})
        ORDER BY i.path ASC, i.line_start ASC
        ",
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(candidate_values_for(
            required_scope(status)?,
            candidate_values,
        )),
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
                target_hint: row.get(6)?,
                resolution_state: row.get(7)?,
                confidence_basis_points: row.get(8)?,
                confidence_tier: row.get(9)?,
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
                        canonical_symbol_id: None,
                        file_id: Some(row.file_id),
                        retrieval_layers: vec![CodeRetrievalLayer::ImportGraph],
                        score: score + 1.0,
                        excerpt: row.module,
                        degraded_reason: None,
                        edge_kind: Some("import".to_owned()),
                        edge_resolution_state: Some(row.resolution_state),
                        edge_target_hint: row.target_hint,
                        edge_confidence_basis_points: Some(row.confidence_basis_points),
                        edge_confidence_tier: Some(row.confidence_tier),
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
    let (candidate_condition, candidate_values) =
        candidate_condition(&["lower(c.content)", "lower(c.path)"], &request.query);
    let sql = format!(
        "
        SELECT c.file_id, c.path, c.language_id, c.content, c.byte_start, c.byte_end,
               c.line_start, c.line_end, c.symbol_snapshot_id, f.parse_status,
               f.degraded_reason
        FROM code_repository_chunks c
        INNER JOIN code_repository_files f
            ON f.source_scope = c.source_scope AND f.path = c.path
        WHERE c.source_scope = ?
          AND ({candidate_condition})
        ORDER BY c.path ASC, c.line_start ASC
        ",
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(candidate_values_for(
            required_scope(status)?,
            candidate_values,
        )),
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
                        canonical_symbol_id: None,
                        file_id: Some(row.file_id),
                        retrieval_layers: chunk_layers(&row.parse_status),
                        score,
                        excerpt: row.content,
                        degraded_reason: row.degraded_reason,
                        edge_kind: None,
                        edge_resolution_state: None,
                        edge_target_hint: None,
                        edge_confidence_basis_points: None,
                        edge_confidence_tier: None,
                    },
                )
            })
        })
        .collect())
}

pub(super) fn required_repository(
    connection: &mut Connection,
    selector: &crate::domain::CodeRepositorySelector,
) -> Result<CodeRepositoryStatus, StorageError> {
    let status = repository_status(connection, &selector.repository)?.ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository '{}' is not registered",
            selector.repository
        ))
    })?;
    let path_filters = merged_filters(&status.path_filters, &selector.path_filters);
    let language_filters = merged_filters(&status.language_filters, &selector.language_filters);
    let scoped_status = repository_scope_status(
        connection,
        &selector.repository,
        &selector.ref_selector,
        &path_filters,
        &language_filters,
    )?
    .ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository '{}' has no index for ref {} and requested filters",
            selector.repository, selector.ref_selector
        ))
    })?;

    Ok(scoped_status)
}

fn merged_filters(left: &[String], right: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    for value in left.iter().chain(right.iter()) {
        if !merged.contains(value) {
            merged.push(value.clone());
        }
    }

    merged
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
    pub(super) canonical_symbol_id: Option<String>,
    pub(super) file_id: Option<String>,
    pub(super) retrieval_layers: Vec<CodeRetrievalLayer>,
    pub(super) score: f64,
    pub(super) excerpt: String,
    pub(super) degraded_reason: Option<String>,
    pub(super) edge_kind: Option<String>,
    pub(super) edge_resolution_state: Option<String>,
    pub(super) edge_target_hint: Option<String>,
    pub(super) edge_confidence_basis_points: Option<u16>,
    pub(super) edge_confidence_tier: Option<String>,
}

pub(super) fn hit_from_parts(status: &CodeRepositoryStatus, parts: HitParts) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: status.repository_id.clone(),
        scope_id: status.last_indexed_scope_id.clone().unwrap_or_default(),
        resolved_commit_sha: status.last_indexed_commit.clone().unwrap_or_default(),
        tree_hash: status.tree_hash.clone().unwrap_or_default(),
        path: parts.path,
        language_id: parts.language_id,
        byte_range: parts.byte_range,
        line_range: parts.line_range,
        symbol_snapshot_id: parts.symbol_snapshot_id,
        canonical_symbol_id: parts.canonical_symbol_id,
        file_id: parts.file_id,
        retrieval_layers: parts.retrieval_layers,
        index_versions: vec![format!(
            "code:{}:{}",
            status
                .last_indexed_scope_id
                .as_deref()
                .unwrap_or("unscoped"),
            status.tree_hash.as_deref().unwrap_or("unindexed")
        )],
        stale: status.stale,
        degraded_reason: parts
            .degraded_reason
            .or_else(|| status.degraded_reason.clone()),
        edge_kind: parts.edge_kind,
        edge_resolution_state: parts.edge_resolution_state,
        edge_target_hint: parts.edge_target_hint,
        edge_confidence_basis_points: parts.edge_confidence_basis_points,
        edge_confidence_tier: parts.edge_confidence_tier,
        score: parts.score,
        excerpt: parts.excerpt,
    }
}

pub(super) fn required_scope(status: &CodeRepositoryStatus) -> Result<&str, StorageError> {
    status.last_indexed_scope_id.as_deref().ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository '{}' does not have an indexed source scope",
            status.alias
        ))
    })
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
    if target.canonical_symbol_id.is_none() {
        target.canonical_symbol_id = source.canonical_symbol_id.clone();
    }
    if target.file_id.is_none() {
        target.file_id = source.file_id.clone();
    }
    if target.edge_kind.is_none() {
        target.edge_kind = source.edge_kind.clone();
        target.edge_resolution_state = source.edge_resolution_state.clone();
        target.edge_target_hint = source.edge_target_hint.clone();
        target.edge_confidence_basis_points = source.edge_confidence_basis_points;
        target.edge_confidence_tier = source.edge_confidence_tier.clone();
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

fn candidate_condition(fields: &[&str], query: &str) -> (String, Vec<Value>) {
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

fn candidate_values_for(repository_id: &str, candidate_values: Vec<Value>) -> Vec<Value> {
    let mut values = Vec::with_capacity(candidate_values.len() + 1);
    values.push(Value::Text(repository_id.to_owned()));
    values.extend(candidate_values);
    values
}

struct SymbolRow {
    symbol_snapshot_id: String,
    canonical_symbol_id: String,
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
    target_hint: Option<String>,
    resolution_state: String,
    confidence_basis_points: u16,
    confidence_tier: String,
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
    target_hint: Option<String>,
    resolution_state: String,
    confidence_basis_points: u16,
    confidence_tier: String,
}

struct ImportRow {
    file_id: String,
    path: String,
    language_id: String,
    module: String,
    line_range: RepositoryCodeRange,
    target_hint: Option<String>,
    resolution_state: String,
    confidence_basis_points: u16,
    confidence_tier: String,
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

    #[test]
    fn candidate_condition_preserves_all_query_terms() {
        let (condition, values) =
            candidate_condition(&["lower(name)", "lower(path)"], "retry budget");

        assert!(condition.contains("lower(name) LIKE ?"));
        assert_eq!(values.len(), 4);
        assert!(values.contains(&Value::Text("%retry%".to_owned())));
        assert!(values.contains(&Value::Text("%budget%".to_owned())));
    }

    #[test]
    fn candidate_condition_caps_bind_values_for_long_queries() {
        let query = (0..300)
            .map(|index| format!("term{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        let fields = ["a", "b", "c", "d", "e"];

        let (_, values) = candidate_condition(&fields, &query);

        assert!(values.len() <= MAX_CANDIDATE_BIND_VALUES);
    }
}
