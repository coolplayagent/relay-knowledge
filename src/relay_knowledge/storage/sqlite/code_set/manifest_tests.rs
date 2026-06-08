use super::*;

#[test]
fn go_work_use_paths_limit_module_prefixes_to_workspace_members() {
    let chunks = vec![
        ManifestChunk {
            path: "go.work".to_owned(),
            content: "go 1.22\nuse (\n    ./component\n)\n".to_owned(),
        },
        ManifestChunk {
            path: "component/go.mod".to_owned(),
            content: "module go.opentelemetry.io/collector/component\n".to_owned(),
        },
        ManifestChunk {
            path: "sandbox/go.mod".to_owned(),
            content: "module example.com/sandbox\n".to_owned(),
        },
    ];
    let go_workspaces = go_workspaces(&chunks);
    let mut prefixes = Vec::new();
    for chunk in &chunks {
        if is_go_mod_path(&chunk.path) && go_module_allowed(&chunk.path, &go_workspaces) {
            collect_go_module_prefixes(&chunk.path, &chunk.content, &mut prefixes);
        }
    }

    assert_eq!(prefixes.len(), 1);
    assert_eq!(prefixes[0].source_path_prefix, "component");
    assert!(
        module_keys_for_path_with_prefixes("component/identifiable.go", &prefixes)
            .contains("go.opentelemetry.io.collector.component")
    );
    assert!(
        !module_keys_for_path_with_prefixes("sandbox/main.go", &prefixes)
            .contains("example.com.sandbox")
    );
}

#[test]
fn nested_go_work_only_filters_modules_under_its_root() {
    let chunks = vec![
        ManifestChunk {
            path: "go.mod".to_owned(),
            content: "module example.com/root\n".to_owned(),
        },
        ManifestChunk {
            path: "examples/go.work".to_owned(),
            content: "go 1.22\nuse ./demo\n".to_owned(),
        },
        ManifestChunk {
            path: "examples/demo/go.mod".to_owned(),
            content: "module example.com/demo\n".to_owned(),
        },
        ManifestChunk {
            path: "examples/other/go.mod".to_owned(),
            content: "module example.com/other\n".to_owned(),
        },
    ];
    let go_workspaces = go_workspaces(&chunks);
    let mut prefixes = Vec::new();
    for chunk in &chunks {
        if is_go_mod_path(&chunk.path) && go_module_allowed(&chunk.path, &go_workspaces) {
            collect_go_module_prefixes(&chunk.path, &chunk.content, &mut prefixes);
        }
    }

    assert_eq!(prefixes.len(), 2);
    assert!(module_keys_for_path_with_prefixes("main.go", &prefixes).contains("example.com.root"));
    assert!(
        module_keys_for_path_with_prefixes("examples/demo/main.go", &prefixes)
            .contains("example.com.demo")
    );
    assert!(
        !module_keys_for_path_with_prefixes("examples/other/main.go", &prefixes)
            .contains("example.com.other")
    );
}

#[test]
fn pnpm_workspace_package_prefixes_map_names_entries_and_exports() {
    let workspaces = pnpm_workspaces(&[ManifestChunk {
        path: "pnpm-workspace.yaml".to_owned(),
        content: "packages:\n  - 'packages/*'\n  - '!packages/fixtures'\n".to_owned(),
    }]);
    let mut prefixes = Vec::new();
    collect_package_prefixes(
        "packages/ui/package.json",
        r#"{
            "name": "@myorg/ui-components",
            "main": "src/index.ts",
            "exports": {
                ".": "./src/index.ts",
                "./button": "./src/button.ts"
            }
        }"#,
        &workspaces,
        &mut prefixes,
    );
    collect_package_prefixes(
        "packages/fixtures/package.json",
        r#"{"name":"@myorg/fixtures","main":"src/index.ts"}"#,
        &workspaces,
        &mut prefixes,
    );

    assert_eq!(prefixes.len(), 1);
    let entry_keys = module_keys_for_path_with_prefixes("packages/ui/src/index.ts", &prefixes);
    let button_keys = module_keys_for_path_with_prefixes("packages/ui/src/button.ts", &prefixes);

    assert!(entry_keys.contains("@myorg.ui.components"));
    assert!(button_keys.contains("@myorg.ui.components.button"));
    assert!(
        !module_keys_for_path_with_prefixes("packages/fixtures/src/index.ts", &prefixes)
            .contains("@myorg.fixtures")
    );
}

