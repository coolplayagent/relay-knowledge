use rusqlite::{Connection, params_from_iter, types::Value};

mod affected;
mod call_focus;
mod dependencies;
mod truncation;
use crate::{
    domain::{
        CodeCallRecord, CodeFeatureFlagRecord, CodeImportRecord, CodeRouteRecord, CodebaseViewCall,
        CodebaseViewFile, CodebaseViewKind, CodebaseViewRequest, CodebaseViewSnapshot,
        CodebaseViewSymbol, RepositoryCodeRange,
    },
    storage::StorageError,
};

pub(super) fn snapshot(
    connection: &Connection,
    source_scope: &str,
    request: &CodebaseViewRequest,
    row_limit: usize,
) -> Result<CodebaseViewSnapshot, StorageError> {
    let probe_limit = row_limit.saturating_add(1);
    let mut imports = imports(connection, source_scope, request, probe_limit)?;
    let imports_truncated = truncate_to_limit(&mut imports, row_limit);
    let mut files = files(connection, source_scope, request, probe_limit)?;
    let files_truncated = truncate_to_limit(&mut files, row_limit);
    let mut import_target_files = if view_uses_supplemental_import_target_files(request.view_kind) {
        resolved_import_target_files(connection, source_scope, request, &imports, probe_limit)?
    } else {
        Vec::new()
    };
    let import_target_files_truncated = truncate_to_limit(&mut import_target_files, row_limit);
    merge_files(&mut files, import_target_files);
    let mut routes = routes(connection, source_scope, request, probe_limit)?;
    let routes_truncated = truncate_to_limit(&mut routes, row_limit);
    let mut symbols = symbols(connection, source_scope, &routes, probe_limit)?;
    truncate_to_limit(&mut symbols, row_limit);
    let call_focus =
        call_focus::call_focus_paths(connection, source_scope, request, &routes, probe_limit)?;
    let mut calls = calls(connection, source_scope, request, &call_focus, probe_limit)?;
    let calls_truncated = truncate_to_limit(&mut calls, row_limit);
    let mut dependencies =
        dependencies::dependencies(connection, source_scope, request, probe_limit)?;
    let dependencies_truncated = truncate_to_limit(&mut dependencies, row_limit);
    let mut feature_flags = feature_flags(connection, source_scope, request, probe_limit)?;
    let feature_flags_truncated = truncate_to_limit(&mut feature_flags, row_limit);
    let truncated = truncation::snapshot_truncated(
        request.view_kind,
        files_truncated,
        import_target_files_truncated,
        &[
            ("imports", imports_truncated),
            ("calls", calls_truncated),
            ("routes", routes_truncated),
            ("dependencies", dependencies_truncated),
            ("feature_flags", feature_flags_truncated),
        ],
    );

    Ok(CodebaseViewSnapshot {
        declared_business_domains: Vec::new(),
        files,
        symbols,
        imports,
        calls,
        routes,
        dependencies,
        feature_flags,
        truncated,
    })
}

fn truncate_to_limit<T>(rows: &mut Vec<T>, limit: usize) -> bool {
    if rows.len() > limit {
        rows.truncate(limit);
        true
    } else {
        false
    }
}

fn view_uses_supplemental_import_target_files(view_kind: CodebaseViewKind) -> bool {
    matches!(
        view_kind,
        CodebaseViewKind::ArchitectureLayers | CodebaseViewKind::DependencyTour
    )
}

fn files(
    connection: &Connection,
    source_scope: &str,
    request: &CodebaseViewRequest,
    limit: usize,
) -> Result<Vec<CodebaseViewFile>, StorageError> {
    let (sql, values) = filtered_sql(
        "
        SELECT path, language_id, parse_status, line_count, is_generated
        FROM code_repository_files
        WHERE source_scope = ?1
        ",
        source_scope,
        request,
        FilterColumns::new("path", Some("language_id")),
        |sql, values| affected::append_file_focus(sql, values, request),
        "
        ORDER BY path ASC
        ",
        limit,
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values.iter()), |row| {
        Ok(CodebaseViewFile {
            path: row.get(0)?,
            language_id: row.get(1)?,
            parse_status: row.get(2)?,
            line_count: row.get(3)?,
            is_generated: row.get::<_, i64>(4)? != 0,
        })
    })?;

    collect_rows(rows)
}

