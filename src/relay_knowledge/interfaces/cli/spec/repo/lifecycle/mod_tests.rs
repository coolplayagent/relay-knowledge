use super::{repo_list, repo_register, repo_remove, repo_report, repo_status};

#[test]
fn lifecycle_specs_keep_distinct_paths_and_operational_effects() {
    let specs = [
        repo_list(),
        repo_register(),
        repo_remove(),
        repo_status(),
        repo_report(),
    ];
    let paths = specs
        .iter()
        .map(|spec| spec.path.as_slice())
        .collect::<Vec<_>>();

    assert_eq!(
        paths,
        [
            ["repo", "list"].as_slice(),
            ["repo", "register"].as_slice(),
            ["repo", "remove"].as_slice(),
            ["repo", "status"].as_slice(),
            ["repo", "report"].as_slice(),
        ]
    );
    assert!(
        repo_register()
            .options
            .iter()
            .any(|option| option.flag == "--alias")
    );
    assert!(repo_remove().options.is_empty());
    assert_eq!(repo_remove().arguments[0].name, "alias");
    assert_eq!(repo_list().operation, "code.repo.list");
    assert!(repo_list().arguments.is_empty());
}
