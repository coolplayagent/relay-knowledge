//! Recoverable v1/v2 to v3 Knowledge Map migration and rollback.

use std::path::{Path, PathBuf};

use tokio::{fs, io::AsyncWriteExt};

use crate::{
    api::RequestContext,
    domain::{KnowledgeMap, RepositoryMapType},
    project::{
        AGENT_CONTRACT_DIR_NAME, KNOWLEDGE_MAP_HISTORY_DIR_NAME, KNOWLEDGE_MAP_RELATIVE_PATH,
        KNOWLEDGE_MAP_TOPICS_DIR_NAME, KNOWLEDGE_MAP_V3_RETAINED_BACKUP_FILE_NAME,
        KNOWLEDGE_MAP_V3_RETAINED_FILE_NAME, LEGACY_AGENT_CONTRACT_DIR_NAME,
        LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH, LEGACY_KNOWLEDGE_MAP_BACKUP_FILE_NAME,
        LEGACY_KNOWLEDGE_MAP_REDIRECT_PREPARED_FILE_NAME,
        LEGACY_KNOWLEDGE_MAP_REDIRECT_PREVIOUS_FILE_NAME, LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH,
        LEGACY_KNOWLEDGE_MAP_ROLLBACK_PREPARED_FILE_NAME,
        LEGACY_KNOWLEDGE_MAP_ROLLBACK_PREVIOUS_FILE_NAME,
    },
};

use super::{
    ARTIFACT_SCHEMA_VERSION, KnowledgeMapMutationResponse, KnowledgeMapSchemaProbe,
    KnowledgeMapService, KnowledgeMapServiceError, LEGACY_ARTIFACT_SCHEMA_VERSION,
    WRITE_LOCK_TIMEOUT, ensure_owned_directory, parse_manifest, parse_v1_map, read_root_file,
};

impl KnowledgeMapService {
    pub async fn migrate_to_v3(
        &self,
        context: &RequestContext,
    ) -> Result<KnowledgeMapMutationResponse, KnowledgeMapServiceError> {
        self.require_knowledge_map("map migrate --to-v3")?;
        self.init(context).await
    }

    pub async fn rollback_v3(
        &self,
        context: &RequestContext,
    ) -> Result<KnowledgeMapMutationResponse, KnowledgeMapServiceError> {
        self.require_knowledge_map("map migrate --rollback")?;
        let _legacy_lock = self.acquire_legacy_write_lock(WRITE_LOCK_TIMEOUT).await?;
        let _lock = self.acquire_write_lock(WRITE_LOCK_TIMEOUT).await?;
        let legacy_backup = self.validate_legacy_backup().await?;
        let rollback_version = map_version_from_validated_legacy_content(&legacy_backup)?;
        let legacy = self.legacy_map_path();
        let rollback_prepared = self.legacy_rollback_prepared_path();
        let rollback_previous = self.legacy_rollback_previous_path();
        let current = self.map_path();
        let ordinary_backup = self.backup_path();
        let retained = self.retained_v3_path();
        let retained_backup = self.retained_v3_backup_path();
        let legacy_existed = regular_file_exists_or_missing(&legacy).await?;
        let current_exists = regular_file_exists_or_missing(&current).await?;
        let ordinary_backup_exists = regular_file_exists_or_missing(&ordinary_backup).await?;
        remove_regular_transition_file(&rollback_prepared).await?;
        remove_regular_transition_file(&rollback_previous).await?;
        if current_exists {
            remove_regular_transition_file(&retained).await?;
        }
        if ordinary_backup_exists {
            remove_regular_transition_file(&retained_backup).await?;
        }
        write_new_synced_file(&rollback_prepared, legacy_backup.as_bytes()).await?;

        let current_moved = if current_exists {
            match fs::rename(&current, &retained).await {
                Ok(()) => true,
                Err(error) => {
                    let _ = fs::remove_file(&rollback_prepared).await;
                    return Err(error.into());
                }
            }
        } else {
            false
        };
        let ordinary_backup_moved = if ordinary_backup_exists {
            match fs::rename(&ordinary_backup, &retained_backup).await {
                Ok(()) => true,
                Err(error) => {
                    restore_visible_rollback_roots(
                        &current,
                        &retained,
                        current_moved,
                        &ordinary_backup,
                        &retained_backup,
                        false,
                    )
                    .await;
                    let _ = fs::remove_file(&rollback_prepared).await;
                    return Err(error.into());
                }
            }
        } else {
            false
        };

        let legacy_moved = if legacy_existed {
            match fs::rename(&legacy, &rollback_previous).await {
                Ok(()) => true,
                Err(error) => {
                    restore_visible_rollback_roots(
                        &current,
                        &retained,
                        current_moved,
                        &ordinary_backup,
                        &retained_backup,
                        ordinary_backup_moved,
                    )
                    .await;
                    let _ = fs::remove_file(&rollback_prepared).await;
                    return Err(error.into());
                }
            }
        } else {
            false
        };
        if let Err(error) = fs::rename(&rollback_prepared, &legacy).await {
            if legacy_moved {
                let _ = fs::rename(&rollback_previous, &legacy).await;
            }
            restore_visible_rollback_roots(
                &current,
                &retained,
                current_moved,
                &ordinary_backup,
                &retained_backup,
                ordinary_backup_moved,
            )
            .await;
            let _ = fs::remove_file(&rollback_prepared).await;
            return Err(error.into());
        }
        remove_regular_transition_file(&rollback_previous).await?;
        Ok(self.mutation_response(
            context,
            rollback_version,
            "restored Knowledge Map v2 root; retained v3 data for forward recovery".to_owned(),
        ))
    }

