use super::{
    SOURCE_LAYOUT_DISCOVERY_MAX_ROOTS, discover_source_layout,
    effective_index_path_filters_for_layouts, effective_path_filter_intersections_for_layouts,
    source_layout_roots_for_path,
};
use crate::{
    code::source::changes::GitTreeEntry,
    domain::{CodeRepositoryRegistration, CodeRepositorySelector},
};

#[test]
fn source_root_derivation_handles_nested_jvm_and_package_layouts() {
    assert_eq!(
        source_layout_roots_for_path("modules/service/src/main/java/com/example/App.java"),
        vec![
            "modules/service/src/main/java".to_owned(),
            "modules/service".to_owned(),
        ]
    );
    assert_eq!(
        source_layout_roots_for_path("packages/ui/src/index.ts"),
        vec!["packages/ui".to_owned()]
    );
    assert_eq!(
        source_layout_roots_for_path("include/api/client.h"),
        vec!["include".to_owned()]
    );
}

#[test]
fn discovery_excludes_broad_dependency_directories() {
    let discovery = discover_source_layout(&[
        entry("packages/ui/src/index.ts"),
        entry("vendor/sdk/src/client.rs"),
        entry("third_party/sdk/src/client.rs"),
        entry("node_modules/sdk/src/client.ts"),
    ]);
    let registration = registration(vec!["src"]);
    let selector = selector(Vec::new());

    let filters = effective_index_path_filters_for_layouts(&registration, &selector, &[&discovery]);

    assert!(filters.contains(&"src".to_owned()));
    assert!(filters.contains(&"packages/ui".to_owned()));
    assert!(!filters.iter().any(|filter| filter.starts_with("vendor")));
    assert!(
        !filters
            .iter()
            .any(|filter| filter.starts_with("third_party"))
    );
    assert!(
        !filters
            .iter()
            .any(|filter| filter.starts_with("node_modules"))
    );
}

#[test]
fn selector_intersection_admits_only_matching_discovered_roots() {
    let discovery = discover_source_layout(&[
        entry("packages/ui/src/index.ts"),
        entry("modules/api/src/main/java/example/Api.java"),
    ]);
    let registration = registration(vec!["src"]);
    let selector = selector(vec!["packages/ui"]);

    let filters =
        effective_path_filter_intersections_for_layouts(&registration, &selector, &[&discovery]);

    assert_eq!(filters, Some(vec!["packages/ui".to_owned()]));
}

#[test]
fn discovery_bounds_the_number_of_auto_source_roots() {
    let entries = (0..SOURCE_LAYOUT_DISCOVERY_MAX_ROOTS + 20)
        .map(|index| entry(&format!("packages/package-{index}/src/lib.rs")))
        .collect::<Vec<_>>();
    let discovery = discover_source_layout(&entries);
    let filters = effective_index_path_filters_for_layouts(
        &registration(vec!["src"]),
        &selector(Vec::new()),
        &[&discovery],
    );

    assert_eq!(filters.len(), SOURCE_LAYOUT_DISCOVERY_MAX_ROOTS + 1);
    assert!(filters.contains(&"packages/package-0".to_owned()));
    assert!(!filters.contains(&format!(
        "packages/package-{}",
        SOURCE_LAYOUT_DISCOVERY_MAX_ROOTS
    )));
}

fn entry(path: &str) -> GitTreeEntry {
    GitTreeEntry {
        path: path.to_owned(),
        byte_count: 1,
    }
}

fn registration(paths: Vec<&str>) -> CodeRepositoryRegistration {
    CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        paths.into_iter().map(str::to_owned).collect(),
        Vec::new(),
    )
    .expect("registration should validate")
}

fn selector(paths: Vec<&str>) -> CodeRepositorySelector {
    CodeRepositorySelector::new(
        "alias",
        "HEAD",
        paths.into_iter().map(str::to_owned).collect(),
        Vec::new(),
    )
    .expect("selector should validate")
}
