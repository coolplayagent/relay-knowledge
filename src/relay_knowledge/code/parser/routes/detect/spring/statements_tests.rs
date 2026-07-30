use super::{
    MAX_SPRING_MAPPING_ANNOTATION_LINES, spring_annotation_statement_from_offset,
    spring_route_annotation_offset, spring_statement_after_annotation,
    spring_tail_after_leading_annotations,
};

#[test]
fn finds_qualified_route_annotations_outside_literals() {
    let line =
        r#"String sample = "@GetMapping"; @org.springframework.web.bind.annotation.GetMapping("#;

    let offset = spring_route_annotation_offset(line);

    assert_eq!(offset, line.rfind('@'));
}

#[test]
fn aggregates_multiline_annotation_and_preserves_method_tail() {
    let lines = strings(&[
        "prefix @GetMapping(",
        r#"path = {"/users", "/members)active"}"#,
        ") public String listUsers() {",
    ]);
    let offset = lines[0].find('@').expect("annotation offset");

    let (statement, consumed) = spring_annotation_statement_from_offset(&lines, 0, offset);

    assert_eq!(consumed, 3);
    assert_eq!(
        spring_statement_after_annotation(&statement).trim(),
        "public String listUsers() {"
    );
}

#[test]
fn strips_multiple_leading_annotations_before_a_method() {
    let tail = "@Deprecated @Transactional(readOnly = true) public String listUsers() {";

    assert_eq!(
        spring_tail_after_leading_annotations(tail),
        "public String listUsers() {"
    );
}

#[test]
fn bounds_unclosed_annotation_aggregation() {
    let lines = (0..MAX_SPRING_MAPPING_ANNOTATION_LINES + 3)
        .map(|index| {
            if index == 0 {
                "@GetMapping(".to_owned()
            } else {
                format!("value{index}")
            }
        })
        .collect::<Vec<_>>();

    let (_, consumed) = spring_annotation_statement_from_offset(&lines, 0, 0);

    assert_eq!(consumed, MAX_SPRING_MAPPING_ANNOTATION_LINES);
}

fn strings(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_owned()).collect()
}
