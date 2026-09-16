//! Bounded module membership proof shared by ownership and configuration queries.
use crate::{domain::CodeTypeOwner, storage::StorageError};
use rusqlite::{Connection, params};
use std::collections::{BTreeSet, VecDeque};

/// Follow indexed Rust `mod` declarations, including explicit `#[path]` values.
/// Missing, conditional or ambiguous crate membership remains unresolved.
pub(super) fn rust_target_paths(
    connection: &Connection,
    scope: &str,
    origin: &str,
    target: &str,
    bytes: &mut usize,
    byte_limit: usize,
) -> Result<Vec<String>, StorageError> {
    let mut parts = target.split('.').collect::<Vec<_>>();
    if parts.len() < 3 || parts.len() > 18 {
        return Ok(Vec::new());
    }
    parts.pop();
    let mut evidence = Evidence {
        connection,
        scope,
        bytes,
        byte_limit,
    };
    let mut paths = match parts.remove(0) {
        "crate" => {
            let mut roots = BTreeSet::new();
            for root in crate::domain::code_rust_modules::roots(origin) {
                if evidence.contains_origin(&root, origin)? {
                    roots.insert(root);
                }
            }
            if roots.len() != 1 {
                return Ok(Vec::new());
            }
            roots
        }
        "self" => BTreeSet::from([origin.to_owned()]),
        _ => return Ok(Vec::new()),
    };
    for module in parts {
        let mut next = BTreeSet::new();
        for path in paths {
            let records = evidence.modules(&path, Some(module))?;
            if records.len() > 1 {
                return Ok(Vec::new());
            }
            for owner in records {
                if owner.resolution_state.as_deref() == Some("resolved") {
                    next.extend(owner.target_paths);
                }
            }
        }
        if next.len() > 4 {
            return Err(capacity("module evidence candidate budget exceeded"));
        }
        paths = next;
    }
    Ok(paths.into_iter().collect())
}

struct Evidence<'a> {
    connection: &'a Connection,
    scope: &'a str,
    bytes: &'a mut usize,
    byte_limit: usize,
}
impl Evidence<'_> {
    fn contains_origin(&mut self, root: &str, origin: &str) -> Result<bool, StorageError> {
        let mut pending = VecDeque::from([(root.to_owned(), 0)]);
        let mut visited = BTreeSet::new();
        while let Some((path, depth)) = pending.pop_front() {
            if path == origin {
                return Ok(true);
            }
            if !visited.insert(path.clone()) {
                continue;
            }
            if visited.len() > 128 || depth >= 16 {
                return Err(capacity("crate membership traversal budget exceeded"));
            }
            let records = self.modules(&path, None)?;
            let mut names = BTreeSet::new();
            for owner in records {
                if !names.insert(owner.target_hint) {
                    return Ok(false);
                }
                if owner.resolution_state.as_deref() == Some("resolved") {
                    for target in owner.target_paths {
                        if pending.len() >= 128 {
                            return Err(capacity("crate membership queue budget exceeded"));
                        }
                        pending.push_back((target, depth + 1));
                    }
                }
            }
        }
        Ok(false)
    }

    fn modules(
        &mut self,
        path: &str,
        name: Option<&str>,
    ) -> Result<Vec<CodeTypeOwner>, StorageError> {
        let mut query=self.connection.prepare(
            "SELECT CASE WHEN length(CAST(type_owner_json AS BLOB))<=65536 THEN type_owner_json END,
             length(CAST(type_owner_json AS BLOB)) FROM code_repository_symbols
             WHERE source_scope=?1 AND path=?2 AND (?3 IS NULL OR name=?3) AND language_id='rust'
             AND json_extract(type_owner_json,'$.relation')='module_declaration' LIMIT 129")?;
        let mut rows = query.query(params![self.scope, path, name])?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            let length: usize = row.get(1)?;
            let cost = length.saturating_add(path.len());
            if length > 65536 || self.bytes.saturating_add(cost) > self.byte_limit {
                return Err(capacity("module evidence byte budget exceeded"));
            }
            if result.len() >= 128 {
                return Err(capacity("module evidence record budget exceeded"));
            }
            *self.bytes = self.bytes.saturating_add(cost);
            let metadata: String = row.get(0)?;
            result.push(
                serde_json::from_str(&metadata)
                    .map_err(|e| StorageError::Invariant(e.to_string()))?,
            );
        }
        Ok(result)
    }
}

fn capacity(reason: &str) -> StorageError {
    StorageError::CapacityExceeded(reason.into())
}

/// A typed Swift import identifies a module only when all indexed Swift files
/// prove a single physical root. The covering language index bounds unrelated work.
pub(super) fn swift_module_root(
    connection: &Connection,
    scope: &str,
    module: &str,
    bytes: &mut usize,
    byte_limit: usize,
) -> Result<Option<String>, StorageError> {
    let mut query=connection.prepare("SELECT path FROM code_repository_files INDEXED BY code_repository_files_language_path_lookup WHERE source_scope=?1 AND language_id='swift' LIMIT 1025")?;
    let mut rows = query.query(params![scope])?;
    let mut roots = BTreeSet::new();
    let mut seen = 0;
    while let Some(row) = rows.next()? {
        seen += 1;
        if seen > 1024 {
            return Err(capacity("Swift module evidence record budget exceeded"));
        }
        let path: String = row.get(0)?;
        *bytes = bytes.saturating_add(path.len());
        if *bytes > byte_limit {
            return Err(capacity("Swift module evidence byte budget exceeded"));
        }
        let components = path.split('/').collect::<Vec<_>>();
        for (i, part) in components
            .iter()
            .enumerate()
            .take(components.len().saturating_sub(1))
        {
            if *part == module {
                roots.insert(components[..=i].join("/"));
            }
        }
        if roots.len() > 1 {
            return Ok(None);
        }
    }
    Ok(roots.into_iter().next())
}
