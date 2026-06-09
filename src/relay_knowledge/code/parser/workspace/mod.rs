//! Monorepo / workspace-aware package member detection.
//!
//! Scans the repository root for well-known workspace manifests and extracts
//! package member declarations so that later cross-repository import
//! resolution can link imports to their correct target packages.

use std::collections::BTreeSet;
#[cfg(test)]
use std::fs;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use crate::domain::{
    CodeMonorepoWorkspace, CodeMonorepoWorkspaceFormat, CodeWorkspaceDetectionConfig,
};

mod cargo_workspace;
mod go_work;
mod pnpm_workspace;

pub(in crate::code) trait WorkspaceSource {
    fn root_path(&self) -> &Path;
    fn read_to_string(&self, relative_path: &str) -> Option<String>;
    fn child_dirs(&self, relative_dir: &str) -> Vec<String>;
    fn descendant_dirs_containing_file(
        &self,
        relative_dir: &str,
        file_name: &str,
        directory_limit: usize,
        entry_limit: usize,
    ) -> Vec<String> {
        bounded_descendant_dirs_containing_file(
            self,
            relative_dir,
            file_name,
            directory_limit,
            entry_limit,
        )
    }
}

fn bounded_descendant_dirs_containing_file<S: WorkspaceSource + ?Sized>(
    source: &S,
    relative_dir: &str,
    file_name: &str,
    directory_limit: usize,
    entry_limit: usize,
) -> Vec<String> {
    if directory_limit == 0 || entry_limit == 0 || file_name.trim().is_empty() {
        return Vec::new();
    }

    let mut result = BTreeSet::new();
    let mut queued = BTreeSet::new();
    let mut stack = Vec::new();
    let mut observed_entries = 0usize;
    queue_limited_child_dirs(
        source,
        relative_dir,
        directory_limit,
        entry_limit,
        &mut observed_entries,
        &mut queued,
        &mut stack,
    );

    let mut visited_dirs = 0usize;
    while let Some(dir) = stack.pop() {
        if visited_dirs >= directory_limit {
            break;
        }
        visited_dirs += 1;

        if source
            .read_to_string(&join_relative_path(&dir, file_name))
            .is_some()
            && result.insert(dir.clone())
            && result.len() >= directory_limit
        {
            break;
        }

        queue_limited_child_dirs(
            source,
            &dir,
            directory_limit,
            entry_limit,
            &mut observed_entries,
            &mut queued,
            &mut stack,
        );
    }

    result.into_iter().collect()
}

fn queue_limited_child_dirs<S: WorkspaceSource + ?Sized>(
    source: &S,
    relative_dir: &str,
    directory_limit: usize,
    entry_limit: usize,
    observed_entries: &mut usize,
    queued: &mut BTreeSet<String>,
    stack: &mut Vec<String>,
) {
    let remaining = directory_limit
        .saturating_sub(queued.len())
        .min(entry_limit.saturating_sub(*observed_entries));
    if remaining == 0 {
        return;
    }

    for child in source.child_dirs(relative_dir).into_iter().take(remaining) {
        *observed_entries += 1;
        if queued.insert(child.clone()) {
            stack.push(child);
        }
    }
}

#[cfg(test)]
pub(in crate::code) struct FilesystemWorkspaceSource<'a> {
    root_path: &'a Path,
}

#[cfg(test)]
impl<'a> FilesystemWorkspaceSource<'a> {
    pub(in crate::code) fn new(root_path: &'a Path) -> Self {
        Self { root_path }
    }
}

#[cfg(test)]
impl WorkspaceSource for FilesystemWorkspaceSource<'_> {
    fn root_path(&self) -> &Path {
        self.root_path
    }

    fn read_to_string(&self, relative_path: &str) -> Option<String> {
        fs::read_to_string(self.root_path.join(relative_path)).ok()
    }

    fn child_dirs(&self, relative_dir: &str) -> Vec<String> {
        let parent = self.root_path.join(relative_dir);
        let Ok(entries) = fs::read_dir(parent) else {
            return Vec::new();
        };
        let mut dirs = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                entry
                    .file_type()
                    .ok()
                    .filter(|file_type| file_type.is_dir())
                    .map(|_| entry.file_name())
            })
            .map(|name| join_relative_path(relative_dir, &PathBuf::from(name).to_string_lossy()))
            .collect::<Vec<_>>();
        dirs.sort();
        dirs
    }
}

/// Detects monorepo workspaces at `root_path` for every format listed in
/// `config.supported_formats`, reading only the well-known manifest files.
///
/// Returns an empty `Vec` when detection is disabled, the root cannot be
/// read, or no recognised workspace manifests are found.  Every returned
/// workspace is guaranteed to contain at least one member; callers should
/// filter out invalid workspaces if they require the two-member minimum
/// enforced by [`CodeMonorepoWorkspace::validate`].
#[cfg(test)]
fn detect_workspaces(
    root_path: &Path,
    config: &CodeWorkspaceDetectionConfig,
) -> Vec<CodeMonorepoWorkspace> {
    let source = FilesystemWorkspaceSource::new(root_path);
    detect_workspaces_from_source(&source, config)
}

pub(in crate::code) fn detect_workspaces_from_source(
    source: &dyn WorkspaceSource,
    config: &CodeWorkspaceDetectionConfig,
) -> Vec<CodeMonorepoWorkspace> {
    if !config.enabled {
        return Vec::new();
    }
    let mut workspaces = Vec::new();
    for format in &config.supported_formats {
        match *format {
            CodeMonorepoWorkspaceFormat::Pnpm => {
                if let Some(members) = pnpm_workspace::detect_pnpm_workspace(source) {
                    workspaces.push(CodeMonorepoWorkspace {
                        format: CodeMonorepoWorkspaceFormat::Pnpm,
                        root_path: source.root_path().display().to_string(),
                        workspace_file_path: source
                            .root_path()
                            .join("pnpm-workspace.yaml")
                            .display()
                            .to_string(),
                        members,
                    });
                }
            }
            CodeMonorepoWorkspaceFormat::GoModules => {
                if let Some(members) = go_work::detect_go_work(source) {
                    workspaces.push(CodeMonorepoWorkspace {
                        format: CodeMonorepoWorkspaceFormat::GoModules,
                        root_path: source.root_path().display().to_string(),
                        workspace_file_path: source
                            .root_path()
                            .join("go.work")
                            .display()
                            .to_string(),
                        members,
                    });
                }
            }
            CodeMonorepoWorkspaceFormat::CargoWorkspace => {
                if let Some(members) = cargo_workspace::detect_cargo_workspace(source) {
                    workspaces.push(CodeMonorepoWorkspace {
                        format: CodeMonorepoWorkspaceFormat::CargoWorkspace,
                        root_path: source.root_path().display().to_string(),
                        workspace_file_path: source
                            .root_path()
                            .join("Cargo.toml")
                            .display()
                            .to_string(),
                        members,
                    });
                }
            }
        }
    }
    workspaces
}

pub(super) fn join_relative_path(parent: &str, child: &str) -> String {
    let parent = parent.trim().trim_matches('/');
    let child = child.trim().trim_matches('/');
    if parent.is_empty() || parent == "." {
        child.to_owned()
    } else if child.is_empty() {
        parent.to_owned()
    } else {
        format!("{parent}/{child}")
    }
}

#[cfg(test)]
mod tests {
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
}
