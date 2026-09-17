use serde::{Deserialize, Serialize};

use super::super::{CodeRepositorySelector, DomainError, FreshnessPolicy};

/// Query kind for repository-scoped software global model facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoftwareGlobalKind {
    Dependencies,
    Sdks,
    Files,
    Topics,
    Relationships,
    Build,
    /// One Maven reactor module per POM and its declared dependency edges.
    Modules,
    Iac,
    Design,
    Systems,
    Apis,
    Resources,
    Tests,
    Deployments,
    Releases,
    Statements,
    Conflicts,
    All,
}

impl SoftwareGlobalKind {
    /// Stable CLI, API, and storage-facing representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dependencies => "dependencies",
            Self::Sdks => "sdks",
            Self::Files => "files",
            Self::Topics => "topics",
            Self::Relationships => "relationships",
            Self::Build => "build",
            Self::Modules => "modules",
            Self::Iac => "iac",
            Self::Design => "design",
            Self::Systems => "systems",
            Self::Apis => "apis",
            Self::Resources => "resources",
            Self::Tests => "tests",
            Self::Deployments => "deployments",
            Self::Releases => "releases",
            Self::Statements => "statements",
            Self::Conflicts => "conflicts",
            Self::All => "all",
        }
    }
}

/// Repository-scoped software global model query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareGlobalRequest {
    pub repository: CodeRepositorySelector,
    pub kind: SoftwareGlobalKind,
    pub freshness_policy: FreshnessPolicy,
    pub limit: usize,
    /// Opaque continuation for dependencies/modules, bound to the indexed scope and filters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

impl SoftwareGlobalRequest {
    /// Validates the requested result bound while preserving repository scope.
    pub fn new(
        repository: CodeRepositorySelector,
        kind: SoftwareGlobalKind,
        freshness_policy: FreshnessPolicy,
        limit: usize,
    ) -> Result<Self, DomainError> {
        let limit = match limit {
            1..=500 => limit,
            0 => return Err(DomainError::invalid("limit", "must be greater than zero")),
            _ => return Err(DomainError::invalid("limit", "must be 500 or less")),
        };

        Ok(Self {
            repository,
            kind,
            freshness_policy,
            limit,
            cursor: None,
        })
    }

    /// Rejects invalid page bounds and cursors on unsupported projection kinds.
    pub fn with_cursor(mut self, cursor: Option<String>) -> Result<Self, DomainError> {
        self.cursor = cursor;
        self.validate()?;
        Ok(self)
    }

    /// Validates deserialized requests as well as constructor-built requests.
    pub fn validate(&self) -> Result<(), DomainError> {
        if !(1..=500).contains(&self.limit) {
            return Err(DomainError::invalid("limit", "must be between 1 and 500"));
        }
        if let Some(cursor) = &self.cursor {
            if !matches!(
                self.kind,
                SoftwareGlobalKind::Dependencies | SoftwareGlobalKind::Modules
            ) || cursor.is_empty()
                || cursor.len() > 4096
                || self.freshness_policy == FreshnessPolicy::GraphOnly
            {
                return Err(DomainError::invalid(
                    "cursor",
                    "requires dependencies/modules, a non-graph-only policy and 1..4096 bytes",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