#[test]
fn package_manifest_file_does_not_inherit_bare_package_key() {
    let mut prefixes = Vec::new();
    collect_package_prefixes(
        "packages/ui/package.json",
        r#"{"name":"@myorg/ui-components","main":"src/index.ts"}"#,
        &[],
        &mut prefixes,
    );

    assert!(
        !module_keys_for_path_with_prefixes("packages/ui/package.json", &prefixes)
            .contains("@myorg.ui.components")
    );
    assert!(
        module_keys_for_path_with_prefixes("packages/ui/src/index.ts", &prefixes)
            .contains("@myorg.ui.components")
    );
}

#[test]
fn package_exports_override_main_entry_aliases() {
    let mut prefixes = Vec::new();
    collect_package_prefixes(
        "packages/ui/package.json",
        r#"{
            "name":"@myorg/ui-components",
            "main":"src/index.ts",
            "exports":{"./button":"./src/button.ts"}
        }"#,
        &[],
        &mut prefixes,
    );

    assert!(
        !module_keys_for_path_with_prefixes("packages/ui/src/index.ts", &prefixes)
            .contains("@myorg.ui.components")
    );
    assert!(
        module_keys_for_path_with_prefixes("packages/ui/src/button.ts", &prefixes)
            .contains("@myorg.ui.components.button")
    );
}

#[test]
fn conditional_exports_choose_one_entry_alias() {
    let mut prefixes = Vec::new();
    collect_package_prefixes(
        "packages/ui/package.json",
        r#"{
            "name":"@myorg/ui-components",
            "exports":{
                ".":{
                    "types":"./dist/index.d.ts",
                    "import":"./dist/index.js",
                    "require":"./dist/index.cjs"
                }
            }
        }"#,
        &[],
        &mut prefixes,
    );

    assert!(
        module_keys_for_path_with_prefixes("packages/ui/dist/index.js", &prefixes)
            .contains("@myorg.ui.components")
    );
    assert!(
        !module_keys_for_path_with_prefixes("packages/ui/dist/index.d.ts", &prefixes)
            .contains("@myorg.ui.components")
    );
    assert!(
        !module_keys_for_path_with_prefixes("packages/ui/dist/index.cjs", &prefixes)
            .contains("@myorg.ui.components")
    );
}

#[test]
fn wildcard_exports_map_matching_subpath_imports() {
    let mut prefixes = Vec::new();
    collect_package_prefixes(
        "packages/ui/package.json",
        r#"{
            "name":"@myorg/ui",
            "exports":{"./components/*":"./src/components/*.ts"}
        }"#,
        &[],
        &mut prefixes,
    );

    assert!(
        module_keys_for_path_with_prefixes("packages/ui/src/components/button.ts", &prefixes)
            .contains("@myorg.ui.components.button")
    );
    assert!(
        !module_keys_for_path_with_prefixes("packages/ui/src/private/button.ts", &prefixes)
            .contains("@myorg.ui.components.button")
    );
}

#[test]
fn exports_disable_generic_package_subpath_keys() {
    let mut prefixes = Vec::new();
    collect_package_prefixes(
        "packages/ui/package.json",
        r#"{
            "name":"@myorg/ui-components",
            "exports":{"./button":"./src/button.ts"}
        }"#,
        &[],
        &mut prefixes,
    );

    assert!(
        module_keys_for_path_with_prefixes("packages/ui/src/button.ts", &prefixes)
            .contains("@myorg.ui.components.button")
    );
    assert!(
        !module_keys_for_path_with_prefixes("packages/ui/src/internal.ts", &prefixes)
            .contains("@myorg.ui.components.src.internal")
    );
}

