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
    /// Authorizes static scopes first, then promotes runtime-proven code scopes.
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
        if self
            .code_repository_alias_is_registered(service, &scope)
            .await
            || self
                .repository_set_alias_members_are_authorized(service, policy, &scope)
                .await
        {
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

    async fn code_repository_alias_is_registered(
        &self,
        service: &RelayKnowledgeService,
        scope: &str,
    ) -> bool {
        service
            .code_repository_is_registered(scope.to_owned())
            .await
            .unwrap_or(false)
    }

    async fn repository_set_alias_members_are_authorized(
        &self,
        service: &RelayKnowledgeService,
        policy: &AgentAccessPolicy,
        scope: &str,
    ) -> bool {
        let Ok(Some(members)) = service
            .code_repository_set_member_scopes(scope.to_owned())
            .await
        else {
            return false;
        };
        if members.is_empty() {
            return false;
        }
        for (repository_alias, source_scope) in members {
            if !self
                .member_scope_is_authorized(policy, &repository_alias, &source_scope)
                .await
            {
                return false;
            }
        }

        true
    }

    async fn member_scope_is_authorized(
        &self,
        policy: &AgentAccessPolicy,
        repository_alias: &str,
        source_scope: &str,
    ) -> bool {
        policy
            .allowed_scopes
            .iter()
            .any(|allowed| allowed == repository_alias || allowed == source_scope)
            || self.runtime_allowed(repository_alias).await
            || self.runtime_allowed(source_scope).await
    }
}

fn mcp_scope_not_authorized(scope: &str) -> AgentAdapterError {
    AgentAdapterError::new(
        AgentAdapterErrorKind::PermissionDenied,
        format!(
            "source_scope '{scope}' is not authorized for this MCP policy; register a code repository alias during runtime, add RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES={scope}, or allow every repository-set member before using a repository-set alias"
        ),
    )
}
