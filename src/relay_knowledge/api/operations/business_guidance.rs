//! Read-only onboarding guidance derived from the published business projection.

use serde::{Deserialize, Serialize};

use crate::domain::{
    BUSINESS_GLOSSARY_SCHEMA_VERSION, BusinessKnowledgeQueryKind, BusinessKnowledgeQueryRequest,
    BusinessKnowledgeResult, BusinessKnowledgeResultStatus, BusinessKnowledgeState,
    BusinessKnowledgeSummary,
};

/// An illustration to author and review, never a seeded business fact.
pub(crate) const BUSINESS_GLOSSARY_EXAMPLE: &str = "schema_version: 1\ndomains:\n  - id: revenue\n    name: Revenue\nterms:\n  - id: monthly-recurring-revenue\n    domain: revenue\n    canonical_name: Monthly Recurring Revenue\n    definition: Recurring subscription revenue normalized to one month.\n    language: en\n    aliases:\n      - value: MRR\n        kind: abbreviation\n    mappings:\n      - relation: calculated_from\n        target_kind: file\n        target: src/billing.rs\n";

/// Default authoring location and schema; routed legacy/additional files may differ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessKnowledgeBootstrap {
    pub default_glossary_path: String,
    pub schema_version: u16,
    pub schema_example: String,
    pub documentation_url: String,
    pub next_steps: Vec<String>,
}

impl Default for BusinessKnowledgeBootstrap {
    fn default() -> Self {
        Self {
            default_glossary_path: crate::project::BUSINESS_GLOSSARY_RELATIVE_PATH.to_owned(),
            schema_version: BUSINESS_GLOSSARY_SCHEMA_VERSION,
            schema_example: BUSINESS_GLOSSARY_EXAMPLE.to_owned(),
            documentation_url: format!("https://github.com/{}/blob/main/docs/en/03-architecture-specs/27-business-knowledge-technical-mapping.md", crate::project::GITHUB_REPOSITORY_FULL_NAME),
            next_steps: vec![
                "Run map init in the registered repository root; new glossaries are empty and existing content is preserved.".to_owned(),
                "Run map route business-knowledge --type knowledge --format json to inspect the authorized glossary paths.".to_owned(),
                "Author domains, terms and mappings in the routed glossary using the schema example; replace example names and targets with reviewed repository facts.".to_owned(),
                "Commit the Knowledge Map, its referenced topic files and the glossary to Git; HEAD indexing does not read uncommitted files.".to_owned(),
                "Run repo index <alias> --ref HEAD --format json, then repo business <alias> --kind all --ref HEAD --format json with your registered alias.".to_owned(),
            ],
        }
    }
}

/// Empty-result reasons are separate from freshness and degradation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessKnowledgeDiagnosticReason {
    GraphOnly,
    StaleProjection,
    NoBusinessSources,
    EmptyGlossary,
    NoMatch,
    NoMappings,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessKnowledgeDiagnostics {
    pub reason: BusinessKnowledgeDiagnosticReason,
    pub message: String,
    pub next_steps: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bootstrap: Option<BusinessKnowledgeBootstrap>,
}

impl BusinessKnowledgeDiagnostics {
    /// Explains already classified outcomes without changing result or freshness semantics.
    pub(crate) fn for_query(
        knowledge: &BusinessKnowledgeSummary,
        request: &BusinessKnowledgeQueryRequest,
        result: &BusinessKnowledgeResult,
    ) -> Option<Self> {
        use BusinessKnowledgeDiagnosticReason as Reason;
        use BusinessKnowledgeState as State;
        let (reason, message, action, authoring) = if knowledge.state == State::Unknown {
            (
                Reason::GraphOnly,
                "The business projection was not read; its readiness is unknown.",
                "Query with allow-stale or wait-until-fresh after indexing the requested ref.",
                false,
            )
        } else if knowledge.projection.stale {
            (
                Reason::StaleProjection,
                "These query results and knowledge counts come from a stale projection.",
                "Re-index the requested ref before assessing current glossary completeness.",
                false,
            )
        } else if knowledge.state == State::NoSources {
            (
                Reason::NoBusinessSources,
                "No authorized business sources were projected for this indexed snapshot.",
                "Check the business-knowledge route and commit its files to the selected Git ref, then re-index. Live filesystem snapshots do not project committed business facts.",
                true,
            )
        } else if knowledge.state == State::EmptyGlossary {
            (
                Reason::EmptyGlossary,
                "The indexed glossary sources contain no business terms.",
                "Author and commit domains, terms and mappings, then re-index. Code indexing does not infer business terms.",
                true,
            )
        } else if result.status == BusinessKnowledgeResultStatus::Ambiguous {
            (
                Reason::Ambiguous,
                "The exact term or alias matches more than one business domain.",
                "Specify a unique domain ID to disambiguate; a shared domain name or a different limit does not resolve ambiguity.",
                false,
            )
        } else if request.kind != BusinessKnowledgeQueryKind::Terms
            && knowledge.state == State::TermsOnly
            && (request.kind == BusinessKnowledgeQueryKind::Mappings
                || result.status == BusinessKnowledgeResultStatus::Matched)
        {
            (
                Reason::NoMappings,
                "Business terms exist, but no technical mappings are declared in the indexed glossary.",
                "Declare reviewed mappings, commit the glossary and re-index; use kind terms to read existing definitions.",
                true,
            )
        } else if result.status == BusinessKnowledgeResultStatus::NoMatch {
            (
                Reason::NoMatch,
                "No eligible terms or mappings match this query and domain filter.",
                "Adjust the query, domain or kind. The indexed glossary already contains business terms; initialization is not required.",
                false,
            )
        } else if request.kind == BusinessKnowledgeQueryKind::All
            && result.returned_mapping_count == 0
        {
            (
                Reason::NoMappings,
                "The returned business terms have no declared technical mappings.",
                "Review mappings for these terms, or adjust the query and domain to select other terms.",
                true,
            )
        } else {
            return None;
        };
        Some(Self {
            reason,
            message: message.to_owned(),
            next_steps: vec![action.to_owned()],
            bootstrap: authoring.then(BusinessKnowledgeBootstrap::default),
        })
    }
}

#[cfg(test)]
#[path = "business_guidance_tests.rs"]
mod tests;
