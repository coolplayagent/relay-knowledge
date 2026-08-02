use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};

/// Freshness policy for hybrid retrieval.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessPolicy {
    #[default]
    AllowStale,
    WaitUntilFresh,
    GraphOnly,
}

/// Retrieval path used to satisfy a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalMode {
    Hybrid,
    GraphOnly,
}

/// Rerank backend requested for the hybrid retrieval candidate set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RerankMode {
    Local,
    External,
    Disabled,
}

impl RerankMode {
    /// Parses a stable environment/config value.
    pub fn parse(value: &str) -> Result<Self, RerankModeError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "external" => Ok(Self::External),
            "disabled" => Ok(Self::Disabled),
            other => Err(RerankModeError {
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

/// Invalid rerank backend mode supplied by runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RerankModeError {
    pub value: String,
}

impl fmt::Display for RerankModeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "rerank backend '{}' must be local, external, or disabled",
            self.value
        )
    }
}

impl Error for RerankModeError {}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
