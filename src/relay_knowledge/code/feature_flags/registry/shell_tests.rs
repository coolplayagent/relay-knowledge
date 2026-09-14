use crate::code::feature_flags::registry::test_support::*;
use crate::code::feature_flags::{FeatureFlagFileInput, registry::extract};

#[test]
fn deferred_and_subshell_exports_do_not_define_parent_configuration() {
    for declaration in [
        "(export FLAG=true)",
        "f() { export FLAG=true; }",
        "set -a; (FLAG=true)",
    ] {
        let rows = facts("bash", &format!("{declaration}\necho \"$FLAG\""));
        assert!(
            rows.iter().all(|r| r.edge_kind != "defines_config"),
            "{declaration}: {rows:?}"
        );
    }
    assert!(
        facts("bash", "{ export FLAG=true; }; echo \"$FLAG\"")
            .iter()
            .any(|r| r.edge_kind == "defines_config")
    );
}

#[test]
fn shell_exports_are_definitions_and_local_compound_assignments_do_not_leak() {
    let rows = facts(
        "bash",
        "export FLAG=true\necho \"$FLAG\"\n{ LOCAL=hidden; }; echo \"$LOCAL\"\n",
    );
    assert_eq!(rows.iter().filter(|r| r.source_key == "FLAG").count(), 2);
    assert!(!rows.iter().any(|r| r.source_key == "LOCAL"));
}

#[test]
fn shell_export_status_survives_assignments_and_respects_unexport_and_subshells() {
    for (source, expected) in [
        ("export FLAG=yes; { FLAG=no; }; echo $FLAG", true),
        ("FLAG=yes; export FLAG; echo $FLAG", true),
        ("export FLAG=yes; export -n FLAG; echo $FLAG", false),
        ("( FLAG=no ); echo $FLAG", true),
        ("if true; then FLAG=no; fi; echo $FLAG", true),
    ] {
        let rows = facts("bash", source);
        assert_eq!(
            rows.iter()
                .any(|r| r.source_key == "FLAG" && r.edge_kind == "reads_config"),
            expected,
            "{source}"
        );
    }
}

#[test]
fn later_export_retains_only_the_latest_unconditional_shell_assignment() {
    let rows = facts(
        "bash",
        r#"FLAG=no; FLAG=yes; export FLAG; echo "$FLAG"
      OTHER=old; if test -f marker; then OTHER=new; fi; export OTHER
      GONE=old; unset GONE; export GONE
    "#,
    );
    let definitions = rows
        .iter()
        .filter(|r| r.edge_kind == "defines_config")
        .collect::<Vec<_>>();
    assert_eq!(definitions.len(), 2, "{rows:?}");
    let flag = definitions.iter().find(|r| r.source_key == "FLAG").unwrap();
    assert_eq!(flag.metadata.default_value.as_deref(), Some("yes"));
    assert!(
        rows.iter()
            .any(|r| r.source_key == "FLAG" && r.edge_kind == "reads_config")
    );
}

#[test]
fn allexport_enables_definitions_and_survives_disable_for_existing_exports() {
    for enable in ["set -a", "set -o allexport"] {
        let code =
            format!("{enable}\nFLAG=yes\nset +a\necho \"$FLAG\"\nLOCAL=no\necho \"$LOCAL\"\n");
        let rows = facts("bash", &code);
        assert!(
            rows.iter()
                .any(|r| r.source_key == "FLAG" && r.edge_kind == "defines_config"),
            "{rows:?}"
        );
        assert!(
            rows.iter()
                .any(|r| r.source_key == "FLAG" && r.edge_kind == "reads_config")
        );
        assert!(!rows.iter().any(|r| r.source_key == "LOCAL"));
    }
    let rows = facts("bash", "(set -a)\nLOCAL=no\necho \"$LOCAL\"\n");
    assert!(!rows.iter().any(|r| r.source_key == "LOCAL"));
}

