//! Recover managed account policy for fresh database connections and attachments.

use super::*;

impl RuntimePaths {
    /// Admits one bounded read-only inspection of this data tree. No result is
    /// retained to authorize later requests or writable database opens.
    pub(crate) async fn ensure_storage_inspection_access(&self) -> Result<(), PathError> {
        let declared = self.validated_windows_data_sid()?;
        let recovered = windows_data_sid_from_path(&self.data_dir)?;
        if let Some(sid) = declared.or(recovered.as_deref()) {
            return windows_storage::prepare_private_directory(
                &self.data_dir,
                sid,
                StorageDirectoryAccess::ExistingOnly,
                None,
            )
            .await;
        }
        #[cfg(windows)]
        if windows_storage::current_sid()? == "S-1-5-18" {
            return windows_storage::validate_service_inspection_tree(&self.database_file()).await;
        }
        Ok(())
    }
}

/// Returns a bounded asynchronous check only when the pathname belongs to the
/// reserved SID tree. The caller must await it immediately before a fresh open.
pub(crate) fn managed_database_validation(
    database_path: &Path,
    access: StorageDirectoryAccess,
) -> Result<Option<impl Future<Output = Result<(), PathError>> + Send + '_>, PathError> {
    if database_path.as_os_str().len() > 4096 {
        return Err(PathError {
            purpose: PathPurpose::Data,
            kind: PathErrorKind::WindowsStorageSecurity {
                reason: "database path exceeds 4096 bytes".to_owned(),
            },
        });
    }
    // At most 4096 bytes of ancestors, with no filesystem traversal here.
    let mut managed = None;
    for data_dir in database_path.ancestors().skip(1) {
        if let Some(sid) = windows_data_sid_from_path(data_dir)? {
            managed = Some((data_dir, sid));
            break;
        }
    }
    #[cfg(windows)]
    let privileged = managed.is_none() && windows_storage::current_sid()? == "S-1-5-18";
    #[cfg(not(windows))]
    let privileged = false;
    if managed.is_none() && !privileged {
        return Ok(None);
    }
    Ok(Some(async move {
        if let Some((data_dir, sid)) = managed {
            windows_storage::prepare_private_directory(data_dir, &sid, access, Some(database_path))
                .await
        } else {
            windows_storage::validate_service_database_path(database_path, access).await
        }
    }))
}
