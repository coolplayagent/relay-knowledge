use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::domain::DomainError;

pub const BUSINESS_GLOSSARY_SCHEMA_VERSION: u16 = 1;
pub const BUSINESS_GLOSSARY_MAX_BYTES: usize = 4 * 1024 * 1024;
pub const BUSINESS_GLOSSARY_MAX_DOMAINS: usize = 256;
pub const BUSINESS_GLOSSARY_MAX_TERMS: usize = 10_000;
pub const BUSINESS_TERM_MAX_ALIASES: usize = 32;
pub const BUSINESS_TERM_MAX_MAPPINGS: usize = 64;

/// Authored repository business glossary schema v1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessGlossary {
    pub schema_version: u16,
    #[serde(default)]
    pub domains: Vec<BusinessDomainDefinition>,
    #[serde(default)]
    pub terms: Vec<BusinessTermDefinition>,
}

impl BusinessGlossary {
    /// Minimal valid document written by `map init`.
    pub const fn empty_v1() -> Self {
        Self {
            schema_version: BUSINESS_GLOSSARY_SCHEMA_VERSION,
            domains: Vec::new(),
            terms: Vec::new(),
        }
    }

    /// Parses and validates a size-bounded authored glossary.
    pub fn parse(content: &[u8]) -> Result<Self, DomainError> {
        if content.len() > BUSINESS_GLOSSARY_MAX_BYTES {
            return Err(DomainError::invalid(
                "business_glossary",
                "must be 4194304 bytes or less",
            ));
        }
        let text = std::str::from_utf8(content)
            .map_err(|_| DomainError::invalid("business_glossary", "must be valid UTF-8"))?;
        let glossary = serde_norway::from_str::<Self>(text)
            .map_err(|error| DomainError::invalid("business_glossary", error.to_string()))?;
        glossary.validate()?;
        Ok(glossary)
    }

    /// Enforces v1 cardinality, identity, and field-size bounds.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != BUSINESS_GLOSSARY_SCHEMA_VERSION {
            return Err(DomainError::invalid("schema_version", "must be 1"));
        }
        enforce_count("domains", self.domains.len(), BUSINESS_GLOSSARY_MAX_DOMAINS)?;
        enforce_count("terms", self.terms.len(), BUSINESS_GLOSSARY_MAX_TERMS)?;

        let mut domain_ids = HashSet::new();
        for domain in &self.domains {
            domain.validate()?;
            if !domain_ids.insert(domain.id.as_str()) {
                return Err(DomainError::invalid("domains", "domain ids must be unique"));
            }
        }
        let mut term_ids = HashSet::new();
        for term in &self.terms {
            term.validate()?;
            if !domain_ids.contains(term.domain.as_str()) {
                return Err(DomainError::invalid(
                    "terms",
                    format!("term '{}' references unknown domain", term.id),
                ));
            }
            if !term_ids.insert((term.domain.as_str(), term.id.as_str())) {
                return Err(DomainError::invalid(
                    "terms",
                    "term ids must be unique within a domain",
                ));
            }
        }
        Ok(())
    }
}

/// Authored business domain definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessDomainDefinition {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl BusinessDomainDefinition {
    fn validate(&self) -> Result<(), DomainError> {
        bounded_text("domain.id", &self.id, 128)?;
        bounded_text("domain.name", &self.name, 1_024)?;
        if let Some(description) = &self.description {
            bounded_text("domain.description", description, 32 * 1_024)?;
        }
        Ok(())
    }
}

/// Lifecycle of one authored business term.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessTermStatus {
    #[default]
    Active,
    Deprecated,
}

impl BusinessTermStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Deprecated => "deprecated",
        }
    }
}

/// Authored business term and its declared technical mappings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessTermDefinition {
    pub id: String,
    pub domain: String,
    pub canonical_name: String,
    pub definition: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub status: BusinessTermStatus,
    #[serde(default)]
    pub aliases: Vec<BusinessAlias>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantics: Option<BusinessSemantics>,
    #[serde(default)]
    pub mappings: Vec<BusinessTechnicalMappingDefinition>,
}

