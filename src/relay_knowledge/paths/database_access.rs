//! Recover managed account policy for fresh database connections and attachments.

use super::*;

/// Returns a bounded asynchronous check only when the pathname belongs to the
/// reserved SID tree. The caller must await it immediately before a fresh open.
pub(crate) fn managed_database_validation(
    database_path: &Path,
) -> Result<Option<impl Future<Output = Result<(), PathError>> + Send + '_>, PathError> {
    if database_path.as_os_str().len() > 4096 {
        return Err(PathError {
            purpose: PathPurpose::Data,
            kind: PathErrorKind::WindowsStorageSecurity {
                reason: "database path exceeds 4096 bytes".to_owned(),
            },
        });
    }
    // At most 4096 bytes of ancestors, with no filesystem traversal here. SID
    // recovery uses the same reserved-layout parser as service-pinned overrides.
    for data_dir in database_path.ancestors().skip(1) {
        if let Some(sid) = windows_data_sid_from_path(data_dir)? {
            return Ok(Some(async move {
                windows_storage::prepare_private_directory(
                    data_dir,
                    &sid,
                    StorageDirectoryAccess::ExistingOnly,
                    Some(database_path),
                )
                .await
            }));
        }
    }
    Ok(None)
}
