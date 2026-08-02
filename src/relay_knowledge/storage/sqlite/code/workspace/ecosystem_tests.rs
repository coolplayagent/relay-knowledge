use crate::domain::CodeMonorepoWorkspaceFormat;

use super::*;

#[test]
fn maps_workspace_formats_and_languages_to_ecosystems() {
    assert_eq!(
        ecosystem_for_format(CodeMonorepoWorkspaceFormat::Pnpm),
        "npm"
    );
    assert_eq!(
        ecosystem_for_format(CodeMonorepoWorkspaceFormat::GoModules),
        "go"
    );
    assert_eq!(
        ecosystem_for_format(CodeMonorepoWorkspaceFormat::CargoWorkspace),
        "rust"
    );
    assert_eq!(ecosystem_for_language("typescript"), Some("npm"));
    assert_eq!(ecosystem_for_language("tsx"), Some("npm"));
    assert_eq!(ecosystem_for_language("go"), Some("go"));
    assert_eq!(ecosystem_for_language("rust"), Some("rust"));
    assert_eq!(ecosystem_for_language("python"), None);
}

#[test]
fn reports_stable_workspace_format_keys_and_manifests() {
    assert_eq!(
        workspace_format_key(CodeMonorepoWorkspaceFormat::Pnpm),
        "pnpm"
    );
    assert_eq!(
        workspace_format_key(CodeMonorepoWorkspaceFormat::GoModules),
        "go_modules"
    );
    assert_eq!(
        workspace_format_key(CodeMonorepoWorkspaceFormat::CargoWorkspace),
        "cargo_workspace"
    );
    assert_eq!(workspace_manifest_file_name("npm"), Some("package.json"));
    assert_eq!(workspace_manifest_file_name("go"), Some("go.mod"));
    assert_eq!(workspace_manifest_file_name("rust"), Some("Cargo.toml"));
    assert_eq!(workspace_manifest_file_name("python"), None);
}

#[test]
fn package_candidates_preserve_path_and_namespace_prefixes() {
    assert_eq!(
        workspace_package_candidates("example.com/svc/api/client"),
        vec![
            "example.com/svc/api/client",
            "example.com/svc/api",
            "example.com/svc",
            "example.com",
            "example",
        ]
    );
    assert_eq!(
        workspace_package_candidates("@scope/core/utils"),
        vec!["@scope/core/utils", "@scope/core", "@scope"]
    );
    assert_eq!(
        workspace_package_candidates("serde::de::Deserialize"),
        vec!["serde::de::Deserialize", "serde::de", "serde"]
    );
    assert!(workspace_package_candidates("  ").is_empty());
}

#[test]
fn normalizes_language_import_statements() {
    assert_eq!(
        [
            workspace_lookup_module("api \"example.com/svc/api\"", "go"),
            workspace_lookup_module("_ `example.com/svc/api`;", "go"),
            workspace_lookup_module("import { x } from \"@scope/core\";", "npm"),
            workspace_lookup_module("await import('@scope/core/client')", "npm"),
            workspace_lookup_module("pub use core::client::Client;", "rust"),
            workspace_lookup_module("extern crate core as core_alias;", "rust"),
            workspace_lookup_module("use crate::{local};", "rust"),
            workspace_lookup_module(" custom.module; ", "python"),
        ],
        [
            "example.com/svc/api",
            "example.com/svc/api",
            "@scope/core",
            "@scope/core/client",
            "core::client::Client",
            "core",
            "crate",
            "custom.module",
        ]
    );
}

#[test]
fn identifies_only_local_or_relative_modules() {
    for module in [
        "./foo",
        "../foo",
        "crate::foo",
        "self::foo",
        "super::foo",
        "crate",
        "self",
        "super",
        "",
        "  ",
    ] {
        assert!(is_local_or_relative_module(module), "{module}");
    }
    assert!(!is_local_or_relative_module("example.com/api"));
    assert!(!is_local_or_relative_module("@scope/pkg"));
}
