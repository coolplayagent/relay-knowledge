//! Cross-artifact contract validation for map manifests and business routes.

use tokio::fs;

use crate::{
    api::RequestContext,
    domain::{BusinessGlossary, KnowledgeMap, KnowledgeMapSourceKind},
    project::{BUSINESS_GLOSSARY_RELATIVE_PATH, LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH},
};

use super::{
    ARTIFACT_SCHEMA_VERSION, KnowledgeMapSchemaProbe, KnowledgeMapService,
    KnowledgeMapServiceError, KnowledgeMapValidationResponse, LEGACY_ARTIFACT_SCHEMA_VERSION,
    history::MISSING_HISTORY_INDEX_MESSAGE, metadata, parse_manifest, parse_v1_map,
    safe_repository_source_path,
};

#[derive(Clone, Copy)]
enum MapContentValidation {
    CurrentContract,
    LegacyRecovery,
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
        let content = self.read_root_content().await?;
        let contract_dir = self.read_contract_dir_name().await?;
        self.validate_map_content_in(contract_dir, &content).await
    }

    pub(super) async fn validate_map_content_in(
        &self,
        contract_dir: &str,
        content: &str,
    ) -> Result<(), KnowledgeMapServiceError> {
        self.validate_map_content_with_policy(
            contract_dir,
            content,
            MapContentValidation::CurrentContract,
        )
        .await
    }

    pub(super) async fn validate_legacy_map_content_in(
        &self,
        contract_dir: &str,
        content: &str,
    ) -> Result<(), KnowledgeMapServiceError> {
        self.validate_map_content_with_policy(
            contract_dir,
            content,
            MapContentValidation::LegacyRecovery,
        )
        .await
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
            let map = parse_v1_map(content)?;
            if matches!(policy, MapContentValidation::CurrentContract)
                && self.map_type == crate::domain::RepositoryMapType::Knowledge
            {
                map.validate_reserved_repository_routes()?;
            }
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
        let manifest = parse_manifest(content)?;
        self.validate_manifest_identity(&manifest)?;
        if matches!(policy, MapContentValidation::CurrentContract)
            && manifest.history.archived_through > 0
            && manifest.history.index.is_none()
        {
            return Err(KnowledgeMapServiceError::Integrity(
                MISSING_HISTORY_INDEX_MESSAGE.to_owned(),
            ));
        }
        self.validate_archived_history_in(contract_dir, &manifest.history)
            .await?;
        let mut topics = Vec::with_capacity(manifest.topics.len());
        let mut sources = Vec::new();
        let mut routes = Vec::new();
        for topic_ref in &manifest.topics {
            let shard = self.load_topic_shard_in(contract_dir, topic_ref).await?;
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
        if matches!(policy, MapContentValidation::CurrentContract)
            && self.map_type == crate::domain::RepositoryMapType::Knowledge
        {
            map.validate_reserved_repository_routes()?;
        }
        Ok(())
    }

    async fn validate_business_glossary_route(&self) -> Result<(), KnowledgeMapServiceError> {
        let (route, sources) = self.load_topic_route("business-knowledge").await?;
        let route = route.ok_or_else(|| {
            KnowledgeMapServiceError::Integrity(
                "required business-knowledge route for reserved source 'repository-business-glossary' is missing"
                    .to_owned(),
            )
        })?;
        if !route
            .source_order
            .iter()
            .any(|source_id| source_id == "repository-business-glossary")
        {
            return Err(KnowledgeMapServiceError::Integrity(
                "business-knowledge route must include reserved source 'repository-business-glossary'"
                    .to_owned(),
            ));
        }

        let source = sources
            .iter()
            .find(|source| source.id == "repository-business-glossary")
            .ok_or_else(|| {
                KnowledgeMapServiceError::Integrity(
                    "business-knowledge route references missing reserved source 'repository-business-glossary'"
                        .to_owned(),
                )
            })?;
        if source.kind != KnowledgeMapSourceKind::File
            || source.uri != BUSINESS_GLOSSARY_RELATIVE_PATH
            || source.source_scope.as_deref() != Some("repo")
        {
            return Err(KnowledgeMapServiceError::Integrity(
                "reserved source 'repository-business-glossary' must use topic 'business-knowledge', kind file, URI 'knowledge/glossary/business-glossary.yaml', and source scope repo"
                    .to_owned(),
            ));
        }
        let source_path = if self.uses_legacy_contract().await? {
            LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH
        } else {
            BUSINESS_GLOSSARY_RELATIVE_PATH
        };
        let path = safe_repository_source_path(&self.repository_root, source_path).await?;
        let content = fs::read(path).await?;
        BusinessGlossary::parse(&content)?;
        Ok(())
    }
}
