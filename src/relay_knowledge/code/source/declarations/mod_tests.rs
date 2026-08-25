use super::*;

#[test]
fn declaration_lines_require_definition_shape_and_identifier_boundaries() {
    for line in [
        "struct Widget {",
        "public final class Widget extends BaseWidget {",
        "@SuppressWarnings(\"serial\") public class Widget {",
        "@Generated(value = \"fixture\") public class Widget {",
        "[DataContract(Name = \"widget\")] public sealed class Widget {",
        "#[derive(Clone)] pub struct Widget {",
        "pub(crate) struct Widget {",
        "export default class Widget {",
        "sealed interface Widget {",
        "case class Widget(value: Int)",
        "private[core] final class Widget",
        "public actor Widget {",
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
        "public Widget(Value value) {",
        "Widget(Value value) : value_(value) {}",
        "explicit Widget(Value value) {}",
        "inline Widget::Widget(Value value) {}",
        "Widget::~Widget() {}",
        "new Widget(value);",
        "await Widget(value);",
        "try Widget(value);",
        "* @see #Widget(Value)",
        "// public class Widget {",
        "# Widget(value) constructs the default instance.",
        "/* Widget(value) is documented here. */",
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
