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
