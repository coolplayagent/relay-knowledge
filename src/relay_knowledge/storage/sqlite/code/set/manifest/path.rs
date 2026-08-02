//! Manifest path normalization, workspace boundaries, and glob matching.

pub(super) fn workspace_relative_path(path: &str, root_path_prefix: &str) -> Option<String> {
    let path = clean(path);
    let root_path_prefix = clean(root_path_prefix);
    if root_path_prefix.is_empty() {
        return Some(path);
    }
    if path == root_path_prefix {
        return Some(String::new());
    }
    let stripped = path.strip_prefix(&root_path_prefix)?.strip_prefix('/')?;
    Some(stripped.to_owned())
}

pub(super) fn is_at_or_below_root(path: &str, root_path_prefix: &str) -> bool {
    let path = clean(path);
    let root_path_prefix = clean(root_path_prefix);
    root_path_prefix.is_empty()
        || path == root_path_prefix
        || path
            .strip_prefix(&root_path_prefix)
            .is_some_and(|relative| relative.starts_with('/'))
}

pub(super) fn workspace_pattern_matches(pattern: &str, path: &str) -> bool {
    let pattern = clean(pattern);
    let path = clean(path);
    if pattern == "." {
        return path.is_empty();
    }
    let pattern_segments = segments(&pattern);
    let path_segments = segments(&path);
    glob_segments_match(&pattern_segments, &path_segments)
}

fn glob_segments_match(pattern: &[&str], path: &[&str]) -> bool {
    match (pattern, path) {
        ([], []) => true,
        ([], _) => false,
        ([head, tail @ ..], _) if *head == "**" => {
            glob_segments_match(tail, path)
                || (!path.is_empty() && glob_segments_match(pattern, &path[1..]))
        }
        ([head, tail @ ..], [path_head, path_tail @ ..]) => {
            wildcard_segment_matches(head, path_head) && glob_segments_match(tail, path_tail)
        }
        _ => false,
    }
}

fn wildcard_segment_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return !value.is_empty();
    }
    if !pattern.contains('*') {
        return pattern == value;
    }
    let mut remainder = value;
    let mut parts = pattern.split('*').peekable();
    if let Some(first) = parts.next().filter(|part| !part.is_empty()) {
        let Some(stripped) = remainder.strip_prefix(first) else {
            return false;
        };
        remainder = stripped;
    }
    while let Some(part) = parts.next() {
        if part.is_empty() {
            continue;
        }
        if parts.peek().is_none() {
            return remainder.ends_with(part);
        }
        let Some(position) = remainder.find(part) else {
            return false;
        };
        remainder = &remainder[position + part.len()..];
    }

    true
}

fn segments(path: &str) -> Vec<&str> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

pub(super) fn join_workspace_path(root: &str, child: &str) -> Option<String> {
    let child = clean(child);
    if child.is_empty() || child.starts_with('/') || child.split('/').any(|part| part == "..") {
        return None;
    }
    let root = clean(root);
    if root.is_empty() || child == "." {
        return Some(if child == "." { root } else { child });
    }

    Some(format!("{root}/{child}"))
}

pub(super) fn parent(path: &str) -> String {
    let path = clean(path);
    path.rsplit_once('/')
        .map(|(parent, _)| parent.to_owned())
        .unwrap_or_default()
}

pub(super) fn package_is_ignored(path: &str) -> bool {
    path.split('/')
        .any(|segment| matches!(segment, "node_modules" | ".pnpm"))
}

pub(super) fn is_go_mod(path: &str) -> bool {
    has_file_name(path, "go.mod")
}

pub(super) fn is_go_work(path: &str) -> bool {
    has_file_name(path, "go.work")
}

pub(super) fn is_pnpm_workspace(path: &str) -> bool {
    clean(path)
        .rsplit('/')
        .next()
        .is_some_and(|name| matches!(name, "pnpm-workspace.yaml" | "pnpm-workspace.yml"))
}

pub(super) fn is_package_json(path: &str) -> bool {
    has_file_name(path, "package.json")
}

fn has_file_name(path: &str, expected: &str) -> bool {
    clean(path)
        .rsplit('/')
        .next()
        .is_some_and(|name| name == expected)
}

pub(super) fn clean(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_owned()
}

#[cfg(test)]
#[path = "path_tests.rs"]
mod tests;
