use std::path::PathBuf;

use tokio::fs;
use tokio::time::Duration;
#[cfg(test)]
use tokio::time::sleep;

#[cfg(test)]
use crate::project::{AGENT_CONTRACT_DIR_NAME, KNOWLEDGE_MAP_FILE_NAME};
use crate::{
    api::RequestContext,
    domain::{
        BusinessGlossary, KnowledgeMap, KnowledgeMapChange, KnowledgeMapRoute, KnowledgeMapSource,
        KnowledgeMapSourceKind, RepositoryMapType, validate_directory_collection,
    },
    project::{
        CODESPEC_MAP_RELATIVE_PATH, KNOWLEDGE_MAP_HISTORY_DIR_NAME, KNOWLEDGE_MAP_RELATIVE_PATH,
        KNOWLEDGE_MAP_TOPICS_DIR_NAME, LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH,
    },
};

mod artifact;
mod contracts;
mod error;
mod fs_contract;
mod governance;
mod history;
mod lock;
mod migration;

pub(crate) use history::MAX_HISTORY_PAGE_SIZE;
#[cfg(test)]
use lock::{
    ADVISORY_LOCK_MARKER, cleanup_transition_locks, transition_lock_prepared_path,
    transition_lock_prepared_path_with_identity,
};

use artifact::*;
pub use contracts::{
    KnowledgeMapAgentSnippetResponse, KnowledgeMapHistoryResponse, KnowledgeMapHistoryWindow,
    KnowledgeMapMutationResponse, KnowledgeMapRouteResponse, KnowledgeMapShowResponse,
    KnowledgeMapSourceAddRequest, KnowledgeMapValidationResponse, KnowledgeMapView,
};
use contracts::{MutableKnowledgeMap, baseline_directories, metadata, now_stamp};
pub use error::KnowledgeMapServiceError;
use fs_contract::*;

const WRITE_LOCK_TIMEOUT: Duration = Duration::from_secs(10);

/// File-backed service for the shared YAML knowledge navigation contract.
pub struct KnowledgeMapService {
    repository_root: PathBuf,
    map_type: RepositoryMapType,
}

impl KnowledgeMapService {
    pub fn new(repository_root: PathBuf) -> Self {
        Self {
            repository_root,
            map_type: RepositoryMapType::Knowledge,
        }
    }

    pub(crate) fn for_type(&self, map_type: RepositoryMapType) -> Self {
        Self {
            repository_root: self.repository_root.clone(),
            map_type,
        }
    }

