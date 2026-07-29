use crate::{candidate_git::git_repository_fixture::GitRepositoryFixture, history::HistoryPaths};

use super::*;

#[test]
fn changed_paths_preserve_diff_order() {
    let diff = "\
diff --git a/src/one.rs b/src/one.rs
index 111..222 100644
diff --git a/docs/two.md b/docs/two.md
index 333..444 100644
";

    assert_eq!(changed_paths_from_diff(diff), ["src/one.rs", "docs/two.md"]);
}

#[test]
fn capture_patch_records_tracked_and_untracked_changes() {
    let repository = GitRepositoryFixture::new();
    let base_ref = repository.head();
    repository.write("tracked.txt", "changed\n");
    repository.write("nested/untracked.txt", "new\n");
    let paths = HistoryPaths::new(repository.path());

    let patch = capture_patch(repository.path(), &paths, "run-1", &base_ref)
        .expect("patch should be captured");

    assert!(patch.has_diff());
    assert!(patch.diff.contains("tracked.txt"));
    assert!(patch.diff.contains("nested/untracked.txt"));
    assert_eq!(patch.base_ref, base_ref);
    assert_eq!(
        std::fs::read_to_string(&patch.path).expect("patch file should exist"),
        patch.diff
    );
    assert_eq!(patch.sha256.len(), 64);
}
