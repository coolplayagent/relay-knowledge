use super::super::test_support;
use super::{ImportContext, ModuleFileResolution, normalize_join, parse_quoted_specifier};

#[test]
fn module_lookup_resolves_source_root_and_preserves_physical_hint() {
    let files = vec![test_support::file("src/main/java/app/Client.java", "java")];
    let context = ImportContext::new(&files, &[]);

    let resolution = context.resolve_first_module_file(&["app/Client.java".to_owned()], true);

    assert!(matches!(
        resolution,
        ModuleFileResolution::Resolved(path) if path == "src/main/java/app/Client.java"
    ));
}

#[test]
fn exact_module_lookup_does_not_infer_a_source_root() {
    let files = vec![test_support::file("docs/guide.md", "markdown")];
    let context = ImportContext::new(&files, &[]);

    assert!(matches!(
        context.resolve_first_exact_module_file(&["guide.md".to_owned()]),
        ModuleFileResolution::Unresolved
    ));
    assert!(matches!(
        context.resolve_first_exact_module_file(&["docs/guide.md".to_owned()]),
        ModuleFileResolution::Resolved(path) if path == "docs/guide.md"
    ));
}

#[test]
fn path_helpers_reject_escapes_and_unclosed_specifiers() {
    assert_eq!(
        normalize_join("src/client", "../model.rs").as_deref(),
        Some("src/model.rs")
    );
    assert_eq!(normalize_join("", "../model.rs"), None);
    assert_eq!(normalize_join("src", "/model.rs"), None);
    assert_eq!(
        parse_quoted_specifier("use \"client.rs\";"),
        Some("client.rs")
    );
    assert_eq!(parse_quoted_specifier("use \"client.rs;"), None);
}
