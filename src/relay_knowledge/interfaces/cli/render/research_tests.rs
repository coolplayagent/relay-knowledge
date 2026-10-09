use super::*;
use crate::{
    api::{InterfaceKind, RequestContext},
    domain::GraphVersion,
};

#[test]
fn default_and_markdown_outputs_preserve_failure_states_and_nested_diagnostics() {
    let metadata = ApiMetadata::graph_only(
        &RequestContext::for_interface(InterfaceKind::Cli),
        GraphVersion::ZERO,
    );
    let cases = [
        (
            "knowledge.map.apply",
            serde_json::json!({"state":"conflict","preview":{"diagnostics":[{"code":"precondition_conflict","message":"map changed"}]}}),
            "precondition_conflict",
        ),
        (
            "sources.audit",
            serde_json::json!({"integrity_valid":false,"captures":[{"diagnostics":[{"code":"hash_mismatch","message":"raw bytes changed"}]}]}),
            "hash_mismatch",
        ),
        (
            "evidence.validate",
            serde_json::json!({"valid":false,"diagnostics":[{"code":"scope_violation"}]}),
            "scope_violation",
        ),
        (
            "research.status",
            serde_json::json!({"readiness":"needs_action","content_verdict":"unknown","next_steps":["Import the bundle"]}),
            "Import the bundle",
        ),
    ];
    for (operation, response, diagnostic) in cases {
        for format in [OutputFormat::Text, OutputFormat::Markdown] {
            let rendered = render_response(operation, metadata.clone(), &response, format).unwrap();
            assert!(rendered.contains(diagnostic));
            let json = if format == OutputFormat::Markdown {
                rendered
                    .strip_prefix("```json\n")
                    .unwrap()
                    .strip_suffix("\n```\n")
                    .unwrap()
            } else {
                rendered.trim()
            };
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(json).unwrap(),
                response
            );
        }
    }
}