    pub async fn init(
        &self,
        context: &RequestContext,
    ) -> Result<KnowledgeMapMutationResponse, KnowledgeMapServiceError> {
        let migrating_legacy = self.map_type == RepositoryMapType::Knowledge
            && fs::try_exists(self.legacy_map_path()).await?
            && !fs::try_exists(self.map_path()).await?;
        let _legacy_lock = if migrating_legacy {
            Some(self.acquire_legacy_write_lock(WRITE_LOCK_TIMEOUT).await?)
        } else {
            None
        };
        let _lock = self.acquire_write_lock(WRITE_LOCK_TIMEOUT).await?;
        self.recover_manifest_backup().await?;
        self.prepare_legacy_migration().await?;
        self.ensure_baseline_files().await?;
        let path = self.map_path();
        if fs::try_exists(&path).await? {
            let existing = fs::read_to_string(&path).await?;
            let legacy = serde_norway::from_str::<KnowledgeMapSchemaProbe>(&existing)
                .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?
                .schema_version
                == 1;
            let mut snapshot = self.load_for_mutation().await?;
            let (software_changed, business_changed, glossary_created) =
                if self.map_type == RepositoryMapType::Knowledge {
                    (
                        snapshot
                            .map
                            .ensure_software_model_route_snapshot(snapshot.archived_through)?,
                        snapshot
                            .map
                            .ensure_business_knowledge_route_snapshot(snapshot.archived_through)?,
                        self.ensure_default_business_glossary().await?,
                    )
                } else {
                    (false, false, false)
                };
            if software_changed || business_changed {
                snapshot.map.record_change(
                    "builtin-routes.ensure",
                    "Ensured repository software-model and business-knowledge routes.".to_owned(),
                    now_stamp(),
                );
                self.write_map(&mut snapshot).await?;
                return Ok(self.mutation_response(
                    context,
                    snapshot.map.map_version,
                    "initialized repository software-model and business-knowledge routes"
                        .to_owned(),
                ));
            }
            if legacy || snapshot.requires_publish {
                self.write_map(&mut snapshot).await?;
                return Ok(self.mutation_response(
                    context,
                    snapshot.map.map_version,
                    if legacy {
                        "migrated knowledge map schema v1 to v2".to_owned()
                    } else {
                        "migrated Knowledge Map v2 history archive index".to_owned()
                    },
                ));
            }
            return Ok(self.mutation_response(
                context,
                snapshot.map.map_version,
                if glossary_created {
                    "created missing repository business glossary".to_owned()
                } else if self.map_type == RepositoryMapType::Codespec {
                    "CodeSpec map and governed baseline directories already exist".to_owned()
                } else {
                    "Knowledge map and built-in repository routes already exist".to_owned()
                },
            ));
        }

        let mut snapshot = MutableKnowledgeMap::initial(self.map_type, now_stamp());
        if self.map_type == RepositoryMapType::Knowledge {
            self.ensure_default_business_glossary().await?;
        }
        self.write_map(&mut snapshot).await?;
        Ok(self.mutation_response(
            context,
            snapshot.map.map_version,
            match self.map_type {
                RepositoryMapType::Knowledge => {
                    "created Knowledge map with software-model and business-knowledge routes"
                        .to_owned()
                }
                RepositoryMapType::Codespec => {
                    "created CodeSpec map with governed baseline directories".to_owned()
                }
            },
        ))
    }

    #[cfg(test)]
    pub async fn show(
        &self,
        context: &RequestContext,
        topic: Option<String>,
    ) -> Result<KnowledgeMapShowResponse, KnowledgeMapServiceError> {
        self.show_filtered(context, topic, None).await
    }

