use super::{module_path_candidates, parse_imported_names};

#[test]
fn relative_python_modules_respect_package_depth() {
    assert_eq!(
        module_path_candidates("src/pkg/api/client.py", "..models"),
        vec!["pkg/models".to_owned()]
    );
    assert!(module_path_candidates("client.py", "...outside").is_empty());
}

#[test]
fn imported_python_names_drop_aliases_wildcards_and_wrapping() {
    assert_eq!(
        parse_imported_names("(Widget as Local, Helper, *)"),
        vec!["Widget".to_owned(), "Helper".to_owned()]
    );
}
