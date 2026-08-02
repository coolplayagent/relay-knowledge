use super::super::test_support;
use super::{ImportContext, ImportResolution};

#[test]
fn symbol_lookup_distinguishes_unique_and_ambiguous_names() {
    let unique = vec![test_support::symbol(
        "src/client.rs",
        "rust",
        "Client",
        "crate.client.Client",
        "struct",
    )];
    let unique_context = ImportContext::new(&[], &unique);
    assert_eq!(
        unique_context.resolve_name("Client"),
        ImportResolution::Resolved
    );

    let mut ambiguous = unique;
    ambiguous.push(test_support::symbol(
        "tests/client.rs",
        "rust",
        "Client",
        "tests.client.Client",
        "struct",
    ));
    let ambiguous_context = ImportContext::new(&[], &ambiguous);
    assert_eq!(
        ambiguous_context.resolve_name("Client"),
        ImportResolution::Ambiguous
    );
}

#[test]
fn namespace_lookup_filters_language_and_symbol_kind() {
    let symbols = vec![
        test_support::symbol(
            "src/App/Client.cs",
            "csharp",
            "Client",
            "repo.App.Client",
            "class",
        ),
        test_support::symbol(
            "src/App/Client.kt",
            "kotlin",
            "Client",
            "repo.App.Client",
            "class",
        ),
    ];
    let context = ImportContext::new(&[], &symbols);

    assert_eq!(
        context.resolve_name_in_namespace_for_language_and_kinds(
            "App",
            "Client",
            "csharp",
            &["class"],
        ),
        ImportResolution::Resolved
    );
    assert_eq!(
        context.resolve_name_in_namespace_for_language_and_kinds(
            "App",
            "Client",
            "csharp",
            &["function"],
        ),
        ImportResolution::Unresolved
    );
}

#[test]
fn directory_tree_lookup_returns_the_unique_symbol_path_hint() {
    let symbols = vec![test_support::symbol(
        "src/templates/account/card.html",
        "gotemplate",
        "card",
        "templates.account.card",
        "template",
    )];
    let context = ImportContext::new(&[], &symbols);

    assert_eq!(
        context.resolve_name_in_directory_tree_for_language_and_kinds_with_hint(
            "card",
            "src/templates",
            "gotemplate",
            &["template"],
        ),
        (
            ImportResolution::Resolved,
            Some("src/templates/account/card.html".to_owned())
        )
    );
}
