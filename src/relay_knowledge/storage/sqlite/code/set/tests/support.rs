//! Shared SQLite fixtures for repository-set owner tests.

use rusqlite::params;

use crate::storage::{CodeRepositorySetMemberSeed, CodeRepositorySetSeed};

pub(in crate::storage::sqlite::code::set) fn set_seed(
    alias: &str,
    now_ms: u64,
) -> CodeRepositorySetSeed {
    CodeRepositorySetSeed {
        alias: alias.to_owned(),
        description: Some(format!("{alias} description")),
        default_ref_policy_json: "{\"default_ref\":\"HEAD\"}".to_owned(),
        now_ms,
    }
}

pub(in crate::storage::sqlite::code::set) fn member_seed(
    set_alias: &str,
    repository_id: &str,
    repository_alias: &str,
    source_scope: &str,
    priority: i32,
) -> CodeRepositorySetMemberSeed {
    CodeRepositorySetMemberSeed {
        set_alias: set_alias.to_owned(),
        repository_id: repository_id.to_owned(),
        repository_alias: repository_alias.to_owned(),
        ref_selector: "HEAD".to_owned(),
        resolved_commit_sha: format!("commit-{source_scope}"),
        source_scope: source_scope.to_owned(),
        path_filters: vec!["src".to_owned()],
        language_filters: vec!["rust".to_owned()],
        priority,
    }
}

pub(in crate::storage::sqlite::code::set) fn insert_repository_scope(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    alias: &str,
    source_scope: &str,
    tree_hash: &str,
    stale: bool,
) -> Result<(), crate::storage::StorageError> {
    insert_repository_scope_with_filters(
        connection,
        repository_id,
        alias,
        source_scope,
        tree_hash,
        stale,
        ("[\"src\"]", "[\"rust\"]"),
    )
}

pub(in crate::storage::sqlite::code::set) fn insert_repository_scope_with_filters(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    alias: &str,
    source_scope: &str,
    tree_hash: &str,
    stale: bool,
    filters_json: (&str, &str),
) -> Result<(), crate::storage::StorageError> {
    let (path_filters_json, language_filters_json) = filters_json;
    connection.execute(
        "
        INSERT OR IGNORE INTO code_repositories (
            repository_id, alias, root_path, path_filters_json, language_filters_json,
            last_indexed_scope_id, last_indexed_commit, tree_hash, state,
            indexed_file_count, symbol_count, reference_count, chunk_count, stale,
            degraded_reason
        )
        VALUES (?1, ?2, '/tmp/repo', ?7, ?8,
                ?3, ?4, ?5, 'indexed', 1, 1, 0, 0, ?6, NULL)
        ",
        params![
            repository_id,
            alias,
            source_scope,
            format!("commit-{source_scope}"),
            tree_hash,
            i64::from(stale),
            path_filters_json,
            language_filters_json,
        ],
    )?;
    connection.execute(
        "
        INSERT INTO code_repository_scopes (
            source_scope, repository_id, resolved_commit_sha, tree_hash,
            path_filters_json, language_filters_json, indexed_file_count,
            symbol_count, reference_count, chunk_count, stale, degraded_reason
        )
        VALUES (?1, ?2, ?3, ?4, ?6, ?7, 1, 1, 0, 0, ?5, NULL)
        ",
        params![
            source_scope,
            repository_id,
            format!("commit-{source_scope}"),
            tree_hash,
            i64::from(stale),
            path_filters_json,
            language_filters_json,
        ],
    )?;
    connection.execute(
        "
        INSERT INTO code_repository_files (
            repository_id, source_scope, file_id, path, language_id, blob_hash,
            byte_len, line_count, parse_status, degraded_reason
        )
        VALUES (?1, ?2, ?3, ?4, 'rust', 'blob', 1, 1, 'parsed', NULL)
        ",
        params![
            repository_id,
            source_scope,
            format!("file-{source_scope}"),
            format!("src/{alias}.rs"),
        ],
    )?;
    Ok(())
}

