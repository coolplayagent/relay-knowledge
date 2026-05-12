use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};

use crate::{
    domain::{
        CodeImpactRequest, CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit,
        CodeRetrievalLayer, CodeRetrievalRequest, RepositoryCodeRange,
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
    let mut hits = Vec::new();
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Hybrid
            | CodeQueryKind::Impact
            | CodeQueryKind::Symbol
            | CodeQueryKind::Definition
    ) {
        hits.extend(search_symbols(connection, &status, &request)?);
    }
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Hybrid | CodeQueryKind::Impact | CodeQueryKind::References
    ) {
        hits.extend(search_references(connection, &status, &request)?);
    }
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Hybrid
            | CodeQueryKind::Impact
            | CodeQueryKind::Callers
            | CodeQueryKind::Callees
    ) {
        hits.extend(search_calls(connection, &status, &request)?);
    }
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Hybrid | CodeQueryKind::Impact | CodeQueryKind::Imports
    ) {
        hits.extend(search_imports(connection, &status, &request)?);
    }
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Hybrid | CodeQueryKind::Impact
    ) {
        hits.extend(search_chunks(connection, &status, &request)?);
    }
    dedupe_sort_truncate(&mut hits, request.limit);

    Ok(hits)
}

pub(super) fn analyze_impact(
    connection: &mut Connection,
    request: CodeImpactRequest,
    changed_paths: Vec<String>,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let status = required_repository(
        connection,
        &request.repository.repository,
        &request.repository.ref_selector,
    )?;
    let changed = changed_paths.into_iter().collect::<BTreeSet<_>>();
    let changed_modules = changed
        .iter()
        .map(|path| path_to_module_key(path))
        .collect::<Vec<_>>();
    let changed_symbols = symbols_for_paths(connection, &status.repository_id, &changed)?;
    let mut hits = Vec::new();

    hits.extend(chunks_for_paths(connection, &status, &changed, &request)?);
    hits.extend(callers_for_symbols(
        connection,
        &status,
        &changed_symbols,
        &request,
    )?);
    hits.extend(importers_for_modules(
        connection,
        &status,
        &changed_modules,
        &request,
    )?);
    for hit in &mut hits {
        hit.retrieval_layers.push(CodeRetrievalLayer::Impact);
        hit.score += 3.0;
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
        ORDER BY path ASC, line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![status.repository_id], |row| {
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
    })?;
    let query = request.query.to_lowercase();
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, request))
        .filter_map(|row| {
            let score = score_text(&query, [&row.name, &row.qualified_name, &row.signature]);
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
        ORDER BY r.path ASC, r.line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![status.repository_id], |row| {
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
    })?;
    let query = request.query.to_lowercase();
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, request))
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
               c.caller_name, c.callee_name, c.line_start, c.line_end
        FROM code_repository_calls c
        INNER JOIN code_repository_files f
            ON f.repository_id = c.repository_id AND f.path = c.path
        WHERE c.repository_id = ?1
        ORDER BY c.path ASC, c.line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![status.repository_id], |row| {
        Ok(CallRow {
            file_id: row.get(0)?,
            path: row.get(1)?,
            language_id: row.get(2)?,
            caller_symbol_snapshot_id: row.get(3)?,
            caller_name: row.get(4)?,
            callee_name: row.get(5)?,
            line_range: RepositoryCodeRange {
                start: row.get(6)?,
                end: row.get(7)?,
            },
        })
    })?;
    let query = request.query.to_lowercase();
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, request))
        .filter_map(|row| {
            let search_fields = match request.code_query_kind {
                CodeQueryKind::Callees => [row.caller_name.as_deref().unwrap_or(""), ""],
                CodeQueryKind::Callers => [&row.callee_name, ""],
                _ => [row.caller_name.as_deref().unwrap_or(""), &row.callee_name],
            };
            let score = score_text(&query, search_fields);
            (score > 0.0).then(|| {
                let caller = row.caller_name.unwrap_or_else(|| "<module>".to_owned());
                hit_from_parts(
                    status,
                    HitParts {
                        path: row.path,
                        language_id: row.language_id,
                        byte_range: RepositoryCodeRange { start: 0, end: 0 },
                        line_range: row.line_range,
                        symbol_snapshot_id: row.caller_symbol_snapshot_id,
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
        ORDER BY i.path ASC, i.line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![status.repository_id], |row| {
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
    })?;
    let query = request.query.to_lowercase();
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, request))
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
        SELECT file_id, path, language_id, content, byte_start, byte_end,
               line_start, line_end, symbol_snapshot_id
        FROM code_repository_chunks
        WHERE repository_id = ?1
        ORDER BY path ASC, line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![status.repository_id], |row| {
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
        })
    })?;
    let query = request.query.to_lowercase();
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, request))
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
                        retrieval_layers: vec![
                            CodeRetrievalLayer::Lexical,
                            CodeRetrievalLayer::TextFallback,
                        ],
                        score,
                        excerpt: row.content,
                        degraded_reason: None,
                    },
                )
            })
        })
        .collect())
}

