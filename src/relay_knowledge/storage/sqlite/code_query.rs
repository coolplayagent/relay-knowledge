use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};

use crate::{
    domain::{
        CodeImpactRequest, CodeQueryKind, RepositoryCodeRange, CodeRepositoryStatus, CodeRetrievalHit,
        CodeRetrievalLayer, CodeRetrievalRequest,
    },
    storage::StorageError,
};

use super::repository_status;

pub(super) fn search_code(
    connection: &mut Connection,
    request: CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let status = required_repository(connection, &request.repository.repository)?;
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
    let status = required_repository(connection, &request.repository.repository)?;
    let changed = changed_paths.into_iter().collect::<BTreeSet<_>>();
    let changed_modules = changed
        .iter()
        .map(|path| path_to_module_key(path))
        .collect::<Vec<_>>();
    let changed_symbols = symbols_for_paths(connection, &status.repository_id, &changed)?;
    let mut hits = Vec::new();

    hits.extend(chunks_for_paths(connection, &status, &changed)?);
    hits.extend(callers_for_symbols(connection, &status, &changed_symbols)?);
    hits.extend(importers_for_modules(
        connection,
        &status,
        &changed_modules,
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
        FROM code_symbols
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
        SELECT file_id, path, name, kind, target_symbol_snapshot_id,
               byte_start, byte_end, line_start, line_end
        FROM code_references
        WHERE repository_id = ?1
        ORDER BY path ASC, line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![status.repository_id], |row| {
        Ok(ReferenceRow {
            file_id: row.get(0)?,
            path: row.get(1)?,
            name: row.get(2)?,
            kind: row.get(3)?,
            target_symbol_snapshot_id: row.get(4)?,
            byte_range: RepositoryCodeRange {
                start: row.get(5)?,
                end: row.get(6)?,
            },
            line_range: RepositoryCodeRange {
                start: row.get(7)?,
                end: row.get(8)?,
            },
        })
    })?;
    let query = request.query.to_lowercase();
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| path_selected(&row.path, request))
        .filter_map(|row| {
            let score = score_text(&query, [&row.name, &row.kind]);
            (score > 0.0).then(|| {
                hit_from_parts(
                    status,
                    HitParts {
                        path: row.path,
                        language_id: "unknown".to_owned(),
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
        SELECT file_id, path, caller_symbol_snapshot_id, caller_name,
               callee_name, line_start, line_end
        FROM code_calls
        WHERE repository_id = ?1
        ORDER BY path ASC, line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![status.repository_id], |row| {
        Ok(CallRow {
            file_id: row.get(0)?,
            path: row.get(1)?,
            caller_symbol_snapshot_id: row.get(2)?,
            caller_name: row.get(3)?,
            callee_name: row.get(4)?,
            line_range: RepositoryCodeRange {
                start: row.get(5)?,
                end: row.get(6)?,
            },
        })
    })?;
    let query = request.query.to_lowercase();
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| path_selected(&row.path, request))
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
                        language_id: "unknown".to_owned(),
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
        SELECT file_id, path, module, line_start, line_end
        FROM code_imports
        WHERE repository_id = ?1
        ORDER BY path ASC, line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![status.repository_id], |row| {
        Ok(ImportRow {
            file_id: row.get(0)?,
            path: row.get(1)?,
            module: row.get(2)?,
            line_range: RepositoryCodeRange {
                start: row.get(3)?,
                end: row.get(4)?,
            },
        })
    })?;
    let query = request.query.to_lowercase();
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| path_selected(&row.path, request))
        .filter_map(|row| {
            let score = score_text(&query, [&row.module, &row.path]);
            (score > 0.0).then(|| {
                hit_from_parts(
                    status,
                    HitParts {
                        path: row.path,
                        language_id: "unknown".to_owned(),
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
        FROM code_chunks
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
    let mut statement = connection
        .prepare("SELECT name FROM code_symbols WHERE repository_id = ?1 ORDER BY name ASC")?;
    let rows = statement.query_map(params![repository_id], |row| row.get::<_, String>(0))?;
    let names = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    let mut path_statement = connection
        .prepare("SELECT name FROM code_symbols WHERE repository_id = ?1 AND path = ?2")?;
    let mut symbols = Vec::new();
    for path in paths {
        let rows = path_statement.query_map(params![repository_id, path], |row| row.get(0))?;
        symbols.extend(
            rows.collect::<Result<Vec<String>, _>>()
                .map_err(StorageError::from)?,
        );
    }
    if symbols.is_empty() {
        symbols = names;
    }

    Ok(symbols)
}

fn chunks_for_paths(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    paths: &BTreeSet<String>,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT file_id, path, language_id, content, byte_start, byte_end,
               line_start, line_end, symbol_snapshot_id
        FROM code_chunks
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
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT file_id, path, caller_symbol_snapshot_id, caller_name,
               callee_name, line_start, line_end
        FROM code_calls
        WHERE repository_id = ?1
        ORDER BY path ASC, line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![status.repository_id], |row| {
        Ok(CallRow {
            file_id: row.get(0)?,
            path: row.get(1)?,
            caller_symbol_snapshot_id: row.get(2)?,
            caller_name: row.get(3)?,
            callee_name: row.get(4)?,
            line_range: RepositoryCodeRange {
                start: row.get(5)?,
                end: row.get(6)?,
            },
        })
    })?;
    let symbol_set = symbols.iter().collect::<BTreeSet<_>>();
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| symbol_set.contains(&row.callee_name))
        .map(|row| {
            let caller = row.caller_name.unwrap_or_else(|| "<module>".to_owned());
            hit_from_parts(
                status,
                HitParts {
                    path: row.path,
                    language_id: "unknown".to_owned(),
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
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT file_id, path, module, line_start, line_end
        FROM code_imports
        WHERE repository_id = ?1
        ORDER BY path ASC, line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![status.repository_id], |row| {
        Ok(ImportRow {
            file_id: row.get(0)?,
            path: row.get(1)?,
            module: row.get(2)?,
            line_range: RepositoryCodeRange {
                start: row.get(3)?,
                end: row.get(4)?,
            },
        })
    })?;
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| modules.iter().any(|module| row.module.contains(module)))
        .map(|row| {
            hit_from_parts(
                status,
                HitParts {
                    path: row.path,
                    language_id: "unknown".to_owned(),
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
) -> Result<CodeRepositoryStatus, StorageError> {
    repository_status(connection, repository)?.ok_or_else(|| {
        StorageError::InvalidInput(format!("code repository '{repository}' is not registered"))
    })
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
            .any(|filter| path == filter || path.starts_with(&format!("{filter}/")))
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
    name: String,
    kind: String,
    target_symbol_snapshot_id: Option<String>,
    byte_range: RepositoryCodeRange,
    line_range: RepositoryCodeRange,
}

struct CallRow {
    file_id: String,
    path: String,
    caller_symbol_snapshot_id: Option<String>,
    caller_name: Option<String>,
    callee_name: String,
    line_range: RepositoryCodeRange,
}

struct ImportRow {
    file_id: String,
    path: String,
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
