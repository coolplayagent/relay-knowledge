use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

const MAX_INTERPOLATION_DEPTH: usize = 16;

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

pub(super) fn interpolate(value: &str, properties: &BTreeMap<String, String>) -> String {
    interpolate_with_depth(value, properties, 0)
}

fn interpolate_with_depth(
    value: &str,
    properties: &BTreeMap<String, String>,
    depth: usize,
) -> String {
    if depth >= MAX_INTERPOLATION_DEPTH {
        return value.to_owned();
    }
    let mut output = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let key = &after[..end];
        if let Some(replacement) = properties.get(key) {
            output.push_str(&interpolate_with_depth(replacement, properties, depth + 1));
        } else {
            output.push_str("${");
            output.push_str(key);
            output.push('}');
        }
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    output
}
