//! Retrieval request planning and optional derived backend adapters.
//!
//! Retrieval owns query-shape validation and budgets before the application
//! service asks storage and derived indexes for data.

use std::time::Duration;
use std::{error::Error, fmt};

pub mod provider;

use crate::domain::{
    FreshnessPolicy, GraphVersion, IndexKind, IndexStatus, RetrievalBackendState,
    RetrievalBackendStatus, RetrieverSource,
};

pub const LOCAL_SEMANTIC_MODEL: &str = "relay-local-token-semantic-v1";
pub const LOCAL_VECTOR_MODEL: &str = "relay-local-hash-ann-v1";
pub const LOCAL_VECTOR_DIMENSION: u32 = 16;
pub const DEFAULT_EMBEDDING_BATCH_SIZE: usize = 32;
pub const DEFAULT_EMBEDDING_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_EMBEDDING_MAX_CONCURRENCY: usize = 4;

/// Validated retrieval request with bounded result count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalPlan {
    pub query: String,
    pub source_scope: Option<String>,
    pub limit: usize,
    pub freshness: FreshnessPolicy,
}

/// Configured owner of a semantic or vector read model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadModelBackendMode {
    Local,
    External,
    Disabled,
}

/// Remote LLM provider family used for embedding calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingProviderKind {
    OpenAiCompatible,
    Echo,
}

impl EmbeddingProviderKind {
    /// Parses a stable environment/config value.
    pub fn parse(value: &str) -> Result<Self, EmbeddingProviderKindError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "openai_compatible" => Ok(Self::OpenAiCompatible),
            "echo" => Ok(Self::Echo),
            other => Err(EmbeddingProviderKindError {
                value: other.to_owned(),
            }),
        }
    }

    /// Stable configuration label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openai_compatible",
            Self::Echo => "echo",
        }
    }
}

/// Invalid embedding provider kind supplied by runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingProviderKindError {
    pub value: String,
}

impl fmt::Display for EmbeddingProviderKindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "embedding provider '{}' must be openai_compatible or echo",
            self.value
        )
    }
}

impl Error for EmbeddingProviderKindError {}

impl ReadModelBackendMode {
    /// Parses a stable environment/config value.
    pub fn parse(value: &str) -> Result<Self, ReadModelBackendModeError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "external" => Ok(Self::External),
            "disabled" => Ok(Self::Disabled),
            other => Err(ReadModelBackendModeError {
                value: other.to_owned(),
            }),
        }
    }

    /// Stable configuration label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::External => "external",
            Self::Disabled => "disabled",
        }
    }
}

/// Invalid read model backend mode supplied by runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadModelBackendModeError {
    pub value: String,
}

impl fmt::Display for ReadModelBackendModeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "retrieval backend '{}' must be local, external, or disabled",
            self.value
        )
    }
}

impl Error for ReadModelBackendModeError {}

/// Model metadata used by semantic/vector refresh workers and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadModelMetadata {
    pub name: String,
    pub dimension: u32,
}

/// Runtime configuration for a remote embedding provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEmbeddingConfig {
    pub provider: EmbeddingProviderKind,
    pub base_url: String,
    pub api_key: String,
    pub batch_size: usize,
    pub timeout: Duration,
    pub max_concurrency: usize,
}

impl RemoteEmbeddingConfig {
    /// Returns a URL label that is safe to expose in diagnostics.
    pub fn redacted_base_url(&self) -> String {
        redacted_url(&self.base_url)
    }
}

/// Runtime read model configuration shared by refresh and retrieval status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadModelBackendConfig {
    pub semantic_mode: ReadModelBackendMode,
    pub vector_mode: ReadModelBackendMode,
    pub semantic_model: ReadModelMetadata,
    pub vector_model: ReadModelMetadata,
    pub image_model: ReadModelMetadata,
    pub remote_embedding: Option<RemoteEmbeddingConfig>,
}