#[test]
fn conditional_shell_override_keeps_the_guaranteed_definition_without_a_default() {
    let rows = facts(
        "bash",
        "FLAG=base; if test -f marker; then FLAG=override; fi; export FLAG; echo \"$FLAG\"",
    );
    let definition = rows
        .iter()
        .find(|r| r.edge_kind == "defines_config" && r.source_key == "FLAG")
        .unwrap();
    assert!(definition.metadata.default_value.is_none());
    assert_eq!(
        definition.metadata.flow_incomplete.as_deref(),
        Some("conditional_reassignment")
    );
    assert!(
        rows.iter()
            .any(|r| r.edge_kind == "reads_config" && r.source_key == "FLAG")
    );
}

#[test]
fn shell_append_assignments_keep_definitions_without_suffix_defaults() {
    for source in [
        "FLAG=base; export FLAG+=suffix",
        "export FLAG+=suffix",
        "FLAG+=suffix; export FLAG",
        "export FLAG=base; FLAG+=suffix",
    ] {
        let rows = facts("bash", source);
        let append = rows
            .iter()
            .find(|r| r.edge_kind == "defines_config" && r.excerpt.contains("+="))
            .unwrap();
        assert!(append.metadata.default_value.is_none(), "{rows:?}");
        assert!(append.metadata.value_type.is_none());
    }
}

#[test]
fn shell_predicate_reads_link_guards_without_marking_branch_body_reads() {
    for source in [
        "if test \"$FEATURE\" = on; then echo \"$BODY\"; fi",
        "if false; then :; elif test \"$FEATURE\" = on; then echo \"$BODY\"; fi",
        "while test \"$FEATURE\" = on; do echo \"$BODY\"; done",
        "until test \"$FEATURE\" = off; do echo \"$BODY\"; done",
        "case \"$FEATURE\" in on) echo \"$BODY\";; esac",
        "for item in $FEATURE; do echo \"$BODY\"; done",
        "test \"$FEATURE\" = on && echo \"$BODY\"",
        "test \"$FEATURE\" = on || echo \"$BODY\"",
    ] {
        let rows = facts("bash", source);
        let read = rows
            .iter()
            .find(|r| r.source_key == "FEATURE" && r.edge_kind == "reads_config")
            .unwrap();
        assert!(
            rows.iter().any(|r| r.edge_kind == "guards_code"
                && r.metadata.read_usage_id.as_deref() == Some(&read.usage_id)),
            "{source}: {rows:?}"
        );
        assert!(
            !rows
                .iter()
                .any(|r| r.source_key == "BODY" && r.edge_kind == "guards_code"),
            "{source}: {rows:?}"
        );
    }
}

#[test]
fn shell_scan_exhaustion_is_explicit_incomplete_analysis() {
    let source = format!(
        "set -a\n{}FLAG=value\necho \"$FLAG\"\n",
        "echo ignored\n".repeat(1100)
    );
    let error = extract(&FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path: "config.sh",
        language_id: "bash",
        content: &source,
        config_facts: &[],
    })
    .map(|_| ())
    .expect_err("scan truncation cannot claim no export");
    assert!(error.to_string().contains("incomplete"));
}

#[test]
fn shell_quote_forms_preserve_static_dollars_and_backslashes() {
    for (value, expected) in [
        ("'$HOME'", Some("$HOME")),
        ("'`command`'", Some("`command`")),
        ("\"\\$HOME\"", Some("$HOME")),
        ("\"\\n\"", Some("\\n")),
        ("'foo'\"bar\"", Some("foobar")),
        ("prefix\\$HOME", Some("prefix$HOME")),
        ("\"$HOME\"", None),
        ("$(command)", None),
    ] {
        let rows = facts("bash", &format!("export FLAG={value}\n"));
        let definition = rows
            .iter()
            .find(|r| r.edge_kind == "defines_config")
            .unwrap();
        assert_eq!(
            definition.metadata.default_value.as_deref(),
            expected,
            "{value}"
        );
    }
}

#[test]
fn conditional_shell_assignments_preserve_potential_inherited_reads() {
    for source in [
        "if test -f local; then FLAG=local; fi; echo $FLAG",
        "if test -f local; then export -n FLAG; fi; echo $FLAG",
    ] {
        let rows = facts("bash", source);
        assert!(
            rows.iter()
                .any(|r| r.source_key == "FLAG" && r.edge_kind == "reads_config")
        );
        assert!(!rows.iter().any(|r| r.edge_kind == "defines_config"));
    }
    let rows = facts(
        "bash",
        "FLAG=local; if test -f local; then FLAG=other; fi; echo $FLAG",
    );
    assert!(!rows.iter().any(|r| r.edge_kind == "reads_config"));
}

