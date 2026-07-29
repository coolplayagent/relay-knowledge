use serde::{Deserialize, Serialize};

use super::{DomainError, error::required_text};

/// Authorized source boundary for evidence and retrieval.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceScope(String);

impl SourceScope {
    /// Validates a source scope supplied by an interface adapter.
    pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
        let scope = required_text("source_scope", value)?;
        if scope.contains('\0') {
            return Err(DomainError::invalid(
                "source_scope",
                "must not contain NUL bytes",
            ));
        }

        Ok(Self(scope))
    }

    /// Returns the normalized scope identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<SourceScope> for String {
    fn from(scope: SourceScope) -> Self {
        scope.0
    }
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod tests;
