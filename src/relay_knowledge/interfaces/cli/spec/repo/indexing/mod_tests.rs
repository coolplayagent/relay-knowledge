use super::{repo_index, repo_index_worker, repo_scope_preview, repo_update};

#[test]
fn indexing_specs_keep_worker_preview_and_incremental_options_separate() {
    let specs = [
        repo_index(),
        repo_index_worker(),
        repo_scope_preview(),
        repo_update(),
    ];
    assert_eq!(
        specs
            .iter()
            .map(|spec| spec.path.join(" "))
            .collect::<Vec<_>>(),
        [
            "repo index",
            "repo index-worker",
            "repo scope preview",
            "repo update"
        ]
    );
    assert!(
        repo_index_worker()
            .options
            .iter()
            .any(|option| option.flag == "--task-id")
    );
    assert!(
        repo_update()
            .options
            .iter()
            .any(|option| option.flag == "--base")
    );
    assert!(repo_update().options.iter().all(|option| !option.required));
    assert_eq!(
        repo_update()
            .options
            .iter()
            .find(|option| option.flag == "--head")
            .and_then(|option| option.default),
        Some("HEAD")
    );
}
