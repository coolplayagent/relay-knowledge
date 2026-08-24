use super::{accept_topic_item_indent, content_digest, facts, stable_id, top_level_yaml_section};
use crate::project::KNOWLEDGE_MAP_RELATIVE_PATH;

#[test]
fn extracts_only_topics_from_the_authoritative_knowledge_map() {
    let mut definitions = Vec::new();
    let content = "schema_version: 1\ntopics:\n  - id: parsing\n  - id: indexing\nignored:\n  - id: storage\n";

    facts(
        KNOWLEDGE_MAP_RELATIVE_PATH,
        "yaml",
        content,
        &mut definitions,
    );

    let names = definitions
        .iter()
        .map(|definition| definition.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["parsing", "indexing"]);
}

#[test]
fn fixes_the_topic_list_indent_after_the_first_item() {
    let mut indent = None;

    assert!(accept_topic_item_indent(&mut indent, 2));
    assert!(accept_topic_item_indent(&mut indent, 2));
    assert!(!accept_topic_item_indent(&mut indent, 4));
    assert_eq!(top_level_yaml_section("topics:"), Some("topics"));
    assert_eq!(top_level_yaml_section("  nested:"), None);
}

#[test]
fn emits_manifest_authorization_and_digest_verified_shard_facts() {
    let shard =
        "schema_version: 2\ntopic:\n  id: build\n  title: Build\nsources: []\nroute: null\n";
    let digest = content_digest(shard.as_bytes());
    let relative = format!("topics/topic-{}-{digest}.yaml", stable_id("build"));
    let root = format!(
        "schema_version: 2\nmap_version: 1\nupdated_at: now\ntopics:\n  - id: build\n    ref: {relative}\n    digest: {digest}\nhistory:\n  archived_through: 0\n  recent: []\n"
    );
    let mut root_facts = Vec::new();
    facts(KNOWLEDGE_MAP_RELATIVE_PATH, "yaml", &root, &mut root_facts);
    let shard_path = format!(".knowledge/{relative}");
    assert!(
        root_facts.iter().any(|fact| {
            fact.kind == "knowledge_map_topic_shard_ref" && fact.name == shard_path
        })
    );
    assert!(
        root_facts
            .iter()
            .any(|fact| { fact.kind == "knowledge_map_topic_shard_topic" && fact.name == "build" })
    );

    let mut shard_facts = Vec::new();
    facts(&shard_path, "yaml", shard, &mut shard_facts);
    assert!(
        shard_facts
            .iter()
            .any(|fact| { fact.kind == "knowledge_map_topic_shard" && fact.name == "build" })
    );

    let mut tampered_facts = Vec::new();
    facts(
        &shard_path,
        "yaml",
        &shard.replace("Build", "Tampered"),
        &mut tampered_facts,
    );
    assert!(tampered_facts.is_empty());
}

#[test]
fn accepts_valid_noncanonical_topic_indentation() {
    let shard =
        "schema_version: 2\ntopic:\n    id: build\n    title: Build\nsources: []\nroute: null\n";
    let digest = content_digest(shard.as_bytes());
    let path = format!(
        ".knowledge/topics/topic-{}-{digest}.yaml",
        stable_id("build")
    );
    let mut definitions = Vec::new();

    facts(&path, "yaml", shard, &mut definitions);

    assert!(
        definitions
            .iter()
            .any(|fact| { fact.kind == "knowledge_map_topic_shard" && fact.name == "build" })
    );
}

#[test]
fn accepts_a_flow_mapping_on_the_line_after_the_topic_key() {
    let shard =
        "schema_version: 2\ntopic:\n  {id: build, title: Build}\nsources: []\nroute: null\n";
    let digest = content_digest(shard.as_bytes());
    let path = format!(
        ".knowledge/topics/topic-{}-{digest}.yaml",
        stable_id("build")
    );
    let mut definitions = Vec::new();

    facts(&path, "yaml", shard, &mut definitions);

    assert!(
        definitions
            .iter()
            .any(|fact| { fact.kind == "knowledge_map_topic_shard" && fact.name == "build" })
    );
}

#[test]
fn malformed_v2_refs_do_not_fall_back_to_legacy_root_topics() {
    let content = "schema_version: 2\ntopics:\n  - id: unauthorized\n    ref: ../escape.yaml\n    digest: invalid\n";
    let mut definitions = Vec::new();

    facts(
        KNOWLEDGE_MAP_RELATIVE_PATH,
        "yaml",
        content,
        &mut definitions,
    );

    assert!(
        !definitions
            .iter()
            .any(|fact| { fact.kind == "knowledge_map_topic" && fact.name == "unauthorized" })
    );
}

#[test]
fn flow_style_v2_topics_emit_authorization_facts() {
    let shard = "schema_version: 2\ntopic: {id: build, title: Build}\nsources: []\nroute: null\n";
    let digest = content_digest(shard.as_bytes());
    let relative = format!("topics/topic-{}-{digest}.yaml", stable_id("build"));
    let root = format!(
        "schema_version: 2\nmap_version: 1\nupdated_at: now\ntopics: [{{id: build, ref: {relative}, digest: {digest}}}]\nhistory: {{archived_through: 0, recent: []}}\n"
    );
    let mut definitions = Vec::new();

    facts(KNOWLEDGE_MAP_RELATIVE_PATH, "yaml", &root, &mut definitions);

    assert!(definitions.iter().any(|fact| {
        fact.kind == "knowledge_map_topic_shard_ref"
            && fact.name == format!(".knowledge/{relative}")
    }));
}