pub(in crate::storage::sqlite::code::set) fn insert_file(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    source_scope: &str,
    file_id: &str,
    path: &str,
    language_id: &str,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT INTO code_repository_files (
            repository_id, source_scope, file_id, path, language_id, blob_hash,
            byte_len, line_count, parse_status, degraded_reason
        )
        VALUES (?1, ?2, ?3, ?4, ?5, 'blob', 1, 1, 'parsed', NULL)
        ",
        params![repository_id, source_scope, file_id, path, language_id],
    )?;
    Ok(())
}

pub(in crate::storage::sqlite::code::set) fn insert_chunk(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    source_scope: &str,
    chunk_id: &str,
    path: &str,
    content: &str,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT INTO code_repository_chunks (
            repository_id, source_scope, chunk_id, file_id, path, language_id, content,
            byte_start, byte_end, line_start, line_end, symbol_snapshot_id
        )
        VALUES (?1, ?2, ?3, ?4, ?5, 'unknown', ?6, 0, ?7, 1, 1, NULL)
        ",
        params![
            repository_id,
            source_scope,
            chunk_id,
            format!("file-{source_scope}"),
            path,
            content,
            content.len() as u32,
        ],
    )?;
    Ok(())
}

pub(in crate::storage::sqlite::code::set) fn insert_import(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    source_scope: &str,
    import_id: &str,
    module: &str,
) -> Result<(), crate::storage::StorageError> {
    insert_import_with_state(
        connection,
        repository_id,
        source_scope,
        import_id,
        module,
        "unresolved",
    )
}

pub(in crate::storage::sqlite::code::set) fn insert_import_with_state(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    source_scope: &str,
    import_id: &str,
    module: &str,
    resolution_state: &str,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT INTO code_repository_imports (
            repository_id, source_scope, import_id, file_id, path, module, target_hint,
            resolution_state, confidence_basis_points, confidence_tier, line_start, line_end
        )
        VALUES (?1, ?2, ?3, ?4, 'src/client.rs', ?5, ?5, ?6, 10000, 'extracted', 1, 1)
        ",
        params![
            repository_id,
            source_scope,
            import_id,
            format!("file-{source_scope}"),
            module,
            resolution_state,
        ],
    )?;
    Ok(())
}

pub(in crate::storage::sqlite::code::set) fn insert_symbol(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    source_scope: &str,
    symbol_id: &str,
    name: &str,
    qualified_name: &str,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT INTO code_repository_symbols (
            repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id,
            file_id, path, language_id, name, qualified_name, kind, signature,
            doc_comment, byte_start, byte_end, line_start, line_end
        )
        VALUES (?1, ?2, ?3, ?4, ?5, 'src/service.rs', 'rust', ?6, ?7,
                'function', 'fn target()', NULL, 0, 10, 1, 1)
        ",
        params![
            repository_id,
            source_scope,
            symbol_id,
            format!("{repository_id}::{qualified_name}"),
            format!("file-{source_scope}"),
            name,
            qualified_name,
        ],
    )?;
    Ok(())
}

pub(in crate::storage::sqlite::code::set) fn insert_cross_edge(
    connection: &mut rusqlite::Connection,
    set_id: &str,
    from_source_scope: &str,
    to_source_scope: &str,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT INTO code_repository_cross_edges (
            edge_id, set_id, from_source_scope, from_repository_id, from_record_kind,
            from_record_id, to_source_scope, to_repository_id, to_record_kind, to_record_id,
            edge_kind, resolution_state, confidence_basis_points, confidence_tier,
            evidence_json, created_at_ms
        )
        VALUES ('stale-edge', ?1, ?2, 'repo-a', 'module_reference',
                'import-service', ?3, 'repo-b', 'code_symbol_snapshot', 'serve-symbol',
                'imports', 'resolved', 10000, 'explicit', '{}', 40)
        ",
        params![set_id, from_source_scope, to_source_scope],
    )?;
    Ok(())
}
