//! Confined filesystem publication helpers shared by repository map workflows.

use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::{fs, time::Duration};

use crate::{
    domain::{KnowledgeMap, RepositoryMapType},
    project::{
        AGENT_CONTRACT_DIR_NAME, CODESPEC_DIR_NAME, CODESPEC_MAP_FILE_NAME,
        CODESPEC_MAP_RELATIVE_PATH, KNOWLEDGE_MAP_FILE_NAME, KNOWLEDGE_MAP_HISTORY_DIR_NAME,
        KNOWLEDGE_MAP_RELATIVE_PATH, KNOWLEDGE_MAP_TOPICS_DIR_NAME,
        KNOWLEDGE_MAP_V3_RETAINED_BACKUP_FILE_NAME, KNOWLEDGE_MAP_V3_RETAINED_FILE_NAME,
        LEGACY_AGENT_CONTRACT_DIR_NAME, LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH,
        LEGACY_KNOWLEDGE_MAP_BACKUP_FILE_NAME, LEGACY_KNOWLEDGE_MAP_PREVIOUS_FILE_NAME,
    },
};

use super::{
    KnowledgeMapService, KnowledgeMapServiceError,
    artifact::{
        ARTIFACT_SCHEMA_VERSION, DIRECTORY_ARTIFACT_SCHEMA_VERSION, KnowledgeMapManifest,
        KnowledgeMapSchemaProbe, LEGACY_ARTIFACT_SCHEMA_VERSION, ensure_regular_file_within,
        is_generated_topic_shard_name, parse_manifest, read_root_file, reject_symlink,
        resolve_contract_ref_in, unsafe_path,
    },
};

pub(super) struct MapRootSnapshot {
    pub(super) content: String,
    pub(super) contract_dir: &'static str,
}

impl KnowledgeMapService {
    pub(super) fn map_path(&self) -> PathBuf {
        self.repository_root
            .join(Path::new(self.contract_dir_name()))
            .join(self.map_file_name())
    }

    pub(super) fn backup_path(&self) -> PathBuf {
        self.map_path().with_extension("yaml.previous")
    }

    pub(super) async fn read_root_content(&self) -> Result<String, KnowledgeMapServiceError> {
        Ok(self.read_root_snapshot().await?.content)
    }

    pub(super) async fn read_root_snapshot(
        &self,
    ) -> Result<MapRootSnapshot, KnowledgeMapServiceError> {
        let path = self.map_path();
        match read_root_file(&self.repository_root, &path).await {
            Ok(content) => {
                return Ok(MapRootSnapshot {
                    content,
                    contract_dir: self.contract_dir_name(),
                });
            }
            Err(KnowledgeMapServiceError::Io(error))
                if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        match read_root_file(&self.repository_root, &self.backup_path()).await {
            Ok(content) => {
                return Ok(MapRootSnapshot {
                    content,
                    contract_dir: self.contract_dir_name(),
                });
            }
            Err(KnowledgeMapServiceError::Io(error))
                if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if self.map_type == RepositoryMapType::Knowledge {
            if let Some(path) = self.readable_retained_v3_root().await? {
                return Ok(MapRootSnapshot {
                    content: read_root_file(&self.repository_root, &path).await?,
                    contract_dir: self.contract_dir_name(),
                });
            }
            match read_root_file(&self.repository_root, &self.legacy_map_path()).await {
                Ok(content) if content.contains("artifact_kind: redirect") => {
                    return Ok(MapRootSnapshot {
                        content: read_root_file(&self.repository_root, &path).await?,
                        contract_dir: self.contract_dir_name(),
                    });
                }
                Ok(content) => {
                    return Ok(MapRootSnapshot {
                        content,
                        contract_dir: LEGACY_AGENT_CONTRACT_DIR_NAME,
                    });
                }
                Err(KnowledgeMapServiceError::Io(error))
                    if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(MapRootSnapshot {
            content: read_root_file(&self.repository_root, &path).await?,
            contract_dir: self.contract_dir_name(),
        })
    }

    pub(super) fn contract_dir_name(&self) -> &'static str {
        match self.map_type {
            RepositoryMapType::Knowledge => AGENT_CONTRACT_DIR_NAME,
            RepositoryMapType::Codespec => CODESPEC_DIR_NAME,
        }
    }

    pub(super) async fn read_contract_dir_name(
        &self,
    ) -> Result<&'static str, KnowledgeMapServiceError> {
        if self.uses_legacy_contract().await? {
            Ok(LEGACY_AGENT_CONTRACT_DIR_NAME)
        } else {
            Ok(self.contract_dir_name())
        }
    }

    pub(super) async fn uses_legacy_contract(&self) -> Result<bool, KnowledgeMapServiceError> {
        Ok(self.map_type == RepositoryMapType::Knowledge
            && fs::try_exists(self.legacy_map_path()).await?
            && !fs::try_exists(self.map_path()).await?
            && !fs::try_exists(self.backup_path()).await?
            && self.readable_retained_v3_root().await?.is_none())
    }

    pub(super) fn map_file_name(&self) -> &'static str {
        match self.map_type {
            RepositoryMapType::Knowledge => KNOWLEDGE_MAP_FILE_NAME,
            RepositoryMapType::Codespec => CODESPEC_MAP_FILE_NAME,
        }
    }

    pub(super) fn relative_path(&self) -> &'static str {
        match self.map_type {
            RepositoryMapType::Knowledge => KNOWLEDGE_MAP_RELATIVE_PATH,
            RepositoryMapType::Codespec => CODESPEC_MAP_RELATIVE_PATH,
        }
    }

