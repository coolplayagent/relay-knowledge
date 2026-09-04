//! Read-model queries for map views and topic routing.

use crate::{
    api::RequestContext,
    domain::{KnowledgeMap, KnowledgeMapRoute, KnowledgeMapSource, RepositoryMapType},
};

use super::{
    ARTIFACT_SCHEMA_VERSION, KnowledgeMapHistoryWindow, KnowledgeMapRouteResponse,
    KnowledgeMapSchemaProbe, KnowledgeMapService, KnowledgeMapServiceError,
    KnowledgeMapShowResponse, KnowledgeMapView, LEGACY_ARTIFACT_SCHEMA_VERSION,
    RECENT_HISTORY_LIMIT, baseline_directories, metadata, parse_manifest, parse_v1_map,
};

impl KnowledgeMapService {
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

    pub(super) async fn load_show_view(
        &self,
    ) -> Result<KnowledgeMapView, KnowledgeMapServiceError> {
        let root = self.read_root_snapshot().await?;
        let probe = serde_norway::from_str::<KnowledgeMapSchemaProbe>(&root.content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        if probe.schema_version == KnowledgeMap::SCHEMA_VERSION {
            self.require_knowledge_map("legacy map show")?;
            let mut map = parse_v1_map(&root.content)?;
            let omitted = map.history.len().saturating_sub(RECENT_HISTORY_LIMIT);
            let recent = map.history.split_off(omitted);
            let omitted_through = recent
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
                    omitted_through,
                    complete: omitted == 0,
                    recent,
                },
            });
        }
        if !matches!(
            probe.schema_version,
            LEGACY_ARTIFACT_SCHEMA_VERSION
                | super::DIRECTORY_ARTIFACT_SCHEMA_VERSION
                | ARTIFACT_SCHEMA_VERSION
        ) {
            return Err(KnowledgeMapServiceError::Yaml(format!(
                "unsupported schema_version {}",
                probe.schema_version
            )));
        }
        let manifest = parse_manifest(&root.content)?;
        self.validate_manifest_identity(&manifest)?;
        let mut topics = Vec::with_capacity(manifest.topics.len());
        let mut sources = Vec::new();
        let mut routes = Vec::new();
        for topic_ref in &manifest.topics {
            let shard = self
                .load_topic_shard_in(root.contract_dir, topic_ref)
                .await?;
            topics.push(shard.topic);
            sources.extend(shard.sources);
            routes.extend(shard.route);
        }
        let omitted_through = if manifest.schema_version == ARTIFACT_SCHEMA_VERSION {
            manifest.history.omitted_through
        } else {
            manifest.history.archived_through
        };
        Ok(KnowledgeMapView {
            artifact_schema_version: manifest.schema_version,
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
                omitted_through,
                complete: omitted_through == 0,
                recent: manifest.history.recent,
            },
        })
    }

    pub(super) async fn load_topic_route(
        &self,
        topic: &str,
    ) -> Result<(Option<KnowledgeMapRoute>, Vec<KnowledgeMapSource>), KnowledgeMapServiceError>
    {
        let root = self.read_root_snapshot().await?;
        let probe = serde_norway::from_str::<KnowledgeMapSchemaProbe>(&root.content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        if probe.schema_version == 1 {
            let map = parse_v1_map(&root.content)?;
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
        let manifest = parse_manifest(&root.content)?;
        self.validate_manifest_identity(&manifest)?;
        super::validate_recent_history(&manifest)?;
        let Some(topic_ref) = manifest.topics.iter().find(|entry| entry.id == topic) else {
            return Ok((None, Vec::new()));
        };
        let shard = self
            .load_topic_shard_in(root.contract_dir, topic_ref)
            .await?;
        Ok((shard.route, shard.sources))
    }
}
