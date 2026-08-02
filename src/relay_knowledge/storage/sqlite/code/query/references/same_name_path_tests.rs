use super::reference_same_name_file_penalty;
use crate::domain::{CodeQueryKind, CodeRepositorySelector, CodeRetrievalRequest, FreshnessPolicy};

#[test]
fn same_name_source_file_is_penalized_for_reference_queries() {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let request = CodeRetrievalRequest::new(
        "InstanceContext",
        selector,
        CodeQueryKind::References,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    assert!(
        reference_same_name_file_penalty(
            5.0,
            "packages/opencode/src/project/instance-context.ts",
            &request,
        ) < 0.0
    );
    assert_eq!(
        reference_same_name_file_penalty(
            5.0,
            "packages/opencode/src/project/other-context.ts",
            &request,
        ),
        0.0
    );
}