    pub async fn show_filtered(
        &self,
        context: &RequestContext,
        topic: Option<String>,
        directory: Option<String>,
    ) -> Result<KnowledgeMapShowResponse, KnowledgeMapServiceError> {
        let mut map = self.load_show_view().await?;
        if let Some(topic) = topic {
            map.sources.retain(|source| source.topic == topic);
            map.routes.retain(|route| route.topic == topic);
            map.topics.retain(|entry| entry.id == topic);
        }
        if let Some(directory) = directory {
            map.directories.retain(|entry| entry.directory == directory);
        }
        Ok(KnowledgeMapShowResponse {
            metadata: metadata(context),
            path: self.relative_path().to_owned(),
            map_type: self.map_type,
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
            path: self.relative_path().to_owned(),
            map_type: self.map_type,
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
        self.require_knowledge_map("map source add")?;
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
        self.require_knowledge_map("map source update")?;
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
        self.require_knowledge_map("map source remove")?;
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
        match self.validate_map_contract().await {
            Ok(()) => {}
            Err(error) => diagnostics.push(error.to_string()),
        }
        if self.map_type == RepositoryMapType::Knowledge {
            match self.validate_business_glossary_route().await {
                Ok(()) => {}
                Err(error) => diagnostics.push(error.to_string()),
            }
        }

        if let Err(error) = self.validate_directory_files().await {
            diagnostics.push(error.to_string());
        }
        if let Err(error) = self.validate_cross_map_relations().await {
            diagnostics.push(error.to_string());
        }

        let agents_path = self.repository_root.join("AGENTS.md");
        match fs::read_to_string(&agents_path).await {
            Ok(contents) if contents.contains(self.relative_path()) => {}
            Ok(_) => diagnostics.push(format!(
                "AGENTS.md does not reference {}",
                self.relative_path()
            )),
            Err(error) => diagnostics.push(format!("failed to read AGENTS.md: {error}")),
        }

        Ok(KnowledgeMapValidationResponse {
            metadata: metadata(context),
            path: self.relative_path().to_owned(),
            map_type: self.map_type,
            valid: diagnostics.is_empty(),
            diagnostics,
        })
    }

    pub fn agent_snippet(&self, context: &RequestContext) -> KnowledgeMapAgentSnippetResponse {
        KnowledgeMapAgentSnippetResponse {
            metadata: metadata(context),
            snippet: format!(
                "CodeSpec map: {CODESPEC_MAP_RELATIVE_PATH}\nKnowledge map: {KNOWLEDGE_MAP_RELATIVE_PATH}"
            ),
        }
    }

    async fn load_or_initial(&self) -> Result<MutableKnowledgeMap, KnowledgeMapServiceError> {
        let path = self.map_path();
        if fs::try_exists(&path).await? || fs::try_exists(self.backup_path()).await? {
            self.load_for_mutation().await
        } else {
            Ok(MutableKnowledgeMap::initial(self.map_type, now_stamp()))
        }
    }

    async fn load_for_mutation(&self) -> Result<MutableKnowledgeMap, KnowledgeMapServiceError> {
        let content = self.read_root_content().await?;
        let probe = serde_norway::from_str::<KnowledgeMapSchemaProbe>(&content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        if probe.schema_version == 1 {
            self.require_knowledge_map("legacy map read")?;
            let mut map = serde_norway::from_str::<KnowledgeMap>(&content)
                .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
            map.schema_version = KnowledgeMap::SCHEMA_VERSION;
            normalize_legacy_builtin_sources(&mut map);
            map.validate()?;
            return Ok(MutableKnowledgeMap {
                map_type: RepositoryMapType::Knowledge,
                directories: baseline_directories(RepositoryMapType::Knowledge),
                map,
                archived_through: 0,
                archive: None,
                history_index: None,
                requires_publish: true,
            });
        }
        if !matches!(
            probe.schema_version,
            LEGACY_ARTIFACT_SCHEMA_VERSION | ARTIFACT_SCHEMA_VERSION
        ) {
            return Err(KnowledgeMapServiceError::Yaml(format!(
                "unsupported schema_version {}",
                probe.schema_version
            )));
        }
        let manifest = parse_manifest(&content)?;
        self.validate_manifest_identity(&manifest)?;
        self.validate_archived_history(&manifest.history).await?;
        let history_index = self.ensure_history_index(&manifest.history).await?;
        let requires_publish = probe.schema_version != ARTIFACT_SCHEMA_VERSION
            || (manifest.history.archive.is_some() && manifest.history.index.is_none());
        let mut topics = Vec::with_capacity(manifest.topics.len());
        let mut sources = Vec::new();
        let mut routes = Vec::new();
        for topic_ref in &manifest.topics {
            let shard = self.load_topic_shard(topic_ref).await?;
            topics.push(shard.topic);
            sources.extend(shard.sources);
            routes.extend(shard.route);
        }
        let mut map = KnowledgeMap {
            schema_version: KnowledgeMap::SCHEMA_VERSION,
            map_version: manifest.map_version,
            updated_at: manifest.updated_at,
            topics,
            sources,
            routes,
            history: manifest.history.recent,
        };
        normalize_legacy_builtin_sources(&mut map);
        map.validate_snapshot(manifest.history.archived_through)?;
        Ok(MutableKnowledgeMap {
            map_type: self.map_type,
            directories: if manifest.directories.is_empty() {
                baseline_directories(self.map_type)
            } else {
                manifest.directories
            },
            map,
            archived_through: manifest.history.archived_through,
            archive: manifest.history.archive,
            history_index,
            requires_publish,
        })
    }

    async fn validate_map_contract(&self) -> Result<(), KnowledgeMapServiceError> {
        let content = self.read_root_content().await?;
        let probe = serde_norway::from_str::<KnowledgeMapSchemaProbe>(&content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        if probe.schema_version == KnowledgeMap::SCHEMA_VERSION {
            parse_v1_map(&content)?;
            return Ok(());
        }
        if !matches!(
            probe.schema_version,
            LEGACY_ARTIFACT_SCHEMA_VERSION | ARTIFACT_SCHEMA_VERSION
        ) {
            return Err(KnowledgeMapServiceError::Yaml(format!(
                "unsupported schema_version {}",
                probe.schema_version
            )));
        }
        let manifest = parse_manifest(&content)?;
        self.validate_manifest_identity(&manifest)?;
        self.validate_archived_history(&manifest.history).await?;
        let mut topics = Vec::with_capacity(manifest.topics.len());
        let mut sources = Vec::new();
        let mut routes = Vec::new();
        for topic_ref in &manifest.topics {
            let shard = self.load_topic_shard(topic_ref).await?;
            topics.push(shard.topic);
            sources.extend(shard.sources);
            routes.extend(shard.route);
        }
        KnowledgeMap {
            schema_version: KnowledgeMap::SCHEMA_VERSION,
            map_version: manifest.map_version,
            updated_at: manifest.updated_at,
            topics,
            sources,
            routes,
            history: manifest.history.recent,
        }
        .validate_snapshot(manifest.history.archived_through)?;
        Ok(())
    }

    async fn load_show_view(&self) -> Result<KnowledgeMapView, KnowledgeMapServiceError> {
        let content = self.read_root_content().await?;
        let probe = serde_norway::from_str::<KnowledgeMapSchemaProbe>(&content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        if probe.schema_version == KnowledgeMap::SCHEMA_VERSION {
            self.require_knowledge_map("legacy map show")?;
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
                directories: baseline_directories(RepositoryMapType::Knowledge),
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
        if !matches!(
            probe.schema_version,
            LEGACY_ARTIFACT_SCHEMA_VERSION | ARTIFACT_SCHEMA_VERSION
        ) {
            return Err(KnowledgeMapServiceError::Yaml(format!(
                "unsupported schema_version {}",
                probe.schema_version
            )));
        }
        let manifest = parse_manifest(&content)?;
        self.validate_manifest_identity(&manifest)?;
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
            directories: if manifest.directories.is_empty() {
                baseline_directories(self.map_type)
            } else {
                manifest.directories
            },
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
        validate_directory_collection(self.map_type, &snapshot.directories, true)?;
        let dir = self.repository_root.join(self.contract_dir_name());
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
            publish_immutable_in(
                &self.repository_root,
                self.contract_dir_name(),
                &relative,
                yaml.as_bytes(),
            )
            .await?;
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
            publish_immutable_in(
                &self.repository_root,
                self.contract_dir_name(),
                &relative,
                yaml.as_bytes(),
            )
            .await?;
            let archive_ref = KnowledgeMapArchiveRef {
                r#ref: relative,
                digest,
            };
            snapshot.history_index = Some(
                self.append_history_index(
                    snapshot.history_index.take(),
                    archive_ref.clone(),
                    &archive,
                )
                .await?,
            );
            snapshot.archived_through = archive.through_version;
            snapshot.archive = Some(archive_ref);
        }
        snapshot.map.validate_snapshot(snapshot.archived_through)?;
        let manifest = KnowledgeMapManifest {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            artifact_kind: Some("map".to_owned()),
            map_type: Some(snapshot.map_type),
            map_version: snapshot.map.map_version,
            updated_at: snapshot.map.updated_at.clone(),
            directories: snapshot.directories.clone(),
            topics: topic_refs,
            history: KnowledgeMapHistoryManifest {
                archived_through: snapshot.archived_through,
                archive: snapshot.archive.clone(),
                index: snapshot.history_index.clone(),
                recent: snapshot.map.history.clone(),
            },
        };
        self.publish_manifest(serialize_yaml(&manifest)?.as_bytes())
            .await?;
        if fs::try_exists(self.legacy_backup_path()).await? {
            self.publish_legacy_redirect().await?;
        }
        cleanup_superseded_topic_shards_in(
            &self.repository_root,
            self.contract_dir_name(),
            &self.backup_path(),
            &manifest,
            Duration::from_secs(60),
        )
        .await;
        snapshot.requires_publish = false;
        Ok(())
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
            let map = parse_v1_map(&content)?;
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
        let contract_dir = self.read_contract_dir_name().await?;
        let content = read_verified_ref_in(
            &self.repository_root,
            contract_dir,
            &topic_ref.r#ref,
            &topic_ref.digest,
        )
        .await?;
        let mut shard = serde_norway::from_str::<KnowledgeMapTopicShard>(&content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        for source in &mut shard.sources {
            if source.id == "repository-business-glossary"
                && source.uri == LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH
            {
                source.uri = crate::project::BUSINESS_GLOSSARY_RELATIVE_PATH.to_owned();
                source.version = source.version.saturating_add(1);
            }
        }
        let expected_ref = format!(
            "{KNOWLEDGE_MAP_TOPICS_DIR_NAME}/topic-{}-{}.yaml",
            stable_id(&topic_ref.id),
            topic_ref.digest
        );
        if topic_ref.r#ref != expected_ref
            || !matches!(
                shard.schema_version,
                LEGACY_ARTIFACT_SCHEMA_VERSION | ARTIFACT_SCHEMA_VERSION
            )
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
            path: self.relative_path().to_owned(),
            map_type: self.map_type,
            map_version,
            summary,
        }
    }

    fn business_glossary_path(&self) -> PathBuf {
        self.repository_root
            .join(crate::project::BUSINESS_GLOSSARY_RELATIVE_PATH)
    }

    async fn ensure_default_business_glossary(&self) -> Result<bool, KnowledgeMapServiceError> {
        let contract = self.repository_root.join(self.contract_dir_name());
        let owned_contract = ensure_owned_directory(&self.repository_root, &contract).await?;
        let path = self.business_glossary_path();
        if fs::try_exists(&path).await? {
            ensure_regular_file_within(&path, &owned_contract).await?;
            let content = fs::read(&path).await?;
            BusinessGlossary::parse(&content)?;
            return Ok(false);
        }
        let yaml = serialize_yaml(&BusinessGlossary::empty_v1())?;
        let temp = temporary_path(&path);
        if let Err(error) = fs::write(&temp, yaml.as_bytes()).await {
            let _ = fs::remove_file(&temp).await;
            return Err(error.into());
        }
        if let Err(error) = fs::rename(&temp, &path).await {
            let _ = fs::remove_file(temp).await;
            return Err(error.into());
        }
        Ok(true)
    }

    async fn validate_business_glossary_route(&self) -> Result<(), KnowledgeMapServiceError> {
        let (route, sources) = self.load_topic_route("business-knowledge").await?;
        let Some(route) = route else {
            return Ok(());
        };
        for source_id in route.source_order {
            let source = sources
                .iter()
                .find(|source| source.id == source_id)
                .ok_or_else(|| {
                    KnowledgeMapServiceError::Integrity(format!(
                        "business-knowledge route references missing source '{source_id}'"
                    ))
                })?;
            if source.kind != KnowledgeMapSourceKind::File
                || source.source_scope.as_deref() != Some("repo")
            {
                return Err(KnowledgeMapServiceError::Integrity(format!(
                    "business glossary source '{}' must use kind file and source scope repo",
                    source.id
                )));
            }
            let source_path = if self.uses_legacy_contract().await?
                && source.id == "repository-business-glossary"
            {
                LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH
            } else {
                &source.uri
            };
            let path = safe_repository_source_path(&self.repository_root, source_path).await?;
            let content = fs::read(path).await?;
            BusinessGlossary::parse(&content)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
