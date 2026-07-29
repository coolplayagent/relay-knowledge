use crate::code::parser::workspace::FilesystemWorkspaceSource;

use super::*;

// ── Content-level YAML parsing tests ───────────────────────────────

#[test]
fn parses_block_style_packages() {
    let content = "packages:\n  - 'packages/core'\n  - 'packages/utils'\n  - 'apps/web'\n";
    let patterns = parse_yaml_packages_list(content);
    assert_eq!(
        patterns,
        vec!["packages/core", "packages/utils", "apps/web"]
    );
}

#[test]
fn parses_inline_packages_array() {
    let content = "packages: ['packages/core', 'packages/utils']\n";
    let patterns = parse_yaml_packages_list(content);
    assert_eq!(patterns, vec!["packages/core", "packages/utils"]);
}

#[test]
fn skips_negated_patterns() {
    let root = test_root();
    let content = "packages:\n  - 'packages/*'\n  - '!packages/test-utils'\n  - 'apps/web'\n";
    tmpdir_with_pkg_json(&root, "packages/core", "@scope/core");
    tmpdir_with_pkg_json(&root, "packages/test-utils", "@scope/test-utils");
    tmpdir_with_pkg_json(&root, "apps/web", "@scope/webapp");

    let source = FilesystemWorkspaceSource::new(&root);
    let members = parse_pnpm_workspace_content(&source, content);
    assert_eq!(members.len(), 2, "should skip negated patterns");
    let names: Vec<_> = members.iter().map(|m| m.package_name.as_str()).collect();
    assert!(names.contains(&"@scope/core"));
    assert!(names.contains(&"@scope/webapp"));
}

#[test]
fn handles_quoted_and_unquoted_values() {
    let content = "packages:\n  - packages/core\n  - \"packages/utils\"\n  - 'apps/web'\n";
    let patterns = parse_yaml_packages_list(content);
    assert_eq!(
        patterns,
        vec!["packages/core", "packages/utils", "apps/web"]
    );
}

#[test]
fn ignores_non_packages_keys() {
    let content = "packages:\n  - 'pkg/a'\nsome_other_key: value\nignored:\n  - not-this\n";
    let patterns = parse_yaml_packages_list(content);
    assert_eq!(patterns, vec!["pkg/a"]);
}

#[test]
fn handles_missing_workspace_file() {
    let tmp = std::env::temp_dir().join("rk-pnpm-none");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let source = FilesystemWorkspaceSource::new(&tmp);
    let result = detect_pnpm_workspace(&source);
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(result.is_none());
}

// ── Package name resolution tests ──────────────────────────────────

#[test]
fn reads_package_json_name() {
    let root = test_root();
    tmpdir_with_pkg_json(&root, "mypkg", "@scope/mylib");
    let content = "packages:\n  - 'mypkg'\n";
    let source = FilesystemWorkspaceSource::new(&root);
    let members = parse_pnpm_workspace_content(&source, content);
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].package_name, "@scope/mylib");
    assert_eq!(members[0].relative_path, "mypkg");
}

#[test]
fn reads_package_json_name_with_trailing_comma() {
    let root = test_root();
    let path = root.join("packages/core");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("package.json"),
        "{\n  \"name\": \"@scope/core\",\n  \"version\": \"1.0.0\"\n}\n",
    )
    .unwrap();

    let source = FilesystemWorkspaceSource::new(&root);
    let members = parse_pnpm_workspace_content(&source, "packages:\n  - 'packages/core'\n");

    assert_eq!(members.len(), 1);
    assert_eq!(members[0].package_name, "@scope/core");
}

#[test]
fn includes_named_workspace_root_package() {
    let root = test_root();
    std::fs::write(
        root.join("package.json"),
        "{\n  \"name\": \"@scope/root\",\n  \"version\": \"1.0.0\"\n}\n",
    )
    .unwrap();
    tmpdir_with_pkg_json(&root, "packages/core", "@scope/core");

    let source = FilesystemWorkspaceSource::new(&root);
    let members = parse_pnpm_workspace_content(&source, "packages:\n  - 'packages/*'\n");

    assert_eq!(members.len(), 2);
    assert_eq!(members[0].package_name, "@scope/root");
    assert_eq!(members[0].relative_path, ".");
    assert_eq!(members[1].package_name, "@scope/core");
    assert_eq!(members[1].relative_path, "packages/core");
}

#[test]
fn fallback_to_dir_name_when_no_package_json() {
    let root = test_root();
    std::fs::create_dir_all(root.join("mydir")).unwrap();

    let source = FilesystemWorkspaceSource::new(&root);
    let content = "packages:\n  - 'mydir'\n";
    let members = parse_pnpm_workspace_content(&source, content);
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].package_name, "mydir");
}

// ── Glob pattern normalization ─────────────────────────────────────

#[test]
fn expands_package_globs_to_concrete_package_dirs() {
    let root = test_root();
    tmpdir_with_pkg_json(&root, "packages/core", "@scope/core");
    tmpdir_with_pkg_json(&root, "packages/ui", "@scope/ui");
    std::fs::create_dir_all(root.join("packages/empty")).unwrap();

    let source = FilesystemWorkspaceSource::new(&root);
    assert_eq!(
        expand_package_pattern(&source, "packages/*"),
        vec!["packages/core", "packages/ui"]
    );
    assert_eq!(
        normalize_workspace_path("./single-pkg"),
        "single-pkg".to_owned()
    );
}

#[test]
fn bounds_recursive_package_glob_expansion() {
    let root = test_root();
    for index in 0..PNPM_RECURSIVE_WORKSPACE_DIR_LIMIT + 2 {
        tmpdir_with_pkg_json(
            &root,
            format!("packages/pkg-{index:04}"),
            &format!("@scope/pkg-{index:04}"),
        );
    }

    let source = FilesystemWorkspaceSource::new(&root);
    let members = parse_pnpm_workspace_content(&source, "packages:\n  - 'packages/**'\n");

    assert_eq!(members.len(), PNPM_RECURSIVE_WORKSPACE_DIR_LIMIT);
}

// ── YAML comment stripping ─────────────────────────────────────────

#[test]
fn strips_inline_comments() {
    let content = "packages: # my packages\n  - 'pkg/a' # comment\n";
    let patterns = parse_yaml_packages_list(content);
    assert_eq!(patterns, vec!["pkg/a"]);
}

// ── Helpers ─────────────────────────────────────────────────────────

fn test_root() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CT: AtomicU64 = AtomicU64::new(0);
    let id = CT.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("rk-pnpm-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn tmpdir_with_pkg_json(root: &Path, dir: impl AsRef<Path>, name: &str) {
    let path = root.join(dir.as_ref());
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("package.json"),
        format!("{{\n  \"name\": \"{name}\"\n}}\n"),
    )
    .unwrap();
}
