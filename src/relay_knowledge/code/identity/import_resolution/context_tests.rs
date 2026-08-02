use super::super::test_support;
use super::ImportContext;

#[test]
fn context_indexes_file_languages_and_duplicate_symbol_names() {
    let files = vec![
        test_support::file("src/lib.rs", "rust"),
        test_support::file("src/app.py", "python"),
    ];
    let symbols = vec![
        test_support::symbol("src/lib.rs", "rust", "Client", "crate.Client", "struct"),
        test_support::symbol("src/app.py", "python", "Client", "app.Client", "class"),
    ];

    let context = ImportContext::new(&files, &symbols);

    assert_eq!(context.language_for_path("src/lib.rs"), Some("rust"));
    assert_eq!(context.language_for_path("src/missing.rs"), None);
    assert_eq!(context.symbols_by_name["Client"].len(), 2);
}
