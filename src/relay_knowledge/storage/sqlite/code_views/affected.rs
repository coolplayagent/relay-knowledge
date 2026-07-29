use rusqlite::types::Value;

use crate::domain::{CodebaseViewKind, CodebaseViewRequest};

use super::{escape_like, normalized_path_filter, push_unique_path};

pub(super) fn append_file_focus(
    sql: &mut String,
    values: &mut Vec<Value>,
    request: &CodebaseViewRequest,
) {
    if request.view_kind != CodebaseViewKind::AffectedScope {
        return;
    }
    let focus = file_focus_paths(&request.changed_paths);
    if focus.paths.is_empty() && !focus.include_root_files {
        return;
    }
    sql.push_str(" AND (");
    let mut has_filter = false;
    for path in &focus.paths {
        append_or(sql, &mut has_filter);
        sql.push_str("path = ? OR path LIKE ? ESCAPE '\\'");
        values.push(Value::Text(path.clone()));
        values.push(Value::Text(format!("{}/%", escape_like(path))));
    }
    if focus.include_root_files {
        append_or(sql, &mut has_filter);
        sql.push_str("path NOT LIKE ?");
        values.push(Value::Text("%/%".to_owned()));
    }
    sql.push(')');
}

struct FileFocus {
    paths: Vec<String>,
    include_root_files: bool,
}

fn file_focus_paths(changed_paths: &[String]) -> FileFocus {
    let mut focus = FileFocus {
        paths: Vec::new(),
        include_root_files: false,
    };
    for path in changed_paths
        .iter()
        .filter_map(|path| normalized_path_filter(path))
    {
        push_unique_path(&mut focus.paths, path.clone());
        if let Some(parent) = changed_file_parent_focus(&path) {
            if parent.is_empty() {
                focus.include_root_files = true;
            } else {
                push_unique_path(&mut focus.paths, parent.to_owned());
            }
        }
    }
    focus
}

fn changed_file_parent_focus(path: &str) -> Option<&str> {
    let (parent, file_name) = path.rsplit_once('/').unwrap_or(("", path));
    let uppercase = file_name.bytes().any(|byte| byte.is_ascii_uppercase());
    (file_name.contains('.') || uppercase).then_some(parent)
}

fn append_or(sql: &mut String, has_filter: &mut bool) {
    if *has_filter {
        sql.push_str(" OR ");
    }
    *has_filter = true;
}

#[cfg(test)]
mod tests {
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
}
