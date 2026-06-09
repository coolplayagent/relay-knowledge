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
    /// Source scope the resolved target lives in (`git_snapshot:<hash>`).
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
mod tests {
    use super::*;

    // ── Construction helpers ──────────────────────────────────────────

    fn pnpm_workspace() -> CodeMonorepoWorkspace {
        CodeMonorepoWorkspace {
            format: CodeMonorepoWorkspaceFormat::Pnpm,
            root_path: "/repos/monorepo".to_owned(),
            workspace_file_path: "/repos/monorepo/pnpm-workspace.yaml".to_owned(),
            members: vec![
                CodeWorkspaceMember {
                    package_name: "@scope/core".to_owned(),
                    relative_path: "packages/core".to_owned(),
                },
                CodeWorkspaceMember {
                    package_name: "@scope/utils".to_owned(),
                    relative_path: "packages/utils".to_owned(),
                },
            ],
        }
    }

    fn go_workspace() -> CodeMonorepoWorkspace {
        CodeMonorepoWorkspace {
            format: CodeMonorepoWorkspaceFormat::GoModules,
            root_path: "/repos/go-svc".to_owned(),
            workspace_file_path: "/repos/go-svc/go.work".to_owned(),
            members: vec![
                CodeWorkspaceMember {
                    package_name: "example.com/svc/api".to_owned(),
                    relative_path: "api".to_owned(),
                },
                CodeWorkspaceMember {
                    package_name: "example.com/svc/core".to_owned(),
                    relative_path: "core".to_owned(),
                },
            ],
        }
    }

    // ── CodeMonorepoWorkspaceFormat serde round-trip ──────────────────

    #[test]
    fn workspace_format_serde_round_trip() {
        let cases = [
            (CodeMonorepoWorkspaceFormat::Pnpm, "\"pnpm\""),
            (CodeMonorepoWorkspaceFormat::GoModules, "\"go_modules\""),
            (
                CodeMonorepoWorkspaceFormat::CargoWorkspace,
                "\"cargo_workspace\"",
            ),
        ];

        for (format, expected_json) in cases {
            let json = serde_json::to_string(&format).expect("serialize format");
            assert_eq!(json, expected_json);

            let round_tripped: CodeMonorepoWorkspaceFormat =
                serde_json::from_str(&json).expect("deserialize format");
            assert_eq!(round_tripped, format);
        }
    }

    // ── CodeMonorepoWorkspace serde round-trip ────────────────────────

    #[test]
    fn workspace_serde_round_trip() {
        let workspace = pnpm_workspace();
        let json = serde_json::to_string_pretty(&workspace).expect("serialize workspace");
        let round_tripped: CodeMonorepoWorkspace =
            serde_json::from_str(&json).expect("deserialize workspace");
        assert_eq!(round_tripped, workspace);
    }

    #[test]
    fn workspace_serde_go_modules() {
        let workspace = go_workspace();
        let json = serde_json::to_string(&workspace).expect("serialize go workspace");
        let round_tripped: CodeMonorepoWorkspace =
            serde_json::from_str(&json).expect("deserialize go workspace");
        assert_eq!(round_tripped, workspace);
    }

    // ── CodeMonorepoWorkspace::validate ───────────────────────────────

    #[test]
    fn validate_succeeds_for_valid_workspace() {
        pnpm_workspace()
            .validate()
            .expect("two-member workspace should validate");
        go_workspace()
            .validate()
            .expect("two-member go workspace should validate");
    }

    #[test]
    fn validate_rejects_empty_members() {
        let workspace = CodeMonorepoWorkspace {
            format: CodeMonorepoWorkspaceFormat::CargoWorkspace,
            root_path: "/repos/ws".to_owned(),
            workspace_file_path: "/repos/ws/Cargo.toml".to_owned(),
            members: vec![],
        };

        let err = workspace
            .validate()
            .expect_err("empty members should fail validation");
        assert!(
            err.to_string().contains("at least 2"),
            "expected at-least-2 message, got: {err}"
        );
    }