#[test]
fn default_entries_are_skipped_when_explicit_entries_exist() {
    let mut prefixes = Vec::new();
    collect_package_prefixes(
        "packages/ui/package.json",
        r#"{"name":"@myorg/ui-components","main":"dist/index.js"}"#,
        &[],
        &mut prefixes,
    );

    assert!(
        module_keys_for_path_with_prefixes("packages/ui/dist/index.js", &prefixes)
            .contains("@myorg.ui.components")
    );
    assert!(
        !module_keys_for_path_with_prefixes("packages/ui/index.js", &prefixes)
            .contains("@myorg.ui.components")
    );
}

#[test]
fn pnpm_workspace_relative_paths_require_root_boundary() {
    assert_eq!(
        workspace_relative_path("packages/ui", "packages").as_deref(),
        Some("ui")
    );
    assert!(workspace_relative_path("packages-ui", "packages").is_none());
}

#[test]
fn nested_pnpm_workspace_only_filters_packages_under_its_root() {
    let workspaces = pnpm_workspaces(&[ManifestChunk {
        path: "examples/pnpm-workspace.yaml".to_owned(),
        content: "packages:\n  - 'packages/*'\n".to_owned(),
    }]);
    let mut prefixes = Vec::new();
    collect_package_prefixes(
        "package.json",
        r#"{"name":"@myorg/root","main":"src/index.ts"}"#,
        &workspaces,
        &mut prefixes,
    );
    collect_package_prefixes(
        "examples/packages/demo/package.json",
        r#"{"name":"@myorg/demo","main":"src/index.ts"}"#,
        &workspaces,
        &mut prefixes,
    );
    collect_package_prefixes(
        "examples/standalone/package.json",
        r#"{"name":"@myorg/standalone","main":"src/index.ts"}"#,
        &workspaces,
        &mut prefixes,
    );

    assert_eq!(prefixes.len(), 2);
    assert!(module_keys_for_path_with_prefixes("src/index.ts", &prefixes).contains("@myorg.root"));
    assert!(
        module_keys_for_path_with_prefixes("examples/packages/demo/src/index.ts", &prefixes)
            .contains("@myorg.demo")
    );
    assert!(
        !module_keys_for_path_with_prefixes("examples/standalone/src/index.ts", &prefixes)
            .contains("@myorg.standalone")
    );
}

#[test]
fn pnpm_workspace_includes_root_package_with_custom_globs() {
    let workspaces = pnpm_workspaces(&[ManifestChunk {
        path: "pnpm-workspace.yaml".to_owned(),
        content: "packages:\n  - 'packages/*'\n".to_owned(),
    }]);
    let mut prefixes = Vec::new();
    collect_package_prefixes(
        "package.json",
        r#"{"name":"@myorg/root","main":"src/index.ts"}"#,
        &workspaces,
        &mut prefixes,
    );
    collect_package_prefixes(
        "packages/ui/package.json",
        r#"{"name":"@myorg/ui","main":"src/index.ts"}"#,
        &workspaces,
        &mut prefixes,
    );

    assert_eq!(prefixes.len(), 2);
    assert!(module_keys_for_path_with_prefixes("src/index.ts", &prefixes).contains("@myorg.root"));
    assert!(
        module_keys_for_path_with_prefixes("packages/ui/src/index.ts", &prefixes)
            .contains("@myorg.ui")
    );
}

#[test]
fn pnpm_workspace_without_package_globs_only_includes_root_package() {
    let workspaces = pnpm_workspaces(&[ManifestChunk {
        path: "pnpm-workspace.yaml".to_owned(),
        content: "catalog:\n  react: ^18.0.0\n".to_owned(),
    }]);
    let mut prefixes = Vec::new();
    collect_package_prefixes(
        "package.json",
        r#"{"name":"@myorg/root","main":"src/index.ts"}"#,
        &workspaces,
        &mut prefixes,
    );
    collect_package_prefixes(
        "packages/ui/package.json",
        r#"{"name":"@myorg/ui","main":"src/index.ts"}"#,
        &workspaces,
        &mut prefixes,
    );

    assert_eq!(prefixes.len(), 1);
    assert!(module_keys_for_path_with_prefixes("src/index.ts", &prefixes).contains("@myorg.root"));
    assert!(
        !module_keys_for_path_with_prefixes("packages/ui/src/index.ts", &prefixes)
            .contains("@myorg.ui")
    );
}
