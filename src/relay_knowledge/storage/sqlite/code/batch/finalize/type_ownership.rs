//! Resolve detached ownership against frozen indexed declarations, one durable page at a time.
use super::pages::{FinalizationPageLimits, checkpoint_row_bytes, require_quantum_bytes};
use crate::{
    domain::{CodeIndexSession, CodeTypeOwner},
    storage::StorageError,
};
use rusqlite::{Connection, Transaction, params};

const MAX_METADATA: usize = 64 * 1024;
const MAX_CANDIDATES: usize = 64;

struct Symbol {
    id: String,
    path: String,
    language: String,
    metadata: Option<String>,
    unchanged_record_bytes: usize,
}

struct SqlBudget<'a>(&'a Connection);
impl Drop for SqlBudget<'_> {
    fn drop(&mut self) {
        self.0.progress_handler(0, None::<fn() -> bool>);
    }
}

/// Legacy in-place sessions may update at most one admitted quantum. A larger
/// update rolls back and must be rebuilt in an unpublished durable scope.
pub(in crate::storage::sqlite::code::batch) fn advance_atomically(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
) -> Result<bool, StorageError> {
    if advance(transaction, session)? {
        return Ok(true);
    }
    let remaining: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM code_repository_symbols WHERE source_scope=?1
          AND symbol_snapshot_id>(SELECT type_owner_cursor FROM code_repository_index_checkpoints WHERE source_scope=?1))",
        [&session.source_scope], |row| row.get(0),
    )?;
    if remaining {
        return Err(capacity(
            "in-place ownership update exceeds one writer quantum; rebuild in a staged scope",
        ));
    }
    Ok(true)
}

pub(in crate::storage::sqlite::code::batch) fn advance(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
) -> Result<bool, StorageError> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut steps = 0;
    transaction.progress_handler(
        1000,
        Some(move || {
            steps += 1000;
            steps > 2_000_000 || std::time::Instant::now() >= deadline
        }),
    );
    let _budget = SqlBudget(transaction);
    match advance_page(transaction, session) {
        Err(StorageError::Sqlite(rusqlite::Error::SqliteFailure(code, _)))
            if code.code == rusqlite::ErrorCode::OperationInterrupted =>
        {
            Err(capacity("SQLite work budget exceeded"))
        }
        other => other,
    }
}

