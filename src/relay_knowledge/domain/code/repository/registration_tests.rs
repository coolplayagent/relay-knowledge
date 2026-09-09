use super::{CodeRepositorySelector, RepositoryCodeRange};

#[test]
fn selector_trims_and_deduplicates_filters() {
    let selector = CodeRepositorySelector::new(
        " repo ",
        " HEAD ",
        vec!["src".to_owned(), " src ".to_owned()],
        vec!["rust".to_owned(), "rust".to_owned()],
    )
    .expect("selector should validate");

    assert_eq!(selector.repository, "repo");
    assert_eq!(selector.ref_selector, "HEAD");
    assert_eq!(selector.path_filters, ["src"]);
    assert_eq!(selector.language_filters, ["rust"]);
}

#[test]
fn code_ranges_must_be_ordered() {
    let error = RepositoryCodeRange::new("line_range", 3, 2).expect_err("range should fail");

    assert_eq!(error.field, "line_range");
}

#[test]
fn filesystem_root_spelling_preserves_discovery_scope_contract() {
    for root in [".", "./", " .\\ ", "./."] {
        assert_eq!(super::normalize_filesystem_path_filter(root), ".");
    }
    assert_eq!(
        super::normalize_filesystem_path_filter(" ./src/lib/ "),
        "src/lib"
    );
    for narrow in ["", "src", "../", "*", "/"] {
        assert_ne!(super::normalize_filesystem_path_filter(narrow), ".");
    }
}
