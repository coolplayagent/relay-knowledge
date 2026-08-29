//! Cross-artifact contract validation for map manifests and business routes.

use tokio::fs;

use crate::{
    api::RequestContext,
    domain::{BusinessGlossary, KnowledgeMap, KnowledgeMapSourceKind},
    project::LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH,
};

use super::{
    ARTIFACT_SCHEMA_VERSION, KnowledgeMapSchemaProbe, KnowledgeMapService,
    KnowledgeMapServiceError, KnowledgeMapValidationResponse, LEGACY_ARTIFACT_SCHEMA_VERSION,
    metadata, parse_manifest, parse_v1_map, safe_repository_source_path,
};

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