#[test]
fn shell_ansi_c_defaults_are_unknown_without_losing_definitions() {
    let rows = facts("bash", r#"export FLAG=$'on\n'; export MIX=pre$'\t'post"#);
    assert_eq!(
        rows.iter()
            .filter(|r| r.edge_kind == "defines_config")
            .count(),
        2
    );
    assert!(
        rows.iter()
            .all(|r| r.metadata.default_value.is_none() && r.metadata.value_type.is_none())
    );
}

#[test]
fn shell_function_exports_do_not_change_variable_export_state() {
    for option in ["-f", "-fn", "-nf"] {
        let rows = facts(
            "bash",
            &format!("FLAG=local; export {option} FLAG; echo $FLAG"),
        );
        assert!(rows.is_empty(), "{option}: {rows:?}");
    }
    let rows = facts("bash", "export FLAG=on; export -f FLAG; echo $FLAG");
    assert!(rows.iter().any(|r| r.edge_kind == "reads_config"));
}

#[test]
fn shell_assignments_keep_previously_enabled_export_attributes() {
    for source in [
        "export FLAG; FLAG=true; echo $FLAG",
        "export FLAG=old; FLAG=true; echo $FLAG",
    ] {
        let rows = facts("bash", source);
        assert!(rows.iter().any(|r| r.edge_kind == "defines_config"
            && r.metadata.default_value.as_deref() == Some("true")));
    }
    for source in [
        "export FLAG; export -n FLAG; FLAG=true",
        "if test -f local; then export FLAG; fi; FLAG=true",
    ] {
        assert!(
            !facts("bash", source)
                .iter()
                .any(|r| r.metadata.default_value.as_deref() == Some("true"))
        );
    }
}

#[test]
fn shell_tilde_expansion_defaults_remain_unknown() {
    let rows = facts(
        "bash",
        "export A=~/cache; export B=first:~user/cache; export C='~/cache'; export D=literal~suffix",
    );
    for key in ["A", "B"] {
        assert!(
            rows.iter()
                .any(|r| r.source_key == key && r.metadata.default_value.is_none())
        );
    }
    assert!(
        rows.iter()
            .any(|r| r.source_key == "C" && r.metadata.default_value.as_deref() == Some("~/cache"))
    );
    assert!(
        rows.iter().any(|r| r.source_key == "D"
            && r.metadata.default_value.as_deref() == Some("literal~suffix"))
    );
}

#[test]
fn shell_prior_assignment_budget_exhaustion_is_explicit() {
    let source = format!("FLAG=value; {}export FLAG", "echo ignored; ".repeat(1100));
    let error = extract(&FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path: "config.sh",
        language_id: "bash",
        content: &source,
        config_facts: &[],
    })
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("prior assignment analysis incomplete")
    );
}

#[test]
fn export_in_brace_groups_finds_enclosing_assignments() {
    for source in [
        "FLAG=base; { export FLAG; }",
        "FLAG=base; { { export FLAG; } }",
    ] {
        let rows = facts("bash", source);
        assert!(rows.iter().any(|r| r.edge_kind == "defines_config"
            && r.source_key == "FLAG"
            && r.metadata.default_value.as_deref() == Some("base")));
    }
    for source in [
        "FLAG=base; { unset FLAG; export FLAG; }",
        "FLAG=base; function f() { export FLAG; }",
    ] {
        let rows = facts("bash", source);
        assert!(!rows.iter().any(|r| r.edge_kind == "defines_config"));
    }
}

#[test]
fn quoted_unset_operands_stop_prior_definition_lookup() {
    for operand in ["'FLAG'", r#""FLAG""#, r#"FL\AG"#] {
        let rows = facts("bash", &format!("FLAG=true; unset {operand}; export FLAG"));
        assert!(
            !rows.iter().any(|r| r.edge_kind == "defines_config"),
            "{operand}: {rows:?}"
        );
    }
    let rows = facts("bash", "FLAG=true; unset -f 'FLAG'; export FLAG");
    assert!(rows.iter().any(|r| r.edge_kind == "defines_config"));
}

