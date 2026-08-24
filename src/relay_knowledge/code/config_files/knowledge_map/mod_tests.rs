use super::{accept_topic_item_indent, content_digest, facts, stable_id, top_level_yaml_section};
use crate::project::KNOWLEDGE_MAP_RELATIVE_PATH;

fn valid_root(relative: &str, digest: &str) -> String {
    format!(
        "schema_version: 2\nmap_version: 1\nupdated_at: now\ntopics:\n  - id: build\n    title: Build\n    description: Build knowledge\n    source_ids: []\n    ref: {relative}\n    digest: {digest}\nhistory:\n  archived_through: 0\n  recent:\n    - version: 1\n      action: init\n      actor: test\n      summary: Initialize map\n"
    )
}

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
    let root = valid_root(&relative, &digest);
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
fn incomplete_or_globally_inconsistent_v2_manifests_authorize_no_shards() {
    let digest = "a".repeat(64);
    let relative = format!("topics/topic-{}-{digest}.yaml", stable_id("build"));
    let valid = valid_root(&relative, &digest);
    let conflicting_id = valid.replace(
        "history:\n",
        &format!(
            "  - id: Build\n    title: Other build\n    description: Conflicts by folded id\n    source_ids: []\n    ref: topics/topic-{}-{digest}.yaml\n    digest: {digest}\nhistory:\n",
            stable_id("Build")
        ),
    );
    let invalid = [
        valid.replacen("map_version: 1\n", "", 1),
        valid.replacen("history:\n", "ignored_history:\n", 1),
        valid.replace("version: 1", "version: 2"),
        conflicting_id,
    ];

    for manifest in invalid {
        let mut definitions = Vec::new();
        facts(
            KNOWLEDGE_MAP_RELATIVE_PATH,
            "yaml",
            &manifest,
            &mut definitions,
        );
        assert!(
            definitions.is_empty(),
            "invalid v2 manifest authorized shard facts: {manifest}"
        );
    }
}

#[test]
fn flow_style_v2_topics_emit_authorization_facts() {
    let shard = "schema_version: 2\ntopic: {id: build, title: Build}\nsources: []\nroute: null\n";
    let digest = content_digest(shard.as_bytes());
    let relative = format!("topics/topic-{}-{digest}.yaml", stable_id("build"));
    let root = format!(
        "schema_version: 2\nmap_version: 1\nupdated_at: now\ntopics: [{{id: build, title: Build, description: Build knowledge, source_ids: [], ref: {relative}, digest: {digest}}}]\nhistory: {{archived_through: 0, recent: [{{version: 1, action: init, actor: test, summary: Initialize map}}]}}\n"
    );
    let mut definitions = Vec::new();

    facts(KNOWLEDGE_MAP_RELATIVE_PATH, "yaml", &root, &mut definitions);

    assert!(definitions.iter().any(|fact| {
        fact.kind == "knowledge_map_topic_shard_ref"
            && fact.name == format!(".knowledge/{relative}")
    }));
}

#[test]
fn quoted_ref_keys_emit_manifest_authorization_facts() {
    let shard = "schema_version: 2\ntopic: {id: build, title: Build}\nsources: []\nroute: null\n";
    let digest = content_digest(shard.as_bytes());
    let relative = format!("topics/topic-{}-{digest}.yaml", stable_id("build"));
    let root = format!(
        "schema_version: 2\nmap_version: 1\nupdated_at: now\ntopics:\n  - id: build\n    title: Build\n    description: Build knowledge\n    source_ids: []\n    \"ref\": {relative}\n    digest: {digest}\nhistory: {{archived_through: 0, recent: [{{version: 1, action: init, actor: test, summary: Initialize map}}]}}\n"
    );
    let mut definitions = Vec::new();

    facts(KNOWLEDGE_MAP_RELATIVE_PATH, "yaml", &root, &mut definitions);

    assert!(definitions.iter().any(|fact| {
        fact.kind == "knowledge_map_topic_shard_ref"
            && fact.name == format!(".knowledge/{relative}")
    }));
}