pub(in crate::storage::sqlite::code::batch) fn checkpoint_cursor(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<Option<String>, StorageError> {
    let (cursor, state): (Option<String>, String) = transaction.query_row(
        "SELECT type_owner_cursor,state FROM code_repository_index_checkpoints WHERE source_scope=?1",
        [source_scope],
        |row| Ok((row.get(0)?,row.get(1)?)),
    )?;
    if state.starts_with("finalizing:resolve_type_ownership:")
        && cursor
            .as_deref()
            .and_then(crate::domain::CodeQueryIndexRepairResumePhase::ownership_checkpoint_state)
            .as_deref()
            != Some(state.as_str())
    {
        return Err(StorageError::Invariant(
            "type ownership checkpoint does not match its durable cursor".into(),
        ));
    }
    Ok(cursor)
}

fn advance_page(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
) -> Result<bool, StorageError> {
    let cursor = checkpoint_cursor(transaction, &session.source_scope)?;
    if cursor.is_none() {
        let has_ownership: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM code_repository_symbols
             WHERE source_scope=?1 AND type_owner_identity IS NOT NULL)",
            [&session.source_scope],
            |row| row.get(0),
        )?;
        if !has_ownership {
            return Ok(true);
        }
    }
    let limits = FinalizationPageLimits::derive(
        &session.source_scope,
        "type-ownership",
        session.resource_budget,
        1,
    )?;
    // Cursor publication and the session's state transition each rewrite the
    // complete checkpoint, including a retained incremental receipt.
    let mut bytes = checkpoint_row_bytes(
        transaction,
        &session.source_scope,
        super::phases::RESOLVE_TYPE_OWNERSHIP,
    )?
    .saturating_mul(2);
    require_quantum_bytes(
        &session.source_scope,
        "type-ownership",
        limits.byte_limit,
        bytes,
    )?;
    let mut query = transaction.prepare(
        "SELECT symbol_snapshot_id, path, language_id,
        CASE WHEN length(CAST(type_owner_json AS BLOB))<=65536 THEN type_owner_json ELSE NULL END,
        coalesce(length(CAST(type_owner_json AS BLOB)),0),
        length(CAST(repository_id AS BLOB)) + length(CAST(source_scope AS BLOB))
        + length(CAST(symbol_snapshot_id AS BLOB)) + length(CAST(canonical_symbol_id AS BLOB))
        + length(CAST(file_id AS BLOB)) + length(CAST(path AS BLOB))
        + length(CAST(language_id AS BLOB)) + length(CAST(name AS BLOB))
        + length(CAST(qualified_name AS BLOB)) + length(CAST(kind AS BLOB))
        + length(CAST(signature AS BLOB)) + length(CAST(coalesce(doc_comment,'') AS BLOB))
        + length(CAST(coalesce(symbol_role_json,'') AS BLOB)) + 4*9 + 15*9 + 9
        FROM code_repository_symbols WHERE source_scope=?1 AND symbol_snapshot_id>?2
        ORDER BY symbol_snapshot_id LIMIT ?3",
    )?;
    let mut rows = query.query(params![
        session.source_scope,
        cursor.as_deref().unwrap_or(""),
        limits.document_limit.min(256)
    ])?;
    let mut symbols = Vec::new();
    let mut cpp_imports = std::collections::BTreeMap::new();
    while let Some(row) = rows.next()? {
        let metadata_bytes: usize = row.get(4)?;
        if metadata_bytes > MAX_METADATA {
            return Err(capacity("ownership metadata exceeds 64 KiB"));
        }
        let symbol = Symbol {
            id: row.get(0)?,
            path: row.get(1)?,
            language: row.get(2)?,
            metadata: row.get(3)?,
            unchanged_record_bytes: row.get(5)?,
        };
        // Charge source evidence and the next cursor; replacement metadata is
        // admitted at its actual serialized size after candidate resolution.
        if symbol.id.len() > 512 {
            return Err(capacity("symbol identity exceeds the cursor budget"));
        }
        let cost = symbol
            .id
            .len()
            .saturating_mul(5)
            .saturating_add(symbol.path.len())
            .saturating_add(symbol.language.len())
            .saturating_add(metadata_bytes)
            .saturating_add(32);
        if bytes.saturating_add(cost) > limits.byte_limit {
            if symbols.is_empty() {
                return Err(capacity(
                    "one ownership record exceeds the durable byte budget",
                ));
            }
            break;
        }
        let mut admitted_bytes = bytes.saturating_add(cost);
        let update = resolve_record(
            transaction,
            session,
            &symbol,
            &mut admitted_bytes,
            limits.byte_limit,
            &mut cpp_imports,
        );
        let update = match update {
            Err(StorageError::CapacityExceeded(_)) if !symbols.is_empty() => break,
            other => other?,
        };
        bytes = admitted_bytes;
        symbols.push((symbol, update));
    }
    drop(rows);
    if symbols.is_empty() {
        return Ok(true);
    }
    for (symbol, update) in &symbols {
        if let Some((metadata, identity)) = update {
            transaction.execute("UPDATE code_repository_symbols SET type_owner_json=?3,type_owner_identity=?4 WHERE source_scope=?1 AND symbol_snapshot_id=?2", params![session.source_scope, symbol.id, metadata, identity])?;
        }
    }
    let next = &symbols.last().expect("nonempty page").0.id;
    let changed = transaction.execute("UPDATE code_repository_index_checkpoints SET type_owner_cursor=?3 WHERE source_scope=?1 AND type_owner_cursor IS ?2", params![session.source_scope, cursor, next])?;
    if changed != 1 {
        return Err(StorageError::Invariant(
            "type ownership cursor changed during its writer quantum".into(),
        ));
    }
    Ok(false)
}

