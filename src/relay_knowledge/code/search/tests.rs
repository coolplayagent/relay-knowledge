use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use super::*;

#[test]
fn internal_scanner_filters_definition_lines_before_enforcing_limit() {
    let mut tree = TempSourceTree::create().expect("temp tree should be created");
    tree.write("src/lib.c", b"return target();\nint target(void);\n")
        .expect("source path should be written");
    let request = SourceGrepRequest {
        query: "target".to_owned(),
        paths: vec!["src/lib.c".to_owned()],
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        limit: 1,
        kind: SourceGrepKind::Definition,
        exclude_generated: false,
    };

    let matches = internal_source_grep_matches(&tree.root, &request.paths, &request, |matched| {
        source_line_defines_identity(&matched.excerpt, "target")
    })
    .expect("internal scanner should apply definition acceptance");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].line_range.start, 2);
    assert_eq!(matches[0].excerpt, "int target(void);");
}

#[test]
fn internal_scanner_includes_template_preamble_for_declaration_lines() {
    let mut tree = TempSourceTree::create().expect("temp tree should be created");
    tree.write(
        "include/cache.h",
        b"template <typename InstanceType>\nclass NoDestructor {};\n",
    )
    .expect("source path should be written");
    let request = SourceGrepRequest {
        query: "NoDestructor".to_owned(),
        paths: vec!["include/cache.h".to_owned()],
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        limit: 1,
        kind: SourceGrepKind::Definition,
        exclude_generated: false,
    };

    let matches = internal_source_grep_matches(&tree.root, &request.paths, &request, |matched| {
        source_grep_accepts(SourceGrepKind::Definition, "NoDestructor", matched)
    })
    .expect("internal scanner should include template context");

    assert_eq!(matches.len(), 1);
    assert_eq!(
        matches[0].line_range,
        RepositoryCodeRange { start: 1, end: 2 }
    );
    assert!(
        matches[0]
            .excerpt
            .contains("template <typename InstanceType>")
    );
    assert!(matches[0].excerpt.contains("class NoDestructor"));
}

#[test]
fn hybrid_scanner_tokenizes_query_and_keeps_initializer_header() {
    let mut tree = TempSourceTree::create().expect("temp tree should be created");
    tree.write(
        "src/generated_table.c",
        b"static const struct rk_table_row rk_rows[] = {\n  [RK_STAGE_READ] = {\n    .name = \"read\",\n    .read = rk_driver_read,\n  },\n};\n",
    )
    .expect("source path should be written");
    let request = SourceGrepRequest {
        query: "compound initializer table row read function pointer".to_owned(),
        paths: vec!["src/generated_table.c".to_owned()],
        path_filters: vec!["src/generated_table.c".to_owned()],
        language_filters: vec!["c".to_owned()],
        limit: 5,
        kind: SourceGrepKind::Hybrid,
        exclude_generated: false,
    };

    let matches = internal_source_grep_matches(&tree.root, &request.paths, &request, |_| true)
        .expect("hybrid scanner should search query terms");

    assert!(matches.iter().any(|matched| {
        matched.excerpt.contains("[RK_STAGE_READ]")
            && matched.excerpt.contains(".read = rk_driver_read")
    }));
}

#[test]
fn unknown_language_filter_allows_document_source_fallback_candidates() {
    assert!(language_filter_allows(
        "docs/operations.md",
        "markdown",
        &["unknown".to_owned()]
    ));
    assert!(!language_filter_allows(
        "src/service.py",
        "python",
        &["unknown".to_owned()]
    ));
}

#[test]
fn internal_scanner_searches_materialized_paths_without_ripgrep() {
    let mut tree = TempSourceTree::create().expect("temp tree should be created");
    tree.write(
        ".github/workflows/ci.yml",
        b"# RK_INTERNAL_SCANNER_REFERENCE\nname: ci\n",
    )
    .expect("hidden path should be written");
    let request = SourceGrepRequest {
        query: "RK_INTERNAL_SCANNER_REFERENCE".to_owned(),
        paths: vec![".github/workflows/ci.yml".to_owned()],
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        limit: 5,
        kind: SourceGrepKind::References,
        exclude_generated: false,
    };

    let matches = internal_source_grep_matches(&tree.root, &request.paths, &request, |_| true)
        .expect("internal scanner should read materialized files");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].path, ".github/workflows/ci.yml");
    assert_eq!(matches[0].line_range.start, 1);
    assert_eq!(matches[0].byte_range.start, 2);
    assert!(matches[0].excerpt.contains("RK_INTERNAL_SCANNER_REFERENCE"));
}

#[test]
fn internal_scanner_returns_bounded_excerpts_for_long_non_definition_lines() {
    let mut tree = TempSourceTree::create().expect("temp tree should be created");
    let prefix = "x".repeat(MAX_GREP_LINE_BYTES + 64);
    let suffix = "y".repeat(MAX_GREP_LINE_BYTES + 64);
    let source = format!("{prefix}RK_LONG_REFERENCE{suffix}\n");
    tree.write("dist/bundle.js", source.as_bytes())
        .expect("long source path should be written");
    let request = SourceGrepRequest {
        query: "RK_LONG_REFERENCE".to_owned(),
        paths: vec!["dist/bundle.js".to_owned()],
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        limit: 5,
        kind: SourceGrepKind::References,
        exclude_generated: false,
    };

    let matches = internal_source_grep_matches(&tree.root, &request.paths, &request, |_| true)
        .expect("internal scanner should return long-line matches");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].line_range.start, 1);
    assert_eq!(matches[0].byte_range.start, prefix.len() as u32);
    assert!(matches[0].excerpt.contains("RK_LONG_REFERENCE"));
    assert!(matches[0].excerpt.len() <= MAX_GREP_LINE_BYTES);
}

