use std::{error::Error, fmt, future::Future};

use axum::{
    Router,
    routing::{get, post},
};
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};

use crate::net::http::HttpServeError;

use super::super::{http_contract::ensure_remote_bind_allowed, metrics};
use super::{
    dispatch::{handle_mcp_delete, handle_mcp_post},
    server::McpServer,
};

impl McpServer {
    /// Builds the Streamable HTTP router without opening sockets.
    pub fn router(self) -> Router {
        let config = self.network.current();
        let endpoint = self.agent.mcp_endpoint.clone();
        let metrics_endpoint = metrics::metrics_endpoint(&endpoint);
        let body_limit = usize::try_from(config.http.max_request_body_bytes).unwrap_or(usize::MAX);

        Router::new()
            .route(&endpoint, post(handle_mcp_post))
            .route(&endpoint, axum::routing::delete(handle_mcp_delete))
            .route(&metrics_endpoint, get(metrics::handle_metrics_get))
            .with_state(self)
            .layer(TraceLayer::new_for_http())
            .layer(RequestBodyLimitLayer::new(body_limit))
    }

    /// Starts the MCP HTTP listener through `net::http`.
    pub async fn serve_until_shutdown(
        self,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> Result<(), McpServeError> {
        let network_config = self.network.current();
        let config = network_config.http;
        let qos_policy = network_config.qos;
        let qos = self.qos.clone();
        let router = self.checked_router()?;

        crate::net::http::serve_router_with_qos(router, config, qos, qos_policy, shutdown)
            .await
            .map_err(McpServeError::Http)
    }

    /// Builds the Streamable HTTP router after validating listener policy.
    pub fn checked_router(self) -> Result<Router, McpServeError> {
        if !self.agent.mcp_streamable_http_enabled {
            return Err(McpServeError::Disabled);
        }
        ensure_remote_bind_allowed(&self.network.current().http, &self.agent.access_policy)?;

        Ok(self.router())
    }
}

/// MCP server startup error.
#[derive(Debug)]
pub enum McpServeError {
    Disabled,
    RemoteBindDisabled,
    Http(HttpServeError),
}

impl fmt::Display for McpServeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => write!(formatter, "MCP Streamable HTTP is not enabled"),
            Self::RemoteBindDisabled => {
                write!(
                    formatter,
                    "MCP remote bind requires allow_remote_clients=true"
                )
            }
            Self::Http(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for McpServeError {}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
