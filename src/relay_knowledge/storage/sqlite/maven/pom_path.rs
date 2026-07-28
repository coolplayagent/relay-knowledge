//! Resolves relative POM paths without escaping the indexed repository root.

use std::path::{Component, Path, PathBuf};

pub(super) fn relative_pom_path(path: &str, relative_path: &str) -> Option<String> {
    let base = Path::new(path).parent().unwrap_or_else(|| Path::new(""));
    let joined = normalize_path(base.join(relative_path))?;
    Some(joined.to_string_lossy().replace('\\', "/"))
}

fn normalize_path(path: PathBuf) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::Prefix(_) | Component::RootDir => return None,
        }
    }
    Some(normalized)
}
