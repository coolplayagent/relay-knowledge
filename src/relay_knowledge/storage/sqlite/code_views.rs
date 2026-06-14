use rusqlite::{Connection, params_from_iter, types::Value};

#[path = "code_views_affected.rs"]
mod code_views_affected;
#[path = "code_views_call_focus.rs"]
mod code_views_call_focus;
#[path = "code_views_dependencies.rs"]
mod code_views_dependencies;
#[path = "code_views_truncation.rs"]
mod code_views_truncation;
use crate::{
    domain::{
        CodeCallRecord, CodeFeatureFlagRecord, CodeImportRecord, CodeRouteRecord, CodebaseViewCall,
        CodebaseViewFile, CodebaseViewRequest, CodebaseViewSnapshot, RepositoryCodeRange,
    },
    storage::StorageError,
};

pub(super) fn snapshot(
    connection: &Connection,
    source_scope: &str,
    request: &CodebaseViewRequest,
    row_limit: usize,
) -> Result<CodebaseViewSnapshot, StorageError> {
    let imports = imports(connection, source_scope, request, row_limit)?;
    let mut files = files(connection, source_scope, request, row_limit)?;
    let files_truncated = files.len() == row_limit;
    let import_target_files =
        resolved_import_target_files(connection, source_scope, request, &imports, row_limit)?;
    let import_target_files_truncated = import_target_files.len() == row_limit;
    merge_files(&mut files, import_target_files);
    let routes = routes(connection, source_scope, request, row_limit)?;
    let call_focus = code_views_call_focus::call_focus_paths(
        connection,
        source_scope,
        request,
        &routes,
        row_limit,
    )?;
    let calls = calls(connection, source_scope, request, &call_focus, row_limit)?;
    let dependencies =
        code_views_dependencies::dependencies(connection, source_scope, request, row_limit)?;
    let feature_flags = feature_flags(connection, source_scope, request, row_limit)?;
    let truncated = code_views_truncation::snapshot_truncated(
        request.view_kind,
        files_truncated,
        import_target_files_truncated,
        &[
            ("imports", imports.len()),
            ("calls", calls.len()),
            ("routes", routes.len()),
            ("dependencies", dependencies.len()),
            ("feature_flags", feature_flags.len()),
        ],
        row_limit,
    );

    Ok(CodebaseViewSnapshot {
        files,
        symbols: Vec::new(),
        imports,
        calls,
        routes,
        dependencies,
        feature_flags,
        truncated,
    })
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
        |sql, values| code_views_affected::append_file_focus(sql, values, request),
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
        FilterColumns::new("path", Some("language_id")),
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
    focus: &code_views_call_focus::CallFocusPaths,
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
        FilterColumns::new("call.path", Some("file.language_id")),
        |sql, values| code_views_call_focus::append_call_focus_filters(sql, values, focus),
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
    append_path_filters(
        &mut sql,
        &mut values,
        columns.path,
        &request.repository.path_filters,
    );
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
}

