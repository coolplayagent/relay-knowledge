use super::{FrameworkGraphRequest, FrameworkKind, FrameworkNodeKind};
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn framework_request_bounds_filters_and_results() {
    let selector = CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap();
    let request = FrameworkGraphRequest::new(
        Some(" navigation ".to_owned()),
        selector.clone(),
        vec![FrameworkKind::Angular, FrameworkKind::Vue],
        vec![FrameworkNodeKind::Component],
        100,
        FreshnessPolicy::AllowStale,
    )
    .unwrap();

    assert_eq!(request.query.as_deref(), Some("navigation"));
    assert!(
        FrameworkGraphRequest::new(
            None,
            selector,
            Vec::new(),
            Vec::new(),
            101,
            FreshnessPolicy::AllowStale,
        )
        .is_err()
    );
}
