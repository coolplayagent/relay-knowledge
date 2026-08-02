use super::super::{SourceGrepKind, SourceGrepRequest, materialization::TempSourceTree};
use super::*;
use crate::{code::source_line_defines_identity, domain::RepositoryCodeRange};

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
        matched
            .excerpt
            .lines()
            .map(str::trim)
            .any(|line| source_line_defines_identity(line, "NoDestructor"))
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
