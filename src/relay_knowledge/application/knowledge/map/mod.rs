use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tokio::fs;
use tokio::time::{Duration, Instant, sleep};

use crate::{
    api::{ApiMetadata, RequestContext},
    domain::{
        KnowledgeMap, KnowledgeMapChange, KnowledgeMapRoute, KnowledgeMapSource,
        KnowledgeMapSourceKind,
    },
    project::{
        AGENT_CONTRACT_DIR_NAME, KNOWLEDGE_MAP_FILE_NAME, KNOWLEDGE_MAP_HISTORY_DIR_NAME,
        KNOWLEDGE_MAP_RELATIVE_PATH, KNOWLEDGE_MAP_TOPICS_DIR_NAME,
    },
};

mod artifact;

use artifact::*;

/// Request to register a source in the repository knowledge map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeMapSourceAddRequest {
    pub id: String,
    pub topic: String,
    pub kind: KnowledgeMapSourceKind,
    pub uri: String,
    pub source_scope: Option<String>,
    pub description: Option<String>,
}

/// Response shared by map mutation commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeMapMutationResponse {
    pub metadata: ApiMetadata,
    pub path: String,
    pub map_version: u64,
    pub summary: String,
}

/// Response returned by read-only map commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeMapShowResponse {
    pub metadata: ApiMetadata,
    pub path: String,
    pub map: KnowledgeMap,
}

/// Response returned by topic routing commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeMapRouteResponse {
    pub metadata: ApiMetadata,
    pub path: String,
    pub topic: String,
    pub route: Option<KnowledgeMapRoute>,
    pub sources: Vec<KnowledgeMapSource>,
}

/// Response returned by validation commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeMapValidationResponse {
    pub metadata: ApiMetadata,
    pub path: String,
    pub valid: bool,
    pub diagnostics: Vec<String>,
}

/// Response that contains the AGENTS.md reference snippet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeMapAgentSnippetResponse {
    pub metadata: ApiMetadata,
    pub snippet: String,
}

/// File-backed service for the shared YAML knowledge navigation contract.
pub struct KnowledgeMapService {
    repository_root: PathBuf,
}

impl KnowledgeMapService {
    pub fn new(repository_root: PathBuf) -> Self {
        Self { repository_root }
    }

    pub async fn init(
        &self,
        context: &RequestContext,
    ) -> Result<KnowledgeMapMutationResponse, KnowledgeMapServiceError> {
        let _lock = self.acquire_write_lock().await?;
        self.recover_manifest_backup().await?;
        let path = self.map_path();
        if fs::try_exists(&path).await? {
            let existing = fs::read_to_string(&path).await?;
            let legacy = serde_norway::from_str::<KnowledgeMapSchemaProbe>(&existing)
                .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?
                .schema_version
                == 1;
            let mut map = self.load_map().await?;
            if map.ensure_software_model_route()? {
                map.record_change(
                    "software-model.ensure",
                    "Added the repository code-map-backed software-model route.".to_owned(),
                    now_stamp(),
                );
                self.write_map(&map).await?;
                return Ok(self.mutation_response(
                    context,
                    map.map_version,
                    "initialized repository software-model route".to_owned(),
                ));
            }
            if legacy {
                self.write_map(&map).await?;
                return Ok(self.mutation_response(
                    context,
                    map.map_version,
                    "migrated knowledge map schema v1 to v2".to_owned(),
                ));
            }
            return Ok(self.mutation_response(
                context,
                map.map_version,
                "knowledge map and repository software-model route already exist".to_owned(),
            ));
        }

        let map = KnowledgeMap::initial(now_stamp());
        self.write_map(&map).await?;
        Ok(self.mutation_response(
            context,
            map.map_version,
            "created knowledge map with repository software-model route".to_owned(),
        ))
    }

