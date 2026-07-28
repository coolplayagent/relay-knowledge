//! Owns npm local-reference and package-lock entry interpretation.

use serde_json::Value;

pub(super) fn npm_requirement_is_local(requirement: &str) -> bool {
    let requirement = requirement.trim();
    requirement.starts_with('.')
        || requirement.starts_with('/')
        || requirement.starts_with("~/")
        || ["file:", "link:", "portal:", "workspace:"]
            .iter()
            .any(|prefix| requirement.starts_with(prefix))
}

pub(super) fn package_lock_entry_is_local(package: &Value) -> bool {
    package.get("link").and_then(Value::as_bool) == Some(true)
        || package
            .get("resolved")
            .and_then(Value::as_str)
            .is_some_and(npm_requirement_is_local)
}

pub(super) fn package_lock_package_name(path: &str, package: &Value) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    package
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .or_else(|| package_lock_package_name_from_path(path))
}

fn package_lock_package_name_from_path(path: &str) -> Option<String> {
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    let mut package_name = None;
    while let Some(segment) = segments.next() {
        if segment != "node_modules" {
            continue;
        }
        let Some(first) = segments.next() else {
            continue;
        };
        package_name = if first.starts_with('@') {
            segments.next().map(|name| format!("{first}/{name}"))
        } else {
            Some(first.to_owned())
        };
    }
    package_name
}
