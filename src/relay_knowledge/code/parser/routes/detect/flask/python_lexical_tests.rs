use super::python_code_lines_without_triple_quoted_strings;

#[test]
fn removes_triple_quoted_regions_and_python_comments() {
    let source = r#"
        label = "users # active"  # trailing comment
        """
        @app.get("/hidden")
        """
        @app.get("/active")
    "#;

    let code = python_code_lines_without_triple_quoted_strings(source).join("\n");

    assert!(code.contains(r#"label = "users # active"  "#));
    assert!(code.contains(r#"@app.get("/active")"#));
    assert!(!code.contains("/hidden"));
    assert!(!code.contains("trailing comment"));
}

#[test]
fn resumes_code_after_same_line_single_and_double_triple_quotes() {
    let source = concat!(
        "before = 1; '''ignored''' after_single = 2\n",
        "left = 3; \"\"\"ignored\"\"\" after_double = 4\n",
    );

    let lines = python_code_lines_without_triple_quoted_strings(source);

    assert_eq!(lines[0], "before = 1;  after_single = 2");
    assert_eq!(lines[1], "left = 3;  after_double = 4");
}
