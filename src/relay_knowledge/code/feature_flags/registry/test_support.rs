use super::*;
pub(super) fn facts(language: &str, source: &str) -> Vec<CodeFeatureFlagRecord> {
    raw_facts(language, source)
        .into_iter()
        .filter(|row| row.edge_kind != "config_type_declaration")
        .collect()
}
pub(super) fn raw_facts(language: &str, source: &str) -> Vec<CodeFeatureFlagRecord> {
    super::extract(&FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path: if language == "gotemplate" {
            "sample.ctmpl"
        } else {
            "sample"
        },
        language_id: language,
        content: source,
        config_facts: &[],
    })
    .unwrap()
}
