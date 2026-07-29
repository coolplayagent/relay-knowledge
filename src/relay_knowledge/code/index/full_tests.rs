use std::{
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::domain::CodeMonorepoWorkspaceFormat;

use super::*;

#[test]
fn workspace_detection_reads_manifests_from_indexed_ref() {
    let repo = TestRepo::create("workspace-indexed-ref");
    repo.write("pnpm-workspace.yaml", "packages:\n  - 'packages/*'\n");
    repo.write(
        "packages/core/package.json",
        "{\n  \"name\": \"@scope/core\",\n  \"version\": \"1.0.0\"\n}\n",
    );
    repo.write(
        "packages/core/src/index.ts",
        "export const core = 'committed';\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    repo.write(
        "packages/core/package.json",
        "{\n  \"name\": \"@scope/worktree-core\",\n  \"version\": \"1.0.0\"\n}\n",
    );

    let registration = CodeRepositoryRegistration::new(
        "repo",
        "fixture",
        repo.path.to_string_lossy().into_owned(),
        Vec::new(),
        Vec::new(),
    )
    .expect("registration");
    let selector =
        CodeRepositorySelector::new("fixture", "HEAD", Vec::new(), Vec::new()).expect("ref");

    let snapshot = build_full_snapshot(
        &registration,
        &selector,
        &repo.path,
        &CodeWorkspaceDetectionConfig::enabled_all(),
    )
    .expect("snapshot");

    let workspace = snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.format == CodeMonorepoWorkspaceFormat::Pnpm)
        .expect("pnpm workspace");
    let package_names = workspace
        .members
        .iter()
        .map(|member| member.package_name.as_str())
        .collect::<Vec<_>>();

    assert!(package_names.contains(&"@scope/core"));
    assert!(!package_names.contains(&"@scope/worktree-core"));
}

#[test]
fn workspace_detection_reads_only_indexed_scope_entries() {
    let repo = TestRepo::create("workspace-indexed-scope");
    repo.write("pnpm-workspace.yaml", "packages:\n  - 'packages/*'\n");
    repo.write("packages/core/package.json", "{\"name\":\"@scope/core\"}\n");
    repo.write("packages/core/src/index.ts", "export const core = 1;\n");
    repo.write("src/app.ts", "export const app = 1;\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);

    let registration = CodeRepositoryRegistration::new(
        "repo",
        "fixture",
        repo.path.to_string_lossy().into_owned(),
        Vec::new(),
        Vec::new(),
    )
    .expect("registration");
    let selector =
        CodeRepositorySelector::new("fixture", "HEAD", vec!["src".to_owned()], Vec::new())
            .expect("ref");

    let snapshot = build_full_snapshot(
        &registration,
        &selector,
        &repo.path,
        &CodeWorkspaceDetectionConfig::enabled_all(),
    )
    .expect("snapshot");

    assert!(snapshot.workspaces.is_empty());
}

#[test]
fn workspace_detection_reads_manifests_for_language_filtered_scope() {
    let repo = TestRepo::create("workspace-language-scope");
    repo.write("pnpm-workspace.yaml", "packages:\n  - 'packages/*'\n");
    repo.write("packages/core/package.json", "{\"name\":\"@scope/core\"}\n");
    repo.write("packages/core/src/index.ts", "export const core = 1;\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);

    let registration = CodeRepositoryRegistration::new(
        "repo",
        "fixture",
        repo.path.to_string_lossy().into_owned(),
        Vec::new(),
        Vec::new(),
    )
    .expect("registration");
    let selector =
        CodeRepositorySelector::new("fixture", "HEAD", Vec::new(), vec!["typescript".to_owned()])
            .expect("ref");

    let snapshot = build_full_snapshot(
        &registration,
        &selector,
        &repo.path,
        &CodeWorkspaceDetectionConfig::enabled_all(),
    )
    .expect("snapshot");

    let package_names = snapshot
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.members.iter())
        .map(|member| member.package_name.as_str())
        .collect::<Vec<_>>();

    assert!(package_names.contains(&"@scope/core"));
}

struct TestRepo {
    path: PathBuf,
}

impl TestRepo {
    fn create(name: &str) -> Self {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let path = std::env::temp_dir().join(format!("rk-{name}-{millis}"));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create repo dir");
        let repo = Self { path };
        repo.git(["init"]);
        repo.git(["config", "user.email", "codex@example.invalid"]);
        repo.git(["config", "user.name", "Codex"]);
        repo
    }

    fn write(&self, relative_path: &str, content: &str) {
        let path = self.path.join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(path, content).expect("write fixture file");
    }

    fn git<const N: usize>(&self, args: [&str; N]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.path)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