#[test]
fn internal_scanner_skips_binary_blobs() {
    let mut tree = TempSourceTree::create().expect("temp tree should be created");
    tree.write("assets/blob.bin", b"prefix RK_BINARY_REFERENCE\0suffix\n")
        .expect("binary path should be written");
    let request = SourceGrepRequest {
        query: "RK_BINARY_REFERENCE".to_owned(),
        paths: vec!["assets/blob.bin".to_owned()],
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        limit: 5,
        kind: SourceGrepKind::References,
        exclude_generated: false,
    };

    let matches = internal_source_grep_matches(&tree.root, &request.paths, &request, |_| true)
        .expect("internal scanner should skip binary blobs without failing");

    assert!(matches.is_empty());
}

#[test]
fn internal_scanner_excludes_generated_files_when_requested() {
    let mut tree = TempSourceTree::create().expect("temp tree should be created");
    tree.write(
        "src/client.ts",
        b"// @generated by fixture\nexport const RK_GENERATED_REFERENCE = true;\n",
    )
    .expect("generated source path should be written");
    let request = SourceGrepRequest {
        query: "RK_GENERATED_REFERENCE".to_owned(),
        paths: vec!["src/client.ts".to_owned()],
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        limit: 5,
        kind: SourceGrepKind::References,
        exclude_generated: true,
    };

    let matches = internal_source_grep_matches(&tree.root, &request.paths, &request, |_| true)
        .expect("internal scanner should skip generated files");

    assert!(matches.is_empty());
}

#[test]
fn internal_scanner_marks_generated_matches_when_allowed() {
    let mut tree = TempSourceTree::create().expect("temp tree should be created");
    tree.write(
        "src/client.ts",
        b"// @generated by fixture\nexport const RK_GENERATED_REFERENCE = true;\n",
    )
    .expect("generated source path should be written");
    let request = SourceGrepRequest {
        query: "RK_GENERATED_REFERENCE".to_owned(),
        paths: vec!["src/client.ts".to_owned()],
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        limit: 5,
        kind: SourceGrepKind::References,
        exclude_generated: false,
    };

    let matches = internal_source_grep_matches(&tree.root, &request.paths, &request, |_| true)
        .expect("internal scanner should retain generated files when allowed");

    assert_eq!(matches.len(), 1);
    assert!(matches[0].is_generated);
}

#[test]
fn internal_scanner_prefers_handwritten_matches_before_limit() {
    let mut tree = TempSourceTree::create().expect("temp tree should be created");
    tree.write(
        "generated/api.ts",
        b"// @generated by fixture\nexport const RK_TARGET = 1;\n",
    )
    .expect("generated source should be written");
    tree.write("src/api.ts", b"export const RK_TARGET = 2;\n")
        .expect("handwritten source should be written");
    let request = SourceGrepRequest {
        query: "RK_TARGET".to_owned(),
        paths: vec!["generated/api.ts".to_owned(), "src/api.ts".to_owned()],
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        limit: 1,
        kind: SourceGrepKind::Hybrid,
        exclude_generated: false,
    };

    let matches = internal_source_grep_matches(&tree.root, &request.paths, &request, |_| true)
        .expect("internal scanner should prefer handwritten matches");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].path, "src/api.ts");
    assert!(!matches[0].is_generated);
}

#[test]
fn internal_scanner_primary_path_does_not_report_ripgrep_unavailable() {
    let mut tree = TempSourceTree::create().expect("temp tree should be created");
    tree.write("src/component.tsx", b"import React from \"react\";\n")
        .expect("source path should be written");
    let request = SourceGrepRequest {
        query: "react".to_owned(),
        paths: vec!["src/component.tsx".to_owned()],
        path_filters: Vec::new(),
        language_filters: vec!["tsx".to_owned()],
        limit: 10,
        kind: SourceGrepKind::Imports,
        exclude_generated: false,
    };

    let outcome =
        source_grep_matches_from_materialized_tree(&tree.root, &request.paths, &request, None)
            .expect("internal scanner should search materialized source");

    assert_eq!(outcome.matches.len(), 1);
    assert_eq!(outcome.matches[0].path, "src/component.tsx");
    assert_eq!(outcome.matches[0].language_id, "tsx");
    assert_eq!(outcome.matches[0].excerpt, "import React from \"react\";");
    assert!(outcome.degraded_reason.is_none());
}

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

#[test]
fn candidate_paths_apply_scope_filters_and_budget() {
    let request = SourceGrepRequest {
        query: "target".to_owned(),
        paths: vec![
            "src/lib.rs".to_owned(),
            "../bad.rs".to_owned(),
            "tests/lib.rs".to_owned(),
            "src/app.py".to_owned(),
        ],
        path_filters: vec!["src".to_owned()],
        language_filters: vec!["rust".to_owned()],
        limit: 5,
        kind: SourceGrepKind::Hybrid,
        exclude_generated: false,
    };

    let candidates = selected_candidate_paths(&request);

    assert_eq!(candidates.paths, ["src/lib.rs"]);
}

#[test]
fn candidate_paths_exclude_generated_paths_when_requested() {
    let request = SourceGrepRequest {
        query: "target".to_owned(),
        paths: vec!["dist/bundle.js".to_owned(), "src/lib.rs".to_owned()],
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        limit: 5,
        kind: SourceGrepKind::Hybrid,
        exclude_generated: true,
    };

    let candidates = selected_candidate_paths(&request);

    assert_eq!(candidates.paths, ["src/lib.rs"]);
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