fn resolve_record(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    symbol: &Symbol,
    bytes: &mut usize,
    limit: usize,
    cpp_imports: &mut std::collections::BTreeMap<String, Vec<String>>,
) -> Result<Option<(String, String)>, StorageError> {
    let Some(metadata) = &symbol.metadata else {
        return Ok(None);
    };
    let mut owner: CodeTypeOwner = serde_json::from_str(metadata).map_err(|error| {
        StorageError::Invariant(format!("invalid indexed type ownership: {error}"))
    })?;
    if owner
        .basis
        .as_deref()
        .is_none_or(|basis| matches!(basis, "lexical" | "rust_module" | "cpp_unresolved_template"))
    {
        return Ok(None);
    }
    if owner.basis.as_deref() == Some("cpp_qualified") {
        if !cpp_imports.contains_key(&symbol.path) {
            let paths = resolved_cpp_import_paths(
                transaction,
                &session.source_scope,
                &symbol.path,
                bytes,
                limit,
            )?;
            cpp_imports.insert(symbol.path.clone(), paths);
        }
        // Rebuild derived evidence on replay. Previously resolved paths must
        // not survive an import becoming ambiguous, removed or unresolved.
        owner.target_paths.clone_from(&cpp_imports[&symbol.path]);
    }
    resolve(
        transaction,
        &session.source_scope,
        symbol,
        &mut owner,
        bytes,
        limit,
    )?;
    let metadata =
        serde_json::to_string(&owner).map_err(|e| StorageError::Invariant(e.to_string()))?;
    if metadata.len() > MAX_METADATA {
        return Err(capacity("resolved ownership metadata exceeds 64 KiB"));
    }
    *bytes = bytes
        .saturating_add(metadata.len())
        .saturating_add(owner.identity.len())
        .saturating_add(symbol.unchanged_record_bytes);
    if *bytes > limit {
        return Err(capacity(
            "replacement ownership metadata exceeds the writer byte budget",
        ));
    }
    Ok(Some((metadata, owner.identity)))
}

