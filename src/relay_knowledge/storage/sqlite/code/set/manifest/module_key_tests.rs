use std::collections::{BTreeMap, BTreeSet};

use super::{
    ModulePrefix, PathAliasPattern, module_keys_for_path_with_prefixes,
    module_keys_for_symbol_path_with_prefixes, normalize_module_key,
};

#[test]
fn normalization_preserves_scoped_identity_as_dot_segments() {
    assert_eq!(normalize_module_key("@myorg/ui-kit"), "@myorg.ui.kit");
    assert_eq!(
        normalize_module_key("use example::runtime::Client"),
        "example.runtime.client"
    );
}

#[test]
fn source_paths_expand_package_and_export_aliases() {
    let prefix = ModulePrefix {
        source_path_prefix: "packages/ui".to_owned(),
        module_key: "@myorg.ui".to_owned(),
        path_aliases: BTreeMap::from([(
            "src/index.ts".to_owned(),
            BTreeSet::from(["@myorg.ui".to_owned()]),
        )]),
        path_alias_patterns: vec![PathAliasPattern {
            path_prefix: "src/components/".to_owned(),
            path_suffix: ".ts".to_owned(),
            alias_prefix: "@myorg.ui.components".to_owned(),
            alias_suffix: String::new(),
        }],
        exposes_package_paths: false,
        exposes_root_package_key: false,
    };

    assert!(
        module_keys_for_path_with_prefixes(
            "packages/ui/src/index.ts",
            std::slice::from_ref(&prefix),
        )
        .contains("@myorg.ui")
    );
    assert!(
        module_keys_for_path_with_prefixes(
            "packages/ui/src/components/button.ts",
            std::slice::from_ref(&prefix),
        )
        .contains("@myorg.ui.components.button")
    );
    assert!(
        !module_keys_for_symbol_path_with_prefixes(
            "packages/ui/src/index.ts",
            std::slice::from_ref(&prefix),
        )
        .contains("@myorg.ui")
    );
}