    pub(super) async fn prepare_legacy_migration(&self) -> Result<(), KnowledgeMapServiceError> {
        let current = self.map_path();
        if self.map_type != RepositoryMapType::Knowledge
            || regular_file_exists_or_missing(&current).await?
        {
            return Ok(());
        }
        let legacy = self.legacy_map_path();
        let legacy_content = match read_root_file(&self.repository_root, &legacy).await {
            Ok(content) => content,
            Err(KnowledgeMapServiceError::Io(error))
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        self.validate_legacy_map_content_in(LEGACY_AGENT_CONTRACT_DIR_NAME, &legacy_content)
            .await?;
        let backup = self.legacy_backup_path();
        if regular_file_exists_or_missing(&backup).await? {
            self.validate_legacy_backup().await?;
        } else {
            write_new_synced_file(&backup, legacy_content.as_bytes()).await?;
        }
        copy_contract_tree(
            &self.repository_root,
            &self
                .repository_root
                .join(LEGACY_AGENT_CONTRACT_DIR_NAME)
                .join(KNOWLEDGE_MAP_TOPICS_DIR_NAME),
            &self
                .repository_root
                .join(AGENT_CONTRACT_DIR_NAME)
                .join(KNOWLEDGE_MAP_TOPICS_DIR_NAME),
        )
        .await?;
        copy_contract_tree(
            &self.repository_root,
            &self
                .repository_root
                .join(LEGACY_AGENT_CONTRACT_DIR_NAME)
                .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME),
            &self
                .repository_root
                .join(AGENT_CONTRACT_DIR_NAME)
                .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME),
        )
        .await?;
        let legacy_glossary = self
            .repository_root
            .join(LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH);
        let glossary = self.business_glossary_path();
        let legacy_glossary_exists = regular_file_exists_or_missing(&legacy_glossary).await?;
        let glossary_exists = regular_file_exists_or_missing(&glossary).await?;
        if legacy_glossary_exists && !glossary_exists {
            if let Some(parent) = glossary.parent() {
                ensure_owned_directory(&self.repository_root, parent).await?;
            }
            let content = read_root_file(&self.repository_root, &legacy_glossary).await?;
            write_new_synced_file(&glossary, content.as_bytes()).await?;
        }
        if let Some(parent) = current.parent() {
            ensure_owned_directory(&self.repository_root, parent).await?;
        }
        write_new_synced_file(&current, legacy_content.as_bytes()).await?;
        Ok(())
    }

    pub(super) async fn publish_legacy_redirect(&self) -> Result<(), KnowledgeMapServiceError> {
        if self.map_type != RepositoryMapType::Knowledge {
            return Ok(());
        }
        self.converge_legacy_redirect().await
    }

    pub(super) async fn legacy_recovery_state_exists(
        &self,
    ) -> Result<bool, KnowledgeMapServiceError> {
        if self.map_type != RepositoryMapType::Knowledge {
            return Ok(false);
        }
        for path in [
            self.legacy_map_path(),
            self.legacy_backup_path(),
            self.legacy_redirect_prepared_path(),
            self.legacy_redirect_previous_path(),
            self.legacy_rollback_prepared_path(),
            self.legacy_rollback_previous_path(),
        ] {
            if path_entry_exists(&path).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) async fn recover_legacy_redirect_transition(
        &self,
    ) -> Result<(), KnowledgeMapServiceError> {
        if self.map_type != RepositoryMapType::Knowledge
            || !path_entry_exists(&self.map_path()).await?
            || !path_entry_exists(&self.legacy_backup_path()).await?
        {
            return Ok(());
        }
        self.converge_legacy_redirect().await
    }

    pub(super) async fn recover_legacy_rollback_transition(
        &self,
    ) -> Result<bool, KnowledgeMapServiceError> {
        if self.map_type != RepositoryMapType::Knowledge {
            return Ok(false);
        }
        let prepared = self.legacy_rollback_prepared_path();
        let previous = self.legacy_rollback_previous_path();
        let prepared_exists = regular_file_exists_or_missing(&prepared).await?;
        let previous_exists = regular_file_exists_or_missing(&previous).await?;
        if !prepared_exists && !previous_exists {
            return Ok(false);
        }

        let expected_legacy = self.validate_legacy_backup().await?;
        let legacy = self.legacy_map_path();
        let legacy_exists = regular_file_exists_or_missing(&legacy).await?;
        let legacy_content = if legacy_exists {
            Some(read_root_file(&self.repository_root, &legacy).await?)
        } else {
            None
        };

        if !prepared_exists && legacy_content.as_deref() == Some(expected_legacy.as_str()) {
            remove_regular_transition_file(&previous).await?;
            return Ok(true);
        }
        if prepared_exists {
            let staged = read_root_file(&self.repository_root, &prepared).await?;
            if staged.as_bytes() != expected_legacy.as_bytes() {
                return Err(KnowledgeMapServiceError::Integrity(
                    "rollback prepared root differs from the retained legacy backup".to_owned(),
                ));
            }
        }

        let current = self.map_path();
        let retained = self.retained_v3_path();
        let ordinary_backup = self.backup_path();
        let retained_backup = self.retained_v3_backup_path();
        let current_exists = regular_file_exists_or_missing(&current).await?;
        let retained_exists = regular_file_exists_or_missing(&retained).await?;
        let ordinary_backup_exists = regular_file_exists_or_missing(&ordinary_backup).await?;
        let retained_backup_exists = regular_file_exists_or_missing(&retained_backup).await?;
        if !current_exists && !retained_exists {
            return Err(KnowledgeMapServiceError::Integrity(
                "rollback recovery has neither a visible nor retained v3 root".to_owned(),
            ));
        }
        if !current_exists {
            fs::rename(&retained, &current).await?;
        }
        if !ordinary_backup_exists && retained_backup_exists {
            fs::rename(&retained_backup, &ordinary_backup).await?;
        }
        if !legacy_exists && previous_exists {
            fs::rename(&previous, &legacy).await?;
        }
        remove_regular_transition_file(&prepared).await?;
        Ok(false)
    }

    async fn converge_legacy_redirect(&self) -> Result<(), KnowledgeMapServiceError> {
        let yaml = format!(
            "schema_version: 3\nartifact_kind: redirect\nmap_type: knowledge\ntarget: {KNOWLEDGE_MAP_RELATIVE_PATH}\n"
        );
        let legacy = self.legacy_map_path();
        let prepared = self.legacy_redirect_prepared_path();
        let previous = self.legacy_redirect_previous_path();
        let rollback_prepared = self.legacy_rollback_prepared_path();
        let rollback_previous = self.legacy_rollback_previous_path();
        let live_is_redirect = match read_root_file(&self.repository_root, &legacy).await {
            Ok(content) => content.as_bytes() == yaml.as_bytes(),
            Err(KnowledgeMapServiceError::Io(error))
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                false
            }
            Err(error) => return Err(error),
        };
        if live_is_redirect {
            let has_residue = path_entry_exists(&prepared).await?
                || path_entry_exists(&previous).await?
                || path_entry_exists(&rollback_prepared).await?
                || path_entry_exists(&rollback_previous).await?;
            if !has_residue {
                return Ok(());
            }
        }

        self.validate_legacy_backup().await?;
        if live_is_redirect {
            remove_regular_transition_file(&prepared).await?;
            remove_regular_transition_file(&previous).await?;
            remove_regular_transition_file(&rollback_prepared).await?;
            remove_regular_transition_file(&rollback_previous).await?;
            return Ok(());
        }

        remove_regular_transition_file(&prepared).await?;
        write_new_synced_file(&prepared, yaml.as_bytes()).await?;
        let moved_legacy = if fs::try_exists(&legacy).await? {
            remove_regular_transition_file(&previous).await?;
            fs::rename(&legacy, &previous).await?;
            true
        } else {
            false
        };
        if let Err(error) = fs::rename(&prepared, &legacy).await {
            if moved_legacy {
                let _ = fs::rename(&previous, &legacy).await;
            }
            let _ = fs::remove_file(prepared).await;
            return Err(error.into());
        }
        remove_regular_transition_file(&previous).await?;
        remove_regular_transition_file(&rollback_prepared).await?;
        remove_regular_transition_file(&rollback_previous).await?;
        Ok(())
    }

    async fn validate_legacy_backup(&self) -> Result<String, KnowledgeMapServiceError> {
        let path = self.legacy_backup_path();
        let content = match read_root_file(&self.repository_root, &path).await {
            Ok(content) => content,
            Err(KnowledgeMapServiceError::Io(error))
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                return Err(KnowledgeMapServiceError::InvalidRequest(
                    "v2 rollback backup is missing".to_owned(),
                ));
            }
            Err(error) => return Err(error),
        };
        self.validate_legacy_map_content_in(LEGACY_AGENT_CONTRACT_DIR_NAME, &content)
            .await?;
        Ok(content)
    }

    pub(super) fn retained_v3_path(&self) -> PathBuf {
        self.repository_root
            .join(AGENT_CONTRACT_DIR_NAME)
            .join(KNOWLEDGE_MAP_V3_RETAINED_FILE_NAME)
    }

    fn retained_v3_backup_path(&self) -> PathBuf {
        self.repository_root
            .join(AGENT_CONTRACT_DIR_NAME)
            .join(KNOWLEDGE_MAP_V3_RETAINED_BACKUP_FILE_NAME)
    }

    fn legacy_redirect_prepared_path(&self) -> PathBuf {
        self.repository_root
            .join(LEGACY_AGENT_CONTRACT_DIR_NAME)
            .join(LEGACY_KNOWLEDGE_MAP_REDIRECT_PREPARED_FILE_NAME)
    }

    fn legacy_redirect_previous_path(&self) -> PathBuf {
        self.repository_root
            .join(LEGACY_AGENT_CONTRACT_DIR_NAME)
            .join(LEGACY_KNOWLEDGE_MAP_REDIRECT_PREVIOUS_FILE_NAME)
    }

    fn legacy_rollback_prepared_path(&self) -> PathBuf {
        self.repository_root
            .join(LEGACY_AGENT_CONTRACT_DIR_NAME)
            .join(LEGACY_KNOWLEDGE_MAP_ROLLBACK_PREPARED_FILE_NAME)
    }

    fn legacy_rollback_previous_path(&self) -> PathBuf {
        self.repository_root
            .join(LEGACY_AGENT_CONTRACT_DIR_NAME)
            .join(LEGACY_KNOWLEDGE_MAP_ROLLBACK_PREVIOUS_FILE_NAME)
    }

    pub(super) fn legacy_map_path(&self) -> PathBuf {
        self.repository_root
            .join(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH)
    }

    pub(super) fn legacy_backup_path(&self) -> PathBuf {
        self.repository_root
            .join(LEGACY_AGENT_CONTRACT_DIR_NAME)
            .join(LEGACY_KNOWLEDGE_MAP_BACKUP_FILE_NAME)
    }
}

