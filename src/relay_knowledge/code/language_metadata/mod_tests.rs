// Direct tests for language metadata and documentation comments.

use super::*;

#[test]
fn strips_supported_extensions_without_rewriting_unknown_paths() {
    assert_eq!(strip_supported_extension("src/app.tsx"), "src/app");
    assert_eq!(strip_supported_extension("Gemfile"), "Gemfile");
    assert_eq!(strip_supported_extension("README.md"), "README");
}

#[test]
fn doc_comment_rules_cover_known_and_unknown_languages() {
    assert_eq!(
        doc_comment_text("/// Runs retry.", "go"),
        Some("Runs retry.")
    );
    assert_eq!(
        doc_comment_text("# Runs retry.", "python"),
        Some("Runs retry.")
    );
    assert_eq!(doc_comment_text("-- Runs retry.", "sql"), None);
}
