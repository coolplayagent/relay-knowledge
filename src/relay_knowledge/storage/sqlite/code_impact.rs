use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::{
        CodeImpactRequest, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        RepositoryCodeRange,
    },
    storage::{CodeImpactChanges, StorageError},
};

use super::code_query::{
    HitParts, chunk_layers, dedupe_sort_truncate, hit_from_parts, language_filter_allows,
    path_filter_allows, required_repository, required_scope,
};

const CODE_PATH_LANGUAGE_SUFFIXES: &[(&str, &str)] = &[
    (".tsx", "tsx"),
    (".jsx", "jsx"),
    (".phtml", "php"),
    (".mts", "typescript"),
    (".cts", "typescript"),
    (".mjs", "javascript"),
    (".cjs", "javascript"),
    (".pyw", "python"),
    (".kts", "kotlin"),
    (".scala", "scala"),
    (".swift", "swift"),
    (".bash", "bash"),
    (".bats", "bash"),
    (".java", "java"),
    (".cpp", "cpp"),
    (".cxx", "cpp"),
    (".c++", "cpp"),
    (".hpp", "cpp"),
    (".hxx", "cpp"),
    (".h++", "cpp"),
    (".rs", "rust"),
    (".py", "python"),
    (".ts", "typescript"),
    (".js", "javascript"),
    (".go", "go"),
    (".kt", "kotlin"),
    (".sc", "scala"),
    (".cc", "cpp"),
    (".hh", "cpp"),
    (".cs", "csharp"),
    (".rb", "ruby"),
    (".php", "php"),
    (".sh", "bash"),
    (".c", "c"),
    (".h", "c"),
];

pub(super) fn analyze_impact(
    connection: &mut Connection,
    request: CodeImpactRequest,
    changes: CodeImpactChanges,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let status = required_repository(connection, &request.repository)?;
    let changed = selected_changed_paths(connection, &status, &request, changes.paths)?;
    let changed_symbols = symbol_seeds_for_paths(connection, required_scope(&status)?, &changed)?;
    let changed_modules =
        import_module_seeds(&changed, &changed_symbols, &changes.deleted_symbol_names);
    let mut hits = Vec::new();

    hits.extend(chunks_for_paths(connection, &status, &changed, &request)?);
    hits.extend(callers_for_symbols(
        connection,
        &status,
        &changed_symbols.symbol_ids,
        &changes.deleted_symbol_names,
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

fn symbol_seeds_for_paths(
    connection: &Connection,
    source_scope: &str,
    paths: &BTreeSet<String>,
) -> Result<ImpactSymbolSeeds, StorageError> {
    let mut path_statement = connection.prepare(
        "
        SELECT symbol_snapshot_id, path, name, qualified_name
        FROM code_repository_symbols
        WHERE source_scope = ?1 AND path = ?2
        ",
    )?;
    let mut symbol_ids = BTreeSet::new();
    let mut import_modules = BTreeSet::new();
    for path in paths {
        import_modules.extend(module_keys_for_path(path));
        let rows = path_statement.query_map(params![source_scope, path], |row| {
            Ok(ImpactSymbolRow {
                symbol_snapshot_id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                qualified_name: row.get(3)?,
            })
        })?;
        for row in rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)?
        {
            symbol_ids.insert(row.symbol_snapshot_id);
            insert_non_empty(&mut import_modules, row.qualified_name);
            insert_non_empty(&mut import_modules, row.name.clone());
            for module in module_keys_for_path(&row.path) {
                import_modules.insert(format!("{module}::{}", row.name));
                import_modules.insert(format!("{module}.{}", row.name));
            }
        }
    }

    Ok(ImpactSymbolSeeds {
        symbol_ids: symbol_ids.into_iter().collect(),
        import_modules: import_modules.into_iter().collect(),
    })
}

fn import_module_seeds(
    changed_paths: &BTreeSet<String>,
    changed_symbols: &ImpactSymbolSeeds,
    deleted_symbol_names: &[String],
) -> Vec<String> {
    let mut modules = changed_symbols
        .import_modules
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for path in changed_paths {
        modules.extend(module_keys_for_path(path));
    }
    for name in deleted_symbol_names {
        insert_non_empty(&mut modules, name.clone());
    }

    modules.into_iter().collect()
}

fn chunks_for_paths(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    paths: &BTreeSet<String>,
    request: &CodeImpactRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT c.file_id, c.path, c.language_id, c.content, c.byte_start, c.byte_end,
               c.line_start, c.line_end, c.symbol_snapshot_id, f.parse_status,
               f.degraded_reason
        FROM code_repository_chunks c
        INNER JOIN code_repository_files f
            ON f.source_scope = c.source_scope AND f.path = c.path
        WHERE c.source_scope = ?1
        ORDER BY c.path ASC, c.line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![required_scope(status)?], |row| {
        Ok(ImpactChunkRow {
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
    })?;
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| paths.contains(&row.path))
        .filter(|row| selected_impact_row(&row.path, &row.language_id, status, request))
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
                    retrieval_layers: chunk_layers(&row.parse_status),
                    score: 4.0,
                    excerpt: row.content,
                    degraded_reason: row.degraded_reason,
                },
            )
        })
        .collect())
}

