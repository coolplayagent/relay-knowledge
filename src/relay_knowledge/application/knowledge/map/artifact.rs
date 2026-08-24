//! Knowledge Map v2 manifest, topic-shard, and history-archive file contracts.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::domain::{
    KnowledgeMap, KnowledgeMapHistoryEntry, KnowledgeMapRoute, KnowledgeMapSource,
    KnowledgeMapTopic,
};

use super::KnowledgeMapServiceError;

pub(super) const RECENT_HISTORY_LIMIT: usize = 16;

#[derive(Debug, Deserialize)]
pub(super) struct KnowledgeMapSchemaProbe {
    pub(super) schema_version: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct KnowledgeMapManifest {
    pub(super) schema_version: u16,
    pub(super) map_version: u64,
    pub(super) updated_at: String,
    pub(super) topics: Vec<KnowledgeMapTopicRef>,
    pub(super) history: KnowledgeMapHistoryManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct KnowledgeMapTopicRef {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) description: String,
    pub(super) r#ref: String,
    pub(super) digest: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct KnowledgeMapTopicShard {
    pub(super) schema_version: u16,
    pub(super) topic: KnowledgeMapTopic,
    pub(super) sources: Vec<KnowledgeMapSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) route: Option<KnowledgeMapRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct KnowledgeMapHistoryManifest {
    pub(super) archived_through: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) archive: Option<KnowledgeMapArchiveRef>,
    pub(super) recent: Vec<KnowledgeMapHistoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct KnowledgeMapArchiveRef {
    pub(super) r#ref: String,
    pub(super) digest: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct KnowledgeMapHistoryArchive {
    pub(super) schema_version: u16,
    pub(super) from_version: u64,
    pub(super) through_version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) previous: Option<KnowledgeMapArchiveRef>,
    pub(super) entries: Vec<KnowledgeMapHistoryEntry>,
}

pub(super) fn parse_manifest(
    content: &str,
) -> Result<KnowledgeMapManifest, KnowledgeMapServiceError> {
    let manifest = serde_norway::from_str::<KnowledgeMapManifest>(content)
        .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
    if manifest.schema_version != KnowledgeMap::SCHEMA_VERSION || manifest.map_version == 0 {
        return Err(KnowledgeMapServiceError::Integrity(
            "manifest schema_version or map_version is invalid".to_owned(),
        ));
    }
    let mut topic_ids = std::collections::HashSet::new();
    let mut folded_topic_ids = std::collections::HashSet::new();
    let mut refs = std::collections::HashSet::new();
    let mut folded_refs = std::collections::HashSet::new();
    for topic in &manifest.topics {
        if topic.id.trim().is_empty()
            || topic.title.trim().is_empty()
            || topic.description.trim().is_empty()
            || !topic_ids.insert(topic.id.as_str())
            || !refs.insert(topic.r#ref.as_str())
            || !folded_topic_ids.insert(topic.id.to_lowercase())
            || !folded_refs.insert(topic.r#ref.to_lowercase())
        {
            return Err(KnowledgeMapServiceError::Integrity(
                "topic ids and shard refs must be non-empty and unique".to_owned(),
            ));
        }
    }
    validate_recent_history(&manifest)?;
    Ok(manifest)
}

pub(super) fn validate_topic_shard(
    shard: &KnowledgeMapTopicShard,
) -> Result<(), KnowledgeMapServiceError> {
    let mut source_ids = std::collections::HashSet::new();
    for source in &shard.sources {
        if source.topic != shard.topic.id || !source_ids.insert(source.id.as_str()) {
            return Err(KnowledgeMapServiceError::Integrity(format!(
                "topic shard '{}' contains a foreign or duplicate source",
                shard.topic.id
            )));
        }
    }
    if let Some(route) = &shard.route {
        let mut routed = std::collections::HashSet::new();
        if route.topic != shard.topic.id
            || route
                .source_order
                .iter()
                .any(|id| !source_ids.contains(id.as_str()) || !routed.insert(id.as_str()))
            || routed.len() != source_ids.len()
        {
            return Err(KnowledgeMapServiceError::Integrity(format!(
                "topic shard '{}' has an invalid route",
                shard.topic.id
            )));
        }
    } else if !shard.sources.is_empty() {
        return Err(KnowledgeMapServiceError::Integrity(format!(
            "topic shard '{}' has sources without a route",
            shard.topic.id
        )));
    }
    KnowledgeMap {
        schema_version: KnowledgeMap::SCHEMA_VERSION,
        map_version: 1,
        updated_at: "shard-validation".to_owned(),
        topics: vec![shard.topic.clone()],
        sources: shard.sources.clone(),
        routes: shard.route.clone().into_iter().collect(),
        history: vec![KnowledgeMapHistoryEntry {
            version: 1,
            action: "validate".to_owned(),
            actor: "system".to_owned(),
            summary: "Validate an isolated topic shard.".to_owned(),
        }],
    }
    .validate()?;
    Ok(())
}

pub(super) fn validate_recent_history(
    manifest: &KnowledgeMapManifest,
) -> Result<(), KnowledgeMapServiceError> {
    if manifest.history.recent.is_empty() || manifest.history.recent.len() > RECENT_HISTORY_LIMIT {
        return Err(KnowledgeMapServiceError::Integrity(format!(
            "recent history must contain 1..={RECENT_HISTORY_LIMIT} entries"
        )));
    }
    let mut expected = manifest.history.archived_through.saturating_add(1);
    for entry in &manifest.history.recent {
        if entry.version != expected {
            return Err(KnowledgeMapServiceError::Integrity(
                "recent history is not contiguous with its archive checkpoint".to_owned(),
            ));
        }
        expected = expected.saturating_add(1);
    }
    if expected.saturating_sub(1) != manifest.map_version {
        return Err(KnowledgeMapServiceError::Integrity(
            "recent history does not end at map_version".to_owned(),
        ));
    }
    if (manifest.history.archived_through == 0) != manifest.history.archive.is_none() {
        return Err(KnowledgeMapServiceError::Integrity(
            "history archive reference and checkpoint disagree".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn serialize_yaml<T: Serialize>(value: &T) -> Result<String, KnowledgeMapServiceError> {
    serde_norway::to_string(value)
        .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))
}

pub(super) fn content_digest(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

pub(super) fn stable_id(value: &str) -> String {
    content_digest(value.as_bytes())[..16].to_owned()
}

pub(super) async fn ensure_contract_dir_is_scoped(
    repository_root: &Path,
    contract_dir: &Path,
) -> Result<PathBuf, KnowledgeMapServiceError> {
    fs::create_dir_all(contract_dir).await?;
    let canonical_repository = fs::canonicalize(repository_root).await?;
    let canonical_contract = fs::canonicalize(contract_dir).await?;
    if !canonical_contract.starts_with(&canonical_repository) {
        return Err(KnowledgeMapServiceError::UnsafePath(
            contract_dir.display().to_string(),
        ));
    }
    Ok(canonical_contract)
}

pub(super) async fn ensure_artifact_parent_is_scoped(
    repository_root: &Path,
    contract_dir: &Path,
    artifact_path: &Path,
) -> Result<(), KnowledgeMapServiceError> {
    let canonical_contract = ensure_contract_dir_is_scoped(repository_root, contract_dir).await?;
    let parent = artifact_path
        .parent()
        .ok_or_else(|| KnowledgeMapServiceError::UnsafePath(artifact_path.display().to_string()))?;
    fs::create_dir_all(parent).await?;
    let canonical_parent = fs::canonicalize(parent).await?;
    if !canonical_parent.starts_with(canonical_contract) {
        return Err(KnowledgeMapServiceError::UnsafePath(
            artifact_path.display().to_string(),
        ));
    }
    Ok(())
}

pub(super) async fn read_root_content(
    path: &Path,
    backup: &Path,
) -> Result<String, KnowledgeMapServiceError> {
    match fs::read_to_string(path).await {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match fs::read_to_string(backup).await {
                Ok(content) => Ok(content),
                Err(backup_error) if backup_error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(fs::read_to_string(path).await?)
                }
                Err(backup_error) => Err(KnowledgeMapServiceError::Io(backup_error)),
            }
        }
        Err(error) => Err(KnowledgeMapServiceError::Io(error)),
    }
}

pub(super) async fn read_verified_ref(
    repository_root: &Path,
    relative: &str,
    expected_digest: &str,
) -> Result<String, KnowledgeMapServiceError> {
    let path = resolve_contract_ref(repository_root, relative)?;
    let canonical_dir = ensure_contract_dir_is_scoped(
        repository_root,
        &repository_root.join(crate::project::AGENT_CONTRACT_DIR_NAME),
    )
    .await?;
    let canonical_path = fs::canonicalize(&path).await?;
    if !canonical_path.starts_with(&canonical_dir) {
        return Err(KnowledgeMapServiceError::UnsafePath(relative.to_owned()));
    }
    let content = fs::read_to_string(path).await?;
    if content_digest(content.as_bytes()) != expected_digest {
        return Err(KnowledgeMapServiceError::Integrity(format!(
            "digest mismatch for '{relative}'"
        )));
    }
    Ok(content)
}

pub(super) async fn publish_immutable(
    repository_root: &Path,
    relative: &str,
    content: &[u8],
) -> Result<(), KnowledgeMapServiceError> {
    let contract_dir = repository_root.join(crate::project::AGENT_CONTRACT_DIR_NAME);
    let path = resolve_contract_ref(repository_root, relative)?;
    ensure_artifact_parent_is_scoped(repository_root, &contract_dir, &path).await?;
    if fs::try_exists(&path).await? {
        let canonical_contract =
            ensure_contract_dir_is_scoped(repository_root, &contract_dir).await?;
        let canonical_path = fs::canonicalize(&path).await?;
        if !canonical_path.starts_with(canonical_contract) {
            return Err(KnowledgeMapServiceError::UnsafePath(relative.to_owned()));
        }
        let existing = fs::read(&path).await?;
        if existing == content {
            return Ok(());
        }
        return Err(KnowledgeMapServiceError::Integrity(format!(
            "immutable map artifact '{}' already exists with different content",
            path.display()
        )));
    }
    let temp = super::temporary_path(&path);
    fs::write(&temp, content).await?;
    match fs::rename(&temp, &path).await {
        Ok(()) => Ok(()),
        Err(error) if fs::try_exists(&path).await? => {
            let _ = fs::remove_file(&temp).await;
            let existing = fs::read(&path).await?;
            if existing == content {
                Ok(())
            } else {
                Err(KnowledgeMapServiceError::Io(error))
            }
        }
        Err(error) => Err(KnowledgeMapServiceError::Io(error)),
    }
}

pub(super) async fn cleanup_superseded_topic_shards(
    repository_root: &Path,
    backup: &Path,
    manifest: &KnowledgeMapManifest,
) {
    let mut retained: std::collections::HashSet<_> = manifest
        .topics
        .iter()
        .filter_map(|topic| Path::new(&topic.r#ref).file_name().map(ToOwned::to_owned))
        .collect();
    if fs::try_exists(backup).await.unwrap_or(true) {
        let Ok(content) = fs::read_to_string(backup).await else {
            return;
        };
        let Ok(probe) = serde_norway::from_str::<KnowledgeMapSchemaProbe>(&content) else {
            return;
        };
        if probe.schema_version == KnowledgeMap::SCHEMA_VERSION {
            let Ok(recovery) = parse_manifest(&content) else {
                return;
            };
            retained.extend(
                recovery
                    .topics
                    .iter()
                    .filter_map(|topic| Path::new(&topic.r#ref).file_name().map(ToOwned::to_owned)),
            );
        }
    }
    let topic_dir = repository_root
        .join(crate::project::AGENT_CONTRACT_DIR_NAME)
        .join(crate::project::KNOWLEDGE_MAP_TOPICS_DIR_NAME);
    if ensure_contract_dir_is_scoped(repository_root, &topic_dir)
        .await
        .is_err()
    {
        return;
    }
    let Ok(mut entries) = fs::read_dir(topic_dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        if !retained.contains(&entry.file_name()) {
            let _ = fs::remove_file(entry.path()).await;
        }
    }
}

fn resolve_contract_ref(
    repository_root: &Path,
    relative: &str,
) -> Result<PathBuf, KnowledgeMapServiceError> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
        || !(relative.starts_with(&format!(
            "{}/",
            crate::project::KNOWLEDGE_MAP_TOPICS_DIR_NAME
        )) || relative.starts_with(&format!(
            "{}/",
            crate::project::KNOWLEDGE_MAP_HISTORY_DIR_NAME
        )))
    {
        return Err(KnowledgeMapServiceError::UnsafePath(relative.to_owned()));
    }
    Ok(repository_root
        .join(crate::project::AGENT_CONTRACT_DIR_NAME)
        .join(relative_path))
}
