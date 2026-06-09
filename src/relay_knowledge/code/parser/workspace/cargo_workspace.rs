//! Detection of Cargo (Rust) workspace members via `Cargo.toml`.

use std::collections::BTreeSet;

#[cfg(test)]
use std::path::PathBuf;

use crate::domain::CodeWorkspaceMember;

use super::{WorkspaceSource, join_relative_path};

/// Known Cargo manifest file name.
const CARGO_TOML: &str = "Cargo.toml";

/// Tries to read the root `Cargo.toml` and extract `[workspace].members`.
///
/// For each member directory, reads that crate's own `Cargo.toml` to
/// extract the `[package].name`.  Members that cannot be resolved to a
/// valid crate are silently skipped.
///
/// Returns `None` when the root `Cargo.toml` does not exist, cannot be
/// read, or contains no `[workspace].members` declaration.
pub(super) fn detect_cargo_workspace(
    source: &dyn WorkspaceSource,
) -> Option<Vec<CodeWorkspaceMember>> {
    let content = source.read_to_string(CARGO_TOML)?;
    let members = parse_cargo_workspace_content(source, &content);
    if members.is_empty() {
        None
    } else {
        Some(members)
    }
}

fn parse_cargo_workspace_content(
    source: &dyn WorkspaceSource,
    content: &str,
) -> Vec<CodeWorkspaceMember> {
    let member_dirs = parse_workspace_members(content);
    let excluded_dirs = parse_workspace_excludes(content)
        .into_iter()
        .flat_map(|exclude| expand_member_pattern(source, &exclude))
        .collect::<BTreeSet<_>>();
    let mut result = Vec::new();

    let root_package_name = read_cargo_import_crate_name(source, "");
    if !root_package_name.is_empty() && !excluded_dirs.contains(".") {
        result.push(CodeWorkspaceMember {
            package_name: root_package_name,
            relative_path: ".".to_owned(),
        });
    }

    for relative_path in member_dirs
        .into_iter()
        .flat_map(|member| expand_member_pattern(source, &member))
    {
        if relative_path.is_empty() || relative_path == "." || relative_path == ".." {
            continue;
        }
        if excluded_dirs.contains(&relative_path) {
            continue;
        }

        let package_name = read_cargo_import_crate_name(source, &relative_path);
        if package_name.is_empty() {
            continue;
        }

        result.push(CodeWorkspaceMember {
            package_name,
            relative_path,
        });
    }

    result
}

fn expand_member_pattern(source: &dyn WorkspaceSource, pattern: &str) -> Vec<String> {
    let pattern = normalize_member_pattern(pattern);
    if pattern.ends_with("/*") {
        let parent = pattern.trim_end_matches("/*");
        return source
            .child_dirs(parent)
            .into_iter()
            .filter(|dir| !read_cargo_import_crate_name(source, dir).is_empty())
            .collect();
    }
    vec![pattern]
}

fn normalize_member_pattern(pattern: &str) -> String {
    pattern
        .trim()
        .strip_prefix("./")
        .unwrap_or_else(|| pattern.trim())
        .trim_matches('/')
        .to_owned()
}

fn parse_workspace_members(content: &str) -> Vec<String> {
    parse_workspace_array(content, "members")
}

fn parse_workspace_excludes(content: &str) -> Vec<String> {
    parse_workspace_array(content, "exclude")
}

