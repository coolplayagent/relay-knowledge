use crate::domain::{CodeIndexSnapshot, CodeParseStatus, CodeRepositoryRegistration};

use super::*;

#[test]
fn oversized_markdown_text_only_files_keep_late_heading_symbols() {
    let padding = "x".repeat(MAX_TEXT_FILE_BYTES + 128);
    let source = format!("# Intro\n\n{padding}\n## Late Heading\n");
    let snapshot = parse_source_snapshot("docs/spec.md", source.as_bytes());

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::TextOnly);
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "heading" && symbol.name == "Late Heading")
    );
    assert!(
        !snapshot
            .chunks
            .iter()
            .any(|chunk| chunk.content.contains("Late Heading")),
        "late heading coverage must not rely on the truncated text chunk"
    );
}

#[test]
fn oversized_knowledge_map_text_only_files_keep_late_topic_symbols() {
    let padding = "x".repeat(MAX_TEXT_FILE_BYTES + 128);
    let source = format!("topics:\n  # {padding}\n  - id: oversized-topic\n");
    let snapshot = parse_source_snapshot(".knowledge/knowledge-map.yaml", source.as_bytes());

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::TextOnly);
    assert!(snapshot.symbols.iter().any(|symbol| {
        symbol.kind == "knowledge_map_topic" && symbol.name == "oversized-topic"
    }));
    assert!(
        !snapshot
            .chunks
            .iter()
            .any(|chunk| chunk.content.contains("oversized-topic")),
        "late topic coverage must not rely on the truncated text chunk"
    );
}

#[test]
fn oversized_knowledge_map_text_only_files_ignore_nested_sequence_ids() {
    let padding = "x".repeat(MAX_TEXT_FILE_BYTES + 128);
    let source = format!(
        "topics:\n  - id: real-topic\n    related:\n      # {padding}\n      - id: nested-topic\n"
    );
    let snapshot = parse_source_snapshot(".knowledge/knowledge-map.yaml", source.as_bytes());

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::TextOnly);
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "knowledge_map_topic" && symbol.name == "real-topic")
    );
    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "knowledge_map_topic" && symbol.name == "nested-topic")
    );
}

#[test]
fn text_only_topic_scan_preserves_valid_lines_after_invalid_utf8() {
    let snapshot = parse_source_snapshot("docs/spec.md", b"\xFF\n## Late Heading\n");

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::TextOnly);
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "heading" && symbol.name == "Late Heading")
    );
}

fn parse_source_snapshot(path: &str, source: &[u8]) -> CodeIndexSnapshot {
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
    parse_indexed_file(&mut build, path, source).expect("file should parse");
    build.finish()
}
