use serde_json::Value;

use crate::domain::{CodebaseViewKind, CodebaseViewRequest};

use super::{
    WebError, code_selector, optional_string_array_field, parse_freshness, string_field,
    usize_field,
};

pub(super) fn code_view_request(payload: &Value) -> Result<CodebaseViewRequest, WebError> {
    CodebaseViewRequest::new(
        code_selector(payload)?,
        parse_view_kind(string_field(payload, "kind")?)?,
        parse_freshness(string_field(payload, "freshness")?)?,
        usize_field(payload, "limit")?,
        optional_string_array_field(payload, "changed_paths")?,
    )
    .map_err(|error| WebError::bad_request(error.to_string()))
}

fn parse_view_kind(value: &str) -> Result<CodebaseViewKind, WebError> {
    match value {
        "architecture-layers" | "architecture_layers" => Ok(CodebaseViewKind::ArchitectureLayers),
        "business-domains" | "business_domains" => Ok(CodebaseViewKind::BusinessDomains),
        "dependency-tour" | "dependency_tour" => Ok(CodebaseViewKind::DependencyTour),
        "process-flow" | "process_flow" => Ok(CodebaseViewKind::ProcessFlow),
        "affected-scope" | "affected_scope" => Ok(CodebaseViewKind::AffectedScope),
        other => Err(WebError::bad_request(format!(
            "unsupported codebase view kind '{other}'"
        ))),
    }
}
