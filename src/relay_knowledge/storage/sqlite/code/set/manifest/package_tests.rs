use super::super::{ManifestChunk, module_keys_for_path_with_prefixes};
use super::{collect_prefixes, workspaces};

#[test]
fn pnpm_workspace_package_prefixes_map_names_entries_and_exports() {
    let workspaces = workspaces(&[ManifestChunk {
        path: "pnpm-workspace.yaml".to_owned(),
        content: "packages:\n  - 'packages/*'\n  - '!packages/fixtures'\n".to_owned(),
    }]);
    let mut prefixes = Vec::new();
    collect_prefixes(
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
    collect_prefixes(
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
    collect_prefixes(
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
    collect_prefixes(
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
    collect_prefixes(
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
    collect_prefixes(
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
    collect_prefixes(
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
    collect_prefixes(
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
fn nested_pnpm_workspace_only_filters_packages_under_its_root() {
    let workspaces = workspaces(&[ManifestChunk {
        path: "examples/pnpm-workspace.yaml".to_owned(),
        content: "packages:\n  - 'packages/*'\n".to_owned(),
    }]);
    let mut prefixes = Vec::new();
    collect_prefixes(
        "package.json",
        r#"{"name":"@myorg/root","main":"src/index.ts"}"#,
        &workspaces,
        &mut prefixes,
    );
    collect_prefixes(
        "examples/packages/demo/package.json",
        r#"{"name":"@myorg/demo","main":"src/index.ts"}"#,
        &workspaces,
        &mut prefixes,
    );
    collect_prefixes(
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
    let workspaces = workspaces(&[ManifestChunk {
        path: "pnpm-workspace.yaml".to_owned(),
        content: "packages:\n  - 'packages/*'\n".to_owned(),
    }]);
    let mut prefixes = Vec::new();
    collect_prefixes(
        "package.json",
        r#"{"name":"@myorg/root","main":"src/index.ts"}"#,
        &workspaces,
        &mut prefixes,
    );
    collect_prefixes(
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
    let workspaces = workspaces(&[ManifestChunk {
        path: "pnpm-workspace.yaml".to_owned(),
        content: "catalog:\n  react: ^18.0.0\n".to_owned(),
    }]);
    let mut prefixes = Vec::new();
    collect_prefixes(
        "package.json",
        r#"{"name":"@myorg/root","main":"src/index.ts"}"#,
        &workspaces,
        &mut prefixes,
    );
    collect_prefixes(
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
