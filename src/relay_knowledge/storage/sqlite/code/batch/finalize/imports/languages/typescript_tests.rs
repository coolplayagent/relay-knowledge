use super::{named_import_bindings, needs_symbol_index};

#[test]
fn typescript_named_bindings_preserve_import_and_local_names() {
    let bindings =
        named_import_bindings("import { type Widget as LocalWidget, Helper } from './model';");

    assert_eq!(bindings.len(), 2);
    assert_eq!(bindings[0].imported_name, "Widget");
    assert_eq!(bindings[0].local_name, "LocalWidget");
    assert_eq!(bindings[1].imported_name, "Helper");
    assert_eq!(bindings[1].local_name, "Helper");
}

#[test]
fn typescript_symbol_index_is_only_needed_for_named_imports() {
    assert!(needs_symbol_index(
        "src/client.ts",
        "import { Widget } from './model';"
    ));
    assert!(!needs_symbol_index(
        "src/client.ts",
        "import './side_effect';"
    ));
}
