use super::super::tests::database;
use super::*;
use crate::domain::{
    CodeRepositorySelector, FreshnessPolicy, SoftwareBuildTarget, SoftwareGlobalKind,
    SoftwareRelationship,
};

#[test]
fn module_pages_obey_scope_filters_keys_and_lookahead_bound() {
    let connection = database();
    let mut request = SoftwareGlobalRequest::new(
        CodeRepositorySelector::new("repo", "HEAD", vec![], vec![]).unwrap(),
        SoftwareGlobalKind::Modules,
        FreshnessPolicy::AllowStale,
        2,
    )
    .unwrap();
    let first: Vec<SoftwareBuildTarget> =
        read_page(&connection, "scope", &request, "", 2, false).unwrap();
    let second: Vec<SoftwareBuildTarget> = read_page(
        &connection,
        "scope",
        &request,
        &first[1].target_id,
        2,
        false,
    )
    .unwrap();
    assert_eq!((first.len(), second.len()), (2, 2));
    assert!(
        first
            .iter()
            .all(|node| second.iter().all(|other| node.target_id != other.target_id))
    );
    let absent: Vec<SoftwareBuildTarget> =
        read_page(&connection, "unknown", &request, "", 2, false).unwrap();
    assert!(absent.is_empty());
    assert!(
        read_page::<SoftwareBuildTarget>(&connection, "scope", &request, "", 502, false).is_err()
    );
    request.repository.path_filters = vec!["a".into()];
    let nodes: Vec<SoftwareBuildTarget> =
        read_page(&connection, "scope", &request, "", 2, false).unwrap();
    let edges: Vec<SoftwareRelationship> =
        read_page(&connection, "scope", &request, "", 2, true).unwrap();
    assert_eq!((nodes.len(), edges.len()), (1, 1));
    request.repository.language_filters = vec!["rust".into()];
    assert!(
        read_page::<SoftwareBuildTarget>(&connection, "scope", &request, "", 2, false)
            .unwrap()
            .is_empty()
    );
    request.repository.language_filters = vec!["java".into()];
    assert_eq!(
        read_page::<SoftwareRelationship>(&connection, "scope", &request, "", 2, true)
            .unwrap()
            .len(),
        1
    );
}
