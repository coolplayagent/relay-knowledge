use super::*;
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct SourceFixture(PathBuf);
impl SourceFixture {
    fn new() -> Self {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("relay-local-io-{}-{id}", std::process::id()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/A.java"),
            "class A { int first() { return 1; } }",
        )
        .unwrap();
        fs::write(
            root.join("src/B.java"),
            "class B { int second() { return 2; } }",
        )
        .unwrap();
        Self(root)
    }
    fn plan_at(&self, reference: &str) -> CodeIndexPlan {
        super::super::prepare_full_index_plan(
            CodeRepositoryRegistration::new(
                "repo",
                "fixture",
                self.0.to_string_lossy(),
                vec!["src".into()],
                vec!["java".into()],
            )
            .unwrap(),
            CodeRepositorySelector::new("fixture", reference, vec![], vec![]).unwrap(),
            CodeIndexResourceBudget::new(1, 1024 * 1024, 10000).unwrap(),
        )
        .unwrap()
    }
}
impl Drop for SourceFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn consume(mut plan: CodeIndexPlan) -> (CodeIndexPlan, Vec<CodeIndexBatch>) {
    let mut batches = Vec::new();
    loop {
        let (next, batch) = plan.parse_next_batch().unwrap();
        plan = next;
        match batch {
            Some(batch) => batches.push(batch),
            None => return (plan, batches),
        }
    }
}

#[test]
fn vanished_after_planning_rebinds_identity_without_fabricating_file_content() {
    let source = SourceFixture::new();
    let plan = source.plan_at("HEAD");
    let old_scope = plan.session().source_scope;
    fs::remove_file(source.0.join("src/A.java")).unwrap();
    let (plan, batches) = consume(plan);
    assert!(plan.needs_source_replan);
    assert_eq!(
        batches.iter().map(|batch| batch.files.len()).sum::<usize>(),
        1
    );
    assert_eq!(
        batches[0].diagnostics[0].io.as_ref().unwrap().error_kind,
        crate::domain::CodePathIoErrorKind::NotFound
    );
    let next = plan.replan_after_source_failures().unwrap().unwrap();
    assert_ne!(old_scope, next.session().source_scope);
    fs::write(
        source.0.join("src/A.java"),
        "class A { int first() { return 1; } }",
    )
    .unwrap();
    let (finished, batches) = consume(next);
    assert!(!finished.needs_source_replan);
    assert_eq!(batches.len(), 2);
    assert!(batches[0].files.is_empty());
    assert_eq!(batches[0].processed_paths().len(), 1);
    assert_eq!(batches[1].files[0].path, "src/B.java");
    assert!(finished.replan_after_source_failures().unwrap().is_none());
    let repaired = source.plan_at("HEAD");
    assert_eq!(repaired.session().source_scope, old_scope);
    assert!(
        consume(repaired)
            .1
            .iter()
            .all(|batch| batch.diagnostics.is_empty())
    );
}

#[test]
fn successfully_read_content_drift_and_root_loss_still_fail() {
    let source = SourceFixture::new();
    let plan = source.plan_at("HEAD");
    fs::write(source.0.join("src/A.java"), "class Changed {}").unwrap();
    assert!(
        plan.parse_next_batch()
            .unwrap_err()
            .to_string()
            .contains("no longer matches")
    );
    let plan = source.plan_at("HEAD");
    fs::remove_dir_all(&source.0).unwrap();
    assert!(matches!(
        plan.parse_next_batch(),
        Err(CodeIndexError::Io(_))
    ));
}

#[test]
fn repeated_new_failures_cannot_restart_a_worker_without_a_bound() {
    let source = SourceFixture::new();
    let mut plan = source.plan_at("HEAD");
    plan.source_replan_count = MAX_SOURCE_REPLANS_PER_ATTEMPT;
    plan.needs_source_replan = true;
    assert!(
        plan.replan_after_source_failures()
            .unwrap_err()
            .to_string()
            .contains("replan budget")
    );
}

#[cfg(windows)]
#[test]
fn windows_sharing_failures_produce_bounded_diagnostic_only_batches_and_recover() {
    use std::os::windows::fs::OpenOptionsExt;
    let source = SourceFixture::new();
    let locks = ["A.java", "B.java"].map(|name| {
        fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(source.0.join("src").join(name))
            .unwrap()
    });
    let plan = source.plan_at("HEAD");
    let partial_scope = plan.session().source_scope;
    assert_eq!(plan.session().total_path_count, 2);
    let (plan, batches) = consume(plan);
    assert_eq!(batches.len(), 2);
    for batch in batches {
        assert!(batch.files.is_empty());
        assert!(batch.chunks.is_empty());
        assert_eq!(batch.processed_paths().len(), 1);
        assert_eq!(
            batch.diagnostics[0].io.as_ref().unwrap().raw_os_error,
            Some(32)
        );
        assert_eq!(batch.source_scope, partial_scope);
    }
    assert!(plan.replan_after_source_failures().unwrap().is_none());
    drop(locks);
    let plan = source.plan_at("HEAD");
    assert_ne!(plan.session().source_scope, partial_scope);
    assert_eq!(
        consume(plan)
            .1
            .iter()
            .map(|batch| batch.files.len())
            .sum::<usize>(),
        2
    );
}

