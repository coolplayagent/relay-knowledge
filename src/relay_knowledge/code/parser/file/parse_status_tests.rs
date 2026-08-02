//! File parse-status tests.

use crate::{
    code::{CodeIndexError, SnapshotBuild, languages::detect_language},
    domain::{CodeParseStatus, CodeRepositoryRegistration},
};

use super::{
    FileStatusInput, record_file_status, syntax_parse_status, tree_sitter_failure_message,
};
use crate::code::parser::{file::contracts::FileParseOutput, syntax::parse_tree};

#[test]
fn degraded_status_records_one_file_and_one_diagnostic() {
    let mut build = status_test_build();

    record_file_status(
        &mut build,
        FileStatusInput {
            path: "src/lib.rs",
            file_id: "file-id",
            language_id: "rust",
            blob_hash: "blob",
            byte_len: 16,
            line_count: 1,
            parse_status: CodeParseStatus::Partial,
            is_generated: false,
            degraded_reason: Some("partial syntax".to_owned()),
        },
    );

    assert_eq!(build.files.len(), 1);
    assert_eq!(build.diagnostics.len(), 1);
    assert_eq!(build.diagnostics[0].message, "partial syntax");
}

#[test]
fn error_free_syntax_is_parsed_without_degradation() {
    let language = detect_language("src/lib.rs").expect("Rust language should resolve");
    let tree = parse_tree(language, "fn main() {}").expect("source should parse");

    assert_eq!(
        syntax_parse_status(
            language.id,
            tree.root_node(),
            "fn main() {}",
            &FileParseOutput::new(),
            &[],
        ),
        (CodeParseStatus::Parsed, None)
    );
}

#[test]
fn tree_sitter_failure_message_names_the_failed_stage() {
    assert_eq!(
        tree_sitter_failure_message(
            "query",
            &CodeIndexError::TreeSitter("invalid query".to_owned()),
        ),
        "tree-sitter query failed: invalid query"
    );
}

fn status_test_build() -> SnapshotBuild {
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    )
}
