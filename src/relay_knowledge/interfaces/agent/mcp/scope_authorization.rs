use std::{collections::BTreeSet, sync::Arc};

use tokio::sync::RwLock;

use crate::{
    api::AgentAccessPolicy,
    application::RelayKnowledgeService,
    interfaces::agent::{AgentAdapterError, AgentAdapterErrorKind, normalize_scope_for_policy},
};

/// Process-local supplement for MCP scopes proven safe at runtime.
#[derive(Clone, Default)]
pub(super) struct RuntimeScopeAuthorizer {
    allowed_scopes: Arc<RwLock<BTreeSet<String>>>,
}

impl RuntimeScopeAuthorizer {
    /// Authorizes static scopes first, then promotes registered repository aliases on demand.
    pub(super) async fn authorize_scope(
        &self,
        service: &RelayKnowledgeService,
        policy: &AgentAccessPolicy,
        scope: Option<String>,
    ) -> Result<Option<String>, AgentAdapterError> {
        let Some(scope) = normalize_scope_for_policy(scope, policy.allow_unspecified_scope)? else {
            return Ok(None);
        };

        if policy
            .allowed_scopes
            .iter()
            .any(|allowed| allowed == &scope)
            || self.runtime_allowed(&scope).await
        {
            return Ok(Some(scope));
        }
        if self.repository_alias_is_registered(service, &scope).await {
            self.remember_runtime_scope(scope.clone()).await;
            return Ok(Some(scope));
        }

        Err(mcp_scope_not_authorized(&scope))
    }

    async fn runtime_allowed(&self, scope: &str) -> bool {
        self.allowed_scopes.read().await.contains(scope)
    }

    async fn remember_runtime_scope(&self, scope: String) {
        self.allowed_scopes.write().await.insert(scope);
    }

    async fn repository_alias_is_registered(
        &self,
        service: &RelayKnowledgeService,
        scope: &str,
    ) -> bool {
        service
            .code_repository_is_registered(scope.to_owned())
            .await
            .unwrap_or(false)
    }
}

fn mcp_scope_not_authorized(scope: &str) -> AgentAdapterError {
    AgentAdapterError::new(
        AgentAdapterErrorKind::PermissionDenied,
        format!(
            "source_scope '{scope}' is not authorized for this MCP policy; register it as a code repository alias during runtime or add RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES={scope}"
        ),
    )
}