impl<'a> FilterColumns<'a> {
    fn new(path: &'a str, language: Option<&'a str>) -> Self {
        Self { path, language }
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
mod tests {
    use rusqlite::{Connection, types::Value};

    use crate::domain::{
        CodeRepositorySelector, CodebaseViewKind, CodebaseViewRequest, FreshnessPolicy,
    };

    use super::{FilterColumns, filtered_sql, snapshot};

    #[test]
    fn snapshot_reads_filtered_view_rows() {
        let connection = Connection::open_in_memory().unwrap();
        create_view_tables(&connection);
        seed_view_rows(&connection);
        let request = request(vec!["src/api".to_owned()], vec!["rust".to_owned()], 10);

        let snapshot = snapshot(&connection, "scope", &request, 10).unwrap();

        assert_eq!(snapshot.files[0].path, "src/api/users.rs");
        assert!(snapshot.symbols.is_empty());
        assert_eq!(snapshot.imports[0].module, "crate::domain::users");
        assert_eq!(snapshot.calls[0].call.callee_name, "load_users");
        assert_eq!(
            snapshot.calls[0].callee_path.as_deref(),
            Some("src/domain/users.rs")
        );
        assert_eq!(snapshot.routes[0].url, "/users");
        assert_eq!(snapshot.dependencies[0].package_name, "serde");
        assert_eq!(snapshot.feature_flags[0].name, "users_enabled");
        assert!(!snapshot.truncated);
        assert!(
            snapshot
                .files
                .iter()
                .all(|file| file.path.starts_with("src/api"))
        );
    }

    #[test]
    fn filtered_sql_applies_path_and_language_before_limit() {
        let request = request(vec!["src\\api".to_owned()], vec!["rust".to_owned()], 10);

        let (sql, values) = filtered_sql(
            "SELECT path FROM code_repository_files WHERE source_scope = ?1",
            "scope",
            &request,
            FilterColumns::new("path", Some("language_id")),
            |_, _| {},
            " ORDER BY path ASC",
            20,
        );

        assert!(sql.contains("path = ? OR path LIKE ? ESCAPE '\\'"));
        assert!(sql.contains("language_id IN (?)"));
        assert!(sql.ends_with(" LIMIT ?"));
        assert_eq!(values[1], Value::Text("src/api".to_owned()));
        assert_eq!(values[2], Value::Text("src/api/%".to_owned()));
    }

    #[test]
    fn process_flow_calls_are_filtered_to_route_paths_before_limit() {
        let connection = Connection::open_in_memory().unwrap();
        create_view_tables(&connection);
        seed_view_rows(&connection);
        connection
            .execute(
                "
                INSERT INTO code_repository_calls VALUES
                    ('repo', 'scope', 'call:noise', 'file:noise', 'src/aaa/no_route.rs',
                     'symbol:noise', 'noise', NULL, 'ignored', NULL, 'unresolved',
                     5000, 'ambiguous', 1, 1)
                ",
                [],
            )
            .unwrap();
        let request = request_kind(
            CodebaseViewKind::ProcessFlow,
            Vec::new(),
            Vec::new(),
            10,
            Vec::new(),
        );

        let snapshot = snapshot(&connection, "scope", &request, 1).unwrap();

        assert_eq!(snapshot.routes[0].path, "src/api/users.rs");
        assert_eq!(snapshot.calls[0].call.path, "src/api/users.rs");
        assert_eq!(snapshot.calls[0].call.callee_name, "load_users");
    }

    #[test]
    fn process_flow_calls_include_resolved_handler_paths_before_limit() {
        let connection = Connection::open_in_memory().unwrap();
        create_view_tables(&connection);
        seed_view_rows(&connection);
        connection
            .execute_batch(
                "
                DELETE FROM code_repository_calls WHERE call_id = 'call:api';
                INSERT INTO code_repository_symbols VALUES
                    ('scope', 'symbol:controller', 'src/controllers/users.rs', 'rust',
                     'list_users', 'controllers::users::list_users', 'function', 11, 18);
                UPDATE code_repository_routes
                   SET handler_symbol_snapshot_id = 'symbol:controller'
                 WHERE route_id = 'route:api';
                INSERT INTO code_repository_calls VALUES
                    ('repo', 'scope', 'call:controller', 'file:controller',
                     'src/controllers/users.rs', 'symbol:controller', 'list_users',
                     'symbol:callee', 'load_users', 'src/domain/users.rs', 'resolved',
                     9000, 'extracted', 12, 12),
                    ('repo', 'scope', 'call:noise', 'file:noise', 'src/aaa/no_route.rs',
                     'symbol:noise', 'noise', NULL, 'ignored', NULL, 'unresolved',
                     5000, 'ambiguous', 1, 1);
                ",
            )
            .unwrap();
        let request = request_kind(
            CodebaseViewKind::ProcessFlow,
            Vec::new(),
            Vec::new(),
            10,
            Vec::new(),
        );

        let snapshot = snapshot(&connection, "scope", &request, 1).unwrap();

        assert_eq!(snapshot.calls[0].call.path, "src/controllers/users.rs");
        assert_eq!(snapshot.calls[0].call.callee_name, "load_users");
    }

    #[test]
    fn affected_scope_calls_match_changed_callee_paths_before_limit() {
        let connection = Connection::open_in_memory().unwrap();
        create_view_tables(&connection);
        seed_view_rows(&connection);
        connection
            .execute(
                "
                INSERT INTO code_repository_calls VALUES
                    ('repo', 'scope', 'call:noise', 'file:noise', 'src/aaa/no_route.rs',
                     'symbol:noise', 'noise', NULL, 'ignored', NULL, 'unresolved',
                     5000, 'ambiguous', 1, 1)
                ",
                [],
            )
            .unwrap();
        let request = request_kind(
            CodebaseViewKind::AffectedScope,
            Vec::new(),
            Vec::new(),
            10,
            vec!["src/domain/users.rs".to_owned()],
        );

        let snapshot = snapshot(&connection, "scope", &request, 1).unwrap();

        assert_eq!(snapshot.calls[0].call.path, "src/api/users.rs");
        assert_eq!(
            snapshot.calls[0].callee_path.as_deref(),
            Some("src/domain/users.rs")
        );
    }

    #[test]
    fn affected_scope_calls_match_changed_directory_prefixes_before_limit() {
        let connection = Connection::open_in_memory().unwrap();
        create_view_tables(&connection);
        seed_view_rows(&connection);
        connection
            .execute(
                "
                INSERT INTO code_repository_calls VALUES
                    ('repo', 'scope', 'call:noise', 'file:noise', 'src/aaa/no_route.rs',
                     'symbol:noise', 'noise', NULL, 'ignored', NULL, 'unresolved',
                     5000, 'ambiguous', 1, 1)
                ",
                [],
            )
            .unwrap();
        let request = request_kind(
            CodebaseViewKind::AffectedScope,
            Vec::new(),
            Vec::new(),
            10,
            vec!["src/domain".to_owned()],
        );

        let snapshot = snapshot(&connection, "scope", &request, 1).unwrap();

        assert_eq!(snapshot.calls[0].call.path, "src/api/users.rs");
        assert_eq!(
            snapshot.calls[0].callee_path.as_deref(),
            Some("src/domain/users.rs")
        );
    }

    #[test]
    fn affected_scope_files_focus_changed_modules_before_limit() {
        let connection = Connection::open_in_memory().unwrap();
        create_view_tables(&connection);
        seed_view_rows(&connection);
        connection
            .execute(
                "
                INSERT INTO code_repository_files VALUES
                    ('scope', 'file:noise', 'src/aaa/noise.rs', 'rust', 'parsed', 10, 0),
                    ('scope', 'file:domain-config', 'src/domain/config.yaml', 'yaml', 'parsed', 6, 0)
                ",
                [],
            )
            .unwrap();
        let request = request_kind(
            CodebaseViewKind::AffectedScope,
            Vec::new(),
            Vec::new(),
            10,
            vec!["src\\domain\\Dockerfile".to_owned()],
        );

        let snapshot = snapshot(&connection, "scope", &request, 1).unwrap();

        assert_eq!(snapshot.files[0].path, "src/domain/config.yaml");
    }

    #[test]
    fn resolved_import_targets_extend_file_scope_before_limit() {
        let connection = Connection::open_in_memory().unwrap();
        create_view_tables(&connection);
        seed_view_rows(&connection);

        let snapshot = snapshot(
            &connection,
            "scope",
            &request(Vec::new(), Vec::new(), 10),
            1,
        )
        .unwrap();

        assert!(
            snapshot
                .files
                .iter()
                .any(|file| file.path == "src/api/users.rs")
        );
        assert!(
            snapshot
                .files
                .iter()
                .any(|file| file.path == "src/domain/users.rs")
        );
    }

    #[test]
    fn unused_dependency_rows_do_not_mark_architecture_snapshot_truncated() {
        let connection = Connection::open_in_memory().unwrap();
        create_view_tables(&connection);
        connection
            .execute_batch(
                "
                INSERT INTO code_repository_symbols VALUES
                    ('scope', 'symbol:one', 'src/unused/one.rs', 'rust', 'one', 'one', 'function', 1, 1),
                    ('scope', 'symbol:two', 'src/unused/two.rs', 'rust', 'two', 'two', 'function', 1, 1);
                INSERT INTO code_repository_dependencies VALUES
                    ('dependency:one', 'scope', 'src/unused/Cargo.toml', 'rust', 'cargo', 'one', '^1', NULL, 'runtime', 'manifest', 0, 1, 1),
                    ('dependency:two', 'scope', 'src/unused/Cargo.toml', 'rust', 'cargo', 'two', '^2', NULL, 'runtime', 'manifest', 0, 2, 2);
                ",
            )
            .unwrap();
        let request = request(Vec::new(), Vec::new(), 10);

        let snapshot = snapshot(&connection, "scope", &request, 1).unwrap();

        assert!(snapshot.symbols.is_empty());
        assert_eq!(snapshot.dependencies[0].package_name, "one");
        assert!(!snapshot.truncated);
    }

    fn request(
        path_filters: Vec<String>,
        language_filters: Vec<String>,
        limit: usize,
    ) -> CodebaseViewRequest {
        request_kind(
            CodebaseViewKind::ArchitectureLayers,
            path_filters,
            language_filters,
            limit,
            Vec::new(),
        )
    }

    fn request_kind(
        view_kind: CodebaseViewKind,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
        limit: usize,
        changed_paths: Vec<String>,
    ) -> CodebaseViewRequest {
        CodebaseViewRequest::new(
            CodeRepositorySelector::new("repo", "HEAD", path_filters, language_filters).unwrap(),
            view_kind,
            FreshnessPolicy::AllowStale,
            limit,
            changed_paths,
        )
        .unwrap()
    }

    fn create_view_tables(connection: &Connection) {
        connection
            .execute_batch(
                "
                CREATE TABLE code_repository_files (
                    source_scope TEXT NOT NULL,
                    file_id TEXT NOT NULL,
                    path TEXT NOT NULL,
                    language_id TEXT NOT NULL,
                    parse_status TEXT NOT NULL,
                    line_count INTEGER NOT NULL,
                    is_generated INTEGER NOT NULL
                );
                CREATE TABLE code_repository_symbols (
                    source_scope TEXT NOT NULL,
                    symbol_snapshot_id TEXT NOT NULL,
                    path TEXT NOT NULL,
                    language_id TEXT NOT NULL,
                    name TEXT NOT NULL,
                    qualified_name TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    line_start INTEGER NOT NULL,
                    line_end INTEGER NOT NULL
                );
                CREATE TABLE code_repository_imports (
                    repository_id TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    import_id TEXT NOT NULL,
                    file_id TEXT NOT NULL,
                    path TEXT NOT NULL,
                    module TEXT NOT NULL,
                    target_hint TEXT,
                    resolution_state TEXT NOT NULL,
                    confidence_basis_points INTEGER NOT NULL,
                    confidence_tier TEXT NOT NULL,
                    line_start INTEGER NOT NULL,
                    line_end INTEGER NOT NULL
                );
                CREATE TABLE code_repository_calls (
                    repository_id TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    call_id TEXT NOT NULL,
                    file_id TEXT NOT NULL,
                    path TEXT NOT NULL,
                    caller_symbol_snapshot_id TEXT,
                    caller_name TEXT,
                    callee_symbol_snapshot_id TEXT,
                    callee_name TEXT NOT NULL,
                    target_hint TEXT,
                    resolution_state TEXT NOT NULL,
                    confidence_basis_points INTEGER NOT NULL,
                    confidence_tier TEXT NOT NULL,
                    line_start INTEGER NOT NULL,
                    line_end INTEGER NOT NULL
                );
                CREATE TABLE code_repository_routes (
                    repository_id TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    route_id TEXT NOT NULL,
                    file_id TEXT NOT NULL,
                    path TEXT NOT NULL,
                    language_id TEXT NOT NULL,
                    url TEXT NOT NULL,
                    http_method TEXT NOT NULL,
                    handler_name TEXT NOT NULL,
                    handler_symbol_snapshot_id TEXT,
                    framework TEXT NOT NULL,
                    line_start INTEGER NOT NULL,
                    line_end INTEGER NOT NULL
                );
                CREATE TABLE code_repository_dependencies (
                    dependency_id TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    path TEXT NOT NULL,
                    language_id TEXT NOT NULL,
                    ecosystem TEXT NOT NULL,
                    package_name TEXT NOT NULL,
                    requirement TEXT,
                    resolved_version TEXT,
                    dependency_group TEXT NOT NULL,
                    source_kind TEXT NOT NULL,
                    is_lockfile INTEGER NOT NULL,
                    line_start INTEGER NOT NULL,
                    line_end INTEGER NOT NULL
                );
                CREATE TABLE code_repository_feature_flags (
                    repository_id TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    feature_flag_id TEXT NOT NULL,
                    usage_id TEXT NOT NULL,
                    file_id TEXT NOT NULL,
                    path TEXT NOT NULL,
                    language_id TEXT NOT NULL,
                    name TEXT NOT NULL,
                    source_kind TEXT NOT NULL,
                    source_key TEXT NOT NULL,
                    edge_kind TEXT NOT NULL,
                    confidence_basis_points INTEGER NOT NULL,
                    confidence_tier TEXT NOT NULL,
                    byte_start INTEGER NOT NULL,
                    byte_end INTEGER NOT NULL,
                    line_start INTEGER NOT NULL,
                    line_end INTEGER NOT NULL,
                    excerpt TEXT NOT NULL
                );
                ",
            )
            .unwrap();
    }

    fn seed_view_rows(connection: &Connection) {
        connection
            .execute_batch(
                "
                INSERT INTO code_repository_files VALUES
                    ('scope', 'file:api', 'src/api/users.rs', 'rust', 'parsed', 40, 0),
                    ('scope', 'file:js', 'src/js/app.js', 'javascript', 'parsed', 20, 0),
                    ('scope', 'file:domain', 'src/domain/users.rs', 'rust', 'parsed', 30, 0);
                INSERT INTO code_repository_symbols VALUES
                    ('scope', 'symbol:handler', 'src/api/users.rs', 'rust', 'index', 'api::users::index', 'function', 4, 8),
                    ('scope', 'symbol:callee', 'src/domain/users.rs', 'rust', 'load_users', 'domain::users::load_users', 'function', 5, 9),
                    ('scope', 'symbol:js', 'src/js/app.js', 'javascript', 'boot', 'boot', 'function', 1, 2);
                INSERT INTO code_repository_imports VALUES
                    ('repo', 'scope', 'import:api', 'file:api', 'src/api/users.rs', 'crate::domain::users', 'src/domain/users.rs', 'resolved', 9000, 'extracted', 2, 2),
                    ('repo', 'scope', 'import:js', 'file:js', 'src/js/app.js', './boot', NULL, 'unresolved', 5000, 'ambiguous', 1, 1);
                INSERT INTO code_repository_calls VALUES
                    ('repo', 'scope', 'call:api', 'file:api', 'src/api/users.rs', 'symbol:handler', 'index', 'symbol:callee', 'load_users', 'src/domain/users.rs', 'resolved', 9000, 'extracted', 6, 6),
                    ('repo', 'scope', 'call:js', 'file:js', 'src/js/app.js', NULL, NULL, NULL, 'boot', NULL, 'unresolved', 5000, 'ambiguous', 2, 2);
                INSERT INTO code_repository_routes VALUES
                    ('repo', 'scope', 'route:api', 'file:api', 'src/api/users.rs', 'rust', '/users', 'GET', 'index', 'symbol:handler', 'fixture', 3, 3),
                    ('repo', 'scope', 'route:js', 'file:js', 'src/js/app.js', 'javascript', '/js', 'GET', 'boot', 'symbol:js', 'fixture', 1, 1);
                INSERT INTO code_repository_dependencies VALUES
                    ('dependency:api', 'scope', 'src/api/Cargo.toml', 'rust', 'cargo', 'serde', '^1', '1.0.0', 'runtime', 'manifest', 0, 1, 1),
                    ('dependency:js', 'scope', 'src/js/package.json', 'javascript', 'npm', 'vite', '^6', NULL, 'dev', 'manifest', 0, 1, 1);
                INSERT INTO code_repository_feature_flags VALUES
                    ('repo', 'scope', 'flag:api', 'usage:api', 'file:api', 'src/api/users.rs', 'rust', 'users_enabled', 'config', 'users.enabled', 'guards', 8500, 'extracted', 10, 20, 7, 7, 'users_enabled'),
                    ('repo', 'scope', 'flag:js', 'usage:js', 'file:js', 'src/js/app.js', 'javascript', 'js_enabled', 'config', 'js.enabled', 'guards', 8500, 'extracted', 10, 20, 7, 7, 'js_enabled');
                ",
            )
            .unwrap();
    }
}