async fn path_entry_exists(path: &Path) -> Result<bool, KnowledgeMapServiceError> {
    match fs::symlink_metadata(path).await {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

async fn regular_file_exists_or_missing(path: &Path) -> Result<bool, KnowledgeMapServiceError> {
    match fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(KnowledgeMapServiceError::UnsafePath(
            path.display().to_string(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn map_version_from_validated_legacy_content(
    content: &str,
) -> Result<u64, KnowledgeMapServiceError> {
    let probe = serde_norway::from_str::<KnowledgeMapSchemaProbe>(content)
        .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
    if probe.schema_version == KnowledgeMap::SCHEMA_VERSION {
        return Ok(parse_v1_map(content)?.map_version);
    }
    if matches!(
        probe.schema_version,
        LEGACY_ARTIFACT_SCHEMA_VERSION | ARTIFACT_SCHEMA_VERSION
    ) {
        return Ok(parse_manifest(content)?.map_version);
    }
    Err(KnowledgeMapServiceError::Yaml(format!(
        "unsupported schema_version {}",
        probe.schema_version
    )))
}

async fn restore_visible_rollback_roots(
    current: &Path,
    retained: &Path,
    current_moved: bool,
    ordinary_backup: &Path,
    retained_backup: &Path,
    ordinary_backup_moved: bool,
) {
    if ordinary_backup_moved {
        let _ = fs::rename(retained_backup, ordinary_backup).await;
    }
    if current_moved {
        let _ = fs::rename(retained, current).await;
    }
}

async fn write_new_synced_file(
    path: &Path,
    content: &[u8],
) -> Result<(), KnowledgeMapServiceError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await?;
    if let Err(error) = file.write_all(content).await {
        drop(file);
        let _ = fs::remove_file(path).await;
        return Err(error.into());
    }
    if let Err(error) = file.sync_all().await {
        drop(file);
        let _ = fs::remove_file(path).await;
        return Err(error.into());
    }
    Ok(())
}

async fn remove_regular_transition_file(path: &Path) -> Result<(), KnowledgeMapServiceError> {
    match fs::symlink_metadata(path).await {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => Err(
            KnowledgeMapServiceError::UnsafePath(path.display().to_string()),
        ),
        Ok(_) => {
            fs::remove_file(path).await?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

async fn copy_contract_tree(
    repository_root: &Path,
    source: &Path,
    target: &Path,
) -> Result<(), KnowledgeMapServiceError> {
    match fs::symlink_metadata(source).await {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(KnowledgeMapServiceError::UnsafePath(
                source.display().to_string(),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    let repository = fs::canonicalize(repository_root).await?;
    let canonical_source = fs::canonicalize(source).await?;
    if !canonical_source.starts_with(&repository) {
        return Err(KnowledgeMapServiceError::UnsafePath(
            source.display().to_string(),
        ));
    }
    let target = ensure_owned_directory(repository_root, target).await?;
    let mut entries = fs::read_dir(source).await?;
    while let Some(entry) = entries.next_entry().await? {
        let file_type = entry.file_type().await?;
        if file_type.is_symlink() || !file_type.is_file() {
            return Err(KnowledgeMapServiceError::UnsafePath(
                entry.path().display().to_string(),
            ));
        }
        let content = read_root_file(repository_root, &entry.path()).await?;
        let destination = target.join(entry.file_name());
        if regular_file_exists_or_missing(&destination).await? {
            let existing = read_root_file(repository_root, &destination).await?;
            if existing.as_bytes() != content.as_bytes() {
                return Err(KnowledgeMapServiceError::Integrity(format!(
                    "migration target '{}' differs from the retained legacy artifact",
                    destination.display()
                )));
            }
        } else {
            write_new_synced_file(&destination, content.as_bytes()).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "migration_tests.rs"]
mod tests;