    pub(super) fn require_knowledge_map(
        &self,
        operation: &str,
    ) -> Result<(), KnowledgeMapServiceError> {
        if self.map_type == RepositoryMapType::Knowledge {
            Ok(())
        } else {
            Err(KnowledgeMapServiceError::InvalidRequest(format!(
                "{operation} only supports --type knowledge"
            )))
        }
    }

    pub(super) fn validate_manifest_identity(
        &self,
        manifest: &KnowledgeMapManifest,
    ) -> Result<(), KnowledgeMapServiceError> {
        let actual = manifest.map_type.unwrap_or(RepositoryMapType::Knowledge);
        if actual != self.map_type {
            return Err(KnowledgeMapServiceError::Integrity(format!(
                "map_type '{}' does not match path '{}'",
                actual.as_str(),
                self.relative_path()
            )));
        }
        if self.map_type == RepositoryMapType::Codespec && !manifest.topics.is_empty() {
            return Err(KnowledgeMapServiceError::Integrity(
                "codespec map must not contain knowledge topic shards".to_owned(),
            ));
        }
        Ok(())
    }
}

pub(super) async fn safe_repository_source_path(
    repository_root: &Path,
    relative: &str,
) -> Result<PathBuf, KnowledgeMapServiceError> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(KnowledgeMapServiceError::UnsafePath(relative.to_owned()));
    }
    let repository = fs::canonicalize(repository_root).await?;
    let path = repository_root.join(relative_path);
    let metadata = fs::symlink_metadata(&path).await?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(KnowledgeMapServiceError::UnsafePath(relative.to_owned()));
    }
    if metadata.len() > 4 * 1024 * 1024 {
        return Err(KnowledgeMapServiceError::Integrity(format!(
            "business glossary '{relative}' exceeds 4194304 bytes"
        )));
    }
    let canonical = fs::canonicalize(&path).await?;
    if !canonical.starts_with(repository) {
        return Err(KnowledgeMapServiceError::UnsafePath(relative.to_owned()));
    }
    Ok(canonical)
}

pub(super) fn parse_v1_map(content: &str) -> Result<KnowledgeMap, KnowledgeMapServiceError> {
    let mut map = serde_norway::from_str::<KnowledgeMap>(content)
        .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
    map.schema_version = KnowledgeMap::SCHEMA_VERSION;
    normalize_legacy_builtin_sources(&mut map);
    map.validate()?;
    Ok(map)
}

/// Parses a legacy map while admitting only repairable omissions in reserved routes.
pub(super) fn parse_v1_map_for_legacy_recovery(
    content: &str,
) -> Result<KnowledgeMap, KnowledgeMapServiceError> {
    let mut map = serde_norway::from_str::<KnowledgeMap>(content)
        .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
    map.schema_version = KnowledgeMap::SCHEMA_VERSION;
    normalize_legacy_builtin_sources(&mut map);
    map.ensure_reserved_repository_routes()?;
    Ok(map)
}

pub(super) fn temporary_path(path: &Path) -> PathBuf {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH);
    let suffix = elapsed.map_or(0, |duration| duration.as_nanos());
    path.with_extension(format!("{}.{}.tmp", std::process::id(), suffix))
}

pub(super) async fn ensure_owned_directory(
    repository_root: &Path,
    directory: &Path,
) -> Result<PathBuf, KnowledgeMapServiceError> {
    let knowledge = repository_root.join(AGENT_CONTRACT_DIR_NAME);
    let codespec = repository_root.join(CODESPEC_DIR_NAME);
    let legacy = repository_root.join(LEGACY_AGENT_CONTRACT_DIR_NAME);
    let contract = if directory.starts_with(&codespec) {
        codespec
    } else if directory.starts_with(&legacy) {
        legacy
    } else {
        knowledge
    };
    fs::create_dir_all(&contract).await?;
    reject_symlink(&contract).await?;
    let repository = fs::canonicalize(repository_root).await?;
    let contract = fs::canonicalize(contract).await?;
    if !contract.starts_with(&repository) {
        return Err(unsafe_path(&contract));
    }
    fs::create_dir_all(directory).await?;
    reject_symlink(directory).await?;
    let directory = fs::canonicalize(directory).await?;
    if !directory.starts_with(&contract) {
        return Err(unsafe_path(&directory));
    }
    Ok(directory)
}