fn callers_for_symbols(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    symbol_ids: &[String],
    deleted_symbol_names: &[String],
    request: &CodeImpactRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT c.file_id, c.path, f.language_id, c.caller_symbol_snapshot_id,
               c.caller_name, c.callee_symbol_snapshot_id, c.callee_name,
               c.line_start, c.line_end
        FROM code_repository_calls c
        INNER JOIN code_repository_files f
            ON f.source_scope = c.source_scope AND f.path = c.path
        WHERE c.source_scope = ?1
        ORDER BY c.path ASC, c.line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![required_scope(status)?], |row| {
        Ok(ImpactCallRow {
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
    })?;
    let symbol_set = symbol_ids.iter().collect::<BTreeSet<_>>();
    let deleted_name_set = deleted_symbol_names.iter().collect::<BTreeSet<_>>();
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| selected_impact_row(&row.path, &row.language_id, status, request))
        .filter(|row| {
            row.callee_symbol_snapshot_id
                .as_ref()
                .is_some_and(|symbol_id| symbol_set.contains(symbol_id))
                || (row.callee_symbol_snapshot_id.is_none()
                    && deleted_name_set.contains(&row.callee_name))
        })
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
            ON f.source_scope = i.source_scope AND f.path = i.path
        WHERE i.source_scope = ?1
        ORDER BY i.path ASC, i.line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![required_scope(status)?], |row| {
        Ok(ImpactImportRow {
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
        .filter(|row| selected_impact_row(&row.path, &row.language_id, status, request))
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

fn selected_changed_paths(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeImpactRequest,
    changed_paths: Vec<String>,
) -> Result<BTreeSet<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT language_id
        FROM code_repository_files
        WHERE repository_id = ?1 AND path = ?2
        ",
    )?;
    let mut selected = BTreeSet::new();
    for path in changed_paths {
        if !path_filter_allows(&path, &status.path_filters)
            || !path_filter_allows(&path, &request.repository.path_filters)
        {
            continue;
        }
        let stored_language_id = statement
            .query_row(params![&status.repository_id, &path], |row| {
                row.get::<_, String>(0)
            })
            .optional()?;
        let language_id = stored_language_id.or_else(|| language_id_for_path(&path));
        if language_id
            .as_deref()
            .map(|language| {
                language_filter_allows(language, &status.language_filters)
                    && language_filter_allows(language, &request.repository.language_filters)
            })
            .unwrap_or_else(|| {
                status.language_filters.is_empty() && request.repository.language_filters.is_empty()
            })
        {
            selected.insert(path);
        }
    }

    Ok(selected)
}

fn selected_impact_row(
    path: &str,
    language_id: &str,
    status: &CodeRepositoryStatus,
    request: &CodeImpactRequest,
) -> bool {
    path_filter_allows(path, &status.path_filters)
        && path_filter_allows(path, &request.repository.path_filters)
        && language_filter_allows(language_id, &status.language_filters)
        && language_filter_allows(language_id, &request.repository.language_filters)
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
        .map(|value| {
            matches!(
                value,
                ':' | '.'
                    | '/'
                    | '\\'
                    | ';'
                    | ','
                    | '{'
                    | '}'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '"'
                    | '\''
                    | '`'
                    | ' '
                    | '\t'
                    | '\n'
                    | '\r'
            )
        })
        .unwrap_or(true)
}

fn language_id_for_path(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let file_name = normalized.rsplit('/').next().unwrap_or(&normalized);
    match file_name {
        ".bash_profile" | ".bashrc" | ".profile" | "bash_profile" | "bashrc" => {
            return Some("bash".to_owned());
        }
        "Gemfile" | "Rakefile" => return Some("ruby".to_owned()),
        _ => {}
    }
    language_suffix_for_path(&normalized).map(|(_, language_id)| language_id.to_owned())
}

fn module_keys_for_path(path: &str) -> BTreeSet<String> {
    let normalized = path.replace('\\', "/");
    let stem = path_without_code_extension(&normalized);
    let mut modules = BTreeSet::new();
    insert_non_empty(&mut modules, stem.replace(['/', '\\'], "::"));
    insert_non_empty(&mut modules, stem.replace(['/', '\\'], "."));
    if let Some(crate_module) = rust_crate_module_key(&stem) {
        modules.insert(crate_module);
    }

    modules
}

fn path_without_code_extension(path: &str) -> String {
    if let Some((suffix, _)) = language_suffix_for_path(path) {
        let stem_end = path.len().saturating_sub(suffix.len());
        return path[..stem_end].to_owned();
    }

    path.to_owned()
}

fn language_suffix_for_path(path: &str) -> Option<(&'static str, &'static str)> {
    let lower = path.to_ascii_lowercase();
    CODE_PATH_LANGUAGE_SUFFIXES
        .iter()
        .copied()
        .find(|(suffix, _)| lower.ends_with(suffix))
}

fn rust_crate_module_key(path_stem: &str) -> Option<String> {
    let module = path_stem.strip_prefix("src/")?;
    if matches!(module, "lib" | "main") {
        return Some("crate".to_owned());
    }
    let module = module.strip_suffix("/mod").unwrap_or(module);
    let module = module.replace(['/', '\\'], "::");
    (!module.is_empty()).then(|| format!("crate::{module}"))
}

fn insert_non_empty(values: &mut BTreeSet<String>, value: String) {
    let value = value.trim();
    if !value.is_empty() {
        values.insert(value.to_owned());
    }
}

struct ImpactSymbolSeeds {
    symbol_ids: Vec<String>,
    import_modules: Vec<String>,
}

struct ImpactSymbolRow {
    symbol_snapshot_id: String,
    path: String,
    name: String,
    qualified_name: String,
}

struct ImpactChunkRow {
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

struct ImpactCallRow {
    file_id: String,
    path: String,
    language_id: String,
    caller_symbol_snapshot_id: Option<String>,
    caller_name: Option<String>,
    callee_symbol_snapshot_id: Option<String>,
    callee_name: String,
    line_range: RepositoryCodeRange,
}

struct ImpactImportRow {
    file_id: String,
    path: String,
    language_id: String,
    module: String,
    line_range: RepositoryCodeRange,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_import_matching_respects_boundaries() {
        assert!(module_import_matches("crate::foo::bar", "foo::bar"));
        assert!(module_import_matches("foo::bar::baz", "foo::bar"));
        assert!(module_import_matches(
            "use crate::foo::bar;",
            "crate::foo::bar"
        ));
        assert!(module_import_matches("from foo.bar import baz", "foo.bar"));
        assert!(!module_import_matches("foo::barista", "foo::bar"));
        assert!(!module_import_matches("foo::bar_baz", "foo::bar"));
        assert!(!module_import_matches("foo::bar-baz", "foo::bar"));
    }
}
