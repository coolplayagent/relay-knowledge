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
mod tests {
    use crate::code::parser::workspace::FilesystemWorkspaceSource;

    use super::*;

    // ── Content-level parsing tests ────────────────────────────────────

    #[test]
    fn parses_use_block() {
        let root = test_root();
        let content = "go 1.21\n\nuse (\n\t./api\n\t./core\n\t./libs/shared\n)\n";
        tmpdir_with_gomod(&root, "api", "example.com/svc/api");
        tmpdir_with_gomod(&root, "core", "example.com/svc/core");
        tmpdir_with_gomod(&root, "libs/shared", "example.com/svc/shared");

        let source = FilesystemWorkspaceSource::new(&root);
        let members = parse_go_work_content(&source, content);
        assert_eq!(members.len(), 3, "should detect 3 use entries");
        assert_eq!(members[0].package_name, "example.com/svc/api");
        assert_eq!(members[0].relative_path, "./api");
        assert_eq!(members[1].package_name, "example.com/svc/core");
        assert_eq!(members[1].relative_path, "./core");
        assert_eq!(members[2].package_name, "example.com/svc/shared");
        assert_eq!(members[2].relative_path, "./libs/shared");
    }

    #[test]
    fn parses_single_use_lines() {
        let root = test_root();
        let content = "go 1.20\n\nuse ./api\nuse ./core\n";
        tmpdir_with_gomod(&root, "api", "example.com/svc/api");
        tmpdir_with_gomod(&root, "core", "example.com/svc/core");

        let source = FilesystemWorkspaceSource::new(&root);
        let members = parse_go_work_content(&source, content);
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].package_name, "example.com/svc/api");
        assert_eq!(members[1].package_name, "example.com/svc/core");
    }

    #[test]
    fn skips_missing_go_mod() {
        let root = test_root();
        let content = "go 1.21\n\nuse (\n\t./api\n\t./missing-pkg\n)\n";
        tmpdir_with_gomod(&root, "api", "example.com/api");
        // missing-pkg has no go.mod

        let source = FilesystemWorkspaceSource::new(&root);
        let members = parse_go_work_content(&source, content);
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].relative_path, "./api");
    }

    #[test]
    fn skips_empty_module_line() {
        let root = test_root();
        let content = "go 1.21\n\nuse ./empty-mod\n";
        std::fs::create_dir_all(root.join("empty-mod")).unwrap();
        // empty go.mod with no module line
        std::fs::write(root.join("empty-mod/go.mod"), "go 1.20\n").unwrap();

        let source = FilesystemWorkspaceSource::new(&root);
        let members = parse_go_work_content(&source, content);
        assert!(members.is_empty());
    }

    #[test]
    fn handles_missing_go_work_file() {
        let root = test_root();
        let source = FilesystemWorkspaceSource::new(&root);
        let result = detect_go_work(&source);
        assert!(result.is_none());
    }

    #[test]
    fn preserves_root_module_and_skips_dotdot() {
        let root = test_root();
        let content = "go 1.21\n\nuse (\n\t.\n\t..\n\t./pkg\n)\n";
        tmpdir_with_gomod(&root, "", "example.com/root");
        tmpdir_with_gomod(&root, "pkg", "example.com/pkg");

        let source = FilesystemWorkspaceSource::new(&root);
        let members = parse_go_work_content(&source, content);
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].package_name, "example.com/root");
        assert_eq!(members[0].relative_path, ".");
        assert_eq!(members[1].package_name, "example.com/pkg");
        assert_eq!(members[1].relative_path, "./pkg");
    }

    #[test]
    fn handles_comments_in_use_block() {
        let root = test_root();
        let content = "go 1.21\n\nuse (\n\t./api // backend\n\t./core /* shared */\n)\n";
        tmpdir_with_gomod(&root, "api", "example.com/api");
        tmpdir_with_gomod(&root, "core", "example.com/core");

        let source = FilesystemWorkspaceSource::new(&root);
        let members = parse_go_work_content(&source, content);
        assert_eq!(members.len(), 2);
    }

    #[test]
    fn strips_inline_comments_from_go_mod_module_name() {
        let root = test_root();
        let content = "go 1.21\n\nuse ./api\n";
        let api = root.join("api");
        std::fs::create_dir_all(&api).unwrap();
        std::fs::write(
            api.join("go.mod"),
            "module example.com/svc/api // API module\n\ngo 1.21\n",
        )
        .unwrap();

        let source = FilesystemWorkspaceSource::new(&root);
        let members = parse_go_work_content(&source, content);

        assert_eq!(members.len(), 1);
        assert_eq!(members[0].package_name, "example.com/svc/api");
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    /// Creates a uniquely-named temp root directory for test isolation.
    fn test_root() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static CT: AtomicU64 = AtomicU64::new(0);
        let id = CT.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("rk-gowork-{}-{}", std::process::id(), id));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn tmpdir_with_gomod(root: &Path, dir: impl AsRef<Path>, module: &str) {
        let path = root.join(dir.as_ref());
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("go.mod"), format!("module {module}\n\ngo 1.21\n")).unwrap();
    }
}
