use std::path::PathBuf;

use tokio::fs;
use tokio::time::Duration;
#[cfg(test)]
use tokio::time::sleep;

#[cfg(test)]
use crate::project::{AGENT_CONTRACT_DIR_NAME, KNOWLEDGE_MAP_FILE_NAME};
use crate::{
    api::RequestContext,
    domain::{BusinessGlossary, KnowledgeMap, RepositoryMapType, validate_directory_collection},
    project::{
        CODESPEC_MAP_RELATIVE_PATH, KNOWLEDGE_MAP_HISTORY_DIR_NAME, KNOWLEDGE_MAP_RELATIVE_PATH,
        KNOWLEDGE_MAP_TOPICS_DIR_NAME, LEGACY_AGENT_CONTRACT_DIR_NAME,
        LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH,
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
mod query;
mod source_mutation;
mod validation;

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
        let _mutation_locks = self.acquire_legacy_aware_mutation_locks().await?;
        let _rollback_committed = self.recover_legacy_rollback_transition().await?;
        self.recover_manifest_backup().await?;
        self.recover_legacy_redirect_transition().await?;
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
                    let (software_changed, business_changed) = snapshot
                        .map
                        .ensure_reserved_repository_routes_snapshot(snapshot.archived_through)?;
                    (
                        software_changed,
                        business_changed,
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
                    } else if snapshot.legacy_glossary_uri_normalized {
                        "migrated Knowledge Map legacy glossary URI to the canonical artifact"
                            .to_owned()
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
            let _normalized_legacy_builtin_sources = normalize_legacy_builtin_sources(&mut map);
            return Ok(MutableKnowledgeMap {
                map_type: RepositoryMapType::Knowledge,
                directories: baseline_directories(RepositoryMapType::Knowledge),
                map,
                archived_through: 0,
                archive: None,
                history_index: None,
                requires_publish: true,
                legacy_glossary_uri_normalized: false,
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
        let mut requires_publish = probe.schema_version != ARTIFACT_SCHEMA_VERSION
            || (manifest.history.archive.is_some() && manifest.history.index.is_none());
        let mut topics = Vec::with_capacity(manifest.topics.len());
        let mut sources = Vec::new();
        let mut routes = Vec::new();
        let mut legacy_glossary_uri_normalized = false;
        for topic_ref in &manifest.topics {
            let (shard, normalized_legacy_glossary_uri) =
                self.load_topic_shard_for_mutation(topic_ref).await?;
            requires_publish |= normalized_legacy_glossary_uri;
            legacy_glossary_uri_normalized |= normalized_legacy_glossary_uri;
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
        let normalized_legacy_builtin_sources = normalize_legacy_builtin_sources(&mut map);
        requires_publish |= normalized_legacy_builtin_sources;
        legacy_glossary_uri_normalized |= normalized_legacy_builtin_sources;
        if probe.schema_version != LEGACY_ARTIFACT_SCHEMA_VERSION
            || self.map_type != RepositoryMapType::Knowledge
        {
            map.validate_snapshot(manifest.history.archived_through)?;
        }
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
            legacy_glossary_uri_normalized,
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

    async fn load_topic_shard(
        &self,
        topic_ref: &KnowledgeMapTopicRef,
    ) -> Result<KnowledgeMapTopicShard, KnowledgeMapServiceError> {
        let contract_dir = self.read_contract_dir_name().await?;
        self.load_topic_shard_in(contract_dir, topic_ref).await
    }

    async fn load_topic_shard_for_mutation(
        &self,
        topic_ref: &KnowledgeMapTopicRef,
    ) -> Result<(KnowledgeMapTopicShard, bool), KnowledgeMapServiceError> {
        let contract_dir = self.read_contract_dir_name().await?;
        self.load_topic_shard_with_legacy_glossary_normalization(contract_dir, topic_ref, true)
            .await
    }

    async fn load_topic_shard_in(
        &self,
        contract_dir: &str,
        topic_ref: &KnowledgeMapTopicRef,
    ) -> Result<KnowledgeMapTopicShard, KnowledgeMapServiceError> {
        self.load_topic_shard_with_legacy_glossary_normalization(contract_dir, topic_ref, false)
            .await
            .map(|(shard, _normalized_legacy_glossary_uri)| shard)
    }

    async fn load_topic_shard_with_legacy_glossary_normalization(
        &self,
        contract_dir: &str,
        topic_ref: &KnowledgeMapTopicRef,
        normalize_visible_legacy_glossary_uri: bool,
    ) -> Result<(KnowledgeMapTopicShard, bool), KnowledgeMapServiceError> {
        let content = read_verified_ref_in(
            &self.repository_root,
            contract_dir,
            &topic_ref.r#ref,
            &topic_ref.digest,
        )
        .await?;
        let mut shard = serde_norway::from_str::<KnowledgeMapTopicShard>(&content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        let mut normalized_legacy_glossary_uri = false;
        if contract_dir == LEGACY_AGENT_CONTRACT_DIR_NAME || normalize_visible_legacy_glossary_uri {
            for source in &mut shard.sources {
                if source.id == "repository-business-glossary"
                    && source.uri == LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH
                {
                    source.uri = crate::project::BUSINESS_GLOSSARY_RELATIVE_PATH.to_owned();
                    source.version = source.version.saturating_add(1);
                    normalized_legacy_glossary_uri = true;
                }
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
        Ok((shard, normalized_legacy_glossary_uri))
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
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "reserved_contract_tests.rs"]
mod reserved_contract_tests;

#[cfg(test)]
#[path = "identity_contract_tests.rs"]
mod identity_contract_tests;
