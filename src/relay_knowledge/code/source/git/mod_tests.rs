// Direct tests for bounded Git execution and batch parsing.

use super::*;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn bounded_git_root_resolution_accepts_a_nested_repository_path() {
    let repo = TestRepo::create("bounded-root");
    repo.write("src/lib.rs", "pub fn nested_root() {}\n");

    let resolved = resolve_git_root(&repo.root.join("src"))
        .expect("bounded root resolution should find the repository");

    assert_eq!(resolved, repo.root);
}

#[test]
fn bounded_identity_resolution_returns_pinned_commit_and_tree_ids() {
    let repo = TestRepo::create("bounded-identity");
    repo.write("src/lib.rs", "pub fn identity() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let expected_commit = repo.git_text(["rev-parse", "HEAD"]);
    let expected_tree = repo.git_text(["rev-parse", "HEAD^{tree}"]);

    let commit = resolve_git_ref_bounded(&repo.root, "HEAD")
        .expect("bounded ref resolution should return a commit");
    let tree = resolve_git_tree_bounded(&repo.root, &commit)
        .expect("bounded tree resolution should accept the pinned commit");

    assert_eq!(commit, expected_commit);
    assert_eq!(tree, expected_tree);
}

#[test]
fn bounded_first_parent_history_excludes_target_and_honors_limit() {
    let repo = TestRepo::create("bounded-first-parent-history");
    let mut commits = Vec::new();
    for index in 0..12 {
        repo.write(
            "src/lib.rs",
            &format!("pub fn value() -> u32 {{ {index} }}\n"),
        );
        repo.git(["add", "."]);
        repo.git(["commit", "-m", &format!("commit-{index}")]);
        commits.push(repo.git_text(["rev-parse", "HEAD"]));
    }

    let ancestors = first_parent_ancestors_bounded(&repo.root, &commits[11], 10)
        .expect("bounded ancestor history should load");

    assert_eq!(ancestors.len(), 10);
    assert_eq!(ancestors[0], commits[10]);
    assert_eq!(ancestors[9], commits[1]);
    assert!(!ancestors.contains(&commits[0]));
    assert!(!ancestors.contains(&commits[11]));
}

#[test]
fn bounded_first_parent_history_ignores_merged_side_branch() {
    let repo = TestRepo::create("bounded-first-parent-merge");
    repo.write("src/lib.rs", "pub fn base() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let base = repo.git_text(["rev-parse", "HEAD"]);
    let main_branch = repo.git_text(["branch", "--show-current"]);
    repo.git(["checkout", "-b", "side"]);
    repo.write("src/side.rs", "pub fn side() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "side"]);
    let side = repo.git_text(["rev-parse", "HEAD"]);
    repo.git(["checkout", &main_branch]);
    repo.write("src/main.rs", "pub fn main_line() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "main"]);
    let main = repo.git_text(["rev-parse", "HEAD"]);
    repo.git(["merge", "--no-ff", "side", "-m", "merge"]);
    let merge = repo.git_text(["rev-parse", "HEAD"]);

    let ancestors = first_parent_ancestors_bounded(&repo.root, &merge, 10)
        .expect("first-parent history should load");

    assert_eq!(ancestors, vec![main, base]);
    assert!(!ancestors.contains(&side));
}

#[test]
fn bounded_ref_resolution_peels_annotated_tags_to_commits() {
    let repo = TestRepo::create("bounded-annotated-tag");
    repo.write("src/lib.rs", "pub fn tagged() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    repo.git(["tag", "-a", "v1", "-m", "release"]);
    let expected_commit = repo.git_text(["rev-parse", "HEAD"]);
    let tag_object = repo.git_text(["rev-parse", "v1"]);

    let resolved = resolve_git_ref_bounded(&repo.root, "v1")
        .expect("annotated tag should resolve to its immutable commit");

    assert_eq!(resolved, expected_commit);
    assert_ne!(resolved, tag_object);
}

#[test]
fn bounded_tree_resolution_requires_a_pinned_full_object_id() {
    let repo = TestRepo::create("bounded-tree-pinned");

    let error = resolve_git_tree_bounded(&repo.root, "HEAD")
        .expect_err("a moving ref must not be accepted as a pinned commit");

    assert!(error.to_string().contains("full SHA-1 or SHA-256"));
}

#[test]
fn bounded_tree_resolution_rejects_a_pinned_non_commit_object() {
    let repo = TestRepo::create("bounded-tree-commit-type");
    repo.write("src/lib.rs", "pub fn tree_object() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let tree = repo.git_text(["rev-parse", "HEAD^{tree}"]);

    let error = resolve_git_tree_bounded(&repo.root, &tree)
        .expect_err("a tree object ID must not be accepted as a commit ID");

    assert!(matches!(error, CodeIndexError::Git { .. }));
}

#[test]
fn bounded_worktree_observation_is_stable_and_changes_with_file_bytes() {
    let repo = TestRepo::create("bounded-worktree-observation");
    repo.write("src/lib.rs", "pub fn clean() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    assert_eq!(
        repository_worktree_observation_bounded(&repo.root)
            .expect("clean observation should succeed"),
        None
    );
    repo.write("src/lib.rs", "pub fn dirty() {}\n");

    let first = repository_worktree_observation_bounded(&repo.root)
        .expect("dirty observation should succeed")
        .expect("dirty worktree should have an identity");
    let unchanged = repository_worktree_observation_bounded(&repo.root)
        .expect("repeat observation should succeed")
        .expect("dirty worktree should remain observable");
    repo.write("src/lib.rs", "pub fn changed_again() {}\n");
    let changed = repository_worktree_observation_bounded(&repo.root)
        .expect("changed observation should succeed")
        .expect("changed worktree should have an identity");

    assert_eq!(first, unchanged);
    assert_ne!(first, changed);
}

#[test]
fn git_dir_bytes_reads_committed_blob_without_worktree_context() {
    let repo = TestRepo::create("git-dir-read");
    repo.write("src/alpha.rs", "pub fn alpha() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);

    let bytes = git_dir_bytes(&repo.root.join(".git"), &["show", "HEAD:src/alpha.rs"])
        .expect("git-dir command should read committed blob");

    assert_eq!(bytes, b"pub fn alpha() {}\n");
}

#[test]
fn git_dir_bytes_maps_failed_command_arguments_and_stderr() {
    let repo = TestRepo::create("git-dir-error");

    let error = git_dir_bytes(&repo.root.join(".git"), &["show", "missing-ref"])
        .expect_err("unknown ref should fail");

    match error {
        CodeIndexError::Git { args, message } => {
            assert_eq!(args, ["show", "missing-ref"]);
            assert!(!message.is_empty());
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn batch_blob_sizes_report_missing_paths_without_failing_batch() {
    let repo = TestRepo::create("batch-blob-sizes");
    repo.write("src/alpha.rs", "pub fn alpha() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let commit = repo.git_text(["rev-parse", "HEAD"]);

    let sizes = git_batch_blob_sizes(
        &repo.root,
        &commit,
        &["src/alpha.rs".to_owned(), "src/missing.rs".to_owned()],
    )
    .expect("batch blob sizes should load");

    assert_eq!(sizes, vec![Some("pub fn alpha() {}\n".len()), None]);
}

#[test]
fn batch_blob_parser_reports_missing_object_before_size_parse() {
    let paths = ["framework/CMakeLists.txt".to_owned()];
    let error = parse_cat_file_batch(
        &paths,
        b"c965924bc65adc4edcc08965db0119f8ec218321:framework/CMakeLists.txt missing\n",
    )
    .expect_err("missing object header should fail");

    assert!(
        error
            .to_string()
            .contains("git cat-file batch object is missing for framework/CMakeLists.txt"),
        "unexpected error: {error}"
    );
}

#[test]
fn one_path_size_fallback_keeps_batch_parse_errors_from_blocking_reads() {
    let repo = TestRepo::create("batch-size-fallback");
    repo.write(
        "framework/CMakeLists.txt",
        "cmake_minimum_required(VERSION 3.16)\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let commit = repo.git_text(["rev-parse", "HEAD"]);
    let paths = ["framework/CMakeLists.txt".to_owned()];

    let sizes = parse_cat_file_batch_sizes(&paths, b"not-a-valid-batch-check-row\n")
        .or_else(|_| git_blob_sizes_one_path_at_a_time(&repo.root, &commit, &paths))
        .expect("fallback size read should succeed");

    assert_eq!(
        sizes,
        vec![Some("cmake_minimum_required(VERSION 3.16)\n".len())]
    );
}

#[test]
fn one_path_blob_fallback_keeps_batch_parse_errors_from_blocking_reads() {
    let repo = TestRepo::create("batch-blob-fallback");
    repo.write("framework/CMakeLists.txt", "add_subdirectory(core)\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let commit = repo.git_text(["rev-parse", "HEAD"]);
    let paths = ["framework/CMakeLists.txt".to_owned()];

    let blobs = parse_cat_file_batch(&paths, b"not-a-valid-batch-row\n")
        .or_else(|_| git_blobs_one_path_at_a_time(&repo.root, &commit, &paths))
        .expect("fallback blob read should succeed");

    assert_eq!(blobs, vec![b"add_subdirectory(core)\n".to_vec()]);
}

#[test]
fn piped_command_closes_stdin_before_waiting() {
    let output = command_output_with_stdin(
        git_hash_object_command(),
        b"alpha\n".to_vec(),
        Duration::from_secs(5),
        vec!["hash-object".to_owned(), "--stdin".to_owned()],
    )
    .expect("stdin-bound command should finish after EOF");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "4a58007052a65fbc2fc3f910f2855f45a4058e74"
    );
}

fn git_hash_object_command() -> Command {
    let mut command = Command::new("git");
    command.args(["hash-object", "--stdin"]);
    command
}

struct TestRepo {
    root: PathBuf,
}

impl TestRepo {
    fn create(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let root = std::env::temp_dir().join(format!(
            "relay-knowledge-{name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("repo directory should be created");
        let repo = Self { root };
        repo.git(["init"]);
        repo.git(["config", "user.email", "relay@example.invalid"]);
        repo.git(["config", "user.name", "Relay Test"]);
        repo
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent directory should exist");
        }
        fs::write(path, content).expect("fixture file should be written");
    }

    fn git<const N: usize>(&self, args: [&str; N]) {
        let output = Command::new("git")
            .current_dir(&self.root)
            .args(args)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_text<const N: usize>(&self, args: [&str; N]) -> String {
        let output = Command::new("git")
            .current_dir(&self.root)
            .args(args)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
