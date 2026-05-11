use crate::{
    api::{ApiMetadata, ProjectStatusResponse, RequestContext},
    domain::GraphVersion,
    env::EnvironmentConfig,
    project::PROJECT_NAME,
};

use super::{RuntimeConfiguration, RuntimeConfigurationError, status::runtime_status};

/// Shared application service used by CLI, Web, and future API adapters.
#[derive(Debug, Clone)]
pub struct RelayKnowledgeService {
    runtime: RuntimeConfiguration,
}

impl RelayKnowledgeService {
    /// Creates a service from already validated foundational configuration.
    pub fn new(runtime: RuntimeConfiguration) -> Self {
        Self { runtime }
    }

    /// Creates a service by reading the current process environment once.
    pub async fn from_process_environment() -> Result<Self, RuntimeConfigurationError> {
        RuntimeConfiguration::from_process_environment()
            .await
            .map(Self::new)
    }

    /// Creates a service from a deterministic environment snapshot.
    pub async fn from_environment(
        environment: &EnvironmentConfig,
    ) -> Result<Self, RuntimeConfigurationError> {
        RuntimeConfiguration::from_environment(environment)
            .await
            .map(Self::new)
    }

    /// Applies network-related settings from a typed environment snapshot.
    pub async fn refresh_network_from_environment(
        &self,
        environment: &EnvironmentConfig,
    ) -> Result<(), RuntimeConfigurationError> {
        self.runtime
            .network
            .refresh_from_environment(environment)
            .map(|_| ())
            .map_err(RuntimeConfigurationError::Network)
    }

    /// Re-reads process environment variables and applies network changes.
    pub async fn refresh_network_from_process_environment(
        &self,
    ) -> Result<(), RuntimeConfigurationError> {
        self.runtime
            .network
            .refresh_from_process_environment()
            .map(|_| ())
            .map_err(RuntimeConfigurationError::NetworkRuntime)
    }

    /// Returns the current project status through the unified API contract.
    pub async fn project_status(&self, context: RequestContext) -> ProjectStatusResponse {
        ProjectStatusResponse {
            project_name: PROJECT_NAME.to_owned(),
            metadata: ApiMetadata::graph_only(&context, GraphVersion::ZERO),
            runtime: runtime_status(&self.runtime),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{api::InterfaceKind, env::PlatformKind};

    #[tokio::test]
    async fn status_includes_foundational_runtime_configuration() {
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("HOME", "/home/alice"),
                ("TMPDIR", "/tmp"),
                ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
                ("RELAY_KNOWLEDGE_HTTP_BIND", "127.0.0.1:9000"),
                ("HTTPS_PROXY", "https://proxy.internal:8443"),
                ("NO_PROXY", "localhost,.internal"),
                ("SSL_VERIFY", "false"),
                ("RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH", "42"),
            ],
        )
        .expect("environment should parse");
        let service = RelayKnowledgeService::from_environment(&environment)
            .await
            .expect("service should compose");
        let context = RequestContext::with_ids(InterfaceKind::Cli, "req", "trace");

        let response = service.project_status(context).await;

        assert_eq!(response.runtime.config_dir, "/srv/relay/config");
        assert_eq!(response.runtime.data_dir, "/srv/relay/data");
        assert_eq!(response.runtime.http_bind, "127.0.0.1:9000");
        assert!(response.runtime.http_proxy_configured);
        assert_eq!(response.runtime.http_no_proxy_rules, 2);
        assert!(!response.runtime.http_ssl_verify);
        assert_eq!(response.runtime.qos_max_queue_depth, 42);
    }

    #[tokio::test]
    async fn status_reflects_refreshed_network_environment() {
        let initial_environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("HOME", "/home/alice"),
                ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
            ],
        )
        .expect("environment should parse");
        let service = RelayKnowledgeService::from_environment(&initial_environment)
            .await
            .expect("service should compose");

        let refreshed_environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("HTTP_PROXY", "http://proxy.internal:8080"),
                ("SSL_VERIFY", "false"),
                ("RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS", "4"),
            ],
        )
        .expect("environment should parse");

        service
            .refresh_network_from_environment(&refreshed_environment)
            .await
            .expect("network refresh should succeed");
        let response = service
            .project_status(RequestContext::with_ids(InterfaceKind::Cli, "req", "trace"))
            .await;

        assert!(response.runtime.http_proxy_configured);
        assert!(!response.runtime.http_ssl_verify);
        assert_eq!(response.runtime.qos_max_in_flight_requests, 4);
    }
}
