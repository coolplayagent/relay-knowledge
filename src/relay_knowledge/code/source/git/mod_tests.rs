// Direct tests for bounded Git execution and batch parsing.

use super::*;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

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