fn resolved_import_target_files(
    connection: &Connection,
    source_scope: &str,
    request: &CodebaseViewRequest,
    imports: &[CodeImportRecord],
    limit: usize,
) -> Result<Vec<CodebaseViewFile>, StorageError> {
    let target_paths = dedupe_paths(imports.iter().filter_map(|import| {
        (import.resolution_state == "resolved")
            .then_some(import.target_hint.as_deref())
            .flatten()
    }));
    if target_paths.is_empty() {
        return Ok(Vec::new());
    }
    let (sql, values) = filtered_sql(
        "
        SELECT path, language_id, parse_status, line_count, is_generated
        FROM code_repository_files
        WHERE source_scope = ?1
        ",
        source_scope,
        request,
        FilterColumns::new("path", Some("language_id")).without_request_path_filter(),
        |sql, values| append_path_filters(sql, values, "path", &target_paths),
        "
        ORDER BY path ASC
        ",
        limit,
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values.iter()), |row| {
        Ok(CodebaseViewFile {
            path: row.get(0)?,
            language_id: row.get(1)?,
            parse_status: row.get(2)?,
            line_count: row.get(3)?,
            is_generated: row.get::<_, i64>(4)? != 0,
        })
    })?;

    collect_rows(rows)
}

fn merge_files(files: &mut Vec<CodebaseViewFile>, supplemental: Vec<CodebaseViewFile>) {
    for file in supplemental {
        if !files.iter().any(|existing| existing.path == file.path) {
            files.push(file);
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
}

fn imports(
    connection: &Connection,
    source_scope: &str,
    request: &CodebaseViewRequest,
    limit: usize,
) -> Result<Vec<CodeImportRecord>, StorageError> {
    let (sql, values) = filtered_sql(
        "
        SELECT import.repository_id, import.source_scope, import.import_id, import.file_id,
               import.path, import.module, import.target_hint, import.resolution_state,
               import.confidence_basis_points, import.confidence_tier,
               import.line_start, import.line_end
        FROM code_repository_imports import
        LEFT JOIN code_repository_files file
          ON file.source_scope = import.source_scope
         AND file.path = import.path
        WHERE import.source_scope = ?1
        ",
        source_scope,
        request,
        FilterColumns::new("import.path", Some("file.language_id")),
        |_, _| {},
        "
        ORDER BY import.path ASC, import.line_start ASC, import.module ASC
        ",
        limit,
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values.iter()), |row| {
        Ok(CodeImportRecord {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            import_id: row.get(2)?,
            file_id: row.get(3)?,
            path: row.get(4)?,
            module: row.get(5)?,
            target_hint: row.get(6)?,
            resolution_state: row.get(7)?,
            confidence_basis_points: row.get(8)?,
            confidence_tier: row.get(9)?,
            line_range: RepositoryCodeRange {
                start: row.get(10)?,
                end: row.get(11)?,
            },
        })
    })?;

    collect_rows(rows)
}

fn calls(
    connection: &Connection,
    source_scope: &str,
    request: &CodebaseViewRequest,
    focus: &call_focus::CallFocusPaths,
    limit: usize,
) -> Result<Vec<CodebaseViewCall>, StorageError> {
    let (sql, values) = filtered_sql(
        "
        SELECT call.repository_id, call.source_scope, call.call_id, call.file_id, call.path,
               call.caller_symbol_snapshot_id, call.caller_name,
               call.callee_symbol_snapshot_id, call.callee_name, call.target_hint,
               call.resolution_state, call.confidence_basis_points, call.confidence_tier,
               call.line_start, call.line_end, callee.path
        FROM code_repository_calls call
        LEFT JOIN code_repository_symbols callee
          ON callee.source_scope = call.source_scope
         AND callee.symbol_snapshot_id = call.callee_symbol_snapshot_id
        LEFT JOIN code_repository_files file
          ON file.source_scope = call.source_scope
         AND file.path = call.path
        WHERE call.source_scope = ?1
        ",
        source_scope,
        request,
        process_flow_call_filter_columns(request),
        |sql, values| call_focus::append_call_focus_filters(sql, values, focus),
        "
        ORDER BY call.path ASC, call.line_start ASC, call.callee_name ASC
        ",
        limit,
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values.iter()), |row| {
        Ok(CodebaseViewCall {
            call: CodeCallRecord {
                repository_id: row.get(0)?,
                source_scope: row.get(1)?,
                call_id: row.get(2)?,
                file_id: row.get(3)?,
                path: row.get(4)?,
                caller_symbol_snapshot_id: row.get(5)?,
                caller_name: row.get(6)?,
                callee_symbol_snapshot_id: row.get(7)?,
                callee_name: row.get(8)?,
                target_hint: row.get(9)?,
                resolution_state: row.get(10)?,
                confidence_basis_points: row.get(11)?,
                confidence_tier: row.get(12)?,
                line_range: RepositoryCodeRange {
                    start: row.get(13)?,
                    end: row.get(14)?,
                },
            },
            callee_path: row.get(15)?,
        })
    })?;

    collect_rows(rows)
}