impl ReadModelBackendConfig {
    /// Uses the built-in deterministic read models.
    pub fn local() -> Self {
        Self {
            semantic_mode: ReadModelBackendMode::Local,
            vector_mode: ReadModelBackendMode::Local,
            semantic_model: ReadModelMetadata {
                name: LOCAL_SEMANTIC_MODEL.to_owned(),
                dimension: LOCAL_VECTOR_DIMENSION,
            },
            vector_model: ReadModelMetadata {
                name: LOCAL_VECTOR_MODEL.to_owned(),
                dimension: LOCAL_VECTOR_DIMENSION,
            },
            image_model: ReadModelMetadata {
                name: "relay-local-image-hash-v1".to_owned(),
                dimension: LOCAL_VECTOR_DIMENSION,
            },
            remote_embedding: None,
        }
    }

    /// Returns whether local index refresh should maintain an index family.
    pub fn refreshes_index(&self, kind: IndexKind) -> bool {
        match kind {
            IndexKind::Bm25 => true,
            IndexKind::Semantic => self.semantic_mode != ReadModelBackendMode::Disabled,
            IndexKind::Vector => self.vector_mode != ReadModelBackendMode::Disabled,
        }
    }

    /// Returns read-model retrievers that must not execute for a request.
    pub fn disabled_retriever_sources(&self) -> Vec<RetrieverSource> {
        let mut disabled = Vec::new();
        if self.semantic_mode == ReadModelBackendMode::Disabled {
            disabled.push(RetrieverSource::Semantic);
        }
        if self.vector_mode == ReadModelBackendMode::Disabled {
            disabled.push(RetrieverSource::Vector);
        }

        disabled
    }
}

fn redacted_url(value: &str) -> String {
    let trimmed = value.trim();
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return trimmed.to_owned();
    };
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.is_empty() {
        return scheme.to_owned();
    }

    format!("{scheme}://{authority}")
}

/// Builds semantic/vector backend status from configured read models and index cursors.
pub fn read_model_backend_statuses(
    plan: &RetrievalPlan,
    graph_version: GraphVersion,
    indexes: &[IndexStatus],
    config: &ReadModelBackendConfig,
) -> Vec<RetrievalBackendStatus> {
    [
        (
            RetrieverSource::Semantic,
            IndexKind::Semantic,
            config.semantic_mode,
            &config.semantic_model,
        ),
        (
            RetrieverSource::Vector,
            IndexKind::Vector,
            config.vector_mode,
            &config.vector_model,
        ),
    ]
    .into_iter()
    .map(|(source, kind, mode, metadata)| {
        read_model_backend_status(source, kind, mode, metadata, plan, graph_version, indexes)
    })
    .collect()
}

fn read_model_backend_status(
    source: RetrieverSource,
    kind: IndexKind,
    mode: ReadModelBackendMode,
    metadata: &ReadModelMetadata,
    plan: &RetrievalPlan,
    graph_version: GraphVersion,
    indexes: &[IndexStatus],
) -> RetrievalBackendStatus {
    if mode == ReadModelBackendMode::Disabled {
        return RetrievalBackendStatus {
            source,
            state: RetrievalBackendState::Unavailable,
            scope_post_filter: plan.source_scope.is_some(),
            indexed_graph_version: None,
            reason: Some(format!(
                "{} read model disabled by configuration",
                source.as_str()
            )),
        };
    }

    let Some(index) = indexes.iter().find(|status| status.kind == kind) else {
        return RetrievalBackendStatus {
            source,
            state: RetrievalBackendState::Unavailable,
            scope_post_filter: plan.source_scope.is_some(),
            indexed_graph_version: None,
            reason: Some(format!("{} index metadata is unavailable", source.as_str())),
        };
    };
    let stale = index.is_stale_for(graph_version);
    let reason = if stale {
        format!(
            "{} read model index is stale at graph version {} while graph is {}; configured {} backend model={} dimension={}",
            source.as_str(),
            index.indexed_graph_version.get(),
            graph_version.get(),
            mode.as_str(),
            metadata.name,
            metadata.dimension
        )
    } else {
        format!(
            "{} read model available through {} backend model={} dimension={}",
            source.as_str(),
            mode.as_str(),
            metadata.name,
            metadata.dimension
        )
    };

    RetrievalBackendStatus {
        source,
        state: if stale {
            RetrievalBackendState::Degraded
        } else {
            RetrievalBackendState::Available
        },
        scope_post_filter: plan.source_scope.is_some(),
        indexed_graph_version: Some(index.indexed_graph_version),
        reason: Some(reason),
    }
}

