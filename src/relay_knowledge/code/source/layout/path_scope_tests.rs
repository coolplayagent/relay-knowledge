use super::{
    intersect_path_filters, path_overlaps_any_filter, path_scope_allows, path_scope_overlaps,
    submodule_child_scope_filters_from_filters,
};
use crate::domain::{CodeRepositoryRegistration, CodeRepositorySelector};

#[test]
fn path_filter_intersection_normalizes_and_keeps_narrower_scope() {
    let left = vec!["./src/".to_owned(), "docs".to_owned()];
    let right = vec!["src/api".to_owned(), "examples".to_owned()];

    assert_eq!(
        intersect_path_filters(&left, &right),
        Some(vec!["src/api".to_owned()])
    );
}

#[test]
fn disjoint_path_filters_have_no_intersection() {
    assert_eq!(
        intersect_path_filters(&["src".to_owned()], &["tests".to_owned()]),
        None
    );
}

#[test]
fn submodule_child_scope_strips_prefix_sorts_and_deduplicates() {
    let filters = vec![
        "vendor/module/tests".to_owned(),
        "vendor/module/src".to_owned(),
        "vendor/module/src".to_owned(),
    ];

    assert_eq!(
        submodule_child_scope_filters_from_filters("vendor/module", &filters),
        Some(vec!["src".to_owned(), "tests".to_owned()])
    );
}

#[test]
fn parent_scope_selects_the_complete_submodule() {
    assert_eq!(
        submodule_child_scope_filters_from_filters(
            "vendor/module",
            &["vendor".to_owned(), "other".to_owned()],
        ),
        Some(Vec::new())
    );
}

#[test]
fn registration_and_selector_scopes_require_both_admissions() {
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        vec!["src".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let selector =
        CodeRepositorySelector::new("alias", "HEAD", vec!["src/api".to_owned()], Vec::new())
            .expect("selector should validate");

    assert!(path_scope_allows(
        "src/api/routes.rs",
        &registration,
        &selector
    ));
    assert!(!path_scope_allows(
        "src/domain/model.rs",
        &registration,
        &selector
    ));
    assert!(path_scope_overlaps("src", &registration, &selector));
    assert!(path_overlaps_any_filter(
        "src/api",
        &["src/api/routes.rs".to_owned()]
    ));
    assert!(!path_overlaps_any_filter(
        "src/apis",
        &["src/api/routes.rs".to_owned()]
    ));
}