pub(super) fn normalize_legacy_builtin_sources(map: &mut KnowledgeMap) -> bool {
    if let Some(source) = map
        .sources
        .iter_mut()
        .find(|source| source.id == "repository-business-glossary")
    {
        if source.uri == LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH {
            source.uri = crate::project::BUSINESS_GLOSSARY_RELATIVE_PATH.to_owned();
            source.version = source.version.saturating_add(1);
            return true;
        }
    }
    false
}

#[cfg(test)]
pub(super) async fn publish_immutable(
    repository_root: &Path,
    relative: &str,
    content: &[u8],
) -> Result<(), KnowledgeMapServiceError> {
    publish_immutable_in(repository_root, AGENT_CONTRACT_DIR_NAME, relative, content).await
}

pub(super) async fn publish_immutable_in(
    repository_root: &Path,
    contract_dir: &str,
    relative: &str,
    content: &[u8],
) -> Result<(), KnowledgeMapServiceError> {
    let path = resolve_contract_ref_in(repository_root, contract_dir, relative)?;
    let parent = path
        .parent()
        .ok_or_else(|| KnowledgeMapServiceError::UnsafePath(relative.to_owned()))?;
    let owned_parent = ensure_owned_directory(repository_root, parent).await?;
    if fs::try_exists(&path).await? {
        ensure_regular_file_within(&path, &owned_parent).await?;
        return if fs::read(&path).await? == content {
            Ok(())
        } else {
            Err(KnowledgeMapServiceError::Integrity(format!(
                "immutable map artifact '{}' has different content",
                path.display()
            )))
        };
    }
    let temp = temporary_path(&path);
    if let Err(error) = fs::write(&temp, content).await {
        let _ = fs::remove_file(&temp).await;
        return Err(error.into());
    }
    if let Err(error) = fs::rename(&temp, path).await {
        let _ = fs::remove_file(temp).await;
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
pub(super) async fn cleanup_superseded_topic_shards(
    repository_root: &Path,
    backup: &Path,
    manifest: &KnowledgeMapManifest,
    grace: Duration,
) {
    cleanup_superseded_topic_shards_in(
        repository_root,
        AGENT_CONTRACT_DIR_NAME,
        backup,
        manifest,
        grace,
    )
    .await;
}

pub(super) async fn cleanup_superseded_topic_shards_in(
    repository_root: &Path,
    contract_dir: &str,
    backup: &Path,
    manifest: &KnowledgeMapManifest,
    grace: Duration,
) {
    let mut retained = manifest.referenced_topic_files();
    for recovery_path in recovery_manifest_paths(repository_root, contract_dir, backup) {
        let content = match read_root_file(repository_root, &recovery_path).await {
            Ok(content) => content,
            Err(KnowledgeMapServiceError::Io(error))
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                continue;
            }
            Err(_) => return,
        };
        let probe = match serde_norway::from_str::<KnowledgeMapSchemaProbe>(&content) {
            Ok(probe) => probe,
            Err(_) => return,
        };
        if probe.schema_version == KnowledgeMap::SCHEMA_VERSION {
            continue;
        }
        if !matches!(
            probe.schema_version,
            LEGACY_ARTIFACT_SCHEMA_VERSION
                | DIRECTORY_ARTIFACT_SCHEMA_VERSION
                | ARTIFACT_SCHEMA_VERSION
        ) {
            return;
        }
        let Ok(recovery) = parse_manifest(&content) else {
            return;
        };
        retained.extend(recovery.referenced_topic_files());
    }
    let directory = repository_root
        .join(contract_dir)
        .join(KNOWLEDGE_MAP_TOPICS_DIR_NAME);
    if ensure_owned_directory(repository_root, &directory)
        .await
        .is_err()
    {
        return;
    }
    let Ok(mut entries) = fs::read_dir(directory).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let file_name = entry.file_name();
        if !is_generated_topic_shard_name(&file_name) {
            continue;
        }
        let mut marker_name = file_name.clone();
        marker_name.push(".retired");
        let marker = entry.path().with_file_name(marker_name);
        if retained.contains(&file_name) {
            let _ = fs::remove_file(marker).await;
            continue;
        }
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)
            .await
        {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => continue,
        }
        let old_enough = fs::symlink_metadata(&marker)
            .await
            .and_then(|metadata| {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(std::io::Error::other("invalid shard retirement marker"));
                }
                metadata.modified()
            })
            .and_then(|modified| modified.elapsed().map_err(std::io::Error::other))
            .is_ok_and(|age| age >= grace);
        if old_enough
            && match fs::remove_file(entry.path()).await {
                Ok(()) => true,
                Err(error) => error.kind() == std::io::ErrorKind::NotFound,
            }
        {
            let _ = fs::remove_file(marker).await;
        }
    }
}

