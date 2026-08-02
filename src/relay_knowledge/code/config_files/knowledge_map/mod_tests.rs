use super::{accept_topic_item_indent, facts, top_level_yaml_section};
use crate::project::KNOWLEDGE_MAP_RELATIVE_PATH;

#[test]
fn extracts_only_topics_from_the_authoritative_knowledge_map() {
    let mut definitions = Vec::new();
    let content = "topics:\n  - id: parsing\n  - id: indexing\nignored:\n  - id: storage\n";

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
