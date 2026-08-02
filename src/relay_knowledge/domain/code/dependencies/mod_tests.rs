use super::*;

#[test]
fn optional_dependency_versions_are_omitted_and_round_trip() {
    let record = CodeDependencyRecord {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        dependency_id: "dependency".to_owned(),
        file_id: "file".to_owned(),
        path: "Cargo.toml".to_owned(),
        language_id: "toml".to_owned(),
        ecosystem: "cargo".to_owned(),
        package_name: "serde".to_owned(),
        requirement: None,
        resolved_version: None,
        dependency_group: "dependencies".to_owned(),
        source_kind: "manifest".to_owned(),
        is_lockfile: false,
        line_range: RepositoryCodeRange { start: 4, end: 4 },
        excerpt: "serde = \"1\"".to_owned(),
    };

    let value = serde_json::to_value(&record).expect("dependency should serialize");

    assert!(value.get("requirement").is_none());
    assert!(value.get("resolved_version").is_none());
    assert_eq!(
        serde_json::from_value::<CodeDependencyRecord>(value)
            .expect("dependency should round trip"),
        record
    );
}