pub(super) const HISTORY_CLEANUP_ENTRY_LIMIT: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HistoryCleanupStatus {
    Complete,
    Pending { removed: usize },
}

pub(super) async fn cleanup_history_artifacts_in(
    repository_root: &Path,
    contract_dir: &str,
) -> Result<HistoryCleanupStatus, KnowledgeMapServiceError> {
    let directory = repository_root
        .join(contract_dir)
        .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME);
    let metadata = match fs::symlink_metadata(&directory).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(HistoryCleanupStatus::Complete);
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(KnowledgeMapServiceError::UnsafePath(
            directory.display().to_string(),
        ));
    }
    let repository = fs::canonicalize(repository_root).await?;
    let canonical = fs::canonicalize(&directory).await?;
    if !canonical.starts_with(&repository) {
        return Err(KnowledgeMapServiceError::UnsafePath(
            directory.display().to_string(),
        ));
    }

    let mut entries = fs::read_dir(&directory).await?;
    let mut removable = Vec::with_capacity(HISTORY_CLEANUP_ENTRY_LIMIT);
    let mut has_more = false;
    while let Some(entry) = entries.next_entry().await? {
        let file_name = entry.file_name();
        if !is_generated_history_artifact_name(&file_name) {
            return Err(KnowledgeMapServiceError::Integrity(format!(
                "history cleanup refuses unrecognized entry '{}'",
                entry.path().display()
            )));
        }
        let metadata = fs::symlink_metadata(entry.path()).await?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(KnowledgeMapServiceError::UnsafePath(
                entry.path().display().to_string(),
            ));
        }
        if removable.len() == HISTORY_CLEANUP_ENTRY_LIMIT {
            has_more = true;
            break;
        }
        removable.push(entry.path());
    }
    drop(entries);
    let removed = removable.len();
    for path in removable {
        match fs::remove_file(path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    if has_more {
        return Ok(HistoryCleanupStatus::Pending { removed });
    }
    fs::remove_dir(&directory).await?;
    Ok(HistoryCleanupStatus::Complete)
}

fn is_generated_history_artifact_name(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str().and_then(|name| name.strip_suffix(".yaml")) else {
        return false;
    };
    let mut parts = name.split('-');
    let first = parts.next();
    let (height, from, through, digest) = if first == Some("index") {
        (parts.next(), parts.next(), parts.next(), parts.next())
    } else {
        (None, first, parts.next(), parts.next())
    };
    parts.next().is_none()
        && height.is_none_or(|value| value.len() == 2 && value.bytes().all(|b| b.is_ascii_digit()))
        && from.is_some_and(|value| value.len() == 20 && value.bytes().all(|b| b.is_ascii_digit()))
        && through
            .is_some_and(|value| value.len() == 20 && value.bytes().all(|b| b.is_ascii_digit()))
        && digest.is_some_and(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

fn recovery_manifest_paths(
    repository_root: &Path,
    contract_dir: &str,
    backup: &Path,
) -> Vec<PathBuf> {
    let mut paths = vec![backup.to_path_buf()];
    if contract_dir == AGENT_CONTRACT_DIR_NAME {
        paths.extend([
            repository_root
                .join(AGENT_CONTRACT_DIR_NAME)
                .join(KNOWLEDGE_MAP_V3_RETAINED_FILE_NAME),
            repository_root
                .join(AGENT_CONTRACT_DIR_NAME)
                .join(KNOWLEDGE_MAP_V3_RETAINED_BACKUP_FILE_NAME),
        ]);
    } else if contract_dir == LEGACY_AGENT_CONTRACT_DIR_NAME {
        paths.extend([
            repository_root
                .join(LEGACY_AGENT_CONTRACT_DIR_NAME)
                .join(LEGACY_KNOWLEDGE_MAP_BACKUP_FILE_NAME),
            repository_root
                .join(LEGACY_AGENT_CONTRACT_DIR_NAME)
                .join(LEGACY_KNOWLEDGE_MAP_PREVIOUS_FILE_NAME),
        ]);
    }
    paths
}
