use crate::domain::{CodeIndexSnapshot, CodeRepositoryRegistration};

use super::*;

#[test]
fn knowledge_map_topics_are_extracted_from_full_source_content() {
    let padding = "x".repeat(8_500);
    let source = format!("topics:\n  # {padding}\n  - id: late-topic\n");
    let snapshot = parse_source_snapshot(".knowledge/knowledge-map.yaml", source.as_bytes());

    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| { symbol.kind == "knowledge_map_topic" && symbol.name == "late-topic" })
    );
    assert!(
        !snapshot
            .chunks
            .iter()
            .filter(|chunk| chunk.symbol_snapshot_id.is_none())
            .any(|chunk| chunk.content.contains("late-topic")),
        "the regression should prove topic extraction does not depend on truncated chunks"
    );
}

#[test]
fn knowledge_map_topics_tolerate_section_header_comments() {
    let source = "\
topics: # routing buckets
  - id: route-topic
sources: # authoritative docs
  - id: not-a-topic
";
    let snapshot = parse_source_snapshot(".knowledge/knowledge-map.yaml", source.as_bytes());

    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| { symbol.kind == "knowledge_map_topic" && symbol.name == "route-topic" })
    );
    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| { symbol.kind == "knowledge_map_topic" && symbol.name == "not-a-topic" })
    );
}

#[test]
fn knowledge_map_topics_ignore_nested_block_scalar_ids() {
    let source = "\
topics:
  - id: real-topic
    description: |
      id: not-a-topic
";
    let snapshot = parse_source_snapshot(".knowledge/knowledge-map.yaml", source.as_bytes());

    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| { symbol.kind == "knowledge_map_topic" && symbol.name == "real-topic" })
    );
    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| { symbol.kind == "knowledge_map_topic" && symbol.name == "not-a-topic" })
    );
}

#[test]
fn knowledge_map_topics_ignore_nested_sequence_ids() {
    let source = "\
topics:
  - id: real-topic
    related:
      - id: nested-topic
";
    let snapshot = parse_source_snapshot(".knowledge/knowledge-map.yaml", source.as_bytes());

    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| { symbol.kind == "knowledge_map_topic" && symbol.name == "real-topic" })
    );
    assert!(
        !snapshot.symbols.iter().any(|symbol| {
            symbol.kind == "knowledge_map_topic" && symbol.name == "nested-topic"
        })
    );
}

#[test]
fn knowledge_map_topics_accept_sequence_spacing_before_id() {
    let source = "\
topics:
  -   id: spaced-topic
";
    let snapshot = parse_source_snapshot(".knowledge/knowledge-map.yaml", source.as_bytes());

    assert!(
        snapshot.symbols.iter().any(|symbol| {
            symbol.kind == "knowledge_map_topic" && symbol.name == "spaced-topic"
        }),
        "valid YAML sequence spacing should not hide topic ids"
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
