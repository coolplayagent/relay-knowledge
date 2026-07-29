//! Detection of pnpm monorepo workspaces via `pnpm-workspace.yaml`.

use std::{collections::BTreeSet, path::Path};

#[cfg(test)]
use std::path::PathBuf;

use serde_json::Value;

use crate::{code::parser::workspace::join_relative_path, domain::CodeWorkspaceMember};

use super::WorkspaceSource;

const PNPM_RECURSIVE_WORKSPACE_DIR_LIMIT: usize = 1024;
const PNPM_RECURSIVE_WORKSPACE_ENTRY_LIMIT: usize = 8192;

/// Tries to read `pnpm-workspace.yaml` at the repository root and extract
/// the `packages` array entries as workspace members.
///
/// Negated patterns (starting with `!`) are silently skipped.
/// Each non-negated entry that resolves to a concrete directory gets a
/// member record.  The package name is read from the directory's
/// `package.json` when available, falling back to the directory name.
///
/// Returns `None` when the workspace file does not exist or cannot be read.
pub(super) fn detect_pnpm_workspace(
    source: &dyn WorkspaceSource,
) -> Option<Vec<CodeWorkspaceMember>> {
    let content = source.read_to_string("pnpm-workspace.yaml")?;
    let members = parse_pnpm_workspace_content(source, &content);
    if members.is_empty() {
        None
    } else {
        Some(members)
    }
}

fn parse_pnpm_workspace_content(
    source: &dyn WorkspaceSource,
    content: &str,
) -> Vec<CodeWorkspaceMember> {
    let package_patterns = parse_yaml_packages_list(content);
    let excluded = expanded_pnpm_exclusions(source, &package_patterns);
    let mut members = Vec::new();
    let mut seen_paths = BTreeSet::new();

    if let Some(package_name) = read_package_json_name(source, "") {
        seen_paths.insert(".".to_owned());
        members.push(CodeWorkspaceMember {
            package_name,
            relative_path: ".".to_owned(),
        });
    }

    for pattern in package_patterns {
        if pattern.starts_with('!') {
            continue;
        }

        for relative_path in expand_package_pattern(source, &pattern) {
            if relative_path.is_empty()
                || relative_path == "."
                || relative_path == ".."
                || excluded.contains(&relative_path)
                || !seen_paths.insert(relative_path.clone())
            {
                continue;
            }

            let package_name = read_package_json_name(source, &relative_path)
                .unwrap_or_else(|| dir_name(&relative_path));

            members.push(CodeWorkspaceMember {
                package_name,
                relative_path,
            });
        }
    }

    members
}

/// Parses a minimal YAML document and extracts the strings inside the
/// `packages:` list.  Handles both block-style (`- value`) and
/// inline-style (`[v1, v2]`) lists.
fn parse_yaml_packages_list(content: &str) -> Vec<String> {
    let mut patterns = Vec::new();
    let mut in_packages = false;

    for line in content.lines() {
        let trimmed = strip_yaml_comment(line).trim().to_string();

        if trimmed.is_empty() {
            continue;
        }

        // Detect `packages:` key
        if let Some(rest) = trimmed.strip_prefix("packages:") {
            in_packages = true;
            let rest = rest.trim();
            // Inline array: `packages: [a, b]`
            if rest.starts_with('[') && rest.ends_with(']') {
                patterns.extend(parse_inline_yaml_array(&rest[1..rest.len() - 1]));
                break;
            }
            continue;
        }

        if !in_packages {
            continue;
        }

        // Block-style list item: `- 'pattern'` or `- "pattern"` or `- pattern`
        if let Some(value) = trimmed.strip_prefix("- ") {
            let value = yaml_unquote(value.trim());
            if value.is_empty() {
                continue;
            }
            patterns.push(value.to_string());
        }
        // If we see a non-list-item key (not starting with `-`), we've left
        // the packages array.
        else if !trimmed.starts_with('-') {
            break;
        }
    }

    patterns
}

/// Strips single or double quotes around a YAML scalar value.
fn yaml_unquote(value: &str) -> &str {
    if value.len() >= 2
        && ((value.starts_with('\'') && value.ends_with('\''))
            || (value.starts_with('"') && value.ends_with('"')))
    {
        return &value[1..value.len() - 1];
    }
    value
}

/// Parses a bare inline YAML array value like `'a', 'b', c`.
fn parse_inline_yaml_array(content: &str) -> Vec<String> {
    content
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| yaml_unquote(s).to_string())
        .collect()
}

/// Strips a YAML comment (`# ...`) from the end of `line`.
fn strip_yaml_comment(line: &str) -> &str {
    // Simple: find first `#` not inside quotes.
    let mut in_single = false;
    let mut in_double = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => return &line[..i],
            _ => {}
        }
    }
    line
}

/// Normalises a glob pattern to a concrete relative path segment.
///
/// Strips prefixes like `./` and suffix patterns like `/*` / `/**` to
/// produce the shortest directory path that the pattern covers.
fn normalize_workspace_path(pattern: &str) -> String {
    let mut p = pattern.trim().to_string();

    // Remove leading ./
    if let Some(rest) = p.strip_prefix("./") {
        p = rest.to_string();
    }
    p.trim_matches('/').to_owned()
}

fn expand_package_pattern(source: &dyn WorkspaceSource, pattern: &str) -> Vec<String> {
    let pattern = normalize_workspace_path(pattern);
    if pattern.ends_with("/*") {
        let parent = pattern.trim_end_matches("/*");
        return source
            .child_dirs(parent)
            .into_iter()
            .filter(|dir| read_package_json_name(source, dir).is_some())
            .collect();
    }
    if pattern.ends_with("/**") {
        let parent = pattern.trim_end_matches("/**");
        return package_dirs_under(source, parent);
    }
    vec![pattern]
}

fn package_dirs_under(source: &dyn WorkspaceSource, parent: &str) -> Vec<String> {
    source
        .descendant_dirs_containing_file(
            parent,
            "package.json",
            PNPM_RECURSIVE_WORKSPACE_DIR_LIMIT,
            PNPM_RECURSIVE_WORKSPACE_ENTRY_LIMIT,
        )
        .into_iter()
        .filter(|dir| read_package_json_name(source, dir).is_some())
        .collect()
}

fn expanded_pnpm_exclusions(source: &dyn WorkspaceSource, patterns: &[String]) -> BTreeSet<String> {
    patterns
        .iter()
        .filter_map(|pattern| pattern.strip_prefix('!'))
        .flat_map(|pattern| expand_package_pattern(source, pattern))
        .collect()
}

/// Reads the `"name"` field from a `package.json` file in `dir`.
fn read_package_json_name(source: &dyn WorkspaceSource, dir: &str) -> Option<String> {
    let content = source.read_to_string(&join_relative_path(dir, "package.json"))?;
    let value = serde_json::from_str::<Value>(&content).ok()?;
    value
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

/// Returns the last component of a relative path as a fallback package name.
fn dir_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

#[cfg(test)]
#[path = "pnpm_workspace_tests.rs"]
mod tests;