    pub async fn show(
        &self,
        context: &RequestContext,
        topic: Option<String>,
    ) -> Result<KnowledgeMapShowResponse, KnowledgeMapServiceError> {
        let mut map = self.load_map().await?;
        if let Some(topic) = topic {
            map.sources.retain(|source| source.topic == topic);
            map.routes.retain(|route| route.topic == topic);
            map.topics.retain(|entry| entry.id == topic);
        }
        Ok(KnowledgeMapShowResponse {
            metadata: metadata(context),
            path: KNOWLEDGE_MAP_RELATIVE_PATH.to_owned(),
            map,
        })
    }

    pub async fn route(
        &self,
        context: &RequestContext,
        topic: String,
    ) -> Result<KnowledgeMapRouteResponse, KnowledgeMapServiceError> {
        let (route, available_sources) = self.load_topic_route(&topic).await?;
        let source_order = route
            .as_ref()
            .map(|route| route.source_order.as_slice())
            .unwrap_or(&[]);
        let sources = source_order
            .iter()
            .filter_map(|id| {
                available_sources
                    .iter()
                    .find(|source| &source.id == id)
                    .cloned()
            })
            .collect();

        Ok(KnowledgeMapRouteResponse {
            metadata: metadata(context),
            path: KNOWLEDGE_MAP_RELATIVE_PATH.to_owned(),
            topic,
            route,
            sources,
        })
    }

    pub async fn add_source(
        &self,
        context: &RequestContext,
        request: KnowledgeMapSourceAddRequest,
    ) -> Result<KnowledgeMapMutationResponse, KnowledgeMapServiceError> {
        let _lock = self.acquire_write_lock().await?;
        self.recover_manifest_backup().await?;
        let mut map = self.load_or_initial().await?;
        let id = request.id.clone();
        let topic = request.topic.clone();
        let source = KnowledgeMapSource::new(
            request.id,
            request.topic,
            request.kind,
            request.uri,
            request.source_scope,
            request.description,
        )?;
        map.add_source(source)?;
        map.record_change(
            "source.add",
            format!("Added source '{id}' to topic '{topic}'."),
            now_stamp(),
        );
        self.write_map(&map).await?;
        Ok(self.mutation_response(context, map.map_version, format!("added source {id}")))
    }

    pub async fn update_source(
        &self,
        context: &RequestContext,
        change: KnowledgeMapChange,
    ) -> Result<KnowledgeMapMutationResponse, KnowledgeMapServiceError> {
        let _lock = self.acquire_write_lock().await?;
        self.recover_manifest_backup().await?;
        let mut map = self.load_map().await?;
        let id = change.id.clone();
        map.update_source(change)?;
        map.record_change(
            "source.update",
            format!("Updated source '{id}'."),
            now_stamp(),
        );
        self.write_map(&map).await?;
        Ok(self.mutation_response(context, map.map_version, format!("updated source {id}")))
    }

    pub async fn remove_source(
        &self,
        context: &RequestContext,
        id: String,
    ) -> Result<KnowledgeMapMutationResponse, KnowledgeMapServiceError> {
        let _lock = self.acquire_write_lock().await?;
        self.recover_manifest_backup().await?;
        let mut map = self.load_map().await?;
        map.remove_source(&id)?;
        map.record_change(
            "source.remove",
            format!("Removed source '{id}'."),
            now_stamp(),
        );
        self.write_map(&map).await?;
        Ok(self.mutation_response(context, map.map_version, format!("removed source {id}")))
    }

    pub async fn validate(
        &self,
        context: &RequestContext,
    ) -> Result<KnowledgeMapValidationResponse, KnowledgeMapServiceError> {
        let mut diagnostics = Vec::new();
        match self.load_map().await {
            Ok(map) => {
                if let Err(error) = map.validate() {
                    diagnostics.push(error.to_string());
                }
            }
            Err(error) => diagnostics.push(error.to_string()),
        }

        let agents_path = self.repository_root.join("AGENTS.md");
        match fs::read_to_string(&agents_path).await {
            Ok(contents) if contents.contains(KNOWLEDGE_MAP_RELATIVE_PATH) => {}
            Ok(_) => diagnostics.push(format!(
                "AGENTS.md does not reference {KNOWLEDGE_MAP_RELATIVE_PATH}"
            )),
            Err(error) => diagnostics.push(format!("failed to read AGENTS.md: {error}")),
        }

        Ok(KnowledgeMapValidationResponse {
            metadata: metadata(context),
            path: KNOWLEDGE_MAP_RELATIVE_PATH.to_owned(),
            valid: diagnostics.is_empty(),
            diagnostics,
        })
    }

