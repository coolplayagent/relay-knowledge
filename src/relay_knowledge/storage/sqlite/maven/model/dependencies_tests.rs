//! Direct effective-dependency identity and replacement contract.

use super::{EffectiveDependency, dependencies::dedupe_dependencies};

#[test]
fn dedupe_keeps_the_latest_dependency_for_each_effective_identity() {
    let first = dependency("1.0", 10);
    let replacement = dependency("2.0", 20);

    let dependencies = dedupe_dependencies(vec![first, replacement]);

    assert_eq!(dependencies.len(), 1);
    assert_eq!(dependencies[0].version.as_deref(), Some("2.0"));
    assert_eq!(dependencies[0].line, 20);
}

fn dependency(version: &str, line: u32) -> EffectiveDependency {
    EffectiveDependency {
        group_id: "com.example".to_owned(),
        artifact_id: "core".to_owned(),
        version: Some(version.to_owned()),
        scope: None,
        dep_type: None,
        classifier: None,
        optional: None,
        profile: None,
        line,
        source_file_id: "pom".to_owned(),
        source_path: "pom.xml".to_owned(),
    }
}
