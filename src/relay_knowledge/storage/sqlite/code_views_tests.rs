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
    assert_eq!(snapshot.symbols[0].symbol_snapshot_id, "symbol:handler");
    assert_eq!(snapshot.symbols[0].path, "src/api/users.rs");
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
    let file_paths = snapshot
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(file_paths, vec!["src/api/users.rs", "src/domain/users.rs"]);
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
        vec!["src/api".to_owned()],
        Vec::new(),
        10,
        Vec::new(),
    );

    let snapshot = snapshot(&connection, "scope", &request, 1).unwrap();

    assert_eq!(snapshot.routes[0].path, "src/api/users.rs");
    assert_eq!(snapshot.calls[0].call.path, "src/api/users.rs");
    assert_eq!(snapshot.calls[0].call.callee_name, "load_users");
    assert!(!snapshot.truncated);
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
fn process_flow_calls_include_resolved_handler_paths_outside_request_path_filter() {
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
        vec!["src/api".to_owned()],
        Vec::new(),
        10,
        Vec::new(),
    );

    let snapshot = snapshot(&connection, "scope", &request, 2).unwrap();

    assert_eq!(snapshot.routes[0].path, "src/api/users.rs");
    assert!(
        snapshot
            .calls
            .iter()
            .any(|call| call.call.path == "src/controllers/users.rs")
    );
    assert!(
        snapshot
            .calls
            .iter()
            .all(|call| call.call.path != "src/aaa/no_route.rs")
    );
}

#[test]
fn process_flow_snapshot_includes_handler_symbols_without_calls() {
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

    assert!(snapshot.calls.is_empty());
    assert_eq!(snapshot.symbols[0].symbol_snapshot_id, "symbol:controller");
    assert_eq!(snapshot.symbols[0].path, "src/controllers/users.rs");
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
        &request(vec!["src/api".to_owned()], Vec::new(), 10),
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
fn business_domain_files_do_not_include_supplemental_import_targets() {
    let connection = Connection::open_in_memory().unwrap();
    create_view_tables(&connection);
    seed_view_rows(&connection);
    let request = request_kind(
        CodebaseViewKind::BusinessDomains,
        vec!["src/api".to_owned()],
        Vec::new(),
        10,
        Vec::new(),
    );

    let snapshot = snapshot(&connection, "scope", &request, 10).unwrap();

    assert_eq!(snapshot.files.len(), 1);
    assert_eq!(snapshot.files[0].path, "src/api/users.rs");
    assert!(
        snapshot
            .files
            .iter()
            .all(|file| file.path != "src/domain/users.rs")
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
