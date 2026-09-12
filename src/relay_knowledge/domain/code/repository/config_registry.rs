//! Static configuration evidence shared by every query interface.
use crate::domain::DomainError;
use serde::{Deserialize, Serialize};

/// Metadata is source evidence; absent values are unknown, never runtime defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CodeConfigMetadata {
    pub source_format: String,
    /// Same-package type whose presence invalidates an implicit java.lang read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implicit_platform_owner: Option<String>,
    /// Same-package types that would invalidate transparent getter conversions.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub conversion_platform_owners: Vec<String>,
    /// Whether a proven getter participates in Java overriding; absent for other facts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub getter_overridable: Option<bool>,
    /// Declaring getter identity, independent of inherited provider aliases.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_getter: Option<String>,
    /// Getter identities inherited without an intervening declaration.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inherited_getters: Vec<String>,
    /// Private methods cannot be inherited, unlike static methods.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub getter_inheritable: Option<bool>,
    /// Declaring Java package for internal types and getter providers, including the empty package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub java_package: Option<String>,
    /// Declared Java field names and access levels, including nonconstant hiding fields.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub java_fields: std::collections::BTreeMap<String, String>,
    /// Same-package alternatives for wildcard-ambiguous supertypes.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub same_package_parents: std::collections::BTreeMap<String, String>,
    /// String-compatible platform-name member signatures and visibility.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub java_methods: std::collections::BTreeMap<String, String>,
    /// Lexical member signature that takes precedence over a static platform import.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub static_import_reference: Option<String>,
    /// Default supplied by a proven Boolean conversion of a property read.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub boolean_converted_default: bool,
    /// Raw property fallback restored if the apparent Boolean conversion is shadowed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unconverted_default: Option<String>,

    /// Java getter visibility: public, protected, private, or package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub getter_visibility: Option<String>,
    /// Same-package candidate to check before treating wildcard imports as ambiguous.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub same_package_reference: Option<String>,
    /// Construction and super calls resolve only against the declaring provider.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub exact_reference: bool,
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
            !matches!(
                source,
                "java" | "properties" | "ini" | "ctmpl" | "shell" | "dotenv"
            )
        }) {
            return Err(DomainError::invalid(
                "source",
                "expected java, properties, ini, ctmpl, shell or dotenv",
            ));
        }
        Ok(self)
    }
}
#[cfg(test)]
#[path = "config_registry_tests.rs"]
mod tests;
