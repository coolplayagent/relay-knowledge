//! Direct effective-plugin identity and merge contract.

use super::{EffectivePlugin, plugins::dedupe_plugins};

#[test]
fn dedupe_merges_the_latest_plugin_metadata_by_profile_identity() {
    let first = plugin("1.0", 10);
    let replacement = plugin("2.0", 20);

    let plugins = dedupe_plugins(vec![first, replacement]);

    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].version.as_deref(), Some("2.0"));
    assert_eq!(plugins[0].line, 20);
}

fn plugin(version: &str, line: u32) -> EffectivePlugin {
    EffectivePlugin {
        artifact_id: "maven-compiler-plugin".to_owned(),
        version: Some(version.to_owned()),
        executions: Vec::new(),
        line,
        source_path: "pom.xml".to_owned(),
        coordinate: "org.apache.maven.plugins:maven-compiler-plugin".to_owned(),
        profile: None,
        inherited: true,
    }
}
