//! Shared AST boundary for deterministic parser-to-storage configuration tests.
use super::*;

pub(crate) fn extract(path: &str, source: &str) -> Vec<CodeFeatureFlagRecord> {
    let language = crate::code::languages::detect_language(path).unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&(language.language)()).unwrap();
    let tree = parser.parse(source, None).unwrap();
    extract_feature_flags(FeatureFlagFileInput {
        line_index: Default::default(),
        syntax_root: Some(tree.root_node()),
        repository_id: "repo",
        source_scope: "scope",
        file_id: path,
        path,
        language_id: language.id,
        content: source,
        config_facts: &[],
    })
    .unwrap()
}
