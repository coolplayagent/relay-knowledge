use super::super::tests::database;
use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy, SoftwareGlobalKind};

#[test]
fn module_view_obeys_scope_language_and_combined_budget() {
    let connection = database();
    let mut request = SoftwareGlobalRequest::new(
        CodeRepositorySelector::new("repo", "HEAD", vec![], vec![]).unwrap(),
        SoftwareGlobalKind::Modules,
        FreshnessPolicy::AllowStale,
        20,
    )
    .unwrap();
    let (modules, edges) = projection(&connection, "scope", &request).unwrap();
    assert_eq!((modules.len(), edges.len()), (4, 5));
    assert!(
        projection(&connection, "other", &request)
            .unwrap()
            .0
            .is_empty()
    );
    request.limit = 8;
    assert!(projection(&connection, "scope", &request).is_err());
    request.limit = 2;
    assert!(projection(&connection, "scope", &request).is_err());
    request.repository.path_filters = vec!["a".into()];
    assert_eq!(
        projection(&connection, "scope", &request).unwrap().0.len(),
        1
    );
    request.repository.language_filters = vec!["rust".into()];
    assert!(
        projection(&connection, "scope", &request)
            .unwrap()
            .0
            .is_empty()
    );
    request.repository.language_filters = vec!["java".into()];
    assert_eq!(
        projection(&connection, "scope", &request).unwrap().1.len(),
        1
    );
}
