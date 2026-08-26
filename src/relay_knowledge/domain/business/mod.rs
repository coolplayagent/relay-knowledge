//! Business glossary, scoped ontology identity, and query contracts.

mod glossary;
mod projection;
mod query;

pub use super::core::{OntologyEntityKind, OntologyIdentity};
pub use glossary::{
    BUSINESS_GLOSSARY_MAX_BYTES, BUSINESS_GLOSSARY_MAX_DOMAINS, BUSINESS_GLOSSARY_MAX_TERMS,
    BUSINESS_GLOSSARY_SCHEMA_VERSION, BUSINESS_TERM_MAX_ALIASES, BUSINESS_TERM_MAX_MAPPINGS,
    BusinessAlias, BusinessAliasKind, BusinessDomainDefinition, BusinessGlossary,
    BusinessMappingRelation, BusinessSemantics, BusinessTechnicalMappingDefinition,
    BusinessTermDefinition, BusinessTermStatus, TechnicalTargetKind,
};
pub use projection::{
    BusinessDefinitionFact, BusinessDomain, BusinessEvidence, BusinessKnowledgeConflict,
    BusinessKnowledgeProjection, BusinessKnowledgeProjectionInput, BusinessKnowledgeSource,
    BusinessKnowledgeStatus, BusinessTechnicalMapping, BusinessTerm,
};
pub use query::{
    BusinessKnowledgeQueryKind, BusinessKnowledgeQueryRequest, BusinessKnowledgeResolution,
};
