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
mod tests {
    use super::*;

    #[test]
    fn trims_and_preserves_source_scope() {
        let scope = SourceScope::parse(" docs/specs ").expect("scope should parse");

        assert_eq!(scope.as_str(), "docs/specs");
    }

    #[test]
    fn rejects_empty_source_scope() {
        let error = SourceScope::parse(" ").expect_err("empty scope should fail");

        assert_eq!(error.field, "source_scope");
    }

    #[test]
    fn rejects_nul_bytes_and_converts_to_string() {
        let error = SourceScope::parse("repo\0branch").expect_err("NUL should fail");
        let scope: String = SourceScope::parse("repo")
            .expect("scope should parse")
            .into();

        assert_eq!(error.field, "source_scope");
        assert_eq!(scope, "repo");
    }
}
