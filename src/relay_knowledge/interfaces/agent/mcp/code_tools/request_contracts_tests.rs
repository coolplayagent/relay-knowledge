//! MCP code-tool request contract tests.

use super::{authorize_code_context_limit, parse_code_query_kind, parse_software_query_kind};
use crate::{
    api::AgentAccessPolicy,
    domain::{CODEGRAPH_CONTEXT_DEFAULT_LIMIT, CodeQueryKind, SoftwareGlobalKind},
};

#[test]
fn agent_kind_aliases_normalize_to_existing_code_and_software_kinds() {
    assert_eq!(
        parse_code_query_kind("caller").unwrap(),
        CodeQueryKind::Callers
    );
    assert_eq!(
        parse_software_query_kind("dependency").unwrap(),
        SoftwareGlobalKind::Dependencies
    );
    assert_eq!(
        parse_software_query_kind("configuration").unwrap(),
        SoftwareGlobalKind::Relationships
    );
    assert_eq!(
        parse_software_query_kind("models").unwrap(),
        SoftwareGlobalKind::Design
    );
}

#[test]
fn code_context_limit_uses_codegraph_default_when_policy_allows_more() {
    let policy = AgentAccessPolicy::new(Vec::new(), true, 50, 65_536, 1_000, false)
        .expect("policy should be valid");

    assert_eq!(
        authorize_code_context_limit(None, &policy).expect("default should pass"),
        CODEGRAPH_CONTEXT_DEFAULT_LIMIT
    );
    assert!(authorize_code_context_limit(Some(21), &policy).is_err());
}
