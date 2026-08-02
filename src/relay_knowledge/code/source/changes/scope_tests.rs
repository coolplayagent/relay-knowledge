use super::*;

fn filters(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn constructors_normalize_paths_and_remove_empty_filters() {
    let filters = filters(&["./src/", "./vendor/lib\\", ""]);

    let scope = TrackedEntryScope::from_path_filters(filters.iter());

    assert_eq!(scope.path_filters, ["src", "vendor/lib"]);
    assert_eq!(scope.entry_filter, TrackedEntryFilter::Nested);
}

#[test]
fn empty_scope_rejects_entries_and_submodule_expansion() {
    let scope = TrackedEntryScope::empty();

    assert!(scope.excludes_all_entries());
    assert!(!scope.allows_entry("", "src/lib.rs"));
    assert!(!scope.allows_submodule_expansion("vendor/lib"));
    assert!(scope.entry_pathspecs("").is_none());
}

#[test]
fn nested_scope_keeps_parent_entries_and_filters_submodule_children() {
    let filters = filters(&["vendor/lib/src"]);
    let scope = TrackedEntryScope::from_path_filters(filters.iter());

    assert!(scope.allows_entry("", "README.md"));
    assert!(scope.allows_entry("vendor/lib/", "src/lib.rs"));
    assert!(!scope.allows_entry("vendor/lib/", "tests/fixture.rs"));
    assert!(scope.allows_submodule_expansion("vendor"));
    assert!(scope.allows_submodule_expansion("vendor/lib"));
    assert!(!scope.allows_submodule_expansion("external"));
}

#[test]
fn entry_scope_applies_filters_to_top_level_entries() {
    let filters = filters(&["src"]);
    let scope = TrackedEntryScope::from_entry_path_filters(filters.iter());

    assert!(scope.allows_entry("", "src/lib.rs"));
    assert!(!scope.allows_entry("", "README.md"));
    assert!(scope.allows_submodule_expansion("src/vendor"));
    assert!(!scope.allows_submodule_expansion("vendor"));
}

#[test]
fn nested_pathspecs_project_child_filters_and_gitlink_candidates() {
    let filters = filters(&["vendor/lib/src/lib.rs", "vendor/lib/tests/api.rs"]);
    let scope = TrackedEntryScope::from_path_filters(filters.iter());

    let pathspecs = scope
        .entry_pathspecs("vendor/lib/")
        .expect("nested child filters should produce pathspecs");

    assert_eq!(pathspecs.paths, ["src/lib.rs", "tests/api.rs"]);
    assert_eq!(pathspecs.gitlink_candidates, ["src", "tests"]);
}

#[test]
fn ancestor_filter_disables_nested_pathspec_narrowing() {
    let filters = filters(&["vendor"]);
    let scope = TrackedEntryScope::from_path_filters(filters.iter());

    assert!(scope.entry_pathspecs("vendor/lib/").is_none());
}

#[test]
fn whole_repository_filter_matches_every_non_empty_path() {
    let filters = filters(&["."]);
    let scope = TrackedEntryScope::from_entry_path_filters(filters.iter());

    assert!(scope.allows_entry("", "src/lib.rs"));
    assert!(scope.allows_submodule_expansion("vendor/lib"));
}
