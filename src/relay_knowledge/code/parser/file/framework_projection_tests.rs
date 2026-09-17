use crate::{
    code::SnapshotBuild,
    domain::{CodeRepositoryRegistration, FrameworkNodeKind},
};

use super::record_framework_graph;

#[test]
fn projection_publishes_vue_component_facts() {
    let registration =
        CodeRepositoryRegistration::new("repository", "vue", "/tmp/vue", Vec::new(), Vec::new())
            .unwrap();
    let mut build = SnapshotBuild::new(
        &registration,
        "HEAD".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    let source = "<script setup>const props = defineProps(['title'])</script><template>{{ title }}</template>";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_html::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    record_framework_graph(
        &mut build,
        "src/App.vue",
        "file",
        "vue",
        source,
        &[],
        tree.root_node(),
    )
    .unwrap();

    assert!(
        build
            .framework_nodes
            .iter()
            .any(|node| node.kind == FrameworkNodeKind::Component)
    );
}