    #[test]
    fn validate_rejects_single_member() {
        let workspace = CodeMonorepoWorkspace {
            format: CodeMonorepoWorkspaceFormat::CargoWorkspace,
            root_path: "/repos/ws".to_owned(),
            workspace_file_path: "/repos/ws/Cargo.toml".to_owned(),
            members: vec![CodeWorkspaceMember {
                package_name: "my-crate".to_owned(),
                relative_path: ".".to_owned(),
            }],
        };

        let err = workspace
            .validate()
            .expect_err("single-member workspace should fail validation");
        assert!(
            err.to_string().contains("at least 2"),
            "expected at-least-2 message, got: {err}"
        );
    }

    #[test]
    fn validate_rejects_empty_root_path() {
        let mut workspace = pnpm_workspace();
        workspace.root_path = "  ".to_owned();
        let err = workspace
            .validate()
            .expect_err("blank root_path should fail");
        assert!(err.to_string().contains("root_path"));
    }

    #[test]
    fn validate_rejects_empty_workspace_file_path() {
        let mut workspace = pnpm_workspace();
        workspace.workspace_file_path = String::new();
        let err = workspace
            .validate()
            .expect_err("empty workspace_file_path should fail");
        assert!(err.to_string().contains("workspace_file_path"));
    }

    #[test]
    fn validate_rejects_member_with_blank_name() {
        let mut workspace = pnpm_workspace();
        workspace.members[0].package_name = "\t".to_owned();
        let err = workspace
            .validate()
            .expect_err("blank member package name should fail");
        assert!(err.to_string().contains("package_name"));
    }

    #[test]
    fn validate_rejects_member_with_blank_path() {
        let mut workspace = go_workspace();
        workspace.members[1].relative_path = "\n  ".to_owned();
        let err = workspace
            .validate()
            .expect_err("blank member relative path should fail");
        assert!(err.to_string().contains("relative_path"));
    }

    // ── CodeWorkspaceMember serde round-trip ──────────────────────────

    #[test]
    fn workspace_member_serde_round_trip() {
        let member = CodeWorkspaceMember {
            package_name: "@scope/pkg".to_owned(),
            relative_path: "packages/pkg".to_owned(),
        };
        let json = serde_json::to_string(&member).expect("serialize member");
        let round_tripped: CodeWorkspaceMember =
            serde_json::from_str(&json).expect("deserialize member");
        assert_eq!(round_tripped, member);
    }

    // ── CodeWorkspacePackageMapping construction and serde ────────────

    #[test]
    fn package_mapping_serde_round_trip() {
        let mapping = CodeWorkspacePackageMapping {
            package_name: "@scope/core".to_owned(),
            ecosystem: "npm".to_owned(),
            repository_id: "repo-1".to_owned(),
            source_scope: "git_snapshot:abcdef1234567890".to_owned(),
            confidence_basis_points: 10_000,
        };
        let json = serde_json::to_string_pretty(&mapping).expect("serialize mapping");
        let round_tripped: CodeWorkspacePackageMapping =
            serde_json::from_str(&json).expect("deserialize mapping");
        assert_eq!(round_tripped, mapping);
    }

    // ── CodeWorkspaceDetectionConfig ──────────────────────────────────

    #[test]
    fn detection_config_serde_round_trip() {
        let config = CodeWorkspaceDetectionConfig {
            enabled: true,
            supported_formats: vec![
                CodeMonorepoWorkspaceFormat::Pnpm,
                CodeMonorepoWorkspaceFormat::CargoWorkspace,
            ],
        };
        let json = serde_json::to_string_pretty(&config).expect("serialize config");
        let round_tripped: CodeWorkspaceDetectionConfig =
            serde_json::from_str(&json).expect("deserialize config");
        assert_eq!(round_tripped, config);

        // Verify JSON contains snake_case format names.
        assert!(json.contains("\"pnpm\""));
        assert!(json.contains("\"cargo_workspace\""));
    }

    #[test]
    fn detection_config_disabled_default() {
        let config = CodeWorkspaceDetectionConfig {
            enabled: false,
            supported_formats: vec![CodeMonorepoWorkspaceFormat::GoModules],
        };
        let json = serde_json::to_string(&config).expect("serialize disabled config");
        let parsed: CodeWorkspaceDetectionConfig =
            serde_json::from_str(&json).expect("deserialize disabled config");
        assert!(!parsed.enabled);
        assert_eq!(parsed.supported_formats.len(), 1);
    }
}
