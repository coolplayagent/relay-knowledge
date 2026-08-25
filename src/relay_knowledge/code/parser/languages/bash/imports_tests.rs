use super::line_imports;

#[test]
fn escaped_dot_builtin_is_a_bounded_source_import() {
    let imports = line_imports(
        "\\. ./lib/runtime.sh\n\\.not-a-command ./lib/noise.sh\n\\.   \n. ./lib/plain.sh\n",
    );

    assert_eq!(imports.len(), 2);
    assert_eq!(imports[0].module, r"\. ./lib/runtime.sh");
    assert_eq!(imports[0].range.line_start, 1);
    assert_eq!(imports[1].module, ". ./lib/plain.sh");
    assert_eq!(imports[1].range.line_start, 4);
}