impl BusinessTermDefinition {
    fn validate(&self) -> Result<(), DomainError> {
        bounded_text("term.id", &self.id, 128)?;
        bounded_text("term.domain", &self.domain, 128)?;
        bounded_text("term.canonical_name", &self.canonical_name, 1_024)?;
        bounded_text("term.definition", &self.definition, 32 * 1_024)?;
        bounded_text("term.language", &self.language, 128)?;
        enforce_count(
            "term.aliases",
            self.aliases.len(),
            BUSINESS_TERM_MAX_ALIASES,
        )?;
        enforce_count(
            "term.mappings",
            self.mappings.len(),
            BUSINESS_TERM_MAX_MAPPINGS,
        )?;
        let mut aliases = HashSet::new();
        for alias in &self.aliases {
            alias.validate()?;
            let folded = alias.value.to_lowercase();
            if !aliases.insert(folded) {
                return Err(DomainError::invalid(
                    "term.aliases",
                    "alias values must be unique without case collisions",
                ));
            }
        }
        if let Some(semantics) = &self.semantics {
            semantics.validate()?;
        }
        for mapping in &self.mappings {
            mapping.validate()?;
        }
        Ok(())
    }
}

fn default_language() -> String {
    "und".to_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessAliasKind {
    Synonym,
    Abbreviation,
}

impl BusinessAliasKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Synonym => "synonym",
            Self::Abbreviation => "abbreviation",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessAlias {
    pub value: String,
    pub kind: BusinessAliasKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

impl BusinessAlias {
    fn validate(&self) -> Result<(), DomainError> {
        bounded_text("alias.value", &self.value, 1_024)?;
        if let Some(language) = &self.language {
            bounded_text("alias.language", language, 128)?;
        }
        Ok(())
    }
}

/// Declarative calculation semantics. Formula text is not executable in v1.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessSemantics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_basis: Option<String>,
    #[serde(default)]
    pub includes: Vec<String>,
    #[serde(default)]
    pub excludes: Vec<String>,
}

impl BusinessSemantics {
    fn validate(&self) -> Result<(), DomainError> {
        if let Some(formula) = &self.formula {
            bounded_text("semantics.formula", formula, 32 * 1_024)?;
        }
        for (field, value) in [
            ("semantics.aggregation", self.aggregation.as_deref()),
            ("semantics.unit", self.unit.as_deref()),
            ("semantics.grain", self.grain.as_deref()),
            ("semantics.time_basis", self.time_basis.as_deref()),
        ] {
            if let Some(value) = value {
                bounded_text(field, value, 1_024)?;
            }
        }
        enforce_count("semantics.includes", self.includes.len(), 256)?;
        enforce_count("semantics.excludes", self.excludes.len(), 256)?;
        for value in self.includes.iter().chain(&self.excludes) {
            bounded_text("semantics.boundary", value, 1_024)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessMappingRelation {
    RepresentedBy,
    CalculatedFrom,
}

impl BusinessMappingRelation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RepresentedBy => "represented_by",
            Self::CalculatedFrom => "calculated_from",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TechnicalTargetKind {
    File,
    Symbol,
    ConfigKey,
    Api,
    SoftwareComponent,
    BuildTarget,
    Iac,
    DesignElement,
    DatabaseTable,
    DatabaseColumn,
    Metric,
    External,
}

impl TechnicalTargetKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Symbol => "symbol",
            Self::ConfigKey => "config_key",
            Self::Api => "api",
            Self::SoftwareComponent => "software_component",
            Self::BuildTarget => "build_target",
            Self::Iac => "iac",
            Self::DesignElement => "design_element",
            Self::DatabaseTable => "database_table",
            Self::DatabaseColumn => "database_column",
            Self::Metric => "metric",
            Self::External => "external",
        }
    }
}

/// Authored edge from one business term to a technical target hint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessTechnicalMappingDefinition {
    pub relation: BusinessMappingRelation,
    pub target_kind: TechnicalTargetKind,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
}

impl BusinessTechnicalMappingDefinition {
    fn validate(&self) -> Result<(), DomainError> {
        bounded_text("mapping.target", &self.target, 1_024)?;
        if let Some(path) = &self.path {
            bounded_text("mapping.path", path, 1_024)?;
        }
        if let Some(source_scope) = &self.source_scope {
            bounded_text("mapping.source_scope", source_scope, 1_024)?;
        }
        Ok(())
    }
}

fn bounded_text(field: &'static str, value: &str, max_bytes: usize) -> Result<(), DomainError> {
    if value.trim().is_empty() {
        return Err(DomainError::invalid(field, "must not be empty"));
    }
    if value.len() > max_bytes {
        return Err(DomainError::invalid(
            field,
            format!("must be {max_bytes} bytes or less"),
        ));
    }
    if value.contains('\0') {
        return Err(DomainError::invalid(field, "must not contain NUL bytes"));
    }
    Ok(())
}

fn enforce_count(field: &'static str, count: usize, max: usize) -> Result<(), DomainError> {
    if count > max {
        return Err(DomainError::invalid(
            field,
            format!("must contain {max} or fewer entries"),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "glossary_tests.rs"]
mod tests;