fn routes(
    connection: &Connection,
    source_scope: &str,
    request: &CodebaseViewRequest,
    limit: usize,
) -> Result<Vec<CodeRouteRecord>, StorageError> {
    let (sql, values) = filtered_sql(
        "
        SELECT repository_id, source_scope, route_id, file_id, path, language_id, url,
               http_method, handler_name, handler_symbol_snapshot_id, framework,
               line_start, line_end
        FROM code_repository_routes
        WHERE source_scope = ?1
        ",
        source_scope,
        request,
        FilterColumns::new("path", Some("language_id")),
        |_, _| {},
        "
        ORDER BY path ASC, line_start ASC, url ASC
        ",
        limit,
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values.iter()), |row| {
        Ok(CodeRouteRecord {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            route_id: row.get(2)?,
            file_id: row.get(3)?,
            path: row.get(4)?,
            language_id: row.get(5)?,
            url: row.get(6)?,
            http_method: row.get(7)?,
            handler_name: row.get(8)?,
            handler_symbol_snapshot_id: row.get(9)?,
            framework: row.get(10)?,
            line_range: RepositoryCodeRange {
                start: row.get(11)?,
                end: row.get(12)?,
            },
        })
    })?;

    collect_rows(rows)
}

fn symbols(
    connection: &Connection,
    source_scope: &str,
    routes: &[CodeRouteRecord],
    limit: usize,
) -> Result<Vec<CodebaseViewSymbol>, StorageError> {
    let symbol_ids = dedupe_symbol_ids(routes);
    if symbol_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut sql = "
        SELECT symbol_snapshot_id, path, language_id, name, qualified_name, kind,
               line_start, line_end
        FROM code_repository_symbols
        WHERE source_scope = ?1
          AND symbol_snapshot_id IN (
        "
    .to_owned();
    let mut values = vec![Value::Text(source_scope.to_owned())];
    for (index, symbol_id) in symbol_ids.iter().enumerate() {
        if index > 0 {
            sql.push_str(", ");
        }
        sql.push('?');
        values.push(Value::Text(symbol_id.clone()));
    }
    sql.push_str(") ORDER BY path ASC, line_start ASC, symbol_snapshot_id ASC LIMIT ?");
    values.push(Value::Integer(limit as i64));

    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values.iter()), |row| {
        Ok(CodebaseViewSymbol {
            symbol_snapshot_id: row.get(0)?,
            path: row.get(1)?,
            language_id: row.get(2)?,
            name: row.get(3)?,
            qualified_name: row.get(4)?,
            kind: row.get(5)?,
            line_range: RepositoryCodeRange {
                start: row.get(6)?,
                end: row.get(7)?,
            },
        })
    })?;

    collect_rows(rows)
}

fn feature_flags(
    connection: &Connection,
    source_scope: &str,
    request: &CodebaseViewRequest,
    limit: usize,
) -> Result<Vec<CodeFeatureFlagRecord>, StorageError> {
    let (sql, values) = filtered_sql(
        "
        SELECT repository_id, source_scope, feature_flag_id, usage_id, file_id, path,
               language_id, name, source_kind, source_key, edge_kind,
               confidence_basis_points, confidence_tier, byte_start, byte_end,
               line_start, line_end, excerpt
        FROM code_repository_feature_flags
        WHERE source_scope = ?1
        ",
        source_scope,
        request,
        FilterColumns::new("path", Some("language_id")),
        |_, _| {},
        "
        ORDER BY name ASC, path ASC, line_start ASC
        ",
        limit,
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values.iter()), |row| {
        Ok(CodeFeatureFlagRecord {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            feature_flag_id: row.get(2)?,
            usage_id: row.get(3)?,
            file_id: row.get(4)?,
            path: row.get(5)?,
            language_id: row.get(6)?,
            name: row.get(7)?,
            source_kind: row.get(8)?,
            source_key: row.get(9)?,
            edge_kind: row.get(10)?,
            confidence_basis_points: row.get(11)?,
            confidence_tier: row.get(12)?,
            byte_range: RepositoryCodeRange {
                start: row.get(13)?,
                end: row.get(14)?,
            },
            line_range: RepositoryCodeRange {
                start: row.get(15)?,
                end: row.get(16)?,
            },
            excerpt: row.get(17)?,
        })
    })?;

    collect_rows(rows)
}

fn collect_rows<T>(
    rows: impl Iterator<Item = rusqlite::Result<T>>,
) -> Result<Vec<T>, StorageError> {
    rows.map(|row| row.map_err(StorageError::from)).collect()
}

