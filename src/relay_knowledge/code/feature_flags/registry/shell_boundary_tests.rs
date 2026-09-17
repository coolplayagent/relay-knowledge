use super::*;

#[test]
fn code_index_persistence_performance_suite_shell_ignores_unrelated_statements() {
    let source = format!(
        "{}\nexport FLAG=true\necho $FLAG $EXTERNAL\n",
        "helper() { echo $OTHER; }\necho noise\n".repeat(1100)
    );
    let rows = facts("bash", &source);
    assert!(
        rows.iter()
            .any(|r| r.source_key == "EXTERNAL" && r.edge_kind == "reads_config")
    );
    assert!(
        rows.iter()
            .any(|r| r.source_key == "FLAG" && r.edge_kind == "defines_config")
    );
}

#[test]
fn shell_binding_depth_never_drops_an_enclosing_assignment_silently() {
    let source = format!(
        "{}FLAG=local;{} echo $FLAG",
        "{ ".repeat(130),
        " };".repeat(130)
    );
    let input = FeatureFlagFileInput {
        line_index: Default::default(),
        syntax_root: None,
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path: "test.sh",
        language_id: "bash",
        content: &source,
        config_facts: &[],
    };
    assert!(
        extract(&input)
            .unwrap_err()
            .to_string()
            .contains("depth exceeded")
    );
}

#[test]
fn shell_positional_parameters_do_not_hide_nested_environment_fallbacks() {
    let rows = facts("bash", "echo ${1:-$FALLBACK} ${2-} $@ $$ $?");
    assert!(
        rows.iter()
            .any(|row| row.source_key == "FALLBACK" && row.edge_kind == "reads_config")
    );
    assert!(
        rows.iter()
            .all(|row| !["1", "2", "@", "$", "?"].contains(&row.source_key.as_str()))
    );
}
