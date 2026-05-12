use rusqlite::{Connection, OptionalExtension, params};

#[path = "code_query.rs"]
mod code_query;

#[path = "code_impact.rs"]
mod code_impact;

#[cfg(test)]
#[path = "code_tests.rs"]
mod code_tests;

use crate::{
    domain::{
        CodeFileFingerprint, CodeImpactRequest, CodeIndexSnapshot, CodeIndexSummary,
        CodeRepositoryRegistration, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalRequest,
    },
    storage::{CodeImpactChanges, CodeRepositoryStore, StorageError, StorageFuture},
};

use super::SqliteGraphStore;

pub(super) fn initialize_code_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS code_repositories (
            repository_id TEXT PRIMARY KEY,
            alias TEXT NOT NULL UNIQUE,
            root_path TEXT NOT NULL,
            path_filters_json TEXT NOT NULL,
            language_filters_json TEXT NOT NULL,
            last_indexed_commit TEXT,
            tree_hash TEXT,
            state TEXT NOT NULL,
            indexed_file_count INTEGER NOT NULL,
            symbol_count INTEGER NOT NULL,
            reference_count INTEGER NOT NULL,
            chunk_count INTEGER NOT NULL,
            stale INTEGER NOT NULL,
            degraded_reason TEXT
        );

        CREATE TABLE IF NOT EXISTS code_repository_files (
            repository_id TEXT NOT NULL,
            file_id TEXT NOT NULL,
            path TEXT NOT NULL,
            language_id TEXT NOT NULL,
            blob_hash TEXT NOT NULL,
            byte_len INTEGER NOT NULL,
            line_count INTEGER NOT NULL,
            parse_status TEXT NOT NULL,
            degraded_reason TEXT,
            PRIMARY KEY (repository_id, path),
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_symbols (
            repository_id TEXT NOT NULL,
            symbol_snapshot_id TEXT PRIMARY KEY,
            file_id TEXT NOT NULL,
            path TEXT NOT NULL,
            language_id TEXT NOT NULL,
            name TEXT NOT NULL,
            qualified_name TEXT NOT NULL,
            kind TEXT NOT NULL,
            signature TEXT NOT NULL,
            doc_comment TEXT,
            byte_start INTEGER NOT NULL,
            byte_end INTEGER NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_references (
            repository_id TEXT NOT NULL,
            reference_id TEXT PRIMARY KEY,
            file_id TEXT NOT NULL,
            path TEXT NOT NULL,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            target_symbol_snapshot_id TEXT,
            byte_start INTEGER NOT NULL,
            byte_end INTEGER NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_imports (
            repository_id TEXT NOT NULL,
            import_id TEXT PRIMARY KEY,
            file_id TEXT NOT NULL,
            path TEXT NOT NULL,
            module TEXT NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_calls (
            repository_id TEXT NOT NULL,
            call_id TEXT PRIMARY KEY,
            file_id TEXT NOT NULL,
            path TEXT NOT NULL,
            caller_symbol_snapshot_id TEXT,
            caller_name TEXT,
            callee_symbol_snapshot_id TEXT,
            callee_name TEXT NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_chunks (
            repository_id TEXT NOT NULL,
            chunk_id TEXT PRIMARY KEY,
            file_id TEXT NOT NULL,
            path TEXT NOT NULL,
            language_id TEXT NOT NULL,
            content TEXT NOT NULL,
            byte_start INTEGER NOT NULL,
            byte_end INTEGER NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            symbol_snapshot_id TEXT,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_file_diagnostics (
            repository_id TEXT NOT NULL,
            path TEXT NOT NULL,
            parse_status TEXT NOT NULL,
            message TEXT NOT NULL,
            PRIMARY KEY (repository_id, path, message),
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_path_tombstones (
            repository_id TEXT NOT NULL,
            old_path TEXT NOT NULL,
            new_path TEXT,
            base_ref TEXT NOT NULL,
            head_ref TEXT NOT NULL,
            PRIMARY KEY (repository_id, old_path, base_ref, head_ref),
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );
        ",
    )?;
    ensure_code_repository_calls_target_column(connection)?;

    Ok(())
}

fn ensure_code_repository_calls_target_column(connection: &Connection) -> Result<(), StorageError> {
    let mut statement = connection.prepare("PRAGMA table_info(code_repository_calls)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let columns = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    if !columns
        .iter()
        .any(|column| column == "callee_symbol_snapshot_id")
    {
        connection.execute(
            "ALTER TABLE code_repository_calls ADD COLUMN callee_symbol_snapshot_id TEXT",
            [],
        )?;
    }

    Ok(())
}

impl CodeRepositoryStore for SqliteGraphStore {
    fn upsert_code_repository(
        &self,
        registration: CodeRepositoryRegistration,
    ) -> StorageFuture<'_, CodeRepositoryStatus> {
        self.run(move |connection| upsert_repository(connection, registration))
    }

    fn code_repository_status(
        &self,
        repository: String,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        self.run(move |connection| repository_status(connection, &repository))
    }

    fn code_file_fingerprints(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Vec<CodeFileFingerprint>> {
        self.run(move |connection| file_fingerprints(connection, &repository_id))
    }

    fn apply_code_index_snapshot(
        &self,
        snapshot: CodeIndexSnapshot,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        self.run(move |connection| apply_snapshot(connection, snapshot))
    }

    fn search_code(
        &self,
        request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        self.run(move |connection| code_query::search_code(connection, request))
    }

    fn analyze_code_impact(
        &self,
        request: CodeImpactRequest,
        changes: CodeImpactChanges,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        self.run(move |connection| code_impact::analyze_impact(connection, request, changes))
    }
}

fn upsert_repository(
    connection: &mut Connection,
    registration: CodeRepositoryRegistration,
) -> Result<CodeRepositoryStatus, StorageError> {
    connection.execute(
        "
        INSERT INTO code_repositories (
            repository_id, alias, root_path, path_filters_json, language_filters_json,
            state, indexed_file_count, symbol_count, reference_count, chunk_count,
            stale, degraded_reason
        )
        VALUES (?1, ?2, ?3, ?4, ?5, 'registered', 0, 0, 0, 0, 1, NULL)
        ON CONFLICT(repository_id) DO UPDATE SET
            alias = excluded.alias,
            root_path = excluded.root_path,
            path_filters_json = excluded.path_filters_json,
            language_filters_json = excluded.language_filters_json,
            stale = 1
        ",
        params![
            registration.repository_id,
            registration.alias,
            registration.root_path,
            serde_json::to_string(&registration.path_filters)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
            serde_json::to_string(&registration.language_filters)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
        ],
    )?;

    repository_status(connection, &registration.repository_id)?.ok_or_else(|| {
        StorageError::InvalidInput("registered code repository was not persisted".to_owned())
    })
}

pub(super) fn repository_status(
    connection: &mut Connection,
    repository: &str,
) -> Result<Option<CodeRepositoryStatus>, StorageError> {
    let lookup = if repository.starts_with("repo:") {
        RepositoryLookup::RepositoryId
    } else {
        RepositoryLookup::AliasOrId
    };
    let query = match lookup {
        RepositoryLookup::RepositoryId => {
            "
            SELECT repository_id, alias, root_path, path_filters_json, language_filters_json,
                   last_indexed_commit, tree_hash,
                   state, indexed_file_count, symbol_count, reference_count, chunk_count,
                   stale, degraded_reason
            FROM code_repositories
            WHERE repository_id = ?1
            "
        }
        RepositoryLookup::AliasOrId => {
            "
            SELECT repository_id, alias, root_path, path_filters_json, language_filters_json,
                   last_indexed_commit, tree_hash,
                   state, indexed_file_count, symbol_count, reference_count, chunk_count,
                   stale, degraded_reason
            FROM code_repositories
            WHERE alias = ?1 OR repository_id = ?1
            "
        }
    };

    connection
        .query_row(query, params![repository], |row| {
            Ok(CodeRepositoryStatus {
                repository_id: row.get(0)?,
                alias: row.get(1)?,
                root_path: row.get(2)?,
                path_filters: parse_json_list(row.get::<_, String>(3)?)?,
                language_filters: parse_json_list(row.get::<_, String>(4)?)?,
                last_indexed_commit: row.get(5)?,
                tree_hash: row.get(6)?,
                state: row.get(7)?,
                indexed_file_count: row.get(8)?,
                symbol_count: row.get(9)?,
                reference_count: row.get(10)?,
                chunk_count: row.get(11)?,
                stale: row.get::<_, i64>(12)? != 0,
                degraded_reason: row.get(13)?,
            })
        })
        .optional()
        .map_err(StorageError::from)
}

enum RepositoryLookup {
    RepositoryId,
    AliasOrId,
}

fn parse_json_list(value: String) -> rusqlite::Result<Vec<String>> {
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

fn file_fingerprints(
    connection: &mut Connection,
    repository_id: &str,
) -> Result<Vec<CodeFileFingerprint>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT path, blob_hash
        FROM code_repository_files
        WHERE repository_id = ?1
        ORDER BY path ASC
        ",
    )?;
    let rows = statement.query_map(params![repository_id], |row| {
        Ok(CodeFileFingerprint {
            path: row.get(0)?,
            blob_hash: row.get(1)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn apply_snapshot(
    connection: &mut Connection,
    snapshot: CodeIndexSnapshot,
) -> Result<CodeIndexSummary, StorageError> {
    let transaction = connection.transaction()?;
    if snapshot.full_replace {
        delete_repository_index(&transaction, &snapshot.repository_id)?;
    } else {
        for path in &snapshot.deleted_paths {
            delete_path_index(&transaction, &snapshot.repository_id, path)?;
        }
        for file in &snapshot.files {
            delete_path_index(&transaction, &snapshot.repository_id, &file.path)?;
        }
    }

    for file in &snapshot.files {
        transaction.execute(
            "
            INSERT INTO code_repository_files (
                repository_id, file_id, path, language_id, blob_hash, byte_len,
                line_count, parse_status, degraded_reason
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ",
            params![
                file.repository_id,
                file.file_id,
                file.path,
                file.language_id,
                file.blob_hash,
                file.byte_len,
                file.line_count,
                file.parse_status.as_str(),
                file.degraded_reason,
            ],
        )?;
    }
    for symbol in &snapshot.symbols {
        transaction.execute(
            "
            INSERT INTO code_repository_symbols (
                repository_id, symbol_snapshot_id, file_id, path, language_id, name,
                qualified_name, kind, signature, doc_comment, byte_start, byte_end,
                line_start, line_end
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            ",
            params![
                symbol.repository_id,
                symbol.symbol_snapshot_id,
                symbol.file_id,
                symbol.path,
                symbol.language_id,
                symbol.name,
                symbol.qualified_name,
                symbol.kind,
                symbol.signature,
                symbol.doc_comment,
                symbol.byte_range.start,
                symbol.byte_range.end,
                symbol.line_range.start,
                symbol.line_range.end,
            ],
        )?;
    }
    for reference in &snapshot.references {
        transaction.execute(
            "
            INSERT INTO code_repository_references (
                repository_id, reference_id, file_id, path, name, kind,
                target_symbol_snapshot_id, byte_start, byte_end, line_start, line_end
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            ",
            params![
                reference.repository_id,
                reference.reference_id,
                reference.file_id,
                reference.path,
                reference.name,
                reference.kind,
                reference.target_symbol_snapshot_id,
                reference.byte_range.start,
                reference.byte_range.end,
                reference.line_range.start,
                reference.line_range.end,
            ],
        )?;
    }
    insert_imports_calls_chunks_diagnostics(&transaction, &snapshot)?;
    update_repository_after_snapshot(&transaction, &snapshot)?;
    transaction.commit()?;

    let status = repository_status(connection, &snapshot.repository_id)?.ok_or_else(|| {
        StorageError::InvalidInput("code repository status is missing after index".to_owned())
    })?;

    Ok(CodeIndexSummary {
        repository_id: snapshot.repository_id,
        resolved_commit_sha: snapshot.resolved_commit_sha,
        tree_hash: snapshot.tree_hash,
        indexed_file_count: status.indexed_file_count,
        changed_path_count: snapshot.changed_path_count,
        skipped_unchanged_count: snapshot.skipped_unchanged_count,
        deleted_path_count: snapshot.deleted_paths.len(),
        symbol_count: status.symbol_count,
        reference_count: status.reference_count,
        chunk_count: status.chunk_count,
        degraded_file_count: snapshot.diagnostics.len(),
    })
}

fn insert_imports_calls_chunks_diagnostics(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
) -> Result<(), StorageError> {
    for import in &snapshot.imports {
        transaction.execute(
            "
            INSERT INTO code_repository_imports (
                repository_id, import_id, file_id, path, module, line_start, line_end
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            params![
                import.repository_id,
                import.import_id,
                import.file_id,
                import.path,
                import.module,
                import.line_range.start,
                import.line_range.end,
            ],
        )?;
    }
    for call in &snapshot.calls {
        transaction.execute(
            "
            INSERT INTO code_repository_calls (
                repository_id, call_id, file_id, path, caller_symbol_snapshot_id,
                caller_name, callee_symbol_snapshot_id, callee_name, line_start, line_end
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ",
            params![
                call.repository_id,
                call.call_id,
                call.file_id,
                call.path,
                call.caller_symbol_snapshot_id,
                call.caller_name,
                call.callee_symbol_snapshot_id,
                call.callee_name,
                call.line_range.start,
                call.line_range.end,
            ],
        )?;
    }
    for chunk in &snapshot.chunks {
        transaction.execute(
            "
            INSERT INTO code_repository_chunks (
                repository_id, chunk_id, file_id, path, language_id, content,
                byte_start, byte_end, line_start, line_end, symbol_snapshot_id
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            ",
            params![
                chunk.repository_id,
                chunk.chunk_id,
                chunk.file_id,
                chunk.path,
                chunk.language_id,
                chunk.content,
                chunk.byte_range.start,
                chunk.byte_range.end,
                chunk.line_range.start,
                chunk.line_range.end,
                chunk.symbol_snapshot_id,
            ],
        )?;
    }
    for diagnostic in &snapshot.diagnostics {
        transaction.execute(
            "
            INSERT OR REPLACE INTO code_repository_file_diagnostics
                (repository_id, path, parse_status, message)
            VALUES (?1, ?2, ?3, ?4)
            ",
            params![
                diagnostic.repository_id,
                diagnostic.path,
                diagnostic.parse_status.as_str(),
                diagnostic.message,
            ],
        )?;
    }
    for tombstone in &snapshot.tombstones {
        transaction.execute(
            "
            INSERT OR REPLACE INTO code_repository_path_tombstones
                (repository_id, old_path, new_path, base_ref, head_ref)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ",
            params![
                tombstone.repository_id,
                tombstone.old_path,
                tombstone.new_path,
                tombstone.base_ref,
                tombstone.head_ref,
            ],
        )?;
    }

    Ok(())
}

fn update_repository_after_snapshot(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
) -> Result<(), StorageError> {
    let file_count = count_code_rows(
        transaction,
        "code_repository_files",
        &snapshot.repository_id,
    )?;
    let symbol_count = count_code_rows(
        transaction,
        "code_repository_symbols",
        &snapshot.repository_id,
    )?;
    let reference_count = count_code_rows(
        transaction,
        "code_repository_references",
        &snapshot.repository_id,
    )?;
    let chunk_count = count_code_rows(
        transaction,
        "code_repository_chunks",
        &snapshot.repository_id,
    )?;
    let degraded_file_count = count_code_rows(
        transaction,
        "code_repository_file_diagnostics",
        &snapshot.repository_id,
    )?;
    let degraded_reason = (degraded_file_count > 0)
        .then(|| format!("{degraded_file_count} file(s) degraded during code indexing"));
    transaction.execute(
        "
        UPDATE code_repositories
        SET last_indexed_commit = ?2,
            tree_hash = ?3,
            state = 'fresh',
            indexed_file_count = ?4,
            symbol_count = ?5,
            reference_count = ?6,
            chunk_count = ?7,
            stale = 0,
            degraded_reason = ?8
        WHERE repository_id = ?1
        ",
        params![
            snapshot.repository_id,
            snapshot.resolved_commit_sha,
            snapshot.tree_hash,
            file_count,
            symbol_count,
            reference_count,
            chunk_count,
            degraded_reason,
        ],
    )?;

    Ok(())
}

fn delete_repository_index(
    transaction: &rusqlite::Transaction<'_>,
    repository_id: &str,
) -> Result<(), StorageError> {
    for table in [
        "code_repository_path_tombstones",
        "code_repository_file_diagnostics",
        "code_repository_chunks",
        "code_repository_calls",
        "code_repository_imports",
        "code_repository_references",
        "code_repository_symbols",
        "code_repository_files",
    ] {
        transaction.execute(
            &format!("DELETE FROM {table} WHERE repository_id = ?1"),
            params![repository_id],
        )?;
    }

    Ok(())
}

fn delete_path_index(
    transaction: &rusqlite::Transaction<'_>,
    repository_id: &str,
    path: &str,
) -> Result<(), StorageError> {
    for table in [
        "code_repository_file_diagnostics",
        "code_repository_chunks",
        "code_repository_calls",
        "code_repository_imports",
        "code_repository_references",
        "code_repository_symbols",
        "code_repository_files",
    ] {
        transaction.execute(
            &format!("DELETE FROM {table} WHERE repository_id = ?1 AND path = ?2"),
            params![repository_id, path],
        )?;
    }

    Ok(())
}

fn count_code_rows(
    transaction: &rusqlite::Transaction<'_>,
    table: &'static str,
    repository_id: &str,
) -> Result<usize, StorageError> {
    transaction
        .query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE repository_id = ?1"),
            params![repository_id],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}
