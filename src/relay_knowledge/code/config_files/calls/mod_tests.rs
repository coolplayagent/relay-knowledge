use super::{cmake_calls, starlark_calls};

#[test]
fn collects_multiline_starlark_calls_with_source_ranges() {
    let calls = starlark_calls(
        "# ignored\nload(\n    \"//tools:defs.bzl\",\n    \"rule\",\n)\n",
        "load",
    );

    assert_eq!(calls.len(), 1);
    assert!(calls[0].text.contains("//tools:defs.bzl"));
    assert_eq!(calls[0].range.line_start, 2);
    assert_eq!(calls[0].range.line_end, 5);
}

#[test]
fn ignores_parentheses_inside_cmake_quotes() {
    let calls = cmake_calls("set(TARGET \"value(with-parens)\")\n");

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].command, "set");
    assert_eq!(calls[0].args, "TARGET \"value(with-parens)\"");
}
