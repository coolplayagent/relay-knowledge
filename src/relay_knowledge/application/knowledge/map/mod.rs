use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::fs;
use tokio::time::Duration;
#[cfg(test)]
use tokio::time::sleep;

use crate::{
    api::RequestContext,
    domain::{KnowledgeMap, KnowledgeMapChange, KnowledgeMapRoute, KnowledgeMapSource},
    project::{
        AGENT_CONTRACT_DIR_NAME, KNOWLEDGE_MAP_FILE_NAME, KNOWLEDGE_MAP_HISTORY_DIR_NAME,
        KNOWLEDGE_MAP_RELATIVE_PATH, KNOWLEDGE_MAP_TOPICS_DIR_NAME,
    },
};

mod artifact;
mod contracts;
mod error;
mod history;
mod lock;

#[cfg(test)]
use history::MAX_HISTORY_PAGE_SIZE;
#[cfg(test)]
use lock::{ADVISORY_LOCK_MARKER, cleanup_transition_locks, transition_lock_prepared_path};

use artifact::*;
pub use contracts::{
    KnowledgeMapAgentSnippetResponse, KnowledgeMapHistoryResponse, KnowledgeMapHistoryWindow,
    KnowledgeMapMutationResponse, KnowledgeMapRouteResponse, KnowledgeMapShowResponse,
    KnowledgeMapSourceAddRequest, KnowledgeMapValidationResponse, KnowledgeMapView,
};
use contracts::{MutableKnowledgeMap, metadata, now_stamp};
pub use error::KnowledgeMapServiceError;

