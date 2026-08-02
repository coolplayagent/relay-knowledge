//! C/C++ recovery source-text contract tests.

use super::{line_code_without_comment, source_line_fragment, source_lines};

#[test]
fn block_comment_masking_preserves_line_bytes_and_literal_markers() {
    let lines =
        source_lines("const char *url = \"/*kept*/\"; /* hidden\nstill hidden */ void Open();");

    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].code.len(), lines[0].byte_end - lines[0].byte_start);
    assert!(lines[0].code.contains("\"/*kept*/\""));
    assert!(!lines[1].code.contains("still hidden"));
    assert!(lines[1].code.ends_with("void Open();"));
}

#[test]
fn line_comment_detection_ignores_markers_inside_literals() {
    let code = "const char *url = \"https://example.test\"; // hidden";

    assert_eq!(
        line_code_without_comment(code),
        "const char *url = \"https://example.test\"; "
    );
}

#[test]
fn source_line_fragments_keep_original_byte_and_line_coordinates() {
    let lines = source_lines("prefix void Open(); suffix");
    let fragment = source_line_fragment(&lines[0], 7, 19);

    assert_eq!(fragment.code, "void Open();");
    assert_eq!(fragment.byte_start, 7);
    assert_eq!(fragment.byte_end, 19);
    assert_eq!(fragment.number, 1);
}
