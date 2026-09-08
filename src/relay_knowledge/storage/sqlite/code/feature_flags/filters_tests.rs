use super::*;

#[test]
fn selectors_escape_literals_and_intersect_separate_authority_scopes() {
    let mut clauses = Vec::new();
    let mut params = Vec::new();
    append_path_filter_clause(&mut clauses, &mut params, &["./src_a/".to_owned()]);
    append_path_filter_clause(&mut clauses, &mut params, &["src_a/private%".to_owned()]);
    append_language_filter_clause(
        &mut clauses,
        &mut params,
        &["java".to_owned(), "java".to_owned()],
    );
    assert_eq!(clauses.len(), 3);
    assert_eq!(params[1], Value::Text("src\\_a/%".to_owned()));
    assert_eq!(params[3], Value::Text("src\\_a/private\\%/%".to_owned()));
    assert_eq!(params[4], Value::Text("java".to_owned()));
    append_path_filter_clause(&mut clauses, &mut params, &[String::new()]);
    assert_eq!(clauses.last().unwrap(), "0 = 1");
}

#[test]
fn whole_root_and_empty_language_filters_preserve_bounded_scope_predicate() {
    let mut clauses = vec!["flag.source_scope = ?".to_owned()];
    let mut params = Vec::new();
    append_path_filter_clause(&mut clauses, &mut params, &[".".to_owned()]);
    append_language_filter_clause(&mut clauses, &mut params, &[]);
    assert_eq!(clauses.len(), 1);
    assert!(params.is_empty());
    append_language_filter_clause(&mut clauses, &mut params, &[String::new()]);
    assert_eq!(clauses.last().unwrap(), "0 = 1");
}
