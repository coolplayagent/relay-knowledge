// Direct tests for worktree-overlay snapshot assembly.

use std::collections::BTreeMap;

use super::*;

#[test]
fn origin_reparse_budget_includes_changed_bytes_and_rejects_overflow() {
    let limit = crate::domain::CodeIndexResourceBudget::DEFAULT_MAX_BYTES_PER_BATCH;
    let mut total = 0;
    charge_origin_reparse_bytes(&mut total, limit / 2).unwrap();
    charge_origin_reparse_bytes(&mut total, limit / 2).unwrap();
    assert_eq!(total, limit);
    assert!(charge_origin_reparse_bytes(&mut total, 1).is_err());
    assert_eq!(total, limit);
    assert!(charge_origin_reparse_bytes(&mut total, usize::MAX).is_err());
    assert_eq!(total, limit);
}

#[test]
fn provider_reparse_removes_only_successfully_reparsed_python_skips() {
    let repo = crate::code::test_fixtures::TempGitRepo::create("overlay-origin-counters");
    let app = "import typing\n@typing.overload\ndef pick(x:int): ...\ndef pick(x): return x\n";
    repo.write("app.py", app);
    repo.write("window.pyw", app);
    repo.write("README.md", "unchanged\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Base"]);
    let mut registration = repo.registration();
    registration.path_filters.clear();
    registration.language_filters.clear();
    let hashes = BTreeMap::from([
        (
            "app.py".into(),
            crate::code::ids::stable_content_hash(app.as_bytes()),
        ),
        (
            "README.md".into(),
            crate::code::ids::stable_content_hash(b"unchanged\n"),
        ),
        (
            "window.pyw".into(),
            crate::code::ids::stable_content_hash(app.as_bytes()),
        ),
    ]);
    repo.write("app.py", "# staged\n");
    repo.write("README.md", "staged\n");
    repo.git(["add", "app.py", "README.md"]);
    repo.write("app.py", app);
    repo.write("README.md", "unchanged\n");
    repo.write("typing.py", "def overload(f): return f\n");
    let plan = plan_worktree_overlay(&registration, &repo.selector(), &repo.path, &hashes).unwrap();
    assert_eq!(plan.skipped_unchanged_count, 2);
    assert_eq!(plan.skipped_python_paths, ["app.py".to_owned()].into());
    let snapshot = build_worktree_overlay_snapshot(
        &registration,
        &repo.selector(),
        &repo.path,
        &hashes,
        None,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(snapshot.files.len(), 3);
    assert_eq!(snapshot.skipped_unchanged_count, 1);
    assert_eq!(snapshot.changed_path_count, 3);
}

#[test]
fn workspace_entries_remove_deletions_and_replace_changed_byte_counts() {
    let previous_hashes = BTreeMap::from([
        ("src/keep.rs".to_owned(), "keep".to_owned()),
        ("src/remove.rs".to_owned(), "remove".to_owned()),
        ("src/update.rs".to_owned(), "old".to_owned()),
    ]);
    let deleted_paths = vec!["src/remove.rs".to_owned()];
    let files_to_parse = vec![
        ("src/new.rs".to_owned(), vec![1, 2, 3]),
        ("src/update.rs".to_owned(), vec![4, 5]),
    ];

    let entries = workspace_overlay_entries(&previous_hashes, &deleted_paths, &files_to_parse);

    assert_eq!(
        entries
            .into_iter()
            .map(|entry| (entry.path, entry.byte_count))
            .collect::<Vec<_>>(),
        [
            ("src/keep.rs".to_owned(), 0),
            ("src/new.rs".to_owned(), 3),
            ("src/update.rs".to_owned(), 2),
        ]
    );
}
