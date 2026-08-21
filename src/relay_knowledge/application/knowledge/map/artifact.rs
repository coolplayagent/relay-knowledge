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

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct KnowledgeMapManifest {
    pub(super) schema_version: u16,
    pub(super) map_version: u64,
    pub(super) updated_at: String,
    pub(super) topics: Vec<KnowledgeMapTopicRef>,
    pub(super) history: KnowledgeMapHistoryManifest,
}

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize)]
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
    let Some(route) = &shard.route else {
        if shard.sources.is_empty() {
            return Ok(());
        }
        return Err(KnowledgeMapServiceError::Integrity(format!(
            "topic shard '{}' has sources without a route",
            shard.topic.id
        )));
    };
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
