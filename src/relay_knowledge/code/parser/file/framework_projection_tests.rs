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
    record_framework_graph(
        &mut build,
        "src/App.vue",
        "file",
        "vue",
        "<script setup>const props = defineProps(['title'])</script><template>{{ title }}</template>",
        &[],
    )
    .unwrap();

    assert!(
        build
            .framework_nodes
            .iter()
            .any(|node| node.kind == FrameworkNodeKind::Component)
    );
}
