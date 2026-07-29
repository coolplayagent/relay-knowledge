//! Detection of Go multi-module workspaces via `go.work`.

#[cfg(test)]
use std::path::{Path, PathBuf};

use crate::domain::CodeWorkspaceMember;

use super::{WorkspaceSource, join_relative_path};

/// Known Go workspace file name.
const GO_WORK_FILE: &str = "go.work";
/// Marker for the `use (...)` directive in go.work.
const USE_DIRECTIVE: &str = "use";

/// Tries to read a `go.work` file at the repository root and extract the
/// module paths from every `use` directive whose target directory contains
/// a `go.mod`.
///
/// Returns `None` when `go.work` does not exist or cannot be read.
pub(super) fn detect_go_work(source: &dyn WorkspaceSource) -> Option<Vec<CodeWorkspaceMember>> {
    let content = source.read_to_string(GO_WORK_FILE)?;
    let members = parse_go_work_content(source, &content);
    if members.is_empty() {
        None
    } else {
        Some(members)
    }
}

fn parse_go_work_content(source: &dyn WorkspaceSource, content: &str) -> Vec<CodeWorkspaceMember> {
    let mut members = Vec::new();
    let mut in_use_block = false;

    for line in content.lines() {
        let trimmed = strip_comment(line, '/').trim().to_string();

        if trimmed.is_empty() {
            continue;
        }

        if trimmed == "use (" {
            in_use_block = true;
            continue;
        }

        if in_use_block && trimmed == ")" {
            in_use_block = false;
            continue;
        }

        let directive_line = if let Some(rest) = strip_directive(&trimmed, USE_DIRECTIVE) {
            rest.trim()
        } else if in_use_block {
            &trimmed
        } else {
            continue;
        };

        let use_path = directive_line.trim_matches('"');
        if use_path.is_empty() || use_path == ".." {
            continue;
        }

        let module_name = read_go_module_name(source, use_path);
        if module_name.is_empty() {
            continue;
        }

        members.push(CodeWorkspaceMember {
            package_name: module_name,
            relative_path: use_path.to_string(),
        });
    }

    members
}

/// Reads the first `module <name>` line from `go.mod` in `dir`.
fn read_go_module_name(source: &dyn WorkspaceSource, dir: &str) -> String {
    let Some(content) = source.read_to_string(&join_relative_path(dir, "go.mod")) else {
        return String::new();
    };
    for line in content.lines() {
        let trimmed = strip_comment(line, '/').trim();
        if let Some(name) = trimmed.strip_prefix("module ") {
            let name = name.trim();
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    String::new()
}

/// Strips Go-style `//` and `/* */` comments.
fn strip_comment(line: &str, _marker: char) -> &str {
    let line_comment = line.find("//");
    let block_comment = line.find("/*");
    match (line_comment, block_comment) {
        (Some(l), Some(b)) => {
            let idx = l.min(b);
            &line[..idx]
        }
        (Some(idx), None) | (None, Some(idx)) => &line[..idx],
        (None, None) => line,
    }
}

/// Strips the leading directive keyword from `line` if present.
fn strip_directive<'a>(line: &'a str, directive: &str) -> Option<&'a str> {
    line.strip_prefix(directive)
        .filter(|rest| rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t'))
}

#[cfg(test)]
#[path = "go_work_tests.rs"]
mod tests;
