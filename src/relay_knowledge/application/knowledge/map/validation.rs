//! Cross-artifact contract validation for map manifests and business routes.

use tokio::fs;

use crate::{
    api::RequestContext,
    domain::{BusinessGlossary, KnowledgeMap, KnowledgeMapSource, KnowledgeMapSourceKind},
    project::{BUSINESS_GLOSSARY_RELATIVE_PATH, LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH},
};

use super::{
    ARTIFACT_SCHEMA_VERSION, DIRECTORY_ARTIFACT_SCHEMA_VERSION, KnowledgeMapManifest,
    KnowledgeMapSchemaProbe, KnowledgeMapService, KnowledgeMapServiceError, KnowledgeMapTopicShard,
    KnowledgeMapValidationResponse, LEGACY_ARTIFACT_SCHEMA_VERSION,
    history::MISSING_HISTORY_INDEX_MESSAGE, metadata, parse_manifest, parse_v1_map,
    parse_v1_map_for_legacy_recovery, read_verified_ref_in, safe_repository_source_path,
};

#[derive(Clone, Copy)]
enum MapContentValidation {
    CurrentContract,
    LegacyRecovery,
}

#[derive(Clone, Copy)]
pub(super) enum LegacyGlossaryReadPolicy {
    ExactRoute,
    LegacyRootCompatibility,
}

