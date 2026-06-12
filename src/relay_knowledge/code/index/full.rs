use std::{collections::BTreeMap, path::Path};

#[cfg(test)]
use std::{fs, path::PathBuf, sync::Mutex};

use crate::domain::{
    CodeIndexSnapshot, CodeRepositoryRegistration, CodeRepositorySelector,
    CodeWorkspaceDetectionConfig,
};

use super::{
    CodeIndexError,
    ids::stable_hash64,
    parser::parse_indexed_file,
    scope::{ScopedSourceSnapshot, scoped_source_snapshot},
    snapshot::{SnapshotBuild, SnapshotScopeFilters},
    source::{
        ensure_filesystem_blobs_match_content_hashes, filesystem_content_hashes_for_paths,
        filesystem_tree_hash_from_path_hashes, source_snapshot_bytes,
    },
};

struct PersistedSnapshotIdentity {
    resolved_commit_sha: String,
    tree_hash: String,
    base_resolved_commit_sha: Option<String>,
}

#[cfg(test)]
struct FullSnapshotReadMutation {
    root: PathBuf,
    path: String,
    content: Vec<u8>,
}

#[cfg(test)]
static FULL_SNAPSHOT_READ_MUTATION: Mutex<Option<FullSnapshotReadMutation>> = Mutex::new(None);

#[cfg(test)]
pub(crate) fn mutate_next_filesystem_full_snapshot_read(root: PathBuf, path: &str, content: &[u8]) {
    *FULL_SNAPSHOT_READ_MUTATION
        .lock()
        .expect("full snapshot mutation should lock") = Some(FullSnapshotReadMutation {
        root,
        path: path.to_owned(),
        content: content.to_vec(),
    });
}

pub(super) fn build_full_snapshot(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
    workspace_detection: &CodeWorkspaceDetectionConfig,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    let snapshot = scoped_source_snapshot(registration, selector, root, &selector.ref_selector)?;
    let identity = PersistedSnapshotIdentity {
        resolved_commit_sha: snapshot.resolved_commit_sha.clone(),
        tree_hash: snapshot.tree_hash.clone(),
        base_resolved_commit_sha: None,
    };
    build_full_snapshot_from_scoped_source(registration, snapshot, identity, workspace_detection)
}

fn build_full_snapshot_with_identity(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
    source_ref_selector: &str,
    identity: PersistedSnapshotIdentity,
    workspace_detection: &CodeWorkspaceDetectionConfig,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    let snapshot = scoped_source_snapshot(registration, selector, root, source_ref_selector)?;
    build_full_snapshot_from_scoped_source(registration, snapshot, identity, workspace_detection)
}

pub(super) fn build_full_snapshot_as_worktree_overlay(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
    source_ref_selector: &str,
    base_commit: &str,
    workspace_detection: &CodeWorkspaceDetectionConfig,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    let overlay_hash = clean_worktree_overlay_hash(base_commit);
    let identity = PersistedSnapshotIdentity {
        resolved_commit_sha: format!("worktree:{base_commit}:{overlay_hash}"),
        tree_hash: format!("worktree:{overlay_hash}"),
        base_resolved_commit_sha: Some(base_commit.to_owned()),
    };
    build_full_snapshot_with_identity(
        registration,
        selector,
        root,
        source_ref_selector,
        identity,
        workspace_detection,
    )
}

pub(crate) fn clean_worktree_overlay_hash(base_commit: &str) -> String {
    let mut input = Vec::new();
    append_hash_part(&mut input, "clean-worktree-overlay");
    append_hash_part(&mut input, base_commit);
    format!("{:016x}", stable_hash64(&input))
}

fn append_hash_part(input: &mut Vec<u8>, value: &str) {
    input.extend_from_slice(&(value.len() as u64).to_le_bytes());
    input.extend_from_slice(value.as_bytes());
}

fn build_full_snapshot_from_scoped_source(
    registration: &CodeRepositoryRegistration,
    snapshot: ScopedSourceSnapshot,
    identity: PersistedSnapshotIdentity,
    workspace_detection: &CodeWorkspaceDetectionConfig,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    let filesystem_path_hashes = filesystem_full_snapshot_path_hashes(&snapshot)?;
    let source_commit = snapshot.resolved_commit_sha.clone();
    let mut build = SnapshotBuild::new_with_scope_filters(
        registration,
        identity.resolved_commit_sha,
        identity.tree_hash,
        SnapshotScopeFilters {
            path_filters: snapshot.path_filters.clone(),
            language_filters: snapshot.language_filters.clone(),
        },
        true,
        snapshot.entries.len(),
        0,
    );
    build.base_resolved_commit_sha = identity.base_resolved_commit_sha;

    #[cfg(test)]
    apply_filesystem_full_snapshot_read_mutation(&snapshot)?;

    build.detect_and_fill_workspaces_at_commit(
        &snapshot.root,
        snapshot.kind,
        &source_commit,
        &snapshot.entries,
        workspace_detection,
    );

    for entry in snapshot.entries {
        let bytes =
            source_snapshot_bytes(&snapshot.root, snapshot.kind, &source_commit, &entry.path)?;
        ensure_filesystem_blobs_match_content_hashes(
            &source_commit,
            std::slice::from_ref(&entry.path),
            std::slice::from_ref(&bytes),
            &filesystem_path_hashes,
        )?;
        parse_indexed_file(&mut build, &entry.path, &bytes)?;
    }

    Ok(build.finish())
}

fn filesystem_full_snapshot_path_hashes(
    snapshot: &ScopedSourceSnapshot,
) -> Result<BTreeMap<String, String>, CodeIndexError> {
    if !snapshot.kind.is_filesystem() {
        return Ok(BTreeMap::new());
    }
    let paths = snapshot
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    let path_hashes = filesystem_content_hashes_for_paths(&snapshot.root, &paths)?;
    let tree_hash = filesystem_tree_hash_from_path_hashes(&path_hashes);
    if tree_hash != snapshot.tree_hash {
        return Err(CodeIndexError::InvalidInput(format!(
            "filesystem source snapshot {} no longer matches planned filesystem content {tree_hash}",
            snapshot.resolved_commit_sha
        )));
    }

    Ok(path_hashes)
}

#[cfg(test)]
fn apply_filesystem_full_snapshot_read_mutation(
    snapshot: &ScopedSourceSnapshot,
) -> Result<(), CodeIndexError> {
    if !snapshot.kind.is_filesystem() {
        return Ok(());
    }
    let mut mutation = FULL_SNAPSHOT_READ_MUTATION
        .lock()
        .expect("full snapshot mutation should lock");
    let Some(next) = mutation.take() else {
        return Ok(());
    };
    if next.root != snapshot.root {
        *mutation = Some(next);
        return Ok(());
    }
    fs::write(snapshot.root.join(next.path), next.content).map_err(CodeIndexError::Io)
}

#[cfg(test)]
mod tests {
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
        let selector = CodeRepositorySelector::new(
            "fixture",
            "HEAD",
            Vec::new(),
            vec!["typescript".to_owned()],
        )
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
}
