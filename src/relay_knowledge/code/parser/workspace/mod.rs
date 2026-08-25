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
    for format in [
        CodeMonorepoWorkspaceFormat::Pnpm,
        CodeMonorepoWorkspaceFormat::GoModules,
        CodeMonorepoWorkspaceFormat::CargoWorkspace,
    ] {
        if !config.supported_formats.contains(&format) {
            continue;
        }
        match format {
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
#[path = "mod_tests.rs"]
mod tests;
