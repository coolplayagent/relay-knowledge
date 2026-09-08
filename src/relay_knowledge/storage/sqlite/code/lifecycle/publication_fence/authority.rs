//! Account-policy enforcement immediately before a new publication attachment.

use super::AUTHORITY_SCHEMA;
use crate::{
    paths::{RuntimePaths, StorageDirectoryAccess},
    storage::StorageError,
};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(in crate::storage) struct PublicationAuthority {
    pub(in crate::storage) path: PathBuf,
    pub(in crate::storage) paths: RuntimePaths,
}

#[cfg(test)]
#[path = "authority_tests.rs"]
mod tests;

pub(super) fn attach_authority(
    connection: &Connection,
    authority: &PublicationAuthority,
) -> Result<(), StorageError> {
    let authority_path = &authority.path;
    let attached = connection
        .query_row(
            "SELECT file FROM pragma_database_list WHERE name = ?1",
            params![AUTHORITY_SCHEMA],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(attached) = attached {
        if Path::new(&attached) == authority_path {
            return Ok(());
        }
        return Err(StorageError::InvalidInput(format!(
            "SQLite publication authority is already attached from '{}'",
            attached
        )));
    }
    if authority.paths.windows_data_sid.is_some() {
        // Every caller executes fenced mutations on the explicit SQLite worker.
        // Validate only for a new ATTACH; an attached handle needs no path reopen.
        tokio::runtime::Handle::try_current()
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?
            .block_on(authority.paths.ensure_storage_database_access(
                authority_path,
                StorageDirectoryAccess::ExistingOnly,
            ))
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    }
    connection.execute(
        &format!("ATTACH DATABASE ?1 AS {AUTHORITY_SCHEMA}"),
        params![authority_path.to_string_lossy().as_ref()],
    )?;
    Ok(())
}
