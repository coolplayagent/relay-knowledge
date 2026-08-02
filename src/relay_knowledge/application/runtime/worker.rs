use std::{error::Error, fmt};

use crate::{domain::WorkerKind, env::EnvironmentConfig};

/// External worker runtime configuration and deterministic fallback policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRuntimeConfig {
    pub embedding_endpoint: Option<String>,
    pub ocr_endpoint: Option<String>,
    pub vision_endpoint: Option<String>,
    pub extractor_endpoint: Option<String>,
    pub max_in_flight: usize,
    pub code_index_max_in_flight: usize,
    pub silent_updates_enabled: bool,
}

impl WorkerRuntimeConfig {
    pub const DEFAULT_MAX_IN_FLIGHT: usize = 2;
    pub const DEFAULT_CODE_INDEX_MAX_IN_FLIGHT: usize = 2;
    pub const MAX_CODE_INDEX_MAX_IN_FLIGHT: usize = 8;

    /// Builds worker config from typed environment overrides.
    pub fn from_environment(
        environment: &EnvironmentConfig,
    ) -> Result<Self, WorkerRuntimeConfigError> {
        Ok(Self {
            embedding_endpoint: validate_worker_endpoint(
                environment.workers.embedding_endpoint.clone(),
            )?,
            ocr_endpoint: validate_worker_endpoint(environment.workers.ocr_endpoint.clone())?,
            vision_endpoint: validate_worker_endpoint(environment.workers.vision_endpoint.clone())?,
            extractor_endpoint: validate_worker_endpoint(
                environment.workers.extractor_endpoint.clone(),
            )?,
            max_in_flight: environment
                .workers
                .max_in_flight
                .unwrap_or(Self::DEFAULT_MAX_IN_FLIGHT),
            code_index_max_in_flight: environment
                .workers
                .code_index_max_in_flight
                .unwrap_or(Self::DEFAULT_CODE_INDEX_MAX_IN_FLIGHT)
                .min(Self::MAX_CODE_INDEX_MAX_IN_FLIGHT),
            silent_updates_enabled: environment.workers.silent_updates_enabled.unwrap_or(false),
        })
    }

    /// Returns the configured endpoint for a worker kind.
    pub fn endpoint_for(&self, kind: WorkerKind) -> Option<&str> {
        match kind {
            WorkerKind::Embedding => self.embedding_endpoint.as_deref(),
            WorkerKind::Ocr => self.ocr_endpoint.as_deref(),
            WorkerKind::Vision => self.vision_endpoint.as_deref(),
            WorkerKind::Extractor => self.extractor_endpoint.as_deref(),
        }
    }
}

/// Worker runtime configuration validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerRuntimeConfigError {
    InvalidEndpoint(String),
}

impl fmt::Display for WorkerRuntimeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEndpoint(value) => write!(
                formatter,
                "worker endpoint '{value}' must use http:// and include a host"
            ),
        }
    }
}

impl Error for WorkerRuntimeConfigError {}

fn validate_worker_endpoint(
    value: Option<String>,
) -> Result<Option<String>, WorkerRuntimeConfigError> {
    value
        .map(|endpoint| {
            let trimmed = endpoint.trim();
            if is_valid_worker_http_endpoint(trimmed) {
                Ok(trimmed.to_owned())
            } else {
                Err(WorkerRuntimeConfigError::InvalidEndpoint(endpoint))
            }
        })
        .transpose()
}

fn is_valid_worker_http_endpoint(value: &str) -> bool {
    let Some(remainder) = value.strip_prefix("http://") else {
        return false;
    };
    let authority = remainder
        .split_once('/')
        .map_or(remainder, |(authority, _)| authority);
    if authority.is_empty() || authority.contains(char::is_whitespace) {
        return false;
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        return !host.is_empty() && port.parse::<u16>().is_ok_and(|port| port > 0);
    }

    !authority.is_empty()
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
