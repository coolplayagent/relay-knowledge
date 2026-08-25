use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use super::*;

#[test]
fn evaluation_home_is_unique_to_run_and_removed_through_trusted_root() {
    let workspace = temporary_workspace("run-home");
    let paths = HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths should exist");
    let first = EvaluationHome::prepare(&paths, "run-first", true).expect("first home");
    let second = EvaluationHome::prepare(&paths, "run-second", true).expect("second home");
    let first_path = first.path().to_path_buf();
    let second_path = second.path().to_path_buf();

    assert_ne!(first_path, second_path);
    fs::write(first_path.join("sentinel"), "first").expect("first sentinel");
    fs::write(second_path.join("sentinel"), "second").expect("second sentinel");
    remove_evaluation_home(&paths.work, &first_path).expect("first cleanup");

    assert!(!first_path.exists());
    assert_eq!(
        fs::read_to_string(second_path.join("sentinel")).expect("second sentinel survives"),
        "second"
    );
    remove_evaluation_home(&paths.work, &second_path).expect("second cleanup");
    first
        .complete_result(Ok(()))
        .expect("first guard completion");
    second
        .complete_result(Ok(()))
        .expect("second guard completion");
    fs::remove_dir_all(workspace).expect("workspace cleanup");
}

#[test]
fn evaluation_home_refuses_symlinked_run_ancestors() {
    let workspace = temporary_workspace("run-symlink");
    let paths = HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths should exist");
    let outside = workspace.join("outside");
    fs::create_dir(&outside).expect("outside directory");
    let run_root = paths.work.join("run-linked");
    create_directory_symlink(&outside, &run_root);

    let error = EvaluationHome::prepare(&paths, "run-linked", false)
        .expect_err("an existing linked run root must not be reused");

    assert!(error.contains("refusing to reuse"));
    assert!(outside.is_dir());
    fs::remove_file(run_root).expect("remove test symlink");
    fs::remove_dir_all(workspace).expect("workspace cleanup");
}

#[test]
fn evaluation_home_refuses_symlinked_or_nondirectory_work_root() {
    let workspace = temporary_workspace("work-root-safety");
    let paths = HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths should exist");
    let outside = workspace.join("outside-work");
    fs::create_dir(&outside).expect("outside work");
    fs::remove_dir(&paths.work).expect("replace work root");
    create_directory_symlink(&outside, &paths.work);

    let symlink_error = EvaluationHome::prepare(&paths, "run-linked-work", false)
        .expect_err("linked work root must be rejected");

    assert!(symlink_error.contains("non-symlink directory"));
    fs::remove_file(&paths.work).expect("remove work symlink");
    fs::write(&paths.work, "not a directory").expect("work-root file");
    let file_error = EvaluationHome::prepare(&paths, "run-file-work", false)
        .expect_err("non-directory work root must be rejected");
    assert!(file_error.contains("non-symlink directory"));
    fs::remove_file(&paths.work).expect("remove work-root file");
    fs::remove_dir_all(workspace).expect("workspace cleanup");
}

#[test]
fn evaluation_cleanup_refuses_symlinked_home_without_touching_target() {
    let workspace = temporary_workspace("home-symlink");
    let paths = HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths should exist");
    let evaluation =
        EvaluationHome::prepare(&paths, "run-linked-home", true).expect("evaluation home");
    let home = evaluation.path().to_path_buf();
    let outside = workspace.join("outside-home");
    fs::create_dir(&outside).expect("outside home");
    fs::write(outside.join("sentinel"), "keep").expect("outside sentinel");
    fs::remove_dir(&home).expect("replace home directory");
    create_directory_symlink(&outside, &home);

    let error = remove_evaluation_home(&paths.work, &home)
        .expect_err("linked evaluation home must be rejected");

    assert!(error.contains("non-symlink directory"));
    assert_eq!(
        fs::read_to_string(outside.join("sentinel")).expect("sentinel survives"),
        "keep"
    );
    fs::remove_file(&home).expect("remove test symlink");
    evaluation
        .complete_result(Ok(()))
        .expect("linked-home guard completion");
    fs::remove_dir_all(workspace).expect("workspace cleanup");
}

#[test]
fn evaluation_cleanup_refuses_symlinked_run_root_without_touching_target() {
    let workspace = temporary_workspace("cleanup-run-symlink");
    let paths = HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths should exist");
    let evaluation =
        EvaluationHome::prepare(&paths, "run-replaced", true).expect("evaluation home");
    let home = evaluation.path().to_path_buf();
    let run_root = home.parent().expect("run root").to_path_buf();
    fs::remove_dir(&home).expect("empty home removal");
    fs::remove_dir(&run_root).expect("empty run root removal");
    let outside = workspace.join("outside-run");
    fs::create_dir(&outside).expect("outside run");
    fs::create_dir(outside.join("home")).expect("outside home");
    fs::write(outside.join("sentinel"), "keep").expect("outside sentinel");
    create_directory_symlink(&outside, &run_root);

    let error =
        remove_evaluation_home(&paths.work, &home).expect_err("linked run root must be rejected");

    assert!(error.contains("non-symlink directory"));
    assert_eq!(
        fs::read_to_string(outside.join("sentinel")).expect("outside sentinel survives"),
        "keep"
    );
    fs::remove_file(&run_root).expect("remove run-root symlink");
    fs::create_dir(&run_root).expect("restore run root");
    fs::create_dir(&home).expect("restore home");
    evaluation
        .complete_result(Ok(()))
        .expect("restored cleanup");
    fs::remove_dir_all(workspace).expect("workspace cleanup");
}

#[test]
fn evaluation_complete_reports_nondirectory_cleanup_failure() {
    let workspace = temporary_workspace("cleanup-error");
    let paths = HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths should exist");
    let evaluation =
        EvaluationHome::prepare(&paths, "run-cleanup-error", false).expect("evaluation home");
    let home = evaluation.path().to_path_buf();
    fs::remove_dir(&home).expect("replace empty home");
    fs::write(&home, "not a directory").expect("home file");

    let error = evaluation
        .complete_result::<()>(Err("original evaluation error".to_owned()))
        .expect_err("cleanup error must be returned to the caller");

    assert!(error.starts_with("original evaluation error;"));
    assert!(error.contains("non-symlink directory"));
    fs::remove_file(&home).expect("remove home file");
    fs::remove_dir(home.parent().expect("run root")).expect("remove run root");
    fs::remove_dir_all(workspace).expect("workspace cleanup");
}

#[test]
fn evaluation_home_drop_cleans_early_error_paths_and_keep_is_explicit() {
    let workspace = temporary_workspace("drop-cleanup");
    let paths = HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths should exist");
    let removed_path = {
        let evaluation =
            EvaluationHome::prepare(&paths, "run-dropped", false).expect("dropped home");
        evaluation.path().to_path_buf()
    };
    let kept = EvaluationHome::prepare(&paths, "run-kept", true).expect("kept home");
    let kept_path = kept.path().to_path_buf();
    drop(kept);

    assert!(!removed_path.exists());
    assert!(kept_path.is_dir());
    fs::remove_dir_all(workspace).expect("workspace cleanup");
}

fn temporary_workspace(label: &str) -> PathBuf {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-self-iteration-{label}-{}-{id}",
        std::process::id(),
    ));
    if root.exists() {
        fs::remove_dir_all(&root).expect("stale test workspace cleanup");
    }
    fs::create_dir_all(root.join(".git")).expect("test git directory");
    root
}

#[cfg(unix)]
fn create_directory_symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("directory symlink");
}

#[cfg(windows)]
fn create_directory_symlink(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link).expect("directory symlink");
}
