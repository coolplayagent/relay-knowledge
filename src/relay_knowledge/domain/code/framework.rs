//! Framework-aware component and template graph contracts.

use serde::{Deserialize, Serialize};

use super::{
    DomainError, FreshnessPolicy,
    error::required_text,
    repository::{CodeRepositorySelector, RepositoryCodeRange},
};

const MAX_FRAMEWORK_FILTERS: usize = 2;
const MAX_FRAMEWORK_KIND_FILTERS: usize = 16;

/// Supported frontend framework families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameworkKind {
    Angular,
    Vue,
}

impl FrameworkKind {
    /// Stable storage and interface representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Angular => "angular",
            Self::Vue => "vue",
        }
    }
}

/// Framework graph node category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameworkNodeKind {
    Component,
    Directive,
    Pipe,
    Template,
    Input,
    Output,
    Prop,
    Emit,
    Model,
    Slot,
    TemplateVariable,
    ControlFlow,
}

impl FrameworkNodeKind {
    /// Stable storage and interface representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Component => "component",
            Self::Directive => "directive",
            Self::Pipe => "pipe",
            Self::Template => "template",
            Self::Input => "input",
            Self::Output => "output",
            Self::Prop => "prop",
            Self::Emit => "emit",
            Self::Model => "model",
            Self::Slot => "slot",
            Self::TemplateVariable => "template_variable",
            Self::ControlFlow => "control_flow",
        }
    }
}

/// Framework graph relationship category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameworkEdgeKind {
    OwnsTemplate,
    Declares,
    Imports,
    Renders,
    BindsInput,
    HandlesOutput,
    Reads,
    Writes,
    UsesDirective,
    ProvidesSlot,
}

impl FrameworkEdgeKind {
    /// Stable storage and interface representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OwnsTemplate => "owns_template",
            Self::Declares => "declares",
            Self::Imports => "imports",
            Self::Renders => "renders",
            Self::BindsInput => "binds_input",
            Self::HandlesOutput => "handles_output",
            Self::Reads => "reads",
            Self::Writes => "writes",
            Self::UsesDirective => "uses_directive",
            Self::ProvidesSlot => "provides_slot",
        }
    }
}

/// One indexed framework component, declaration, or template construct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeFrameworkNodeRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub node_id: String,
    pub file_id: String,
    pub path: String,
    pub framework: FrameworkKind,
    pub kind: FrameworkNodeKind,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_snapshot_id: Option<String>,
    pub byte_range: RepositoryCodeRange,
    pub line_range: RepositoryCodeRange,
}

/// One indexed relationship between framework nodes or an unresolved target hint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeFrameworkEdgeRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub edge_id: String,
    pub file_id: String,
    pub path: String,
    pub framework: FrameworkKind,
    pub kind: FrameworkEdgeKind,
    pub source_node_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hint: Option<String>,
    pub resolution_state: String,
    pub confidence_basis_points: u16,
    pub confidence_tier: String,
    pub byte_range: RepositoryCodeRange,
    pub line_range: RepositoryCodeRange,
}

/// Bounded framework graph query over one indexed repository scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameworkGraphRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub repository: CodeRepositorySelector,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frameworks: Vec<FrameworkKind>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<FrameworkNodeKind>,
    pub limit: usize,
    pub freshness_policy: FreshnessPolicy,
}

impl FrameworkGraphRequest {
    /// Validates optional search text and bounds filters and result fan-out.
    pub fn new(
        query: Option<String>,
        repository: CodeRepositorySelector,
        frameworks: Vec<FrameworkKind>,
        kinds: Vec<FrameworkNodeKind>,
        limit: usize,
        freshness_policy: FreshnessPolicy,
    ) -> Result<Self, DomainError> {
        if !(1..=100).contains(&limit) {
            return Err(DomainError::invalid("limit", "must be between 1 and 100"));
        }
        if frameworks.len() > MAX_FRAMEWORK_FILTERS {
            return Err(DomainError::invalid(
                "frameworks",
                "must contain 2 or fewer entries",
            ));
        }
        if kinds.len() > MAX_FRAMEWORK_KIND_FILTERS {
            return Err(DomainError::invalid(
                "kinds",
                "must contain 16 or fewer entries",
            ));
        }
        let query = query
            .map(|value| required_text("query", value))
            .transpose()?;

        Ok(Self {
            query,
            repository,
            frameworks,
            kinds,
            limit,
            freshness_policy,
        })
    }
}

/// Framework graph rows returned from one repository scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameworkGraph {
    pub nodes: Vec<CodeFrameworkNodeRecord>,
    pub edges: Vec<CodeFrameworkEdgeRecord>,
    pub truncated: bool,
}

#[cfg(test)]
#[path = "framework_tests.rs"]
mod tests;
