//! Resolve explicit Rust imports only through indexed module declarations.
use super::*;

pub(super) fn resolve(
    connection: &Connection,
    scope: &str,
    rows: &mut [FeatureFlagRow],
    cache: &mut HashMap<String, Option<String>>,
    bytes: &mut usize,
) -> Result<(), StorageError> {
    for row in rows {
        let Some(reference) = row
            .metadata
            .reference
            .as_ref()
            .filter(|r| r.starts_with("rust-import|"))
        else {
            continue;
        };
        if !cache.contains_key(reference) {
            if cache.len() >= 1000 {
                return Err(incomplete("module import identity budget exceeded"));
            }
            let mut parts = reference.split('|');
            parts.next();
            let origin = parts.next().unwrap_or_default();
            let target = parts.next().unwrap_or_default();
            let paths = crate::storage::sqlite::code::semantic_modules::rust_target_paths(
                connection, scope, origin, target, bytes, MAX_BYTES,
            )?;
            let mut present = Vec::new();
            for path in paths {
                let exists: bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM code_repository_files WHERE source_scope=?1 AND path=?2 AND language_id='rust')",rusqlite::params![scope,path],|r|r.get(0))?;
                if exists {
                    present.push(path);
                }
            }
            let resolved = (present.len() == 1).then(|| {
                let path = present[0].strip_suffix(".rs").unwrap_or(&present[0]);
                let path = path.strip_suffix("/mod").unwrap_or(path);
                format!(
                    "rust|{path}||{}",
                    target.rsplit('.').next().unwrap_or_default()
                )
            });
            cache.insert(reference.clone(), resolved);
        }
        if let Some(resolved) = &cache[reference] {
            row.metadata.reference = Some(resolved.clone());
        } else {
            row.metadata.flow_incomplete = Some("unresolved_module_membership".into());
        }
    }
    Ok(())
}
