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
    project::{AGENT_CONTRACT_DIR_NAME, KNOWLEDGE_MAP_FILE_NAME, KNOWLEDGE_MAP_RELATIVE_PATH},
};

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
        let path = self.map_path();
        if fs::try_exists(&path).await? {
            let map = self.load_map().await?;
            map.validate()?;
            return Ok(self.mutation_response(
                context,
                map.map_version,
                "knowledge map already exists".to_owned(),
            ));
        }

        let map = KnowledgeMap::initial(now_stamp());
        self.write_map(&map).await?;
        Ok(self.mutation_response(context, map.map_version, "created knowledge map".to_owned()))
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
        let map = self.load_map().await?;
        let route = map
            .routes
            .iter()
            .find(|route| route.topic == topic)
            .cloned();
        let source_order = route
            .as_ref()
            .map(|route| route.source_order.as_slice())
            .unwrap_or(&[]);
        let sources = source_order
            .iter()
            .filter_map(|id| map.sources.iter().find(|source| &source.id == id).cloned())
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
        if fs::try_exists(&path).await? {
            self.load_map().await
        } else {
            Ok(KnowledgeMap::initial(now_stamp()))
        }
    }

    async fn load_map(&self) -> Result<KnowledgeMap, KnowledgeMapServiceError> {
        let content = fs::read_to_string(self.map_path()).await?;
        let map = serde_norway::from_str::<KnowledgeMap>(&content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        map.validate()?;
        Ok(map)
    }

    async fn write_map(&self, map: &KnowledgeMap) -> Result<(), KnowledgeMapServiceError> {
        map.validate()?;
        let dir = self.repository_root.join(AGENT_CONTRACT_DIR_NAME);
        fs::create_dir_all(&dir).await?;
        let path = self.map_path();
        let temp_path = dir.join(format!(
            "{KNOWLEDGE_MAP_FILE_NAME}.{}.{}.tmp",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        let yaml = serde_norway::to_string(map)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        fs::write(&temp_path, yaml).await?;
        if cfg!(target_os = "windows") && fs::try_exists(&path).await? {
            fs::remove_file(&path).await?;
        }
        fs::rename(&temp_path, &path).await?;
        Ok(())
    }

    async fn acquire_write_lock(&self) -> Result<KnowledgeMapWriteLock, KnowledgeMapServiceError> {
        let dir = self.repository_root.join(AGENT_CONTRACT_DIR_NAME);
        fs::create_dir_all(&dir).await?;
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
}

/// Error surfaced by the file-backed knowledge map service.
#[derive(Debug)]
pub enum KnowledgeMapServiceError {
    Io(std::io::Error),
    Yaml(String),
    Domain(crate::domain::DomainError),
    LockTimeout(PathBuf),
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
mod tests {
    use super::*;

    #[tokio::test]
    async fn writes_and_reads_yaml_contract() {
        let root = std::env::temp_dir().join(format!(
            "relay-knowledge-map-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time should work")
                .as_nanos()
        ));
        fs::create_dir_all(&root).await.expect("root should create");
        fs::write(
            root.join("AGENTS.md"),
            format!("Knowledge map: {KNOWLEDGE_MAP_RELATIVE_PATH}"),
        )
        .await
        .expect("agents should write");
        let service = KnowledgeMapService::new(root.clone());
        let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);

        service.init(&context).await.expect("init should work");
        service
            .add_source(
                &context,
                KnowledgeMapSourceAddRequest {
                    id: "build-cargo".to_owned(),
                    topic: "build".to_owned(),
                    kind: KnowledgeMapSourceKind::Config,
                    uri: "Cargo.toml".to_owned(),
                    source_scope: Some("repo".to_owned()),
                    description: None,
                },
            )
            .await
            .expect("source should add");
        service
            .update_source(
                &context,
                crate::domain::KnowledgeMapChange {
                    id: "build-cargo".to_owned(),
                    topic: None,
                    kind: None,
                    uri: None,
                    source_scope: None,
                    description: Some("Cargo package manifest".to_owned()),
                },
            )
            .await
            .expect("existing map should be replaceable");
        let route = service
            .route(&context, "build".to_owned())
            .await
            .expect("route should load");
        let validation = service
            .validate(&context)
            .await
            .expect("validate should run");

        assert_eq!(route.sources[0].id, "build-cargo");
        assert!(validation.valid);
        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn concurrent_source_adds_preserve_both_changes() {
        let root = std::env::temp_dir().join(format!(
            "relay-knowledge-map-concurrent-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time should work")
                .as_nanos()
        ));
        fs::create_dir_all(&root).await.expect("root should create");
        let service = KnowledgeMapService::new(root.clone());
        let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
        service.init(&context).await.expect("init should work");

        let first = service.add_source(
            &context,
            KnowledgeMapSourceAddRequest {
                id: "build-cargo".to_owned(),
                topic: "build".to_owned(),
                kind: KnowledgeMapSourceKind::Config,
                uri: "Cargo.toml".to_owned(),
                source_scope: Some("repo".to_owned()),
                description: None,
            },
        );
        let second = service.add_source(
            &context,
            KnowledgeMapSourceAddRequest {
                id: "build-readme".to_owned(),
                topic: "build".to_owned(),
                kind: KnowledgeMapSourceKind::Doc,
                uri: "README.md".to_owned(),
                source_scope: Some("repo".to_owned()),
                description: None,
            },
        );

        let (first, second) = tokio::join!(first, second);
        first.expect("first add should succeed");
        second.expect("second add should succeed");
        let route = service
            .route(&context, "build".to_owned())
            .await
            .expect("route should load");

        assert_eq!(route.sources.len(), 2);
        let _ = fs::remove_dir_all(root).await;
    }
}
