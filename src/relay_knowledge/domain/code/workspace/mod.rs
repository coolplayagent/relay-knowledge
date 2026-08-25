//! Defines monorepo workspace discovery contracts and validation.

use serde::{Deserialize, Serialize};

use super::{DomainError, error::required_text};

/// Recognised monorepo workspace manifest formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeMonorepoWorkspaceFormat {
    /// pnpm workspace: `pnpm-workspace.yaml` or `package.json` with `workspaces` field.
    Pnpm,
    /// Go multi-module workspace: `go.work` or multiple `go.mod` files.
    GoModules,
    /// Rust workspace: `Cargo.toml` with a `[workspace]` section.
    CargoWorkspace,
}

/// A detected monorepo workspace that groups multiple packages under a common root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeMonorepoWorkspace {
    /// Format of the workspace manifest.
    pub format: CodeMonorepoWorkspaceFormat,
    /// Absolute path to the workspace root directory on the canonical host.
    pub root_path: String,
    /// Absolute path to the workspace definition file (e.g. `pnpm-workspace.yaml`, `go.work`).
    pub workspace_file_path: String,
    /// Packages discovered inside the workspace.
    pub members: Vec<CodeWorkspaceMember>,
}

impl CodeMonorepoWorkspace {
    /// Validates that the workspace contains at least two member packages
    /// and that every required text field is non-empty after trimming.
    pub fn validate(&self) -> Result<(), DomainError> {
        let _ = required_text("root_path", &self.root_path)?;
        let _ = required_text("workspace_file_path", &self.workspace_file_path)?;

        if self.members.len() < 2 {
            return Err(DomainError::invalid(
                "members",
                "monorepo workspace must contain at least 2 member packages",
            ));
        }

        for member in &self.members {
            let _ = required_text("member.package_name", &member.package_name)?;
            let _ = required_text("member.relative_path", &member.relative_path)?;
        }

        Ok(())
    }
}

/// A single package member inside a monorepo workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeWorkspaceMember {
    /// Canonical package name as declared in the manifest (e.g. `@scope/pkg`, `gosdk`).
    pub package_name: String,
    /// Relative path from the workspace root to this package directory.
    pub relative_path: String,
}

/// Maps a workspace member's package name to an indexed repository scope.
///
/// This is the bridge record that the cross-repo resolver uses to translate
/// an unresolved import module into a candidate source scope and repository
/// after workspace detection has grouped the packages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeWorkspacePackageMapping {
    /// Package name as discovered from the workspace manifest.
    pub package_name: String,
    /// Target ecosystem derived from the workspace format (e.g. `"go"`, `"rust"`, `"npm"`).
    pub ecosystem: String,
    /// Repository identifier the indexed scope belongs to.
    pub repository_id: String,
    /// Source scope the resolved target lives in (`git_snapshot:<hash>` with
    /// an optional canonical `workspace-v1:<mask>` semantic suffix).
    pub source_scope: String,
    /// Confidence in basis points (0–10 000) that this mapping is correct.
    pub confidence_basis_points: u32,
}

/// Configuration controlling automated monorepo workspace detection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeWorkspaceDetectionConfig {
    /// Whether automatic workspace detection is active.
    pub enabled: bool,
    /// Workspace manifest formats that the detector should look for.
    pub supported_formats: Vec<CodeMonorepoWorkspaceFormat>,
}

impl CodeWorkspaceDetectionConfig {
    /// Returns a disabled configuration that still records the supported
    /// formats to use when callers opt in.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            supported_formats: Self::all_supported_formats(),
        }
    }

    /// Enables workspace detection for every supported manifest format.
    pub fn enabled_all() -> Self {
        Self {
            enabled: true,
            supported_formats: Self::all_supported_formats(),
        }
    }

    fn all_supported_formats() -> Vec<CodeMonorepoWorkspaceFormat> {
        vec![
            CodeMonorepoWorkspaceFormat::Pnpm,
            CodeMonorepoWorkspaceFormat::GoModules,
            CodeMonorepoWorkspaceFormat::CargoWorkspace,
        ]
    }
}

impl Default for CodeWorkspaceDetectionConfig {
    fn default() -> Self {
        Self::disabled()
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