#[test]
fn conditional_assignments_clear_defaults_before_reads_but_fixed_assignments_recover() {
    for condition in [
        "if test -f marker; then FLAG=override; fi",
        "test -f marker && FLAG=override",
        "for x in one two; do FLAG=override; done",
    ] {
        let rows = facts(
            "bash",
            &format!(r#"export FLAG=base; {condition}; echo "$FLAG""#),
        );
        assert!(
            rows.iter().any(|r| r.edge_kind == "defines_config"
                && r.metadata.default_value.is_none()
                && r.metadata.flow_incomplete.is_some()),
            "{condition}"
        );
        assert!(
            rows.iter()
                .any(|r| r.edge_kind == "reads_config" && r.metadata.flow_incomplete.is_some())
        );
        let rows = facts(
            "bash",
            &format!(r#"export FLAG=base; {condition}; FLAG=fixed; echo "$FLAG""#),
        );
        assert!(rows.iter().any(|r| r.edge_kind == "defines_config"
            && r.metadata.default_value.as_deref() == Some("fixed")));
        assert!(
            rows.iter()
                .filter(|r| r.edge_kind == "reads_config")
                .all(|r| r.metadata.flow_incomplete.is_none())
        );
    }
}

#[test]
fn conditional_unsets_invalidate_defaults_without_affecting_function_unsets() {
    for unset in ["unset FLAG", "unset 'FLAG'", "unset -v FLAG"] {
        let rows = facts(
            "bash",
            &format!(r#"export FLAG=base; if test -f marker; then {unset}; fi; echo "$FLAG""#),
        );
        assert!(
            rows.iter().any(|r| r.edge_kind == "defines_config"
                && r.metadata.default_value.is_none()
                && r.metadata.flow_incomplete.is_some()),
            "{unset}"
        );
        assert!(
            rows.iter()
                .any(|r| r.edge_kind == "reads_config" && r.metadata.flow_incomplete.is_some())
        );
    }
    let rows = facts(
        "bash",
        r#"export FLAG=base; if test -f marker; then unset -f FLAG; fi; echo "$FLAG""#,
    );
    assert!(rows.iter().all(|r| r.metadata.flow_incomplete.is_none()));
}

#[test]
fn scoped_assignments_satisfy_reads_without_creating_parent_definitions() {
    for body in [
        r#"export FLAG=internal; echo "$FLAG""#,
        r#"FLAG=internal; export FLAG; echo "$FLAG""#,
        r#"set -a; FLAG=internal; echo "$FLAG""#,
    ] {
        for source in [format!("f() {{ {body}; }}"), format!("( {body} )")] {
            assert!(
                facts("bash", &source)
                    .iter()
                    .all(|r| r.source_key != "FLAG"),
                "{source}"
            );
        }
        let conditional = facts("bash", &format!("if test -f marker; then {body}; fi"));
        assert!(conditional.iter().any(|r| r.source_key == "FLAG"
            && r.edge_kind == "defines_config"
            && r.metadata.flow_incomplete.is_some()));
    }
    let rows = facts("bash", r#"f() { export FLAG; echo "$FLAG"; }"#);
    assert!(rows.iter().any(|r| r.edge_kind == "reads_config"));
    let rows = facts(
        "bash",
        r#"if test -f marker; then export FLAG=internal; else echo "$FLAG"; fi"#,
    );
    assert!(rows.iter().any(|r| r.edge_kind == "reads_config"));
}

#[test]
fn parameter_expansions_keep_static_fallbacks_and_unknown_flow() {
    for (expression, expected) in [
        ("${FLAG:-false}", Some("false")),
        ("${FLAG-false}", Some("false")),
        ("${FLAG:=false}", Some("false")),
        ("${FLAG=false}", Some("false")),
        ("${FLAG:=$OTHER}", None),
        ("${FLAG=$OTHER}", None),
        ("${FLAG:-'off mode'}", Some("off mode")),
        ("${FLAG:-}", Some("")),
        ("${FLAG:-$OTHER}", None),
    ] {
        let rows = facts("bash", &format!("echo {expression}"));
        let read = rows
            .iter()
            .find(|r| r.source_key == "FLAG" && r.edge_kind == "reads_config")
            .unwrap();
        assert_eq!(
            read.metadata.default_value.as_deref(),
            expected,
            "{expression}"
        );
        assert_eq!(read.metadata.flow_incomplete.is_some(), expected.is_none());
    }
}

#[test]
fn escaped_parameter_fallbacks_respect_serialized_metadata_budget() {
    let source = format!("echo ${{FLAG:-'{}'}}", "\\".repeat(40000));
    let rows = facts("bash", &source);
    let read = rows
        .iter()
        .find(|r| r.source_key == "FLAG" && r.edge_kind == "reads_config")
        .unwrap();
    assert!(read.metadata.default_value.is_none());
    assert!(read.metadata.flow_incomplete.is_some());
}

#[test]
fn oversized_exported_shell_defaults_retain_incomplete_definitions() {
    for value in ["x".repeat(70000), "\\".repeat(40000)] {
        let rows = facts("bash", &format!("export CERT='{value}'\n"));
        assert_eq!(rows.len(), 1);
        assert!(rows[0].metadata.default_value.is_none());
        assert!(rows[0].metadata.flow_incomplete.is_some());
    }
}
#[test]
fn conditional_allexport_retains_possible_definitions_as_incomplete() {
    for source in [
        "set -a; if test -f marker; then set +a; fi; FLAG=value; echo $FLAG",
        "if test -f marker; then set -a; fi; FLAG=value; echo $FLAG",
    ] {
        let rows = facts("bash", source);
        let definition = rows
            .iter()
            .find(|r| r.source_key == "FLAG" && r.edge_kind == "defines_config")
            .unwrap();
        assert!(definition.metadata.flow_incomplete.is_some(), "{rows:?}");
        assert!(definition.metadata.default_value.is_none());
    }
    let rows = facts(
        "bash",
        "set -a; if test -f marker; then set +a; fi; set -a; FLAG=value",
    );
    assert!(
        rows.iter()
            .any(|r| r.source_key == "FLAG" && r.metadata.flow_incomplete.is_none())
    );
}

#[test]
fn leading_short_circuit_exports_are_definite_but_right_operands_are_conditional() {
    for op in ["&&", "||"] {
        let rows = facts("bash", &format!("export FLAG=true {op} :; echo $FLAG"));
        assert!(
            rows.iter().any(|r| r.edge_kind == "defines_config"
                && r.metadata.default_value.as_deref() == Some("true")),
            "{rows:?}"
        );
        assert!(
            rows.iter()
                .filter(|r| r.source_key == "FLAG")
                .all(|r| r.metadata.flow_incomplete.is_none())
        );
        let rows = facts("bash", &format!(": {op} export FLAG=true; echo $FLAG"));
        assert!(rows.iter().any(|r| r.edge_kind == "defines_config"
            && r.metadata.default_value.is_none()
            && r.metadata.flow_incomplete.is_some()));
        assert!(rows.iter().any(|r| r.metadata.flow_incomplete.is_some()));
    }
    let rows = facts("bash", "set -a && :; FLAG=true; echo $FLAG");
    assert!(rows.iter().any(|r| r.edge_kind == "defines_config"));
}

#[test]
fn bare_exports_use_export_annotations_and_preserve_assignment_value_evidence() {
    for source in [
        "FLAG=true\n# @config domain=payments hot-reload=true\nexport FLAG",
        "FLAG=true; # @config domain=payments hot-reload=true\nexport FLAG",
    ] {
        let rows = facts("bash", source);
        let row = rows
            .iter()
            .find(|r| r.source_key == "FLAG" && r.edge_kind == "defines_config")
            .unwrap();
        assert_eq!(row.metadata.default_value.as_deref(), Some("true"));
        assert_eq!(row.metadata.domain.as_deref(), Some("payments"));
        assert_eq!(row.metadata.hot_reload, Some(true));
        assert!(row.excerpt.contains("FLAG=true"));
    }
}

#[test]
fn conditional_unsets_before_bare_exports_keep_uncertain_definition_evidence() {
    let rows = facts(
        "bash",
        "FLAG=true; if test -f marker; then unset FLAG; fi; export FLAG; echo $FLAG",
    );
    let definition = rows
        .iter()
        .find(|r| r.source_key == "FLAG" && r.edge_kind == "defines_config")
        .unwrap();
    assert!(definition.metadata.default_value.is_none());
    assert!(definition.metadata.flow_incomplete.is_some());
    let rows = facts("bash", "FLAG=true; unset FLAG; export FLAG; echo $FLAG");
    assert!(rows.iter().all(|r| r.edge_kind != "defines_config"));
}

#[test]
fn export_execution_matrix_preserves_possible_parent_definitions() {
    for source in [
        "if test x; then export FLAG=true; fi",
        "FLAG=true; if test x; then export FLAG; fi",
        "while test x; do export FLAG=true; done",
        "for x in one; do export FLAG=true; done",
        "case x in x) export FLAG=true;; esac",
        ": && export FLAG=true",
        ": || export FLAG=true",
    ] {
        let rows = facts("bash", source);
        let definition = rows
            .iter()
            .find(|r| r.source_key == "FLAG" && r.edge_kind == "defines_config")
            .unwrap_or_else(|| panic!("{source}: {rows:?}"));
        assert!(definition.metadata.default_value.is_none(), "{source}");
        assert!(definition.metadata.flow_incomplete.is_some(), "{source}");
    }
}

#[test]
fn quoted_export_option_matrix_matches_unquoted_options() {
    for option in ["-n", "'-n'", "\"-n\"", "-\"n\"", "\\-n"] {
        let rows = facts(
            "bash",
            &format!("FLAG=true; export {option} FLAG; echo \"$FLAG\""),
        );
        assert!(
            !rows.iter().any(|r| r.source_key == "FLAG"),
            "{option}: {rows:?}"
        );
    }
    for command in ["export \"-x\"", "declare '-x'", "export --"] {
        let rows = facts("bash", &format!("{command} FLAG=true; echo $FLAG"));
        assert!(
            rows.iter()
                .any(|r| r.source_key == "FLAG" && r.edge_kind == "defines_config"),
            "{command}: {rows:?}"
        );
    }
}

#[test]
fn quoted_set_option_matrix_preserves_allexport_state() {
    for enable in [
        r#"set "-a""#,
        "set '-a'",
        r#"set -"a""#,
        r#"set \-a"#,
        r#"set "-o" "allexport""#,
    ] {
        let rows = facts("bash", &format!("{enable}; FLAG=true; echo $FLAG"));
        assert!(
            rows.iter()
                .any(|r| r.source_key == "FLAG" && r.edge_kind == "defines_config"),
            "{enable}: {rows:?}"
        );
        for disable in [r#"set "+a""#, r#"set "+o" "allexport""#] {
            let rows = facts(
                "bash",
                &format!("{enable}; {disable}; FLAG=true; echo $FLAG"),
            );
            assert!(
                !rows.iter().any(|r| r.source_key == "FLAG"),
                "{enable}; {disable}: {rows:?}"
            );
        }
    }
    let rows = facts("bash", r#"set "--" "-a"; FLAG=true; echo $FLAG"#);
    assert!(!rows.iter().any(|r| r.source_key == "FLAG"));
}

#[test]
fn allexport_static_word_budget_errors_are_observable() {
    let source = format!("set -{}; FLAG=true", "\"a\"".repeat(1100));
    let error = extract(&FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path: "config.sh",
        language_id: "bash",
        content: &source,
        config_facts: &[],
    })
    .unwrap_err();
    assert!(
        error.to_string().contains("lexical budget exceeded"),
        "{error}"
    );
}

#[test]
fn decoded_builtin_names_preserve_export_definitions_and_disable_state() {
    for export in ["export", "\"export\"", "'export'", "ex\"port\"", "\\export"] {
        for operand in ["FLAG", "\"FLAG\"", "FLAG=true", "\"FLAG=true\""] {
            let rows = facts(
                "bash",
                &format!("FLAG=true; {export} {operand}; echo $FLAG"),
            );
            assert!(
                rows.iter().any(|row| row.source_key == "FLAG"
                    && row.edge_kind == "defines_config"
                    && row.metadata.default_value.as_deref() == Some("true")),
                "{export} {operand}: {rows:?}"
            );
            assert!(
                rows.iter()
                    .any(|row| row.source_key == "FLAG" && row.edge_kind == "reads_config"),
                "{export} {operand}: {rows:?}"
            );
        }
        let rows = facts(
            "bash",
            &format!("FLAG=true; {export} '-n' FLAG; echo $FLAG"),
        );
        assert!(
            !rows.iter().any(|row| row.source_key == "FLAG"),
            "{export}: {rows:?}"
        );
    }
    for unset in ["unset", "\"unset\"", "un'set'", "\\unset"] {
        let rows = facts("bash", &format!("FLAG=true; {unset} FLAG; \"export\" FLAG"));
        assert!(
            !rows.iter().any(|row| row.edge_kind == "defines_config"),
            "{unset}: {rows:?}"
        );
    }
    for export in ["export", "\"export\""] {
        let rows = facts(
            "bash",
            &format!("FLAG=true; {export} -n FLAG; {export} FLAG; echo $FLAG"),
        );
        assert!(
            rows.iter().any(|row| row.source_key == "FLAG"
                && row.edge_kind == "defines_config"
                && row.metadata.default_value.as_deref() == Some("true")),
            "{export}: {rows:?}"
        );
    }
    for source in [
        r#"FLAG=true; "$COMMAND" FLAG"#,
        r#"FLAG=true; echo -x FLAG"#,
        r#"FLAG=true; ("export" FLAG)"#,
        r#"FLAG=true; f() { "export" FLAG; }"#,
    ] {
        assert!(
            !facts("bash", source)
                .iter()
                .any(|row| row.edge_kind == "defines_config"),
            "{source}"
        );
    }
    let rows = facts("bash", r#""export" FLAG=$OTHER; echo $FLAG"#);
    assert!(rows.iter().any(|row| row.source_key == "FLAG"
        && row.edge_kind == "defines_config"
        && row.metadata.default_value.is_none()));
    for operand in [r#""FLAG=$OTHER""#, "FLAG=~", "FLAG=path:~"] {
        let rows = facts("bash", &format!("\"export\" {operand}; echo $FLAG"));
        assert!(
            rows.iter().any(|row| row.source_key == "FLAG"
                && row.edge_kind == "defines_config"
                && row.metadata.default_value.is_none()),
            "{operand}: {rows:?}"
        );
    }
    let rows = facts("bash", r#""export" "FLAG=~""#);
    assert!(
        rows.iter()
            .any(|row| row.source_key == "FLAG"
                && row.metadata.default_value.as_deref() == Some("~"))
    );
    let rows = facts("bash", r#""export" "$NAME=true""#);
    assert!(!rows.iter().any(|row| row.edge_kind == "defines_config"));
    for export in ["export", "\"export\""] {
        let rows = facts(
            "bash",
            &format!("FLAG=false; {export} FLAG=true; export FLAG"),
        );
        let definitions = rows
            .iter()
            .filter(|row| row.source_key == "FLAG" && row.edge_kind == "defines_config")
            .collect::<Vec<_>>();
        assert!(!definitions.is_empty());
        assert!(
            definitions
                .iter()
                .all(|row| row.metadata.default_value.as_deref() == Some("true")),
            "{export}: {rows:?}"
        );
    }
}

#[test]
fn loop_bindings_replace_inherited_values_only_inside_the_body() {
    for keyword in ["for", "select"] {
        let source = format!(
            r#"{keyword} FLAG in "$FLAG" on off; do if test "$FLAG" = on; then echo "$OTHER"; fi; done; echo "$FLAG""#
        );
        let rows = facts("bash", &source);
        let reads = rows
            .iter()
            .filter(|row| row.source_key == "FLAG" && row.edge_kind == "reads_config")
            .collect::<Vec<_>>();
        assert_eq!(reads.len(), 2, "{keyword}: {rows:?}");
        assert!(
            reads
                .iter()
                .any(|row| row.metadata.flow_incomplete.is_none())
        );
        assert!(
            reads
                .iter()
                .any(|row| row.metadata.flow_incomplete.as_deref()
                    == Some("conditional_reassignment"))
        );
        assert!(
            rows.iter()
                .any(|row| row.source_key == "OTHER" && row.edge_kind == "reads_config")
        );
    }
    for source in [
        r#"export FLAG=before; for FLAG in on off; do echo "$FLAG"; done"#,
        r#"for FLAG in on off; do for INNER in "$FLAG"; do echo "$FLAG"; done; done"#,
    ] {
        assert!(
            !facts("bash", source)
                .iter()
                .any(|row| row.source_key == "FLAG" && row.edge_kind == "reads_config"),
            "{source}"
        );
    }
    let rows = facts(
        "bash",
        "FLAG=before; for FLAG in on off; do :; done; export FLAG",
    );
    assert!(rows.iter().any(|row| row.source_key == "FLAG"
        && row.edge_kind == "defines_config"
        && row.metadata.default_value.is_none()
        && row.metadata.flow_incomplete.is_some()));
}

#[test]
fn export_command_word_budget_errors_remain_observable() {
    let source = format!("{} FLAG=true", "\"ex\"".repeat(1100));
    let error = extract(&FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path: "config.sh",
        language_id: "bash",
        content: &source,
        config_facts: &[],
    })
    .unwrap_err();
    assert!(
        error.to_string().contains("lexical budget exceeded"),
        "{error}"
    );
}

#[test]
fn export_prefix_assignments_bind_only_matching_operands_and_latest_values() {
    for export in ["export", "\"export\"", "declare -x"] {
        for prior in ["", "FOO=old; "] {
            for operand in ["FOO", "\"FOO\""] {
                let source = format!("{prior}FOO=bar {export} {operand}; export FOO");
                let rows = facts("bash", &source);
                let definitions = rows
                    .iter()
                    .filter(|row| row.source_key == "FOO" && row.edge_kind == "defines_config")
                    .collect::<Vec<_>>();
                assert!(!definitions.is_empty(), "{source}");
                assert!(
                    definitions
                        .iter()
                        .all(|row| row.metadata.default_value.as_deref() == Some("bar")),
                    "{source}: {rows:?}"
                );
            }
        }
    }
    for source in [
        "FOO=bar export FOO=baz; export FOO",
        "FOO=bar \"export\" FOO=baz; export FOO",
        "FOO=bar FOO=baz export FOO; export FOO",
    ] {
        let rows = facts("bash", source);
        let definitions = rows
            .iter()
            .filter(|row| row.source_key == "FOO" && row.edge_kind == "defines_config")
            .collect::<Vec<_>>();
        assert!(!definitions.is_empty(), "{source}");
        assert!(
            definitions
                .iter()
                .all(|row| row.metadata.default_value.as_deref() == Some("baz")),
            "{source}: {rows:?}"
        );
    }
    for source in [
        "FOO=bar export OTHER; export FOO",
        "FOO=bar export -n FOO; export FOO",
        "FOO=bar echo FOO; export FOO",
        "(FOO=bar export FOO)",
        "f() { FOO=bar export FOO; }",
    ] {
        assert!(
            !facts("bash", source)
                .iter()
                .any(|row| row.source_key == "FOO" && row.edge_kind == "defines_config"),
            "{source}"
        );
    }
}

#[test]
fn prefix_value_reads_observe_prior_prefixes_but_arguments_observe_outer_values() {
    for command in ["export FOO", "\"export\" FOO", "echo"] {
        let source = format!("FOO=$FOO BAR=$FOO {command} \"$FOO\"");
        let rows = facts("bash", &source);
        let positions = source
            .match_indices("$FOO")
            .map(|(offset, _)| offset)
            .collect::<Vec<_>>();
        let mut reads = rows
            .iter()
            .filter(|row| row.source_key == "FOO" && row.edge_kind == "reads_config")
            .map(|row| row.byte_range.start as usize)
            .collect::<Vec<_>>();
        reads.sort_unstable();
        assert_eq!(
            reads,
            vec![positions[0], positions[2]],
            "{source}: {rows:?}"
        );
    }
    for source in [
        "FOO=$OTHER export FOO",
        "if ready; then FOO=bar export FOO; fi",
    ] {
        let rows = facts("bash", source);
        assert!(
            rows.iter().any(|row| row.source_key == "FOO"
                && row.edge_kind == "defines_config"
                && row.metadata.default_value.is_none()),
            "{source}: {rows:?}"
        );
    }
}
