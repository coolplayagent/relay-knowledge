use rusqlite::types::Value;

use crate::domain::{
    CodeRepositorySelector, CodebaseViewKind, CodebaseViewRequest, FreshnessPolicy,
};

use super::append_file_focus;

#[test]
fn root_changed_files_focus_root_siblings() {
    let request = CodebaseViewRequest::new(
        CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap(),
        CodebaseViewKind::AffectedScope,
        FreshnessPolicy::AllowStale,
        10,
        vec!["Cargo.toml".to_owned()],
    )
    .unwrap();
    let mut sql = "SELECT path FROM code_repository_files WHERE source_scope = ?1".to_owned();
    let mut values = Vec::new();

    append_file_focus(&mut sql, &mut values, &request);

    assert!(sql.contains("path = ? OR path LIKE ? ESCAPE '\\'"));
    assert!(sql.contains("path NOT LIKE ?"));
    assert_eq!(values[0], Value::Text("Cargo.toml".to_owned()));
    assert_eq!(values[2], Value::Text("%/%".to_owned()));
}