fn resolve(
    transaction: &Transaction<'_>,
    scope: &str,
    symbol: &Symbol,
    owner: &mut CodeTypeOwner,
    bytes: &mut usize,
    byte_limit: usize,
) -> Result<(), StorageError> {
    let hint = owner.import_target.as_deref().unwrap_or(&owner.target_hint);
    let name = hint.rsplit('.').next().unwrap_or(hint);
    // Declarations use their short symbol name; template arguments are checked
    // in the persisted lookup identity after this bounded indexed lookup.
    let name = if symbol.language == "cpp" {
        name.split('<').next().unwrap_or(name)
    } else {
        name
    };
    let swift_root = if owner.basis.as_deref() == Some("swift_extension") {
        if let Some((module, _)) = owner
            .import_target
            .as_deref()
            .and_then(|s| s.split_once('.'))
        {
            crate::storage::sqlite::code::semantic_modules::swift_module_root(
                transaction,
                scope,
                module,
                bytes,
                byte_limit,
            )?
        } else {
            None
        }
    } else {
        None
    };
    let proven_paths = if owner.basis.as_deref() == Some("rust_impl") {
        if let Some(target) = &owner.import_target {
            crate::storage::sqlite::code::semantic_modules::rust_target_paths(
                transaction,
                scope,
                &symbol.path,
                target,
                bytes,
                byte_limit,
            )?
        } else {
            Vec::new()
        }
    } else {
        owner.target_paths.clone()
    };
    let mut query = transaction.prepare(
        "SELECT path, type_owner_json FROM code_repository_symbols
        WHERE source_scope=?1 AND name=?2 AND language_id=?3 AND type_owner_json IS NOT NULL
          AND json_extract(type_owner_json,'$.relation')='declaration'
          AND length(CAST(type_owner_json AS BLOB))<=65536 LIMIT 65",
    )?;
    let mut rows = query.query(params![scope, name, symbol.language])?;
    let mut candidates = std::collections::BTreeSet::new();
    let mut seen = 0;
    while let Some(row) = rows.next()? {
        seen += 1;
        if seen > MAX_CANDIDATES {
            break;
        }
        let path: String = row.get(0)?;
        let metadata: String = row.get(1)?;
        *bytes = bytes
            .saturating_add(metadata.len())
            .saturating_add(path.len());
        if *bytes > byte_limit {
            return Err(capacity(
                "type declaration evidence exceeds the writer byte budget",
            ));
        }
        let candidate: CodeTypeOwner = serde_json::from_str(&metadata)
            .map_err(|error| StorageError::Invariant(error.to_string()))?;
        let same_module = owner.lookup_identity == candidate.lookup_identity;
        let local = path == symbol.path && same_module;
        let package_receiver = owner.basis.as_deref() == Some("go_receiver") && same_module;
        let explicit_import = proven_paths.contains(&path)
            && match owner.basis.as_deref() {
                Some("rust_impl") => {
                    candidate.target_hint == name
                        && matches!(candidate.visibility.as_deref(), Some("public" | "crate"))
                }
                Some("cpp_qualified") => candidate.target_hint == owner.target_hint,
                _ => false,
            };
        // Swift's existing indexed module contract uses a unique module
        // directory and an explicit typed import. Same-name files alone cannot
        // bind an extension in another file.
        let swift_import = owner.basis.as_deref() == Some("swift_extension")
            && candidate.visibility.as_deref() == Some("public")
            && owner
                .import_target
                .as_deref()
                .and_then(|target| target.split_once('.'))
                .is_some_and(|(_, imported)| {
                    imported == candidate.target_hint
                        && swift_root.as_ref().is_some_and(|root| {
                            path.strip_prefix(root)
                                .is_some_and(|rest| rest.starts_with('/'))
                        })
                });
        if local || package_receiver || explicit_import || swift_import {
            candidates.insert(candidate.identity);
        }
    }
    owner.identity = format!("unresolved|{}|{}", symbol.path, symbol.id);
    let state = if seen > MAX_CANDIDATES || candidates.len() > 1 {
        "ambiguous"
    } else if let Some(candidate) = candidates.into_iter().next() {
        owner.identity = candidate;
        "resolved"
    } else {
        "unresolved"
    };
    owner.resolution_state = Some(state.to_owned());
    Ok(())
}

fn capacity(reason: &str) -> StorageError {
    StorageError::CapacityExceeded(format!("type ownership finalization incomplete: {reason}"))
}

fn resolved_cpp_import_paths(
    transaction: &Transaction<'_>,
    scope: &str,
    path: &str,
    bytes: &mut usize,
    byte_limit: usize,
) -> Result<Vec<String>, StorageError> {
    let mut statement = transaction.prepare(
        "SELECT DISTINCT target_hint FROM code_repository_imports
         WHERE source_scope=?1 AND path=?2 AND resolution_state='resolved'
           AND target_hint IS NOT NULL LIMIT 65",
    )?;
    let mut rows = statement.query(params![scope, path])?;
    let mut paths = Vec::new();
    while let Some(row) = rows.next()? {
        let target: String = row.get(0)?;
        *bytes = bytes.saturating_add(target.len());
        if *bytes > byte_limit || paths.len() == MAX_CANDIDATES {
            return Err(capacity(
                "resolved import evidence exceeds the writer budget",
            ));
        }
        paths.push(target);
    }
    Ok(paths)
}

#[cfg(test)]
#[path = "type_ownership_tests.rs"]
mod tests;
