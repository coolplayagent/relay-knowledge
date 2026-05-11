//! Application services that orchestrate domain behavior behind stable API types.

use crate::{
    api::{ApiMetadata, ProjectStatusResponse, RequestContext},
    domain::GraphVersion,
    project_name,
};

/// Shared application service used by CLI, Web, and future API adapters.
#[derive(Debug, Clone, Default)]
pub struct RelayKnowledgeService;

impl RelayKnowledgeService {
    /// Creates a service with the default local composition.
    pub fn new() -> Self {
        Self
    }

    /// Returns the current project status through the unified API contract.
    pub fn project_status(&self, context: RequestContext) -> ProjectStatusResponse {
        ProjectStatusResponse {
            project_name: project_name().to_owned(),
            metadata: ApiMetadata::graph_only(&context, GraphVersion::ZERO),
        }
    }
}
