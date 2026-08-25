//! Knowledge Map v2 manifest, topic-shard, and history-archive file contracts.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::domain::{
    KnowledgeMap, KnowledgeMapHistoryEntry, KnowledgeMapRoute, KnowledgeMapSource,
    KnowledgeMapTopic,
};

use super::error::KnowledgeMapServiceError;

pub(super) const RECENT_HISTORY_LIMIT: usize = 16;
pub(super) const ARTIFACT_SCHEMA_VERSION: u16 = 2;
pub(super) const HISTORY_INDEX_FANOUT: usize = 64;
pub(super) const HISTORY_INDEX_MAX_HEIGHT: u8 = 10;

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

impl KnowledgeMapManifest {
    pub(super) fn referenced_topic_files(&self) -> std::collections::HashSet<std::ffi::OsString> {
        self.topics
            .iter()
            .filter_map(|topic| Path::new(&topic.r#ref).file_name().map(ToOwned::to_owned))
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct KnowledgeMapTopicRef {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) description: String,
    pub(super) source_ids: Vec<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) index: Option<KnowledgeMapHistoryIndexRef>,
    pub(super) recent: Vec<KnowledgeMapHistoryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct KnowledgeMapArchiveRef {
    pub(super) r#ref: String,
    pub(super) digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct KnowledgeMapHistoryIndexRef {
    pub(super) from_version: u64,
    pub(super) through_version: u64,
    pub(super) height: u8,
    pub(super) r#ref: String,
    pub(super) digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct KnowledgeMapHistoryIndexNode {
    pub(super) schema_version: u16,
    pub(super) from_version: u64,
    pub(super) through_version: u64,
    pub(super) height: u8,
    pub(super) entries: Vec<KnowledgeMapHistoryIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct KnowledgeMapHistoryIndexEntry {
    pub(super) from_version: u64,
    pub(super) through_version: u64,
    #[serde(flatten)]
    pub(super) target: KnowledgeMapHistoryIndexTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum KnowledgeMapHistoryIndexTarget {
    Archive {
        r#ref: String,
        digest: String,
    },
    Node {
        height: u8,
        r#ref: String,
        digest: String,
    },
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
    if manifest.schema_version != ARTIFACT_SCHEMA_VERSION || manifest.map_version == 0 {
        return Err(KnowledgeMapServiceError::Integrity(
            "manifest schema_version or map_version is invalid".to_owned(),
        ));
    }
    let mut topic_ids = std::collections::HashSet::new();
    let mut folded_topic_ids = std::collections::HashSet::new();
    let mut refs = std::collections::HashSet::new();
    let mut folded_refs = std::collections::HashSet::new();
    let mut source_ids = std::collections::HashSet::new();
    for topic in &manifest.topics {
        let mut topic_source_ids = std::collections::HashSet::new();
        let expected_ref = format!(
            "{}/topic-{}-{}.yaml",
            crate::project::KNOWLEDGE_MAP_TOPICS_DIR_NAME,
            stable_id(&topic.id),
            topic.digest
        );
        if topic.id.trim().is_empty()
            || topic.title.trim().is_empty()
            || topic.description.trim().is_empty()
            || !is_lower_hex_digest(&topic.digest)
            || topic.r#ref != expected_ref
            || !topic_ids.insert(topic.id.as_str())
            || !refs.insert(topic.r#ref.as_str())
            || !folded_topic_ids.insert(topic.id.to_lowercase())
            || !folded_refs.insert(topic.r#ref.to_lowercase())
            || topic.source_ids.iter().any(|source_id| {
                source_id.trim().is_empty()
                    || !topic_source_ids.insert(source_id.as_str())
                    || !source_ids.insert(source_id.as_str())
            })
        {
            return Err(KnowledgeMapServiceError::Integrity(
                "topic metadata, content refs, and source ids must be valid and globally unique"
                    .to_owned(),
            ));
        }
    }
    validate_recent_history(&manifest)?;
    if let Some(archive) = &manifest.history.archive {
        if !is_scoped_contract_ref(
            &archive.r#ref,
            crate::project::KNOWLEDGE_MAP_HISTORY_DIR_NAME,
        ) || !is_lower_hex_digest(&archive.digest)
        {
            return Err(KnowledgeMapServiceError::Integrity(
                "history archive ref or digest is invalid".to_owned(),
            ));
        }
    }
    if manifest.history.archived_through == 0 && manifest.history.index.is_some() {
        return Err(KnowledgeMapServiceError::Integrity(
            "history index must be absent when no history is archived".to_owned(),
        ));
    }
    if let Some(index) = &manifest.history.index {
        validate_history_index_ref_shape(index, manifest.history.archived_through)?;
        if index.from_version != 1 {
            return Err(KnowledgeMapServiceError::Integrity(
                "history index root must begin at version 1".to_owned(),
            ));
        }
    }
    Ok(manifest)
}

pub(super) fn validate_history_index_ref_shape(
    index: &KnowledgeMapHistoryIndexRef,
    archived_through: u64,
) -> Result<(), KnowledgeMapServiceError> {
    let expected_ref = format!(
        "{}/index-{:02}-{:020}-{:020}-{}.yaml",
        crate::project::KNOWLEDGE_MAP_HISTORY_DIR_NAME,
        index.height,
        index.from_version,
        index.through_version,
        index.digest
    );
    if index.from_version == 0
        || index.from_version > index.through_version
        || index.through_version != archived_through
        || index.height > HISTORY_INDEX_MAX_HEIGHT
        || !is_lower_hex_digest(&index.digest)
        || index.r#ref != expected_ref
    {
        return Err(KnowledgeMapServiceError::Integrity(
            "history index root ref is invalid".to_owned(),
        ));
    }
    Ok(())
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
    let mut expected = manifest
        .history
        .archived_through
        .checked_add(1)
        .ok_or_else(|| {
            KnowledgeMapServiceError::Integrity("history version overflow".to_owned())
        })?;
    for entry in &manifest.history.recent {
        entry
            .validate()
            .map_err(|error| KnowledgeMapServiceError::Integrity(error.to_string()))?;
        if entry.version != expected {
            return Err(KnowledgeMapServiceError::Integrity(
                "recent history is not contiguous with its archive checkpoint".to_owned(),
            ));
        }
        expected = expected.checked_add(1).ok_or_else(|| {
            KnowledgeMapServiceError::Integrity("history version overflow".to_owned())
        })?;
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
    if let Some(archive) = &manifest.history.archive {
        validate_archive_ref_shape(archive, manifest.history.archived_through)?;
    }
    Ok(())
}

fn validate_archive_ref_shape(
    archive: &KnowledgeMapArchiveRef,
    archived_through: u64,
) -> Result<(), KnowledgeMapServiceError> {
    let name = archive
        .r#ref
        .strip_prefix(&format!(
            "{}/",
            crate::project::KNOWLEDGE_MAP_HISTORY_DIR_NAME
        ))
        .and_then(|value| value.strip_suffix(".yaml"));
    let Some(name) = name else {
        return Err(KnowledgeMapServiceError::Integrity(
            "history archive ref is not content addressed".to_owned(),
        ));
    };
    let mut parts = name.split('-');
    let from_text = parts.next();
    let through_text = parts.next();
    let from = from_text.and_then(|value| value.parse::<u64>().ok());
    let through = through_text.and_then(|value| value.parse::<u64>().ok());
    let digest = parts.next();
    if parts.next().is_some()
        || from.is_none_or(|value| value == 0)
        || from_text.is_none_or(|value| value.len() != 20)
        || through_text.is_none_or(|value| value.len() != 20)
        || through != Some(archived_through)
        || from.is_some_and(|value| value > archived_through)
        || digest != Some(archive.digest.as_str())
    {
        return Err(KnowledgeMapServiceError::Integrity(
            "history archive ref is not content addressed for its checkpoint".to_owned(),
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

pub(super) async fn read_root_file(
    repository_root: &Path,
    path: &Path,
) -> Result<String, KnowledgeMapServiceError> {
    let contract = path
        .parent()
        .ok_or_else(|| KnowledgeMapServiceError::UnsafePath(path.display().to_string()))?;
    reject_symlink(contract).await?;
    reject_symlink(path).await?;
    let repository = fs::canonicalize(repository_root).await?;
    let contract = fs::canonicalize(contract).await?;
    let root = fs::canonicalize(path).await?;
    if !contract.starts_with(repository) || !root.starts_with(contract) {
        return Err(KnowledgeMapServiceError::UnsafePath(
            path.display().to_string(),
        ));
    }
    Ok(fs::read_to_string(path).await?)
}

pub(super) async fn reject_symlink(path: &Path) -> Result<(), KnowledgeMapServiceError> {
    if fs::symlink_metadata(path).await?.file_type().is_symlink() {
        return Err(KnowledgeMapServiceError::UnsafePath(
            path.display().to_string(),
        ));
    }
    Ok(())
}

pub(super) async fn canonical_regular_file(
    path: &Path,
) -> Result<PathBuf, KnowledgeMapServiceError> {
    reject_symlink(path).await?;
    Ok(fs::canonicalize(path).await?)
}

pub(super) async fn ensure_regular_file_within(
    path: &Path,
    directory: &Path,
) -> Result<(), KnowledgeMapServiceError> {
    if !canonical_regular_file(path).await?.starts_with(directory) {
        return Err(unsafe_path(path));
    }
    Ok(())
}

pub(super) fn unsafe_path(path: &Path) -> KnowledgeMapServiceError {
    KnowledgeMapServiceError::UnsafePath(path.display().to_string())
}

pub(super) fn is_generated_topic_shard_name(file_name: &std::ffi::OsStr) -> bool {
    let Some(stem) = file_name
        .to_str()
        .and_then(|name| name.strip_prefix("topic-"))
        .and_then(|name| name.strip_suffix(".yaml"))
    else {
        return false;
    };
    let Some((topic_id, digest)) = stem.split_once('-') else {
        return false;
    };
    topic_id.len() == 16 && topic_id.bytes().all(is_lower_hex_byte) && is_lower_hex_digest(digest)
}

fn is_lower_hex_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(is_lower_hex_byte)
}

fn is_lower_hex_byte(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn is_scoped_contract_ref(relative: &str, directory: &str) -> bool {
    let path = Path::new(relative);
    let mut components = path.components();
    !path.is_absolute()
        && matches!(components.next(), Some(std::path::Component::Normal(value)) if value == directory)
        && matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

pub(super) async fn read_verified_ref(
    repository_root: &Path,
    relative: &str,
    expected_digest: &str,
) -> Result<String, KnowledgeMapServiceError> {
    let path = resolve_contract_ref(repository_root, relative)?;
    match fs::symlink_metadata(&path).await {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(KnowledgeMapServiceError::UnsafePath(relative.to_owned()));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(KnowledgeMapServiceError::MissingArtifact {
                path: relative.to_owned(),
                expected_digest: expected_digest.to_owned(),
            });
        }
        Err(error) => return Err(error.into()),
    }
    let canonical_repository = fs::canonicalize(repository_root).await?;
    let canonical_contract =
        fs::canonicalize(repository_root.join(crate::project::AGENT_CONTRACT_DIR_NAME)).await?;
    let artifact_dir = if relative.starts_with(&format!(
        "{}/",
        crate::project::KNOWLEDGE_MAP_TOPICS_DIR_NAME
    )) {
        repository_root
            .join(crate::project::AGENT_CONTRACT_DIR_NAME)
            .join(crate::project::KNOWLEDGE_MAP_TOPICS_DIR_NAME)
    } else {
        repository_root
            .join(crate::project::AGENT_CONTRACT_DIR_NAME)
            .join(crate::project::KNOWLEDGE_MAP_HISTORY_DIR_NAME)
    };
    reject_symlink(&artifact_dir).await?;
    let canonical_artifact_dir = fs::canonicalize(artifact_dir).await?;
    let canonical_path = canonical_regular_file(&path).await?;
    if !canonical_contract.starts_with(canonical_repository)
        || !canonical_artifact_dir.starts_with(&canonical_contract)
        || !canonical_path.starts_with(&canonical_artifact_dir)
    {
        return Err(KnowledgeMapServiceError::UnsafePath(relative.to_owned()));
    }
    let content = fs::read_to_string(path).await?;
    let actual_digest = content_digest(content.as_bytes());
    if actual_digest != expected_digest {
        return Err(KnowledgeMapServiceError::Integrity(format!(
            "digest mismatch for '{relative}': expected {expected_digest}, found {actual_digest}"
        )));
    }
    Ok(content)
}

pub(super) fn resolve_contract_ref(
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
