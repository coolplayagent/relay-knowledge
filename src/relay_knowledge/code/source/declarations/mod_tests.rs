use super::*;

#[test]
fn declaration_lines_require_definition_shape_and_identifier_boundaries() {
    for line in [
        "struct Widget {",
        "#define Widget(value) value",
        "using Widget = sdk::Type;",
        "static int Widget(int value) {",
    ] {
        assert!(source_line_defines_identity(line, "Widget"), "{line}");
    }
    for line in [
        "return Widget(value);",
        "struct WidgetFactory {",
        "widget = Widget(value);",
    ] {
        assert!(!source_line_defines_identity(line, "Widget"), "{line}");
    }
}

#[test]
fn git_blob_paths_reject_absolute_parent_empty_and_backslash_components() {
    assert!(safe_git_blob_path("src/module/file.rs"));
    for path in [
        "",
        "/src/file.rs",
        "src/../file.rs",
        "src//file.rs",
        r"src\file.rs",
    ] {
        assert!(!safe_git_blob_path(path), "{path}");
    }
}