impl RetrievalPlan {
    /// Validates query text and result limits.
    pub fn new(
        query: impl Into<String>,
        source_scope: Option<String>,
        limit: usize,
        freshness: FreshnessPolicy,
    ) -> Result<Self, RetrievalPlanError> {
        let query = query.into();
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err(RetrievalPlanError::EmptyQuery);
        }

        let limit = match limit {
            1..=50 => limit,
            0 => return Err(RetrievalPlanError::ZeroLimit),
            _ => return Err(RetrievalPlanError::LimitTooLarge { max: 50 }),
        };

        Ok(Self {
            query: trimmed.to_owned(),
            source_scope,
            limit,
            freshness,
        })
    }
}

/// Retrieval planning error mapped to stable API errors by application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetrievalPlanError {
    EmptyQuery,
    ZeroLimit,
    LimitTooLarge { max: usize },
}

impl fmt::Display for RetrievalPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyQuery => write!(formatter, "query must not be empty"),
            Self::ZeroLimit => write!(formatter, "limit must be greater than zero"),
            Self::LimitTooLarge { max } => write!(formatter, "limit must be {max} or less"),
        }
    }
}

impl Error for RetrievalPlanError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_query_and_preserves_retrieval_policy() {
        let plan = RetrievalPlan::new(
            " SQLite ",
            Some("docs".to_owned()),
            5,
            FreshnessPolicy::GraphOnly,
        )
        .expect("plan should validate");

        assert_eq!(plan.query, "SQLite");
        assert_eq!(plan.source_scope, Some("docs".to_owned()));
        assert_eq!(plan.limit, 5);
        assert_eq!(plan.freshness, FreshnessPolicy::GraphOnly);
    }

    #[test]
    fn rejects_empty_and_unbounded_queries() {
        let empty = RetrievalPlan::new(" ", None, 1, FreshnessPolicy::AllowStale)
            .expect_err("empty query should fail");
        let zero = RetrievalPlan::new("x", None, 0, FreshnessPolicy::AllowStale)
            .expect_err("zero limit should fail");
        let too_large = RetrievalPlan::new("x", None, 51, FreshnessPolicy::AllowStale)
            .expect_err("large limit should fail");

        assert_eq!(empty.to_string(), "query must not be empty");
        assert_eq!(zero.to_string(), "limit must be greater than zero");
        assert_eq!(too_large.to_string(), "limit must be 50 or less");
    }

    #[test]
    fn read_model_statuses_report_available_local_backends() {
        let plan = RetrievalPlan::new(
            "SQLite",
            Some("docs".to_owned()),
            5,
            FreshnessPolicy::AllowStale,
        )
        .expect("plan should validate");
        let statuses = read_model_backend_statuses(
            &plan,
            GraphVersion::new(7),
            &[
                IndexStatus {
                    kind: IndexKind::Semantic,
                    index_version: 1,
                    indexed_graph_version: GraphVersion::new(7),
                    state: crate::domain::IndexState::Fresh,
                    last_error: None,
                },
                IndexStatus {
                    kind: IndexKind::Vector,
                    index_version: 1,
                    indexed_graph_version: GraphVersion::new(7),
                    state: crate::domain::IndexState::Fresh,
                    last_error: None,
                },
            ],
            &ReadModelBackendConfig::local(),
        );

        assert_eq!(statuses[0].state, RetrievalBackendState::Available);
        assert_eq!(statuses[1].state, RetrievalBackendState::Available);
        assert!(statuses.iter().all(|status| status.scope_post_filter));
    }

    #[test]
    fn read_model_statuses_report_stale_or_disabled_backends() {
        let plan = RetrievalPlan::new("SQLite", None, 5, FreshnessPolicy::AllowStale)
            .expect("plan should validate");
        let mut config = ReadModelBackendConfig::local();
        config.vector_mode = ReadModelBackendMode::Disabled;

        let statuses = read_model_backend_statuses(
            &plan,
            GraphVersion::new(9),
            &[IndexStatus {
                kind: IndexKind::Semantic,
                index_version: 1,
                indexed_graph_version: GraphVersion::new(8),
                state: crate::domain::IndexState::Fresh,
                last_error: None,
            }],
            &config,
        );

        assert_eq!(statuses[0].state, RetrievalBackendState::Degraded);
        assert_eq!(statuses[1].state, RetrievalBackendState::Unavailable);
    }
}