fn symbols_for_paths(
    connection: &Connection,
    repository_id: &str,
    paths: &BTreeSet<String>,
) -> Result<Vec<String>, StorageError> {
    let mut path_statement = connection.prepare(
        "SELECT name FROM code_repository_symbols WHERE repository_id = ?1 AND path = ?2",
    )?;
    let mut symbols = Vec::new();
    for path in paths {
        let rows = path_statement.query_map(params![repository_id, path], |row| row.get(0))?;
        symbols.extend(
            rows.collect::<Result<Vec<String>, _>>()
                .map_err(StorageError::from)?,
        );
    }
    symbols.sort();
    symbols.dedup();

    Ok(symbols)
}

fn chunks_for_paths(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    paths: &BTreeSet<String>,
    request: &CodeImpactRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT file_id, path, language_id, content, byte_start, byte_end,
               line_start, line_end, symbol_snapshot_id
        FROM code_repository_chunks
        WHERE repository_id = ?1
        ORDER BY path ASC, line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![status.repository_id], |row| {
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
        })
    })?;
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| paths.contains(&row.path))
        .filter(|row| selected_impact_row(&row.path, &row.language_id, request))
        .map(|row| {
            hit_from_parts(
                status,
                HitParts {
                    path: row.path,
                    language_id: row.language_id,
                    byte_range: row.byte_range,
                    line_range: row.line_range,
                    symbol_snapshot_id: row.symbol_snapshot_id,
                    file_id: Some(row.file_id),
                    retrieval_layers: vec![CodeRetrievalLayer::Lexical],
                    score: 4.0,
                    excerpt: row.content,
                    degraded_reason: None,
                },
            )
        })
        .collect())
}

fn callers_for_symbols(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    symbols: &[String],
    request: &CodeImpactRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT c.file_id, c.path, f.language_id, c.caller_symbol_snapshot_id,
               c.caller_name, c.callee_name, c.line_start, c.line_end
        FROM code_repository_calls c
        INNER JOIN code_repository_files f
            ON f.repository_id = c.repository_id AND f.path = c.path
        WHERE c.repository_id = ?1
        ORDER BY c.path ASC, c.line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![status.repository_id], |row| {
        Ok(CallRow {
            file_id: row.get(0)?,
            path: row.get(1)?,
            language_id: row.get(2)?,
            caller_symbol_snapshot_id: row.get(3)?,
            caller_name: row.get(4)?,
            callee_name: row.get(5)?,
            line_range: RepositoryCodeRange {
                start: row.get(6)?,
                end: row.get(7)?,
            },
        })
    })?;
    let symbol_set = symbols.iter().collect::<BTreeSet<_>>();
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| selected_impact_row(&row.path, &row.language_id, request))
        .filter(|row| symbol_set.contains(&row.callee_name))
        .map(|row| {
            let caller = row.caller_name.unwrap_or_else(|| "<module>".to_owned());
            hit_from_parts(
                status,
                HitParts {
                    path: row.path,
                    language_id: row.language_id,
                    byte_range: RepositoryCodeRange { start: 0, end: 0 },
                    line_range: row.line_range,
                    symbol_snapshot_id: row.caller_symbol_snapshot_id,
                    file_id: Some(row.file_id),
                    retrieval_layers: vec![CodeRetrievalLayer::CallGraph],
                    score: 2.5,
                    excerpt: format!("{caller} calls {}", row.callee_name),
                    degraded_reason: None,
                },
            )
        })
        .collect())
}

