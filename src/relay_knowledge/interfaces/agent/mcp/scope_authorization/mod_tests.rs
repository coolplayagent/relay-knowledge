use super::{RuntimeScopeAuthorizer, mcp_repository_set_not_authorized, mcp_scope_not_authorized};
use crate::interfaces::agent::AgentAdapterErrorKind;

#[tokio::test]
async fn runtime_repository_scope_cache_is_explicit_and_idempotent() {
    let authorizer = RuntimeScopeAuthorizer::default();

    assert!(!authorizer.runtime_repository_allowed("repo").await);
    authorizer
        .remember_runtime_repository_scope("repo".to_owned())
        .await;
    authorizer
        .remember_runtime_repository_scope("repo".to_owned())
        .await;

    assert!(authorizer.runtime_repository_allowed("repo").await);
    assert_eq!(authorizer.allowed_repository_scopes.read().await.len(), 1);
}

#[test]
fn authorization_errors_distinguish_repository_and_repository_set_scopes() {
    let repository = mcp_scope_not_authorized("repo");
    let repository_set = mcp_repository_set_not_authorized("workspace");

    assert_eq!(repository.kind, AgentAdapterErrorKind::PermissionDenied);
    assert!(repository.message.contains("source_scope 'repo'"));
    assert_eq!(repository_set.kind, AgentAdapterErrorKind::PermissionDenied);
    assert!(
        repository_set
            .message
            .contains("repository_set 'workspace'")
    );
}