/// Extracts a string array key from a `[workspace]` section in TOML.
///
/// Handles both inline (`members = ["a", "b"]`) and multi-line
/// (`members = [\n  "a",\n  "b",\n]`) formats.
fn parse_workspace_array(content: &str, key: &str) -> Vec<String> {
    let mut in_workspace_section = false;
    let mut items = Vec::new();
    let mut inline_buffer = String::new();
    let mut collecting_array = false;

    for line in content.lines() {
        let trimmed = strip_toml_comment(line).trim().to_string();

        if trimmed.is_empty() {
            continue;
        }

        // Detect `[workspace]` section header
        if trimmed == "[workspace]" {
            in_workspace_section = true;
            continue;
        }

        // Section ended
        if trimmed.starts_with('[') && trimmed.ends_with(']') && in_workspace_section {
            break;
        }

        if !in_workspace_section {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix(key) {
            if let Some(after_eq) = rest.trim().strip_prefix('=') {
                let array_str = after_eq.trim();
                if let Some(inner) = array_str.strip_prefix('[') {
                    // Could be inline or multi-line opening
                    if array_str.ends_with(']') && !inner.is_empty() {
                        items.extend(parse_toml_array_items(&array_str[1..array_str.len() - 1]));
                        break;
                    } else {
                        // Multi-line opening: collect until `]`
                        inline_buffer.push_str(inner);
                        inline_buffer.push('\n');
                        collecting_array = true;
                    }
                }
            }
            continue;
        }

        // Accumulate multi-line array content
        if collecting_array {
            let ends_array = trimmed.ends_with(']');
            let item_line = trimmed.trim_end_matches(']').trim();
            if !item_line.is_empty() {
                inline_buffer.push_str(item_line);
                inline_buffer.push('\n');
            }
            if ends_array {
                items.extend(parse_toml_array_items(&inline_buffer));
                inline_buffer.clear();
                break;
            }
        }
    }

    // If we hit EOF with a pending buffer
    if !inline_buffer.is_empty() {
        items.extend(parse_toml_array_items(&inline_buffer));
    }

    items
}

/// Parses comma-separated, possibly-quoted array items from TOML.
fn parse_toml_array_items(content: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut quote_char: Option<char> = None;

    for ch in content.chars() {
        match ch {
            '"' | '\'' => {
                if in_quote && quote_char == Some(ch) {
                    in_quote = false;
                    quote_char = None;
                } else if !in_quote {
                    in_quote = true;
                    quote_char = Some(ch);
                } else {
                    current.push(ch);
                }
            }
            ',' if !in_quote => {
                let item = current.trim().to_string();
                if !item.is_empty() {
                    items.push(item);
                }
                current.clear();
            }
            '\n' | '\r' => {
                // Newlines between items are whitespace, not terminators
                if in_quote {
                    current.push(' ');
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }

    // Last item
    let item = current.trim().to_string();
    if !item.is_empty() {
        items.push(item);
    }

    items
}

/// Reads the `[package].name` from a crate's own `Cargo.toml`.
#[cfg(test)]
fn read_cargo_package_name(source: &dyn WorkspaceSource, dir: &str) -> String {
    let Some(content) = source.read_to_string(&join_relative_path(dir, CARGO_TOML)) else {
        return String::new();
    };
    cargo_manifest_section_name(&content, "package").unwrap_or_default()
}

fn read_cargo_import_crate_name(source: &dyn WorkspaceSource, dir: &str) -> String {
    let Some(content) = source.read_to_string(&join_relative_path(dir, CARGO_TOML)) else {
        return String::new();
    };
    if let Some(lib_name) = cargo_manifest_section_name(&content, "lib") {
        return lib_name;
    }
    cargo_manifest_section_name(&content, "package")
        .map(|package_name| cargo_import_crate_name(&package_name))
        .unwrap_or_default()
}

fn cargo_manifest_section_name(content: &str, section: &str) -> Option<String> {
    let mut in_section = false;

    for line in content.lines() {
        let trimmed = strip_toml_comment(line).trim().to_string();

        if trimmed.is_empty() {
            continue;
        }

        if trimmed == format!("[{section}]") {
            in_section = true;
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') && in_section {
            break;
        }

        if !in_section {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("name") {
            if let Some(after_eq) = rest.trim().strip_prefix('=') {
                let name = after_eq.trim();
                let name = name.trim_matches('"').trim_matches('\'');
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
    }

    None
}

fn cargo_import_crate_name(package_name: &str) -> String {
    package_name.replace('-', "_")
}

/// Strips a TOML comment (`# ...`) from the end of `line`.
fn strip_toml_comment(line: &str) -> &str {
    let mut in_quote = false;
    let mut quote_char: Option<char> = None;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' | '\'' => {
                if in_quote && quote_char == Some(ch) {
                    in_quote = false;
                    quote_char = None;
                } else if !in_quote {
                    in_quote = true;
                    quote_char = Some(ch);
                }
            }
            '#' if !in_quote => return &line[..i],
            _ => {}
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use crate::code::parser::workspace::FilesystemWorkspaceSource;

    use super::*;

    // ── parse_workspace_members ─────────────────────────────────────────

    #[test]
    fn parses_inline_members() {
        let content =
            "[package]\nname = \"root\"\n\n[workspace]\nmembers = [\"crate-a\", \"crate-b\"]\n";
        let members = parse_workspace_members(content);
        assert_eq!(members, vec!["crate-a", "crate-b"]);
    }

    #[test]
    fn parses_multiline_members() {
        let content = "\
[workspace]\n\
members = [\n  \"crate-a\",\n  \"crate-b\",\n  \"libs/shared\",\n]\n\
[dependencies]\n";
        let members = parse_workspace_members(content);
        assert_eq!(members, vec!["crate-a", "crate-b", "libs/shared"]);
    }

    #[test]
    fn returns_empty_when_no_workspace_section() {
        let content = "[package]\nname = \"standalone\"\n[dependencies]\nserde = \"1\"\n";
        let members = parse_workspace_members(content);
        assert!(members.is_empty());
    }

    #[test]
    fn stops_at_next_section() {
        let content = "\
[workspace]\nmembers = [\"a\"]\n\n[profile.release]\nopt-level = 3\n\
[workspace.metadata]\nmembers = [\"ignored\"]\n";
        let members = parse_workspace_members(content);
        // `[workspace.metadata]` looks like a different section to the simple parser
        // but [profile.release] ends the workspace section.
        assert_eq!(members, vec!["a"]);
    }

    #[test]
    fn handles_single_member() {
        let content = "[workspace]\nmembers = [\"only-crate\"]\n";
        let members = parse_workspace_members(content);
        assert_eq!(members, vec!["only-crate"]);
    }

    #[test]
    fn handles_glob_members() {
        // Cargo supports glob patterns like `crates/*`
        let content = "[workspace]\nmembers = [\"crates/*\"]\n";
        let members = parse_workspace_members(content);
        assert_eq!(members, vec!["crates/*"]);
    }

    #[test]
    fn parses_workspace_excludes() {
        let content = "\
[workspace]\n\
members = [\"crates/*\"]\n\
exclude = [\n  \"crates/experimental\",\n]\n";
        let excludes = parse_workspace_excludes(content);
        assert_eq!(excludes, vec!["crates/experimental"]);
    }

    // ── read_cargo_package_name ────────────────────────────────────────

    #[test]
    fn reads_package_name() {
        let tmp = tmpdir_with_cargo_toml("my-crate", "my-crate-name");
        let source = FilesystemWorkspaceSource::new(tmp.parent().expect("parent"));
        let name = read_cargo_package_name(&source, "my-crate");
        assert_eq!(name, "my-crate-name");
    }

    #[test]
    fn returns_empty_for_missing_toml() {
        let tmp = std::env::temp_dir().join("rk-cargo-no-toml");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let source = FilesystemWorkspaceSource::new(&tmp);
        let name = read_cargo_package_name(&source, "");
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(name.is_empty());
    }

    #[test]
    fn returns_empty_when_no_package_section() {
        let tmp = std::env::temp_dir().join("rk-cargo-no-pkg");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        let source = FilesystemWorkspaceSource::new(&tmp);
        let name = read_cargo_package_name(&source, "");
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(name.is_empty());
    }

    // ── Full pipeline test ─────────────────────────────────────────────

    #[test]
    fn detects_cargo_workspace_with_multiple_crates() {
        let tmp = std::env::temp_dir().join("rk-cargo-ws-full");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // Root Cargo.toml with [workspace]
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crate-a\", \"crate-b\"]\n",
        )
        .unwrap();

        // Member crate-a
        std::fs::create_dir_all(tmp.join("crate-a")).unwrap();
        std::fs::write(
            tmp.join("crate-a/Cargo.toml"),
            "[package]\nname = \"my-crate-a\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        // Member crate-b
        std::fs::create_dir_all(tmp.join("crate-b")).unwrap();
        std::fs::write(
            tmp.join("crate-b/Cargo.toml"),
            "[package]\nname = \"my-crate-b\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let source = FilesystemWorkspaceSource::new(&tmp);
        let result = detect_cargo_workspace(&source).expect("should detect workspace");
        let _ = std::fs::remove_dir_all(&tmp);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].package_name, "my_crate_a");
        assert_eq!(result[0].relative_path, "crate-a");
        assert_eq!(result[1].package_name, "my_crate_b");
        assert_eq!(result[1].relative_path, "crate-b");
    }

    #[test]
    fn expands_glob_members_to_concrete_crates() {
        let tmp = std::env::temp_dir().join("rk-cargo-ws-glob");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.join("crates/core")).unwrap();
        std::fs::write(
            tmp.join("crates/core/Cargo.toml"),
            "[package]\nname = \"core-crate\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.join("crates/ui")).unwrap();
        std::fs::write(
            tmp.join("crates/ui/Cargo.toml"),
            "[package]\nname = \"ui-crate\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.join("crates/empty")).unwrap();

        let source = FilesystemWorkspaceSource::new(&tmp);
        let result = detect_cargo_workspace(&source).expect("should detect workspace");
        let _ = std::fs::remove_dir_all(&tmp);

        let names = result
            .iter()
            .map(|member| member.package_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["core_crate", "ui_crate"]);
    }

    #[test]
    fn excludes_workspace_members_after_glob_expansion() {
        let tmp = std::env::temp_dir().join("rk-cargo-ws-exclude");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"crates/experimental\"]\n",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.join("crates/core")).unwrap();
        std::fs::write(
            tmp.join("crates/core/Cargo.toml"),
            "[package]\nname = \"core-crate\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.join("crates/experimental")).unwrap();
        std::fs::write(
            tmp.join("crates/experimental/Cargo.toml"),
            "[package]\nname = \"experimental-crate\"\n",
        )
        .unwrap();

        let source = FilesystemWorkspaceSource::new(&tmp);
        let result = detect_cargo_workspace(&source).expect("should detect workspace");
        let _ = std::fs::remove_dir_all(&tmp);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].package_name, "core_crate");
        assert_eq!(result[0].relative_path, "crates/core");
    }

    #[test]
    fn includes_root_package_as_implicit_workspace_member() {
        let tmp = std::env::temp_dir().join("rk-cargo-ws-root-package");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[package]\nname = \"root-crate\"\n\n[workspace]\nmembers = [\"crates/core\"]\n",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.join("crates/core")).unwrap();
        std::fs::write(
            tmp.join("crates/core/Cargo.toml"),
            "[package]\nname = \"core-crate\"\n",
        )
        .unwrap();

        let source = FilesystemWorkspaceSource::new(&tmp);
        let result = detect_cargo_workspace(&source).expect("should detect workspace");
        let _ = std::fs::remove_dir_all(&tmp);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].package_name, "root_crate");
        assert_eq!(result[0].relative_path, ".");
        assert_eq!(result[1].package_name, "core_crate");
    }

    #[test]
    fn prefers_library_name_for_import_crate_name() {
        let tmp = std::env::temp_dir().join("rk-cargo-ws-lib-name");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/foo\"]\n",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.join("crates/foo")).unwrap();
        std::fs::write(
            tmp.join("crates/foo/Cargo.toml"),
            "[package]\nname = \"foo-rs\"\n\n[lib]\nname = \"foo\"\n",
        )
        .unwrap();

        let source = FilesystemWorkspaceSource::new(&tmp);
        let result = detect_cargo_workspace(&source).expect("should detect workspace");
        let _ = std::fs::remove_dir_all(&tmp);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].package_name, "foo");
        assert_eq!(result[0].relative_path, "crates/foo");
    }

    #[test]
    fn skips_member_without_package_name() {
        let tmp = std::env::temp_dir().join("rk-cargo-skip");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        std::fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nmembers = [\"valid\", \"empty\"]\n",
        )
        .unwrap();

        std::fs::create_dir_all(tmp.join("valid")).unwrap();
        std::fs::write(
            tmp.join("valid/Cargo.toml"),
            "[package]\nname = \"valid-crate\"\n",
        )
        .unwrap();

        std::fs::create_dir_all(tmp.join("empty")).unwrap();
        // No Cargo.toml in empty/

        let source = FilesystemWorkspaceSource::new(&tmp);
        let result = detect_cargo_workspace(&source).expect("should detect workspace");
        let _ = std::fs::remove_dir_all(&tmp);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].package_name, "valid_crate");
    }

    #[test]
    fn stores_hyphenated_package_as_import_crate_name() {
        assert_eq!(cargo_import_crate_name("my-crate"), "my_crate");
        assert_eq!(cargo_import_crate_name("serde"), "serde");
    }

    #[test]
    fn handles_missing_root_toml() {
        let tmp = std::env::temp_dir().join("rk-cargo-none");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let source = FilesystemWorkspaceSource::new(&tmp);
        let result = detect_cargo_workspace(&source);
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(result.is_none());
    }

    // ── TOML comment stripping ─────────────────────────────────────────

    #[test]
    fn strips_inline_toml_comments() {
        let content = "[workspace] # top\nmembers = [\"a\"] # comment\n";
        let members = parse_workspace_members(content);
        assert_eq!(members, vec!["a"]);
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    fn tmpdir_with_cargo_toml(dir_name: &str, package_name: &str) -> PathBuf {
        let path = std::env::temp_dir().join("rk-cargo-test").join(dir_name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("Cargo.toml"),
            format!("[package]\nname = \"{package_name}\"\n"),
        )
        .unwrap();
        path
    }
}