fn importers_for_modules(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    modules: &[String],
    request: &CodeImpactRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT i.file_id, i.path, f.language_id, i.module, i.line_start, i.line_end
        FROM code_repository_imports i
        INNER JOIN code_repository_files f
            ON f.repository_id = i.repository_id AND f.path = i.path
        WHERE i.repository_id = ?1
        ORDER BY i.path ASC, i.line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![status.repository_id], |row| {
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
    })?;
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| selected_impact_row(&row.path, &row.language_id, request))
        .filter(|row| {
            modules
                .iter()
                .any(|module| module_import_matches(&row.module, module))
        })
        .map(|row| {
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
                    score: 2.0,
                    excerpt: row.module,
                    degraded_reason: None,
                },
            )
        })
        .collect())
}

fn required_repository(
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

fn selected_row(path: &str, language_id: &str, request: &CodeRetrievalRequest) -> bool {
    path_selected(path, request)
        && (request.repository.language_filters.is_empty()
            || request
                .repository
                .language_filters
                .iter()
                .any(|filter| filter == language_id))
}

fn path_selected(path: &str, request: &CodeRetrievalRequest) -> bool {
    request.repository.path_filters.is_empty()
        || request
            .repository
            .path_filters
            .iter()
            .any(|filter| path_matches_filter(path, filter))
}

fn selected_impact_row(path: &str, language_id: &str, request: &CodeImpactRequest) -> bool {
    let path_ok = request.repository.path_filters.is_empty()
        || request
            .repository
            .path_filters
            .iter()
            .any(|filter| path_matches_filter(path, filter));
    let language_ok = request.repository.language_filters.is_empty()
        || request
            .repository
            .language_filters
            .iter()
            .any(|filter| filter == language_id);

    path_ok && language_ok
}

fn path_matches_filter(path: &str, filter: &str) -> bool {
    let filter = filter.trim_end_matches(['/', '\\']);
    !filter.is_empty() && (path == filter || path.starts_with(&format!("{filter}/")))
}

fn module_import_matches(imported_module: &str, changed_module: &str) -> bool {
    imported_module
        .match_indices(changed_module)
        .any(|(start, value)| {
            let end = start + value.len();
            module_boundary(imported_module[..start].chars().next_back())
                && module_boundary(imported_module[end..].chars().next())
        })
}

fn module_boundary(character: Option<char>) -> bool {
    character
        .map(|value| matches!(value, ':' | '.' | '/' | '\\' | '-'))
        .unwrap_or(true)
}

struct HitParts {
    path: String,
    language_id: String,
    byte_range: RepositoryCodeRange,
    line_range: RepositoryCodeRange,
    symbol_snapshot_id: Option<String>,
    file_id: Option<String>,
    retrieval_layers: Vec<CodeRetrievalLayer>,
    score: f64,
    excerpt: String,
    degraded_reason: Option<String>,
}

fn hit_from_parts(status: &CodeRepositoryStatus, parts: HitParts) -> CodeRetrievalHit {
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

fn dedupe_sort_truncate(hits: &mut Vec<CodeRetrievalHit>, limit: usize) {
    let mut best = BTreeMap::<(String, u32, String), CodeRetrievalHit>::new();
    for hit in hits.drain(..) {
        let key = (hit.path.clone(), hit.line_range.start, hit.excerpt.clone());
        match best.get(&key) {
            Some(existing) if existing.score >= hit.score => {}
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

fn path_to_module_key(path: &str) -> String {
    path.trim_end_matches(".rs")
        .trim_end_matches(".py")
        .trim_end_matches(".ts")
        .trim_end_matches(".tsx")
        .replace(['/', '\\'], "::")
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_import_matching_respects_boundaries() {
        assert!(module_import_matches("crate::foo::bar", "foo::bar"));
        assert!(module_import_matches("foo::bar::baz", "foo::bar"));
        assert!(!module_import_matches("foo::barista", "foo::bar"));
        assert!(!module_import_matches("foo::bar_baz", "foo::bar"));
    }

    #[test]
    fn path_filters_accept_trailing_slashes() {
        assert!(path_matches_filter("src/lib.rs", "src/"));
        assert!(path_matches_filter("src/lib.rs", "src"));
        assert!(!path_matches_filter("src-other/lib.rs", "src/"));
    }
}
