use super::*;
use std::{fs, path::PathBuf};

use std::sync::atomic::{AtomicU64, Ordering};

fn test_root(prefix: &str) -> PathBuf {
    static CT: AtomicU64 = AtomicU64::new(0);
    let id = CT.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("rk-ws-{prefix}-{}-{id}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

// ── detect_workspaces with disabled config ─────────────────────────

#[test]
fn disabled_config_returns_empty() {
    let config = CodeWorkspaceDetectionConfig {
        enabled: false,
        supported_formats: vec![],
    };
    let result = detect_workspaces(Path::new("/nonexistent"), &config);
    assert!(result.is_empty());
}

// ── detect_workspaces with empty supported_formats ─────────────────

#[test]
fn enabled_but_empty_formats_returns_empty() {
    let config = CodeWorkspaceDetectionConfig {
        enabled: true,
        supported_formats: vec![],
    };
    let result = detect_workspaces(Path::new("/dev/null"), &config);
    assert!(result.is_empty());
}

// ── detect_workspaces for missing manifests ────────────────────────

#[test]
fn missing_manifests_return_empty() {
    let config = CodeWorkspaceDetectionConfig {
        enabled: true,
        supported_formats: vec![
            CodeMonorepoWorkspaceFormat::Pnpm,
            CodeMonorepoWorkspaceFormat::GoModules,
            CodeMonorepoWorkspaceFormat::CargoWorkspace,
        ],
    };
    let dir = test_root("missing");
    let result = detect_workspaces(&dir, &config);
    assert!(result.is_empty());
}

#[test]
fn workspace_format_order_and_duplicates_do_not_change_detected_facts() {
    let root = test_root("canonical-formats");
    fs::write(
        root.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/a'\n  - 'packages/b'\n",
    )
    .unwrap();
    for name in ["a", "b"] {
        fs::create_dir_all(root.join(format!("packages/{name}"))).unwrap();
        fs::write(
            root.join(format!("packages/{name}/package.json")),
            format!("{{\"name\":\"{name}\"}}"),
        )
        .unwrap();
    }
    let canonical = CodeWorkspaceDetectionConfig {
        enabled: true,
        supported_formats: vec![CodeMonorepoWorkspaceFormat::Pnpm],
    };
    let duplicated = CodeWorkspaceDetectionConfig {
        enabled: true,
        supported_formats: vec![
            CodeMonorepoWorkspaceFormat::Pnpm,
            CodeMonorepoWorkspaceFormat::Pnpm,
        ],
    };

    assert_eq!(
        detect_workspaces(&root, &canonical),
        detect_workspaces(&root, &duplicated)
    );
}

// ── Full pipeline: Pnpm workspace ──────────────────────────────────

#[test]
fn detects_pnpm_workspace_from_yaml() {
    let root = test_root("pnpm");
    fs::write(
        root.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/lib'\n  - 'apps/web'\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("packages/lib")).unwrap();
    fs::write(
        root.join("packages/lib/package.json"),
        "{\"name\": \"@scope/lib\"}\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("apps/web")).unwrap();
    fs::write(
        root.join("apps/web/package.json"),
        "{\"name\": \"@scope/webapp\"}\n",
    )
    .unwrap();

    let config = CodeWorkspaceDetectionConfig {
        enabled: true,
        supported_formats: vec![CodeMonorepoWorkspaceFormat::Pnpm],
    };
    let result = detect_workspaces(&root, &config);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].format, CodeMonorepoWorkspaceFormat::Pnpm);
    assert_eq!(result[0].members.len(), 2);
}

// ── Full pipeline: Go workspace ────────────────────────────────────

#[test]
fn detects_go_work_from_file() {
    let root = test_root("go");
    fs::write(
        root.join("go.work"),
        "go 1.21\n\nuse (\n\t./api\n\t./core\n)\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("api")).unwrap();
    fs::write(
        root.join("api/go.mod"),
        "module example.com/svc/api\n\ngo 1.21\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("core")).unwrap();
    fs::write(
        root.join("core/go.mod"),
        "module example.com/svc/core\n\ngo 1.21\n",
    )
    .unwrap();

    let config = CodeWorkspaceDetectionConfig {
        enabled: true,
        supported_formats: vec![CodeMonorepoWorkspaceFormat::GoModules],
    };
    let result = detect_workspaces(&root, &config);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].format, CodeMonorepoWorkspaceFormat::GoModules);
    assert_eq!(result[0].members.len(), 2);
}

// ── Full pipeline: Cargo workspace ─────────────────────────────────

#[test]
fn detects_cargo_workspace_from_toml() {
    let root = test_root("cargo");
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crate-a\", \"crate-b\"]\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("crate-a")).unwrap();
    fs::write(
        root.join("crate-a/Cargo.toml"),
        "[package]\nname = \"my-crate-a\"\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("crate-b")).unwrap();
    fs::write(
        root.join("crate-b/Cargo.toml"),
        "[package]\nname = \"my-crate-b\"\n",
    )
    .unwrap();

    let config = CodeWorkspaceDetectionConfig {
        enabled: true,
        supported_formats: vec![CodeMonorepoWorkspaceFormat::CargoWorkspace],
    };
    let result = detect_workspaces(&root, &config);
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].format,
        CodeMonorepoWorkspaceFormat::CargoWorkspace
    );
    assert_eq!(result[0].members.len(), 2);
}

// ── detect_workspaces: all formats together ────────────────────────

#[test]
fn detects_all_formats_when_all_enabled() {
    let root = test_root("all");

    // pnpm workspace
    fs::write(root.join("pnpm-workspace.yaml"), "packages:\n  - 'pkg'\n").unwrap();
    fs::create_dir_all(root.join("pkg")).unwrap();
    fs::write(root.join("pkg/package.json"), "{\"name\": \"pkg\"}\n").unwrap();

    // go.work
    fs::write(root.join("go.work"), "go 1.21\n\nuse ./svc\n").unwrap();
    fs::create_dir_all(root.join("svc")).unwrap();
    fs::write(
        root.join("svc/go.mod"),
        "module example.com/svc\n\ngo 1.21\n",
    )
    .unwrap();

    // Cargo.toml
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"lib\"]\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("lib")).unwrap();
    fs::write(root.join("lib/Cargo.toml"), "[package]\nname = \"lib\"\n").unwrap();

    let config = CodeWorkspaceDetectionConfig {
        enabled: true,
        supported_formats: vec![
            CodeMonorepoWorkspaceFormat::Pnpm,
            CodeMonorepoWorkspaceFormat::GoModules,
            CodeMonorepoWorkspaceFormat::CargoWorkspace,
        ],
    };
    let result = detect_workspaces(&root, &config);
    assert_eq!(result.len(), 3);
    let formats: Vec<CodeMonorepoWorkspaceFormat> = result.iter().map(|w| w.format).collect();
    assert!(formats.contains(&CodeMonorepoWorkspaceFormat::Pnpm));
    assert!(formats.contains(&CodeMonorepoWorkspaceFormat::GoModules));
    assert!(formats.contains(&CodeMonorepoWorkspaceFormat::CargoWorkspace));
}