impl KnowledgeMapService {
    pub async fn validate(
        &self,
        context: &RequestContext,
    ) -> Result<KnowledgeMapValidationResponse, KnowledgeMapServiceError> {
        let mut diagnostics = Vec::new();
        if let Err(error) = self.validate_map_contract().await {
            diagnostics.push(error.to_string());
        }
        if let Err(error) = self.validate_recent_history_layout().await {
            diagnostics.push(error.to_string());
        }
        if self.map_type == crate::domain::RepositoryMapType::Knowledge
            && let Err(error) = self.validate_business_glossary_route().await
        {
            diagnostics.push(error.to_string());
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

    pub(super) async fn validate_map_contract(&self) -> Result<(), KnowledgeMapServiceError> {
        let root = self.read_root_snapshot().await?;
        self.validate_map_content_with_policy(
            root.contract_dir,
            &root.content,
            MapContentValidation::CurrentContract,
        )
        .await
    }

    async fn validate_recent_history_layout(&self) -> Result<(), KnowledgeMapServiceError> {
        let root = self.read_root_snapshot().await?;
        let probe = serde_norway::from_str::<KnowledgeMapSchemaProbe>(&root.content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        if probe.schema_version != ARTIFACT_SCHEMA_VERSION {
            return Ok(());
        }
        let mut contract_dirs = vec![self.contract_dir_name()];
        if self.map_type == crate::domain::RepositoryMapType::Knowledge {
            contract_dirs.push(crate::project::LEGACY_AGENT_CONTRACT_DIR_NAME);
        }
        for contract_dir in contract_dirs {
            let history = self
                .repository_root
                .join(contract_dir)
                .join(crate::project::KNOWLEDGE_MAP_HISTORY_DIR_NAME);
            match fs::symlink_metadata(&history).await {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(KnowledgeMapServiceError::UnsafePath(
                        history.display().to_string(),
                    ));
                }
                Ok(_) => {
                    return Err(KnowledgeMapServiceError::Integrity(format!(
                        "obsolete history directory '{}' remains; run `relay-knowledge map init` to finish cleanup",
                        history.display()
                    )));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    pub(super) async fn validate_legacy_map_content_in(
        &self,
        contract_dir: &str,
        content: &str,
        glossary_read_policy: LegacyGlossaryReadPolicy,
    ) -> Result<(), KnowledgeMapServiceError> {
        self.validate_map_content_with_policy(
            contract_dir,
            content,
            MapContentValidation::LegacyRecovery,
        )
        .await?;
        if let Some(glossary) = self
            .read_routed_legacy_business_glossary(contract_dir, content, glossary_read_policy)
            .await?
        {
            BusinessGlossary::parse(&glossary)?;
        }
        Ok(())
    }

    /// Confirms that a visible root has reached the current v4 publication contract.
    ///
    /// Older roots are valid migration inputs, but must not trigger legacy-reader
    /// redirect publication before the v4 root and its referenced artifacts validate.
    pub(super) async fn validate_visible_current_map_content(
        &self,
        content: &str,
    ) -> Result<bool, KnowledgeMapServiceError> {
        let probe = serde_norway::from_str::<KnowledgeMapSchemaProbe>(content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        if probe.schema_version != ARTIFACT_SCHEMA_VERSION {
            return Ok(false);
        }
        self.validate_map_content_with_policy(
            self.contract_dir_name(),
            content,
            MapContentValidation::CurrentContract,
        )
        .await?;
        Ok(true)
    }

    async fn validate_map_content_with_policy(
        &self,
        contract_dir: &str,
        content: &str,
        policy: MapContentValidation,
    ) -> Result<(), KnowledgeMapServiceError> {
        let probe = serde_norway::from_str::<KnowledgeMapSchemaProbe>(content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        if probe.schema_version == KnowledgeMap::SCHEMA_VERSION {
            let map = if matches!(policy, MapContentValidation::LegacyRecovery)
                && self.map_type == crate::domain::RepositoryMapType::Knowledge
            {
                parse_v1_map_for_legacy_recovery(content)?
            } else {
                parse_v1_map(content)?
            };
            if matches!(policy, MapContentValidation::CurrentContract)
                && self.map_type == crate::domain::RepositoryMapType::Knowledge
            {
                map.validate_reserved_repository_routes()?;
            }
            return Ok(());
        }
        if !matches!(
            probe.schema_version,
            LEGACY_ARTIFACT_SCHEMA_VERSION
                | DIRECTORY_ARTIFACT_SCHEMA_VERSION
                | ARTIFACT_SCHEMA_VERSION
        ) {
            return Err(KnowledgeMapServiceError::Yaml(format!(
                "unsupported schema_version {}",
                probe.schema_version
            )));
        }
        let manifest = parse_manifest(content)?;
        let history_checkpoint = history_checkpoint(&manifest);
        self.validate_manifest_identity(&manifest)?;
        if manifest.schema_version != ARTIFACT_SCHEMA_VERSION {
            self.validate_archived_history_in(contract_dir, &manifest.history)
                .await?;
        }
        if manifest.schema_version != ARTIFACT_SCHEMA_VERSION
            && matches!(policy, MapContentValidation::CurrentContract)
            && manifest.history.archived_through > 0
            && manifest.history.index.is_none()
        {
            return Err(KnowledgeMapServiceError::Integrity(
                MISSING_HISTORY_INDEX_MESSAGE.to_owned(),
            ));
        }
        let mut topics = Vec::with_capacity(manifest.topics.len());
        let mut sources = Vec::new();
        let mut routes = Vec::new();
        for topic_ref in &manifest.topics {
            let shard = self.load_topic_shard_in(contract_dir, topic_ref).await?;
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
        if matches!(policy, MapContentValidation::LegacyRecovery)
            && self.map_type == crate::domain::RepositoryMapType::Knowledge
        {
            map.ensure_reserved_repository_routes_snapshot(history_checkpoint)?;
        } else {
            map.validate_snapshot(history_checkpoint)?;
        }
        if matches!(policy, MapContentValidation::CurrentContract)
            && self.map_type == crate::domain::RepositoryMapType::Knowledge
        {
            map.validate_reserved_repository_routes()?;
        }
        Ok(())
    }

    pub(super) async fn read_routed_legacy_business_glossary(
        &self,
        contract_dir: &str,
        content: &str,
        read_policy: LegacyGlossaryReadPolicy,
    ) -> Result<Option<Vec<u8>>, KnowledgeMapServiceError> {
        if self.map_type != crate::domain::RepositoryMapType::Knowledge
            || contract_dir != crate::project::LEGACY_AGENT_CONTRACT_DIR_NAME
        {
            return Ok(None);
        }
        let Some(source) = self
            .routed_business_glossary_source_in(contract_dir, content)
            .await?
        else {
            return Ok(None);
        };
        if !matches!(
            source.uri.as_str(),
            LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH | BUSINESS_GLOSSARY_RELATIVE_PATH
        ) {
            return Err(KnowledgeMapServiceError::Integrity(
                "routed reserved source 'repository-business-glossary' has an unsupported URI"
                    .to_owned(),
            ));
        }
        let path = match read_policy {
            LegacyGlossaryReadPolicy::ExactRoute => {
                safe_repository_source_path(&self.repository_root, &source.uri).await?
            }
            LegacyGlossaryReadPolicy::LegacyRootCompatibility
                if source.uri == BUSINESS_GLOSSARY_RELATIVE_PATH
                    && !fs::try_exists(
                        self.repository_root.join(BUSINESS_GLOSSARY_RELATIVE_PATH),
                    )
                    .await? =>
            {
                let legacy_path = self
                    .repository_root
                    .join(LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH);
                if !fs::try_exists(&legacy_path).await? {
                    return Ok(None);
                }
                safe_repository_source_path(
                    &self.repository_root,
                    LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH,
                )
                .await?
            }
            LegacyGlossaryReadPolicy::LegacyRootCompatibility
                if source.uri == BUSINESS_GLOSSARY_RELATIVE_PATH =>
            {
                safe_repository_source_path(&self.repository_root, BUSINESS_GLOSSARY_RELATIVE_PATH)
                    .await?
            }
            LegacyGlossaryReadPolicy::LegacyRootCompatibility => {
                safe_repository_source_path(&self.repository_root, &source.uri).await?
            }
        };
        Ok(Some(fs::read(path).await?))
    }

    async fn validate_business_glossary_route(&self) -> Result<(), KnowledgeMapServiceError> {
        let root = self.read_root_snapshot().await?;
        let source = self
            .routed_business_glossary_source_in(root.contract_dir, &root.content)
            .await?
            .ok_or_else(|| {
            KnowledgeMapServiceError::Integrity(
                "required business-knowledge route for reserved source 'repository-business-glossary' is missing"
                    .to_owned(),
            )
        })?;
        if source.kind != KnowledgeMapSourceKind::File
            || !matches!(
                source.uri.as_str(),
                BUSINESS_GLOSSARY_RELATIVE_PATH | LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH
            )
            || source.source_scope.as_deref() != Some("repo")
        {
            return Err(KnowledgeMapServiceError::Integrity(
                "reserved source 'repository-business-glossary' must use topic 'business-knowledge', kind file, a recognized glossary URI, and source scope repo"
                    .to_owned(),
            ));
        }
        let path = safe_repository_source_path(&self.repository_root, &source.uri).await?;
        let content = fs::read(path).await?;
        BusinessGlossary::parse(&content)?;
        Ok(())
    }

    pub(super) async fn routed_business_glossary_source_in(
        &self,
        contract_dir: &str,
        content: &str,
    ) -> Result<Option<KnowledgeMapSource>, KnowledgeMapServiceError> {
        let probe = serde_norway::from_str::<KnowledgeMapSchemaProbe>(content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        if probe.schema_version == KnowledgeMap::SCHEMA_VERSION {
            let map = serde_norway::from_str::<KnowledgeMap>(content)
                .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
            let route_includes_glossary = map.routes.iter().any(|route| {
                route.topic == "business-knowledge"
                    && route
                        .source_order
                        .iter()
                        .any(|source_id| source_id == "repository-business-glossary")
            });
            return Ok(route_includes_glossary
                .then(|| {
                    map.sources.into_iter().find(|source| {
                        source.id == "repository-business-glossary"
                            && source.topic == "business-knowledge"
                    })
                })
                .flatten());
        }
        let manifest = parse_manifest(content)?;
        let Some(topic_ref) = manifest
            .topics
            .iter()
            .find(|topic| topic.id == "business-knowledge")
        else {
            return Ok(None);
        };
        let shard_content = read_verified_ref_in(
            &self.repository_root,
            contract_dir,
            &topic_ref.r#ref,
            &topic_ref.digest,
        )
        .await?;
        let shard = serde_norway::from_str::<KnowledgeMapTopicShard>(&shard_content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        let route_includes_glossary = shard.route.is_some_and(|route| {
            route
                .source_order
                .iter()
                .any(|source_id| source_id == "repository-business-glossary")
        });
        Ok(route_includes_glossary
            .then(|| {
                shard
                    .sources
                    .into_iter()
                    .find(|source| source.id == "repository-business-glossary")
            })
            .flatten())
    }
}

fn history_checkpoint(manifest: &KnowledgeMapManifest) -> u64 {
    if manifest.schema_version == ARTIFACT_SCHEMA_VERSION {
        manifest.history.omitted_through
    } else {
        manifest.history.archived_through
    }
}