    pub fn agent_snippet(&self, context: &RequestContext) -> KnowledgeMapAgentSnippetResponse {
        KnowledgeMapAgentSnippetResponse {
            metadata: metadata(context),
            snippet: format!("Knowledge map: {KNOWLEDGE_MAP_RELATIVE_PATH}"),
        }
    }

    async fn load_or_initial(&self) -> Result<KnowledgeMap, KnowledgeMapServiceError> {
        let path = self.map_path();
        if fs::try_exists(&path).await? || fs::try_exists(self.backup_path()).await? {
            self.load_map().await
        } else {
            Ok(KnowledgeMap::initial(now_stamp()))
        }
    }

    async fn load_map(&self) -> Result<KnowledgeMap, KnowledgeMapServiceError> {
        let content = self.read_root_content().await?;
        let probe = serde_norway::from_str::<KnowledgeMapSchemaProbe>(&content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        match probe.schema_version {
            1 => {
                let mut map = serde_norway::from_str::<KnowledgeMap>(&content)
                    .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
                map.schema_version = KnowledgeMap::SCHEMA_VERSION;
                map.validate()?;
                Ok(map)
            }
            KnowledgeMap::SCHEMA_VERSION => self.load_v2_map(&content).await,
            version => Err(KnowledgeMapServiceError::Yaml(format!(
                "unsupported schema_version {version}"
            ))),
        }
    }

    async fn write_map(&self, map: &KnowledgeMap) -> Result<(), KnowledgeMapServiceError> {
        map.validate()?;
        let dir = self.repository_root.join(AGENT_CONTRACT_DIR_NAME);
        fs::create_dir_all(&dir).await?;
        let mut topic_refs = Vec::with_capacity(map.topics.len());
        for topic in &map.topics {
            let shard = KnowledgeMapTopicShard {
                schema_version: KnowledgeMap::SCHEMA_VERSION,
                topic: topic.clone(),
                sources: map
                    .sources
                    .iter()
                    .filter(|source| source.topic == topic.id)
                    .cloned()
                    .collect(),
                route: map
                    .routes
                    .iter()
                    .find(|route| route.topic == topic.id)
                    .cloned(),
            };
            let yaml = serialize_yaml(&shard)?;
            let digest = content_digest(yaml.as_bytes());
            let relative = format!(
                "{KNOWLEDGE_MAP_TOPICS_DIR_NAME}/topic-{}-{digest}.yaml",
                stable_id(&topic.id)
            );
            self.publish_immutable(&relative, yaml.as_bytes()).await?;
            topic_refs.push(KnowledgeMapTopicRef {
                id: topic.id.clone(),
                title: topic.title.clone(),
                description: topic.description.clone(),
                r#ref: relative,
                digest,
            });
        }

        let split =
            map.history.len().saturating_sub(1) / RECENT_HISTORY_LIMIT * RECENT_HISTORY_LIMIT;
        let archived = &map.history[..split];
        let mut archive_ref = None;
        for chunk in archived.chunks(RECENT_HISTORY_LIMIT) {
            let archive = KnowledgeMapHistoryArchive {
                schema_version: KnowledgeMap::SCHEMA_VERSION,
                from_version: chunk.first().expect("non-empty archive chunk").version,
                through_version: chunk.last().expect("non-empty archive chunk").version,
                previous: archive_ref,
                entries: chunk.to_vec(),
            };
            let yaml = serialize_yaml(&archive)?;
            let digest = content_digest(yaml.as_bytes());
            let relative = format!(
                "{KNOWLEDGE_MAP_HISTORY_DIR_NAME}/{:020}-{:020}-{digest}.yaml",
                archive.from_version, archive.through_version
            );
            self.publish_immutable(&relative, yaml.as_bytes()).await?;
            archive_ref = Some(KnowledgeMapArchiveRef {
                r#ref: relative,
                digest,
            });
        }
        let manifest = KnowledgeMapManifest {
            schema_version: KnowledgeMap::SCHEMA_VERSION,
            map_version: map.map_version,
            updated_at: map.updated_at.clone(),
            topics: topic_refs,
            history: KnowledgeMapHistoryManifest {
                archived_through: archived.last().map(|entry| entry.version).unwrap_or(0),
                archive: archive_ref,
                recent: map.history[split..].to_vec(),
            },
        };
        self.publish_manifest(serialize_yaml(&manifest)?.as_bytes())
            .await
    }

    async fn load_v2_map(&self, content: &str) -> Result<KnowledgeMap, KnowledgeMapServiceError> {
        let manifest = parse_manifest(content)?;
        let mut topics = Vec::with_capacity(manifest.topics.len());
        let mut sources = Vec::new();
        let mut routes = Vec::new();
        for topic_ref in &manifest.topics {
            let shard = self.load_topic_shard(topic_ref).await?;
            topics.push(shard.topic);
            sources.extend(shard.sources);
            if let Some(route) = shard.route {
                routes.push(route);
            }
        }
        let mut history = self.load_archived_history(&manifest.history).await?;
        history.extend(manifest.history.recent);
        let map = KnowledgeMap {
            schema_version: KnowledgeMap::SCHEMA_VERSION,
            map_version: manifest.map_version,
            updated_at: manifest.updated_at,
            topics,
            sources,
            routes,
            history,
        };
        map.validate()?;
        Ok(map)
    }

    async fn load_topic_route(
        &self,
        topic: &str,
    ) -> Result<(Option<KnowledgeMapRoute>, Vec<KnowledgeMapSource>), KnowledgeMapServiceError>
    {
        let content = self.read_root_content().await?;
        let probe = serde_norway::from_str::<KnowledgeMapSchemaProbe>(&content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        if probe.schema_version == 1 {
            let map = self.load_map().await?;
            return Ok((
                map.routes
                    .iter()
                    .find(|route| route.topic == topic)
                    .cloned(),
                map.sources
                    .into_iter()
                    .filter(|source| source.topic == topic)
                    .collect(),
            ));
        }
        let manifest = parse_manifest(&content)?;
        validate_recent_history(&manifest)?;
        let Some(topic_ref) = manifest.topics.iter().find(|entry| entry.id == topic) else {
            return Ok((None, Vec::new()));
        };
        let shard = self.load_topic_shard(topic_ref).await?;
        Ok((shard.route, shard.sources))
    }

    async fn load_topic_shard(
        &self,
        topic_ref: &KnowledgeMapTopicRef,
    ) -> Result<KnowledgeMapTopicShard, KnowledgeMapServiceError> {
        let content = self
            .read_verified_ref(&topic_ref.r#ref, &topic_ref.digest)
            .await?;
        let shard = serde_norway::from_str::<KnowledgeMapTopicShard>(&content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        if !topic_ref
            .r#ref
            .starts_with(&format!("{KNOWLEDGE_MAP_TOPICS_DIR_NAME}/"))
            || shard.schema_version != KnowledgeMap::SCHEMA_VERSION
            || shard.topic.id != topic_ref.id
            || shard.topic.title != topic_ref.title
            || shard.topic.description != topic_ref.description
        {
            return Err(KnowledgeMapServiceError::Integrity(format!(
                "topic shard '{}' identity, metadata, or schema does not match the manifest",
                topic_ref.r#ref
            )));
        }
        validate_topic_shard(&shard)?;
        Ok(shard)
    }

    async fn load_archived_history(
        &self,
        history: &KnowledgeMapHistoryManifest,
    ) -> Result<Vec<crate::domain::KnowledgeMapHistoryEntry>, KnowledgeMapServiceError> {
        let Some(mut archive_ref) = history.archive.clone() else {
            if history.archived_through != 0 {
                return Err(KnowledgeMapServiceError::Integrity(
                    "history archive is missing for a non-zero checkpoint".to_owned(),
                ));
            }
            return Ok(Vec::new());
        };
        let mut expected_through = history.archived_through;
        let mut chunks = Vec::new();
        let mut visited = std::collections::HashSet::new();
        loop {
            if !archive_ref
                .r#ref
                .starts_with(&format!("{KNOWLEDGE_MAP_HISTORY_DIR_NAME}/"))
                || !visited.insert(archive_ref.r#ref.clone())
            {
                return Err(KnowledgeMapServiceError::Integrity(
                    "history archive chain has an unsafe ref or cycle".to_owned(),
                ));
            }
            let content = self
                .read_verified_ref(&archive_ref.r#ref, &archive_ref.digest)
                .await?;
            let archive = serde_norway::from_str::<KnowledgeMapHistoryArchive>(&content)
                .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
            if archive.schema_version != KnowledgeMap::SCHEMA_VERSION
                || archive.through_version != expected_through
                || archive.entries.is_empty()
                || archive.entries.len() > RECENT_HISTORY_LIMIT
                || archive.entries.first().map(|entry| entry.version) != Some(archive.from_version)
                || archive.entries.last().map(|entry| entry.version)
                    != Some(archive.through_version)
            {
                return Err(KnowledgeMapServiceError::Integrity(format!(
                    "history archive '{}' does not match its checkpoint",
                    archive_ref.r#ref
                )));
            }
            expected_through = archive.from_version.saturating_sub(1);
            chunks.push(archive.entries);
            match archive.previous {
                Some(previous) => archive_ref = previous,
                None if expected_through == 0 => break,
                None => {
                    return Err(KnowledgeMapServiceError::Integrity(
                        "history archive chain ends before version 1".to_owned(),
                    ));
                }
            }
        }
        chunks.reverse();
        Ok(chunks.into_iter().flatten().collect())
    }

    async fn read_verified_ref(
        &self,
        relative: &str,
        expected_digest: &str,
    ) -> Result<String, KnowledgeMapServiceError> {
        let path = self.resolve_contract_ref(relative)?;
        let canonical_dir = ensure_contract_dir_is_scoped(
            &self.repository_root,
            &self.repository_root.join(AGENT_CONTRACT_DIR_NAME),
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

    async fn publish_immutable(
        &self,
        relative: &str,
        content: &[u8],
    ) -> Result<(), KnowledgeMapServiceError> {
        let path = self.resolve_contract_ref(relative)?;
        ensure_artifact_parent_is_scoped(
            &self.repository_root,
            &self.repository_root.join(AGENT_CONTRACT_DIR_NAME),
            &path,
        )
        .await?;
        if fs::try_exists(&path).await? {
            let existing = fs::read(&path).await?;
            if existing == content {
                return Ok(());
            }
            return Err(KnowledgeMapServiceError::Integrity(format!(
                "immutable map artifact '{}' already exists with different content",
                path.display()
            )));
        }
        let temp = temporary_path(&path);
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

    async fn publish_manifest(&self, content: &[u8]) -> Result<(), KnowledgeMapServiceError> {
        let path = self.map_path();
        let temp = temporary_path(&path);
        let backup = self.backup_path();
        fs::write(&temp, content).await?;
        let existed = fs::try_exists(&path).await?;
        if existed {
            if fs::try_exists(&backup).await? {
                fs::remove_file(&backup).await?;
            }
            fs::rename(&path, &backup).await?;
        }
        if let Err(error) = fs::rename(&temp, &path).await {
            if existed {
                let _ = fs::rename(&backup, &path).await;
            }
            return Err(KnowledgeMapServiceError::Io(error));
        }
        if existed {
            fs::remove_file(backup).await?;
        }
        Ok(())
    }

    async fn read_root_content(&self) -> Result<String, KnowledgeMapServiceError> {
        let path = self.map_path();
        if fs::try_exists(&path).await? {
            return Ok(fs::read_to_string(path).await?);
        }
        Ok(fs::read_to_string(self.backup_path()).await?)
    }

    async fn recover_manifest_backup(&self) -> Result<(), KnowledgeMapServiceError> {
        let path = self.map_path();
        let backup = self.backup_path();
        if !fs::try_exists(&path).await? && fs::try_exists(&backup).await? {
            fs::rename(backup, path).await?;
        }
        Ok(())
    }

    fn resolve_contract_ref(&self, relative: &str) -> Result<PathBuf, KnowledgeMapServiceError> {
        let relative_path = Path::new(relative);
        if relative_path.is_absolute()
            || relative_path
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
            || !(relative.starts_with(&format!("{KNOWLEDGE_MAP_TOPICS_DIR_NAME}/"))
                || relative.starts_with(&format!("{KNOWLEDGE_MAP_HISTORY_DIR_NAME}/")))
        {
            return Err(KnowledgeMapServiceError::UnsafePath(relative.to_owned()));
        }
        Ok(self
            .repository_root
            .join(AGENT_CONTRACT_DIR_NAME)
            .join(relative_path))
    }

    async fn acquire_write_lock(&self) -> Result<KnowledgeMapWriteLock, KnowledgeMapServiceError> {
        let dir = self.repository_root.join(AGENT_CONTRACT_DIR_NAME);
        ensure_contract_dir_is_scoped(&self.repository_root, &dir).await?;
        let path = dir.join(format!("{KNOWLEDGE_MAP_FILE_NAME}.lock"));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .await
            {
                Ok(_) => return Ok(KnowledgeMapWriteLock { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if Instant::now() >= deadline {
                        return Err(KnowledgeMapServiceError::LockTimeout(path));
                    }
                    sleep(Duration::from_millis(25)).await;
                }
                Err(error) => return Err(KnowledgeMapServiceError::Io(error)),
            }
        }
    }

    fn mutation_response(
        &self,
        context: &RequestContext,
        map_version: u64,
        summary: String,
    ) -> KnowledgeMapMutationResponse {
        KnowledgeMapMutationResponse {
            metadata: metadata(context),
            path: KNOWLEDGE_MAP_RELATIVE_PATH.to_owned(),
            map_version,
            summary,
        }
    }

    fn map_path(&self) -> PathBuf {
        self.repository_root
            .join(Path::new(AGENT_CONTRACT_DIR_NAME))
            .join(KNOWLEDGE_MAP_FILE_NAME)
    }

    fn backup_path(&self) -> PathBuf {
        self.map_path().with_extension("yaml.previous")
    }
}

/// Error surfaced by the file-backed knowledge map service.
#[derive(Debug)]
pub enum KnowledgeMapServiceError {
    Io(std::io::Error),
    Yaml(String),
    Domain(crate::domain::DomainError),
    LockTimeout(PathBuf),
    Integrity(String),
    UnsafePath(String),
}

impl fmt::Display for KnowledgeMapServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Yaml(error) => write!(formatter, "invalid knowledge map YAML: {error}"),
            Self::Domain(error) => write!(formatter, "{error}"),
            Self::LockTimeout(path) => write!(
                formatter,
                "timed out waiting for knowledge map write lock '{}'",
                path.display()
            ),
            Self::Integrity(message) => write!(formatter, "invalid knowledge map: {message}"),
            Self::UnsafePath(path) => {
                write!(formatter, "unsafe knowledge map artifact path '{path}'")
            }
        }
    }
}

impl Error for KnowledgeMapServiceError {}

impl From<std::io::Error> for KnowledgeMapServiceError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<crate::domain::DomainError> for KnowledgeMapServiceError {
    fn from(error: crate::domain::DomainError) -> Self {
        Self::Domain(error)
    }
}

fn metadata(context: &RequestContext) -> ApiMetadata {
    ApiMetadata::graph_only(context, crate::domain::GraphVersion::ZERO)
}

fn temporary_path(path: &Path) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    path.with_extension(format!("{}.{}.tmp", std::process::id(), suffix))
}

struct KnowledgeMapWriteLock {
    path: PathBuf,
}

impl Drop for KnowledgeMapWriteLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn now_stamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format!("unix:{seconds}")
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
