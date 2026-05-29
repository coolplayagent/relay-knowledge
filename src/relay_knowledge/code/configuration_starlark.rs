use std::path::Path;

use super::{strip_inline_hash_comment, valid_target};

pub(super) fn load_text_without_comments(text: &str) -> String {
    text.lines()
        .map(strip_inline_hash_comment)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn target_label(path: &str, label: &str) -> Option<String> {
    if label.ends_with(".bzl") {
        return None;
    }
    if pseudo_label(label) {
        return None;
    }
    if label.starts_with("//") {
        return Some(package_label(label));
    }
    if label.starts_with('@') {
        return Some(label.to_owned());
    }
    if let Some(local) = label.strip_prefix(':') {
        return valid_target(local).then(|| local_label(path, local));
    }

    Some(label.to_owned())
}

fn pseudo_label(label: &str) -> bool {
    label.starts_with("//visibility:") || label.starts_with("//conditions:")
}

fn package_label(label: &str) -> String {
    if label.contains(':') {
        return label.to_owned();
    }
    let package = label.trim_start_matches("//");
    let Some(target) = package
        .rsplit('/')
        .next()
        .filter(|segment| !segment.is_empty())
    else {
        return label.to_owned();
    };

    format!("{label}:{target}")
}

fn local_label(path: &str, target: &str) -> String {
    let package = Path::new(path)
        .parent()
        .and_then(Path::to_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    format!("//{package}:{target}")
}

pub(super) fn qualified_target_name(path: &str, name: &str) -> Option<String> {
    let file_name = Path::new(path).file_name()?.to_str()?;
    if !matches!(file_name, "BUILD" | "BUILD.bazel") || name.starts_with("//") {
        return None;
    }

    let package = Path::new(path)
        .parent()
        .and_then(Path::to_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    Some(format!("//{package}:{name}"))
}
