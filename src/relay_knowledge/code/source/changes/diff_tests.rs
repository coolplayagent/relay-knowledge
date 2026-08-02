use std::path::Path;

use super::diff_changes;
use crate::code::{CodeIndexError, source::change_status::GitChange, test_fixtures::TempGitRepo};

#[test]
fn diff_changes_reports_detected_renames() {
    let repo = TempGitRepo::create("changes-diff-rename");
    repo.write("src/alpha.rs", "pub fn alpha() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let base = repo.git_text(["rev-parse", "HEAD"]);
    repo.git(["mv", "src/alpha.rs", "src/beta.rs"]);
    repo.git(["commit", "-m", "rename"]);
    let head = repo.git_text(["rev-parse", "HEAD"]);

    let changes = diff_changes(&repo.path, &base, &head).expect("diff should load");

    assert_eq!(
        changes,
        vec![GitChange::Renamed {
            old_path: "src/alpha.rs".to_owned(),
            new_path: "src/beta.rs".to_owned(),
        }]
    );
}

#[test]
fn diff_changes_rejects_option_shaped_base_before_git_lookup() {
    let error = diff_changes(Path::new("/missing/repository"), "--output", "HEAD")
        .expect_err("option-shaped base ref should fail");

    assert_invalid_ref(error, "base_ref");
}

#[test]
fn diff_changes_rejects_option_shaped_head_before_git_lookup() {
    let error = diff_changes(Path::new("/missing/repository"), "HEAD", "--output")
        .expect_err("option-shaped head ref should fail");

    assert_invalid_ref(error, "head_ref");
}

fn assert_invalid_ref(error: CodeIndexError, field: &str) {
    match error {
        CodeIndexError::InvalidInput(message) => {
            assert_eq!(message, format!("{field} must not start with '-'"));
        }
        other => panic!("unexpected error: {other}"),
    }
}
