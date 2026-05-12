use crate::domain::{CodeParseStatus, CodeRepositoryRegistration};

use super::*;

#[test]
fn tree_sitter_captures_symbols_references_imports_and_chunks() {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    let source = br#"
use std::time::Duration;

/// Runs retries.
fn retry_policy() {
    sleep(Duration::from_secs(1));
}
"#;

    parse_indexed_file(&mut build, "src/lib.rs", source).expect("file should parse");
    let snapshot = build.finish();

    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "retry_policy")
    );
    assert!(
        snapshot
            .references
            .iter()
            .any(|reference| reference.name == "sleep")
    );
    assert!(
        snapshot
            .imports
            .iter()
            .any(|import| import.module.contains("std::time"))
    );
    assert!(
        snapshot
            .chunks
            .iter()
            .any(|chunk| chunk.content.contains("retry_policy"))
    );
}

#[test]
fn syntax_error_files_are_partial_and_keep_reliable_facts() {
    let snapshot = parse_source_snapshot(
        "src/lib.rs",
        br#"
fn retry_policy() -> u32 {
    let broken = ;
    3
}
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Partial);
    assert!(
        snapshot
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("error nodes"))
    );
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "retry_policy")
    );
    assert!(
        snapshot
            .chunks
            .iter()
            .any(|chunk| chunk.content.contains("retry_policy"))
    );
}

#[test]
fn parser_panics_are_recorded_as_failed_file_diagnostics() {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    let content = "fn retry_policy() {}\n";

    parse_syntax_file(
        &mut build,
        SyntaxFileInput {
            path: "src/lib.rs",
            file_id: "file",
            language: LanguageSpec {
                id: "rust",
                language: || panic!("parser boom"),
                tags_query: "",
            },
            blob_hash: "hash",
            byte_len: content.len(),
            line_count: 1,
            content,
        },
    )
    .expect("parser failure should be isolated");
    let snapshot = build.finish();

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Failed);
    assert!(
        snapshot
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("tree-sitter parse failed"))
    );
    assert!(snapshot.symbols.is_empty());
}

#[test]
fn manual_call_extraction_preserves_same_line_calls() {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    let content = "fn run() { foo(); foo(); }\n";
    let language = detect_language("src/lib.rs").expect("rust should be configured");
    let parsed = parse_tree(language, content).expect("source should parse");
    let context = FileParseContext {
        build: &build,
        path: "src/lib.rs",
        file_id: "file",
        language_id: language.id,
        content,
    };
    let mut output = FileParseOutput {
        symbols: Vec::new(),
        references: Vec::new(),
    };

    collect_manual_nodes(&context, parsed.root_node(), &mut output)
        .expect("manual extraction should succeed");
    let foo_ranges = output
        .references
        .iter()
        .filter(|reference| reference.name == "foo")
        .map(|reference| reference.byte_range.clone())
        .collect::<Vec<_>>();

    assert_eq!(foo_ranges.len(), 2);
    assert_ne!(foo_ranges[0], foo_ranges[1]);
}

#[test]
fn rust_attributes_are_not_doc_comments() {
    let snapshot = parse_source_snapshot(
        "src/lib.rs",
        br#"
#[derive(Debug)]
pub struct Settings;
"#,
    );
    let settings = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Settings")
        .expect("struct symbol should be extracted");

    assert_eq!(settings.doc_comment, None);
}

#[test]
fn python_hash_comments_are_doc_comments() {
    let snapshot = parse_source_snapshot(
        "src/app.py",
        br#"
# Runs the worker.
def run_worker():
    pass
"#,
    );
    let worker = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "run_worker")
        .expect("function symbol should be extracted");

    assert_eq!(worker.doc_comment.as_deref(), Some("Runs the worker."));
}

#[test]
fn text_only_files_keep_bm25_fallback_chunks() {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );

    parse_indexed_file(&mut build, "README.txt", b"RetryPolicy appears in docs")
        .expect("file should index as text");
    let snapshot = build.finish();

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::TextOnly);
    assert_eq!(snapshot.chunks.len(), 1);
    assert!(snapshot.diagnostics[0].message.contains("grammar"));
}

#[test]
fn invalid_utf8_files_degrade_to_lossy_text_chunks() {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );

    parse_indexed_file(
        &mut build,
        "src/lib.rs",
        b"fn retry_policy() {}\n\xff\nfn caller() {}",
    )
    .expect("invalid utf8 should degrade instead of failing");
    let snapshot = build.finish();

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::TextOnly);
    assert!(snapshot.diagnostics[0].message.contains("not valid UTF-8"));
    assert!(snapshot.chunks[0].content.contains("retry_policy"));
}

#[test]
fn generated_record_ids_are_scoped_by_repository() {
    let first = parse_fixture_snapshot("repo-a");
    let second = parse_fixture_snapshot("repo-b");

    assert_ne!(
        first.references[0].reference_id,
        second.references[0].reference_id
    );
    assert_eq!(
        first
            .references
            .iter()
            .filter(|reference| reference.name == "retry_policy")
            .count(),
        2
    );
    assert_ne!(first.imports[0].import_id, second.imports[0].import_id);
    assert_ne!(first.chunks[0].chunk_id, second.chunks[0].chunk_id);
    assert_ne!(first.calls[0].call_id, second.calls[0].call_id);
}

#[test]
fn oversized_files_truncate_on_utf8_boundary() {
    let mut bytes = vec![b'a'; MAX_TEXT_FILE_BYTES - 1];
    bytes.extend("é".as_bytes());
    bytes.extend(b"tail");

    let (status, reason, content) =
        validate_text_content("src/lib.rs", &bytes, detect_language("src/lib.rs"))
            .expect("oversized utf8 should degrade");
    let content = content.expect("oversized file should keep fallback content");

    assert_eq!(status, CodeParseStatus::TextOnly);
    assert!(
        reason
            .expect("reason should explain budget")
            .contains("exceeds")
    );
    assert_eq!(content.len(), MAX_TEXT_FILE_BYTES - 1);
    assert!(!content.contains('\u{fffd}'));
}

fn parse_fixture_snapshot(repository_id: &str) -> crate::domain::CodeIndexSnapshot {
    parse_source_snapshot_for_repository(
        repository_id,
        "src/main.rs",
        br#"
use crate::retry_policy;

fn run_worker() {
    retry_policy(); retry_policy();
}
"#,
    )
}

fn parse_source_snapshot(path: &str, source: &[u8]) -> crate::domain::CodeIndexSnapshot {
    parse_source_snapshot_for_repository("repo", path, source)
}

fn parse_source_snapshot_for_repository(
    repository_id: &str,
    path: &str,
    source: &[u8],
) -> crate::domain::CodeIndexSnapshot {
    let registration = CodeRepositoryRegistration::new(
        repository_id,
        "alias",
        "/tmp/repo",
        Vec::new(),
        Vec::new(),
    )
    .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );

    parse_indexed_file(&mut build, path, source).expect("file should parse");

    build.finish()
}
