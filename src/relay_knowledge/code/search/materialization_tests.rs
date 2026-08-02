use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use super::*;

#[test]
fn materialization_skips_oversized_blob_and_keeps_later_candidates() {
    let repo = TestRepo::create("grep-materialization-budget");
    repo.write("large.txt", "abcdef");
    repo.write("small.txt", "xy");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "budget fixture"]);
    let mut tree = TempSourceTree::create().expect("temp tree should be created");
    let paths = vec!["large.txt".to_owned(), "small.txt".to_owned()];
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        repo.root.display().to_string(),
        Vec::new(),
        Vec::new(),
    )
    .expect("registration should validate");

    let materialized = materialize_source_blobs_at_root(
        &registration,
        &repo.root,
        "HEAD",
        &paths,
        SourceMaterializationOptions {
            path_filters: &[],
            language_filters: &[],
            exclude_generated: false,
            max_bytes: 5,
        },
        &mut tree,
    )
    .expect("materialization should succeed");

    assert_eq!(materialized.file_count, 1);
    assert!(materialized.degraded_reason.is_some());
    assert!(!tree.root.join("large.txt").exists());
    assert_eq!(
        fs::read_to_string(tree.root.join("small.txt")).expect("small blob should exist"),
        "xy"
    );
}

#[test]
fn materialization_excludes_generated_headers_before_byte_budgeting() {
    let repo = TestRepo::create("grep-generated-materialization-budget");
    let generated = "// @generated\nexport const target = 1;\n";
    let handwritten = "export const target = 2;\n";
    repo.write("src/generated.ts", generated);
    repo.write("src/handwritten.ts", handwritten);
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "generated budget fixture"]);
    let mut tree = TempSourceTree::create().expect("temp tree should be created");
    let paths = vec![
        "src/generated.ts".to_owned(),
        "src/handwritten.ts".to_owned(),
    ];
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        repo.root.display().to_string(),
        Vec::new(),
        Vec::new(),
    )
    .expect("registration should validate");

    let materialized = materialize_source_blobs_at_root(
        &registration,
        &repo.root,
        "HEAD",
        &paths,
        SourceMaterializationOptions {
            path_filters: &[],
            language_filters: &[],
            exclude_generated: true,
            max_bytes: generated.len() + handwritten.len() - 1,
        },
        &mut tree,
    )
    .expect("materialization should succeed");

    assert_eq!(materialized.file_count, 1);
    assert!(!tree.root.join("src/generated.ts").exists());
    assert_eq!(
        fs::read_to_string(tree.root.join("src/handwritten.ts"))
            .expect("handwritten blob should exist"),
        handwritten
    );
}

#[test]
fn materialization_excluding_generated_skips_oversized_candidates() {
    let repo = TestRepo::create("grep-generated-oversized-budget");
    repo.write("src/large.ts", "abcdef");
    repo.write("src/generated.ts", "// @generated\nxx\n");
    repo.write("src/handwritten.ts", "xy");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "generated oversized fixture"]);
    let mut tree = TempSourceTree::create().expect("temp tree should be created");
    let paths = vec![
        "src/large.ts".to_owned(),
        "src/generated.ts".to_owned(),
        "src/handwritten.ts".to_owned(),
    ];
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        repo.root.display().to_string(),
        Vec::new(),
        Vec::new(),
    )
    .expect("registration should validate");

    let materialized = materialize_source_blobs_at_root(
        &registration,
        &repo.root,
        "HEAD",
        &paths,
        SourceMaterializationOptions {
            path_filters: &[],
            language_filters: &[],
            exclude_generated: true,
            max_bytes: 5,
        },
        &mut tree,
    )
    .expect("materialization should succeed");

    assert_eq!(materialized.file_count, 1);
    assert!(materialized.degraded_reason.is_some());
    assert!(!tree.root.join("src/large.ts").exists());
    assert!(!tree.root.join("src/generated.ts").exists());
    assert_eq!(
        fs::read_to_string(tree.root.join("src/handwritten.ts"))
            .expect("handwritten blob should exist"),
        "xy"
    );
}

#[test]
fn generated_exclusion_materialization_budget_caps_read_overfetch() {
    let mut budget = SourceMaterializationBudget::new(10, true);

    for _ in 0..GENERATED_EXCLUSION_READ_BUDGET_MULTIPLIER {
        assert!(budget.may_read_known_size(10));
        budget.record_read(10);
    }

    assert!(!budget.may_read_known_size(1));
    assert!(budget.is_exhausted());
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
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
