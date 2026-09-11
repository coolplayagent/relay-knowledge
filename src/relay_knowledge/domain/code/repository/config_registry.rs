//! Static configuration evidence shared by every query interface.
use crate::domain::DomainError;
use serde::{Deserialize, Serialize};

/// Metadata is source evidence; absent values are unknown, never runtime defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CodeConfigMetadata {
    pub source_format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hot_reload: Option<bool>,
    /// Fully qualified constants/getters, or parent types on internal hierarchy facts.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<String>,
    /// A symbolic key/getter dependency, resolved only in the served snapshot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<String>,
    /// Connects a guarded location to the concrete read that supplied its value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_usage_id: Option<String>,
    /// Static read whose enclosing getter result cannot be linked soundly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow_incomplete: Option<String>,
}

/// Optional filters apply to whole configuration groups, retaining their usage evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CodeConfigFilter {
    pub domain: Option<String>,
    pub source: Option<String>,
    pub hot_reload: Option<bool>,
    pub consistency: bool,
}
impl CodeConfigFilter {
    /// Normalize text and reject unsupported source formats before query work starts.
    pub fn validate(mut self) -> Result<Self, DomainError> {
        for (field, value) in [("domain", &mut self.domain), ("source", &mut self.source)] {
            if let Some(text) = value {
                *text = text.trim().to_lowercase();
                if text.is_empty() || text.len() > 128 {
                    return Err(DomainError::invalid(field, "must contain 1 to 128 bytes"));
                }
            }
        }
        if self.source.as_deref().is_some_and(|source| {
            !matches!(source, "java" | "properties" | "ini" | "ctmpl" | "shell")
        }) {
            return Err(DomainError::invalid(
                "source",
                "expected java, properties, ini, ctmpl or shell",
            ));
        }
        Ok(self)
    }
}
#[cfg(test)]
#[path = "config_registry_tests.rs"]
mod tests;