const WRITE_LOCK_TIMEOUT: Duration = Duration::from_secs(10);

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
        let _lock = self.acquire_write_lock(WRITE_LOCK_TIMEOUT).await?;
        self.recover_manifest_backup().await?;
        let path = self.map_path();
        if fs::try_exists(&path).await? {
            let existing = fs::read_to_string(&path).await?;
            let legacy = serde_norway::from_str::<KnowledgeMapSchemaProbe>(&existing)
                .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?
                .schema_version
                == 1;
            let mut snapshot = self.load_for_mutation().await?;
            if snapshot
                .map
                .ensure_software_model_route_snapshot(snapshot.archived_through)?
            {
                snapshot.map.record_change(
                    "software-model.ensure",
                    "Added the repository code-map-backed software-model route.".to_owned(),
                    now_stamp(),
                );
                self.write_map(&mut snapshot).await?;
                return Ok(self.mutation_response(
                    context,
                    snapshot.map.map_version,
                    "initialized repository software-model route".to_owned(),
                ));
            }
            if legacy {
                self.write_map(&mut snapshot).await?;
                return Ok(self.mutation_response(
                    context,
                    snapshot.map.map_version,
                    "migrated knowledge map schema v1 to v2".to_owned(),
                ));
            }
            return Ok(self.mutation_response(
                context,
                snapshot.map.map_version,
                "knowledge map and repository software-model route already exist".to_owned(),
            ));
        }

        let mut snapshot = MutableKnowledgeMap::initial(now_stamp());
        self.write_map(&mut snapshot).await?;
        Ok(self.mutation_response(
            context,
            snapshot.map.map_version,
            "created knowledge map with repository software-model route".to_owned(),
        ))
    }

    pub async fn show(
        &self,
        context: &RequestContext,
        topic: Option<String>,
    ) -> Result<KnowledgeMapShowResponse, KnowledgeMapServiceError> {
        let mut map = self.load_show_view().await?;
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
        let _lock = self.acquire_write_lock(WRITE_LOCK_TIMEOUT).await?;
        self.recover_manifest_backup().await?;
        let mut snapshot = self.load_or_initial().await?;
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
        snapshot
            .map
            .add_source_snapshot(source, snapshot.archived_through)?;
        snapshot.map.record_change(
            "source.add",
            format!("Added source '{id}' to topic '{topic}'."),
            now_stamp(),
        );
        self.write_map(&mut snapshot).await?;
        Ok(self.mutation_response(
            context,
            snapshot.map.map_version,
            format!("added source {id}"),
        ))
    }

    pub async fn update_source(
        &self,
        context: &RequestContext,
        change: KnowledgeMapChange,
    ) -> Result<KnowledgeMapMutationResponse, KnowledgeMapServiceError> {
        let _lock = self.acquire_write_lock(WRITE_LOCK_TIMEOUT).await?;
        self.recover_manifest_backup().await?;
        let mut snapshot = self.load_for_mutation().await?;
        let id = change.id.clone();
        snapshot
            .map
            .update_source_snapshot(change, snapshot.archived_through)?;
        snapshot.map.record_change(
            "source.update",
            format!("Updated source '{id}'."),
            now_stamp(),
        );
        self.write_map(&mut snapshot).await?;
        Ok(self.mutation_response(
            context,
            snapshot.map.map_version,
            format!("updated source {id}"),
        ))
    }

    pub async fn remove_source(
        &self,
        context: &RequestContext,
        id: String,
    ) -> Result<KnowledgeMapMutationResponse, KnowledgeMapServiceError> {
        let _lock = self.acquire_write_lock(WRITE_LOCK_TIMEOUT).await?;
        self.recover_manifest_backup().await?;
        let mut snapshot = self.load_for_mutation().await?;
        snapshot
            .map
            .remove_source_snapshot(&id, snapshot.archived_through)?;
        snapshot.map.record_change(
            "source.remove",
            format!("Removed source '{id}'."),
            now_stamp(),
        );
        self.write_map(&mut snapshot).await?;
        Ok(self.mutation_response(
            context,
            snapshot.map.map_version,
            format!("removed source {id}"),
        ))
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

    async fn load_or_initial(&self) -> Result<MutableKnowledgeMap, KnowledgeMapServiceError> {
        let path = self.map_path();
        if fs::try_exists(&path).await? || fs::try_exists(self.backup_path()).await? {
            self.load_for_mutation().await
        } else {
            Ok(MutableKnowledgeMap::initial(now_stamp()))
        }
    }

    async fn load_for_mutation(&self) -> Result<MutableKnowledgeMap, KnowledgeMapServiceError> {
        let content = self.read_root_content().await?;
        let probe = serde_norway::from_str::<KnowledgeMapSchemaProbe>(&content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        if probe.schema_version == 1 {
            let mut map = serde_norway::from_str::<KnowledgeMap>(&content)
                .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
            map.schema_version = KnowledgeMap::SCHEMA_VERSION;
            map.validate()?;
            return Ok(MutableKnowledgeMap {
                map,
                archived_through: 0,
                archive: None,
            });
        }
        if probe.schema_version != ARTIFACT_SCHEMA_VERSION {
            return Err(KnowledgeMapServiceError::Yaml(format!(
                "unsupported schema_version {}",
                probe.schema_version
            )));
        }
        let manifest = parse_manifest(&content)?;
        let archived_history = self.load_archived_history(&manifest.history).await?;
        let mut topics = Vec::with_capacity(manifest.topics.len());
        let mut sources = Vec::new();
        let mut routes = Vec::new();
        for topic_ref in &manifest.topics {
            let shard = self.load_topic_shard(topic_ref).await?;
            topics.push(shard.topic);
            sources.extend(shard.sources);
            routes.extend(shard.route);
        }
        let map = KnowledgeMap {
            schema_version: KnowledgeMap::SCHEMA_VERSION,
            map_version: manifest.map_version,
            updated_at: manifest.updated_at,
            topics,
            sources,
            routes,
            history: manifest.history.recent,
        };
        map.validate_snapshot(manifest.history.archived_through)?;
        let mut validation_map = map.clone();
        let mut complete_history = archived_history;
        complete_history.extend(validation_map.history.clone());
        validation_map.history = complete_history;
        validation_map.validate()?;
        Ok(MutableKnowledgeMap {
            map,
            archived_through: manifest.history.archived_through,
            archive: manifest.history.archive,
        })
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
            ARTIFACT_SCHEMA_VERSION => self.load_v2_map(&content).await,
            version => Err(KnowledgeMapServiceError::Yaml(format!(
                "unsupported schema_version {version}"
            ))),
        }
    }

    async fn load_show_view(&self) -> Result<KnowledgeMapView, KnowledgeMapServiceError> {
        let content = self.read_root_content().await?;
        let probe = serde_norway::from_str::<KnowledgeMapSchemaProbe>(&content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        if probe.schema_version == KnowledgeMap::SCHEMA_VERSION {
            let mut map = serde_norway::from_str::<KnowledgeMap>(&content)
                .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
            map.validate()?;
            let omitted = map.history.len().saturating_sub(RECENT_HISTORY_LIMIT);
            let recent = map.history.split_off(omitted);
            let archived_through = recent
                .first()
                .map_or(0, |entry| entry.version.saturating_sub(1));
            return Ok(KnowledgeMapView {
                artifact_schema_version: KnowledgeMap::SCHEMA_VERSION,
                map_version: map.map_version,
                updated_at: map.updated_at,
                topics: map.topics,
                sources: map.sources,
                routes: map.routes,
                history: KnowledgeMapHistoryWindow {
                    archived_through,
                    complete: omitted == 0,
                    recent,
                },
            });
        }
        if probe.schema_version != ARTIFACT_SCHEMA_VERSION {
            return Err(KnowledgeMapServiceError::Yaml(format!(
                "unsupported schema_version {}",
                probe.schema_version
            )));
        }
        let manifest = parse_manifest(&content)?;
        let mut topics = Vec::with_capacity(manifest.topics.len());
        let mut sources = Vec::new();
        let mut routes = Vec::new();
        for topic_ref in &manifest.topics {
            let shard = self.load_topic_shard(topic_ref).await?;
            topics.push(shard.topic);
            sources.extend(shard.sources);
            routes.extend(shard.route);
        }
        Ok(KnowledgeMapView {
            artifact_schema_version: ARTIFACT_SCHEMA_VERSION,
            map_version: manifest.map_version,
            updated_at: manifest.updated_at,
            topics,
            sources,
            routes,
            history: KnowledgeMapHistoryWindow {
                archived_through: manifest.history.archived_through,
                complete: manifest.history.archived_through == 0,
                recent: manifest.history.recent,
            },
        })
    }

    async fn write_map(
        &self,
        snapshot: &mut MutableKnowledgeMap,
    ) -> Result<(), KnowledgeMapServiceError> {
        snapshot.map.validate_snapshot(snapshot.archived_through)?;
        let dir = self.repository_root.join(AGENT_CONTRACT_DIR_NAME);
        fs::create_dir_all(&dir).await?;
        let mut topic_refs = Vec::with_capacity(snapshot.map.topics.len());
        for topic in &snapshot.map.topics {
            let shard = KnowledgeMapTopicShard {
                schema_version: ARTIFACT_SCHEMA_VERSION,
                topic: topic.clone(),
                sources: snapshot
                    .map
                    .sources
                    .iter()
                    .filter(|source| source.topic == topic.id)
                    .cloned()
                    .collect(),
                route: snapshot
                    .map
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
            publish_immutable(&self.repository_root, &relative, yaml.as_bytes()).await?;
            topic_refs.push(KnowledgeMapTopicRef {
                id: topic.id.clone(),
                title: topic.title.clone(),
                description: topic.description.clone(),
                source_ids: shard
                    .sources
                    .iter()
                    .map(|source| source.id.clone())
                    .collect(),
                r#ref: relative,
                digest,
            });
        }

        while snapshot.map.history.len() > RECENT_HISTORY_LIMIT {
            let chunk: Vec<_> = snapshot.map.history.drain(..RECENT_HISTORY_LIMIT).collect();
            let archive = KnowledgeMapHistoryArchive {
                schema_version: ARTIFACT_SCHEMA_VERSION,
                from_version: chunk.first().expect("non-empty archive chunk").version,
                through_version: chunk.last().expect("non-empty archive chunk").version,
                previous: snapshot.archive.clone(),
                entries: chunk,
            };
            let yaml = serialize_yaml(&archive)?;
            let digest = content_digest(yaml.as_bytes());
            let relative = format!(
                "{KNOWLEDGE_MAP_HISTORY_DIR_NAME}/{:020}-{:020}-{digest}.yaml",
                archive.from_version, archive.through_version
            );
            publish_immutable(&self.repository_root, &relative, yaml.as_bytes()).await?;
            snapshot.archived_through = archive.through_version;
            snapshot.archive = Some(KnowledgeMapArchiveRef {
                r#ref: relative,
                digest,
            });
        }
        snapshot.map.validate_snapshot(snapshot.archived_through)?;
        let manifest = KnowledgeMapManifest {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            map_version: snapshot.map.map_version,
            updated_at: snapshot.map.updated_at.clone(),
            topics: topic_refs,
            history: KnowledgeMapHistoryManifest {
                archived_through: snapshot.archived_through,
                archive: snapshot.archive.clone(),
                recent: snapshot.map.history.clone(),
            },
        };
        self.publish_manifest(serialize_yaml(&manifest)?.as_bytes())
            .await?;
        cleanup_superseded_topic_shards(
            &self.repository_root,
            &self.backup_path(),
            &manifest,
            Duration::from_secs(60),
        )
        .await;
        Ok(())
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
        let content =
            read_verified_ref(&self.repository_root, &topic_ref.r#ref, &topic_ref.digest).await?;
        let shard = serde_norway::from_str::<KnowledgeMapTopicShard>(&content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        let expected_ref = format!(
            "{KNOWLEDGE_MAP_TOPICS_DIR_NAME}/topic-{}-{}.yaml",
            stable_id(&topic_ref.id),
            topic_ref.digest
        );
        if topic_ref.r#ref != expected_ref
            || shard.schema_version != ARTIFACT_SCHEMA_VERSION
            || shard.topic.id != topic_ref.id
            || shard.topic.title != topic_ref.title
            || shard.topic.description != topic_ref.description
            || !shard
                .sources
                .iter()
                .map(|source| source.id.as_str())
                .eq(topic_ref.source_ids.iter().map(String::as_str))
        {
            return Err(KnowledgeMapServiceError::Integrity(format!(
                "topic shard '{}' identity, metadata, or schema does not match the manifest",
                topic_ref.r#ref
            )));
        }
        validate_topic_shard(&shard)?;
        Ok(shard)
    }

    async fn publish_manifest(&self, content: &[u8]) -> Result<(), KnowledgeMapServiceError> {
        let path = self.map_path();
        let temp = temporary_path(&path);
        let backup = self.backup_path();
        if let Err(error) = fs::write(&temp, content).await {
            let _ = fs::remove_file(&temp).await;
            return Err(error.into());
        }
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
            let _ = fs::remove_file(temp).await;
            return Err(KnowledgeMapServiceError::Io(error));
        }
        Ok(())
    }

    async fn recover_manifest_backup(&self) -> Result<(), KnowledgeMapServiceError> {
        let path = self.map_path();
        let backup = self.backup_path();
        if !fs::try_exists(&path).await? && fs::try_exists(&backup).await? {
            fs::rename(backup, path).await?;
        }
        Ok(())
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

    async fn read_root_content(&self) -> Result<String, KnowledgeMapServiceError> {
        let path = self.map_path();
        match read_root_file(&self.repository_root, &path).await {
            Ok(content) => Ok(content),
            Err(KnowledgeMapServiceError::Io(error))
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                match read_root_file(&self.repository_root, &self.backup_path()).await {
                    Ok(content) => Ok(content),
                    Err(KnowledgeMapServiceError::Io(error))
                        if error.kind() == std::io::ErrorKind::NotFound =>
                    {
                        read_root_file(&self.repository_root, &path).await
                    }
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        }
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH);
    let suffix = elapsed.map_or(0, |duration| duration.as_nanos());
    path.with_extension(format!("{}.{}.tmp", std::process::id(), suffix))
}

async fn ensure_owned_directory(
    repository_root: &Path,
    directory: &Path,
) -> Result<PathBuf, KnowledgeMapServiceError> {
    let contract = repository_root.join(AGENT_CONTRACT_DIR_NAME);
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

async fn publish_immutable(
    repository_root: &Path,
    relative: &str,
    content: &[u8],
) -> Result<(), KnowledgeMapServiceError> {
    let path = resolve_contract_ref(repository_root, relative)?;
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

async fn cleanup_superseded_topic_shards(
    repository_root: &Path,
    backup: &Path,
    manifest: &KnowledgeMapManifest,
    grace: Duration,
) {
    let mut retained = manifest.referenced_topic_files();
    if let Ok(content) = fs::read_to_string(backup).await {
        let Ok(recovery) = parse_manifest(&content) else {
            return;
        };
        retained.extend(recovery.referenced_topic_files());
    }
    let directory = repository_root
        .join(AGENT_CONTRACT_DIR_NAME)
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
