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
fn slash_only_groups_do_not_become_unrestricted_intersections() {
    for filter in ["/", "///", "\\", "/\\/"] {
        let restrictive = vec![filter.to_owned()];
        assert_eq!(intersect_path_filters(&restrictive, &[]), None);
        assert_eq!(intersect_path_filters(&[], &restrictive), None);
        assert_eq!(intersect_path_filters(&restrictive, &restrictive), None);
        assert_eq!(
            intersect_path_filters(&restrictive, &["src".to_owned()]),
            None
        );
        assert_eq!(
            intersect_path_filters(
                &[filter.to_owned(), "./src/".to_owned()],
                &[filter.to_owned(), "src/api".to_owned()],
            ),
            Some(vec!["src/api".to_owned()])
        );
        assert_eq!(
            intersect_path_filters(&[filter.to_owned(), ".".to_owned()], &[]),
            Some(vec![".".to_owned()])
        );
    }
    assert_eq!(intersect_path_filters(&[], &[]), Some(Vec::new()));
}

#[test]
fn slash_only_scopes_stop_submodule_filesystem_and_layout_discovery() {
    use super::super::{
        effective_path_filter_intersections_for_layouts, filesystem_policy_for_selector,
    };
    use super::submodule_child_scope_filters;

    for filter in ["/", "///", "\\", "/\\/"] {
        for restrict_registration in [false, true] {
            let mut registration = CodeRepositoryRegistration::new(
                "repo",
                "alias",
                "/tmp/repo",
                Vec::new(),
                Vec::new(),
            )
            .unwrap();
            let mut selector =
                CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new()).unwrap();
            if restrict_registration {
                registration.path_filters = vec![filter.to_owned()];
            } else {
                selector = CodeRepositorySelector::new(
                    "alias",
                    "HEAD",
                    vec![filter.to_owned()],
                    Vec::new(),
                )
                .expect("slash-only selector remains a valid request with no matching paths");
            }
            assert_eq!(
                submodule_child_scope_filters("vendor/module", &registration, &selector),
                None
            );
            assert!(filesystem_policy_for_selector(&registration, &selector).path_scope_denied);
            assert_eq!(
                effective_path_filter_intersections_for_layouts(&registration, &selector, &[]),
                None
            );
        }
    }
}

#[test]
fn slash_only_members_preserve_valid_mixed_and_root_discovery_scopes() {
    use super::super::{
        effective_path_filter_intersections_for_layouts, filesystem_policy_for_selector,
    };
    use super::submodule_child_scope_filters;

    for (registration_filters, selector_filters, expected, child) in [
        (
            vec!["/", "vendor/module"],
            vec!["\\", "vendor/module/src"],
            vec!["vendor/module/src"],
            vec!["src"],
        ),
        (vec![], vec![], vec![], vec![]),
        (vec!["/", "."], vec!["."], vec!["."], vec![]),
    ] {
        let registration = CodeRepositoryRegistration::new(
            "repo",
            "alias",
            "/tmp/repo",
            registration_filters
                .into_iter()
                .map(str::to_owned)
                .collect(),
            Vec::new(),
        )
        .unwrap();
        let selector = CodeRepositorySelector::new(
            "alias",
            "HEAD",
            selector_filters.into_iter().map(str::to_owned).collect(),
            Vec::new(),
        )
        .unwrap();
        assert_eq!(
            effective_path_filter_intersections_for_layouts(&registration, &selector, &[]),
            Some(expected.into_iter().map(str::to_owned).collect())
        );
        assert_eq!(
            submodule_child_scope_filters("vendor/module", &registration, &selector),
            Some(child.into_iter().map(str::to_owned).collect())
        );
        assert!(!filesystem_policy_for_selector(&registration, &selector).path_scope_denied);
    }
}

#[test]
fn slash_only_registration_stops_layout_discovery_before_normalization() {
    use super::super::effective_path_filter_intersections_for_layouts;

    for filter in ["/", "///", "\\", "/\\/"] {
        let registration = CodeRepositoryRegistration::new(
            "repo",
            "alias",
            "/tmp/repo",
            vec![filter.to_owned()],
            Vec::new(),
        )
        .unwrap();
        let selector =
            CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new()).unwrap();
        assert_eq!(
            effective_path_filter_intersections_for_layouts(&registration, &selector, &[]),
            None
        );
    }
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
