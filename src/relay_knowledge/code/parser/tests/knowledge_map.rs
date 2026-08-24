use crate::domain::{CodeIndexSnapshot, CodeRepositoryRegistration};
use sha2::{Digest, Sha256};

use super::*;

#[test]
fn knowledge_map_topics_are_extracted_from_full_source_content() {
    let padding = "x".repeat(8_500);
    let source = format!("schema_version: 1\ntopics:\n  # {padding}\n  - id: late-topic\n");
    let snapshot = parse_source_snapshot(".knowledge/knowledge-map.yaml", source.as_bytes());

    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| { symbol.kind == "knowledge_map_topic" && symbol.name == "late-topic" })
    );
    assert!(
        snapshot
            .chunks
            .iter()
            .filter(|chunk| chunk.symbol_snapshot_id.is_none())
            .any(|chunk| chunk.content.contains("late-topic")),
        "bounded source windows should retain late authorized content"
    );
}

#[test]
fn knowledge_map_topics_tolerate_section_header_comments() {
    let source = "\
schema_version: 1
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
schema_version: 1
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
schema_version: 1
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
schema_version: 1
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

#[test]
fn v2_manifest_and_verified_shard_emit_joinable_authorization_symbols() {
    let shard = "schema_version: 2\ntopic:\n  id: build\n  title: Build\n  description: Build knowledge\nsources: []\nroute: null\n";
    let digest = format!("{:x}", Sha256::digest(shard.as_bytes()));
    let topic_key = format!("{:x}", Sha256::digest(b"build"));
    let path = format!(".knowledge/topics/topic-{}-{digest}.yaml", &topic_key[..16]);
    let root = format!(
        "schema_version: 2\nmap_version: 1\nupdated_at: now\ntopics:\n  - id: build\n    title: Build\n    description: Build knowledge\n    source_ids: []\n    ref: {}\n    digest: {digest}\nhistory:\n  archived_through: 0\n  recent:\n    - version: 1\n      action: init\n      actor: test\n      summary: Initialize map\n",
        path.strip_prefix(".knowledge/").expect("owned path")
    );

    let root_snapshot = parse_source_snapshot(".knowledge/knowledge-map.yaml", root.as_bytes());
    let shard_snapshot = parse_source_snapshot(&path, shard.as_bytes());

    assert!(
        root_snapshot.symbols.iter().any(|symbol| {
            symbol.kind == "knowledge_map_topic_shard_ref" && symbol.name == path
        })
    );
    assert!(root_snapshot.symbols.iter().any(|symbol| {
        symbol.kind == "knowledge_map_topic_shard_topic" && symbol.name == "build"
    }));
    assert!(
        shard_snapshot
            .symbols
            .iter()
            .any(|symbol| { symbol.kind == "knowledge_map_topic_shard" && symbol.name == "build" })
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
