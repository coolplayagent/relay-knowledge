use super::*;
use crate::domain::JavaNamespaceEvidence;

#[test]
fn insertion_cost_counts_repeated_owner_strings_and_unknown_rows() {
    let mut file = RepositoryCodeFileRecord {
        repository_id: "repo".into(),
        source_scope: "scope".into(),
        file_id: "file".into(),
        path: "App.java".into(),
        language_id: "java".into(),
        blob_hash: "blob".into(),
        byte_len: 1,
        line_count: 1,
        parse_status: CodeParseStatus::Parsed,
        is_generated: false,
        degraded_reason: None,
        java_namespace: None,
    };
    let common = 5 + 8 + 128;
    assert_eq!(file.namespace_projection_cost(), (1, common + 7));
    file.java_namespace = Some(JavaNamespaceEvidence {
        source_set: crate::domain::JavaSourceSet::Repository,
        package: "demo".into(),
        top_level_types: vec!["One".into(), "Two".into()],
        complete: true,
    });
    assert_eq!(
        file.namespace_projection_cost(),
        (3, 3 * (common + 4 + 10) + 6)
    );
    file.java_namespace.as_mut().unwrap().complete = false;
    assert_eq!(file.namespace_projection_cost(), (1, common + 10));
    file.language_id = "rust".into();
    assert_eq!(file.namespace_projection_cost(), (0, 0));
}