#[test]
fn source_io_directory_iteration_failure_discards_the_entire_subtree() {
    use crate::code::source::local_io::test_fault;
    let source = SourceFixture::new();
    fs::create_dir_all(source.0.join("src/broken")).unwrap();
    for name in ["C.java", "D.java"] {
        fs::write(source.0.join("src/broken").join(name), "class Child {}").unwrap();
    }
    let complete = source.plan_at("HEAD").session().source_scope;
    let guard = test_fault::inject(
        source.0.join("src/broken"),
        CodePathIoOperation::ReadDirectory,
        2,
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "fixture enumeration failure",
        ),
    );
    let plan = source.plan_at("HEAD");
    assert_ne!(plan.session().source_scope, complete);
    assert_eq!(plan.session().total_path_count, 3);
    let (_, batches) = consume(plan);
    let files = batches.iter().flat_map(|b| &b.files).collect::<Vec<_>>();
    assert_eq!(files.len(), 2);
    assert!(files.iter().all(|f| !f.path.starts_with("src/broken/")));
    let diagnostics = batches
        .iter()
        .flat_map(|b| &b.diagnostics)
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].path, "src/broken");
    assert_eq!(
        diagnostics[0].io.as_ref().unwrap().path_kind,
        CodePathKind::Directory
    );
    drop(guard);
    assert_eq!(source.plan_at("HEAD").session().source_scope, complete);
}

#[test]
fn source_io_unselected_files_are_not_read_or_reported() {
    use crate::code::source::local_io::{self, test_fault};
    let source = SourceFixture::new();
    let excluded = source.0.join("src/ignored.txt");
    fs::write(&excluded, "excluded by Java selection").unwrap();
    for operation in [CodePathIoOperation::Read, CodePathIoOperation::Metadata] {
        let guard = test_fault::inject(
            excluded.clone(),
            operation,
            0,
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "must remain unused"),
        );
        let (_, batches) = consume(source.plan_at("HEAD"));
        assert!(batches.iter().all(|b| b.diagnostics.is_empty()));
        let error = match operation {
            CodePathIoOperation::Read => local_io::read_file(&excluded).unwrap_err(),
            _ => local_io::symlink_metadata(&excluded).unwrap_err(),
        };
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        drop(guard);
    }
}

#[cfg(windows)]
#[test]
fn source_io_invalid_function_is_isolated_at_the_deterministic_read_boundary() {
    use crate::code::source::local_io::test_fault;
    let source = SourceFixture::new();
    let complete = source.plan_at("HEAD").session().source_scope;
    let guard = test_fault::inject(
        source.0.join("src/A.java"),
        CodePathIoOperation::Read,
        0,
        std::io::Error::from_raw_os_error(1),
    );
    let plan = source.plan_at("HEAD");
    assert_ne!(plan.session().source_scope, complete);
    let (_, batches) = consume(plan);
    assert_eq!(batches.iter().map(|b| b.files.len()).sum::<usize>(), 1);
    let diagnostic = batches.iter().flat_map(|b| &b.diagnostics).next().unwrap();
    assert_eq!(diagnostic.io.as_ref().unwrap().raw_os_error, Some(1));
    assert_eq!(
        diagnostic.io.as_ref().unwrap().error_kind,
        crate::domain::CodePathIoErrorKind::Unsupported
    );
    drop(guard);
    assert_eq!(source.plan_at("HEAD").session().source_scope, complete);
}

#[test]
fn source_io_pinned_plan_rejects_identity_change_after_discovery() {
    let source = SourceFixture::new();
    let pin = source.plan_at("HEAD").session().resolved_commit_sha;
    let plan = source.plan_at(&pin);
    fs::remove_file(source.0.join("src/A.java")).unwrap();
    let (plan, _) = consume(plan);
    let error = plan.replan_after_source_failures().unwrap_err();
    assert!(error.to_string().contains(&pin));
    assert!(error.to_string().contains("no longer matches"));
    // An unpinned request can still publish the newly observed filesystem state.
    let live = source.plan_at("HEAD");
    assert_ne!(live.session().resolved_commit_sha, pin);
    assert_eq!(
        consume(live).1.iter().map(|b| b.files.len()).sum::<usize>(),
        1
    );
}
