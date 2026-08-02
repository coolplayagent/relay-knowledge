use super::*;

#[test]
fn config_names_require_exact_submodule_paths_and_are_deduplicated() {
    let bytes = b"submodule.alpha.path vendor/lib\n\
                  submodule.beta.path vendor/other\n\
                  submodule.alpha.path\tvendor/lib\n\
                  unrelated.key vendor/lib\n";
    let mut names = BTreeSet::new();

    collect_submodule_names_from_config(Some(bytes), "vendor/lib", &mut names);

    assert_eq!(names, BTreeSet::from(["alpha".to_owned()]));
}

#[test]
fn gitmodules_names_follow_sections_and_exact_paths() {
    let bytes = br#"
        [submodule "alpha"]
            path = vendor/lib
        [submodule "beta"]
            path = vendor/other
        [submodule "custom/name"]
            path = vendor/lib
    "#;
    let mut names = BTreeSet::new();

    collect_submodule_names_from_gitmodules(Some(bytes), "vendor/lib", &mut names);

    assert_eq!(
        names,
        BTreeSet::from(["alpha".to_owned(), "custom/name".to_owned()])
    );
}

#[test]
fn config_and_section_lexers_reject_incomplete_records() {
    assert_eq!(
        split_config_key_value("submodule.alpha.path vendor/lib"),
        Some(("submodule.alpha.path", "vendor/lib"))
    );
    assert_eq!(split_config_key_value("missing-value"), None);
    assert_eq!(
        gitmodules_section_name("[submodule \"custom/name\"]"),
        Some("custom/name".to_owned())
    );
    assert_eq!(gitmodules_section_name("[submodule alpha]"), None);
}

#[test]
fn submodule_names_cannot_escape_the_git_modules_directory() {
    for name in ["", "/absolute", "../escape", "nested/../../escape"] {
        assert!(
            validate_submodule_name(name).is_err(),
            "{name:?} should be rejected"
        );
    }

    validate_submodule_name("group/nested-module").expect("nested name should be accepted");
}

#[test]
fn commit_matching_requires_a_git_object_layout() {
    let missing = Path::new("missing-submodule-git-dir");

    assert!(submodule_git_dir_matches_commit(missing, None));
    assert!(!submodule_git_dir_matches_commit(missing, Some("deadbeef")));
}

#[test]
fn missing_submodule_worktree_is_reported_as_invalid_input() {
    let error = submodule_worktree_root(Path::new("missing-repository"), "vendor/lib")
        .expect_err("missing worktree should fail");

    assert!(
        matches!(error, CodeIndexError::InvalidInput(message) if message.contains("vendor/lib"))
    );
}