fn filtered_sql(
    select_and_where: &str,
    source_scope: &str,
    request: &CodebaseViewRequest,
    columns: FilterColumns<'_>,
    extra_filters: impl FnOnce(&mut String, &mut Vec<Value>),
    order_by: &str,
    limit: usize,
) -> (String, Vec<Value>) {
    let mut sql = select_and_where.to_owned();
    let mut values = vec![Value::Text(source_scope.to_owned())];
    if columns.apply_request_path_filter {
        append_path_filters(
            &mut sql,
            &mut values,
            columns.path,
            &request.repository.path_filters,
        );
    }
    if let Some(language_column) = columns.language {
        append_language_filters(
            &mut sql,
            &mut values,
            language_column,
            &request.repository.language_filters,
        );
    }
    extra_filters(&mut sql, &mut values);
    sql.push_str(order_by);
    sql.push_str(" LIMIT ?");
    values.push(Value::Integer(limit as i64));
    (sql, values)
}

struct FilterColumns<'a> {
    path: &'a str,
    language: Option<&'a str>,
    apply_request_path_filter: bool,
}

impl<'a> FilterColumns<'a> {
    fn new(path: &'a str, language: Option<&'a str>) -> Self {
        Self {
            path,
            language,
            apply_request_path_filter: true,
        }
    }

    fn without_request_path_filter(mut self) -> Self {
        self.apply_request_path_filter = false;
        self
    }
}

fn process_flow_call_filter_columns(request: &CodebaseViewRequest) -> FilterColumns<'_> {
    let columns = FilterColumns::new("call.path", Some("file.language_id"));
    if request.view_kind == CodebaseViewKind::ProcessFlow {
        columns.without_request_path_filter()
    } else {
        columns
    }
}

pub(super) fn append_path_filter_set(
    sql: &mut String,
    values: &mut Vec<Value>,
    column: &str,
    paths: &[String],
    has_clause: bool,
) -> bool {
    if paths.is_empty() {
        return has_clause;
    }
    if has_clause {
        sql.push_str(" OR ");
    }
    for (index, path) in paths.iter().enumerate() {
        if index > 0 {
            sql.push_str(" OR ");
        }
        sql.push_str(column);
        sql.push_str(" = ? OR ");
        sql.push_str(column);
        sql.push_str(" LIKE ? ESCAPE '\\'");
        values.push(Value::Text(path.clone()));
        values.push(Value::Text(format!("{}/%", escape_like(path))));
    }
    true
}

fn dedupe_paths<'a>(paths: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut deduped = Vec::new();
    for path in paths.filter_map(normalized_path_filter) {
        if path != "." && !deduped.contains(&path) {
            deduped.push(path);
        }
    }
    deduped
}

fn dedupe_symbol_ids(routes: &[CodeRouteRecord]) -> Vec<String> {
    let mut deduped = Vec::new();
    for symbol_id in routes
        .iter()
        .filter_map(|route| route.handler_symbol_snapshot_id.as_deref())
    {
        if !deduped.iter().any(|existing| existing == symbol_id) {
            deduped.push(symbol_id.to_owned());
        }
    }
    deduped
}

fn append_path_filters(
    sql: &mut String,
    values: &mut Vec<Value>,
    column: &str,
    filters: &[String],
) {
    let filters = filters
        .iter()
        .filter_map(|filter| normalized_path_filter(filter))
        .filter(|filter| filter != ".")
        .collect::<Vec<_>>();
    if filters.is_empty() {
        return;
    }
    sql.push_str(" AND (");
    for (index, filter) in filters.iter().enumerate() {
        if index > 0 {
            sql.push_str(" OR ");
        }
        sql.push_str(column);
        sql.push_str(" = ? OR ");
        sql.push_str(column);
        sql.push_str(" LIKE ? ESCAPE '\\'");
        values.push(Value::Text(filter.clone()));
        values.push(Value::Text(format!("{}/%", escape_like(filter))));
    }
    sql.push(')');
}

fn push_unique_path(paths: &mut Vec<String>, path: String) {
    if path != "." && !path.is_empty() && !paths.contains(&path) {
        paths.push(path);
    }
}

fn append_language_filters(
    sql: &mut String,
    values: &mut Vec<Value>,
    column: &str,
    filters: &[String],
) {
    if filters.is_empty() {
        return;
    }
    sql.push_str(" AND ");
    sql.push_str(column);
    sql.push_str(" IN (");
    for (index, filter) in filters.iter().enumerate() {
        if index > 0 {
            sql.push_str(", ");
        }
        sql.push('?');
        values.push(Value::Text(filter.clone()));
    }
    sql.push(')');
}

fn normalized_path_filter(filter: &str) -> Option<String> {
    let mut filter = filter.replace('\\', "/");
    while filter.ends_with('/') {
        filter.pop();
    }
    while filter.starts_with("./") {
        filter.drain(..2);
    }
    (!filter.is_empty()).then_some(filter)
}

fn escape_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
