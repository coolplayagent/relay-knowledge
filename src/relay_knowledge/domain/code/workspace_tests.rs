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
