use crate::code::feature_flags::registry::test_support::*;
use crate::code::feature_flags::{FeatureFlagFileInput, registry::extract};

#[test]
fn conditional_deferred_and_subshell_exports_do_not_define_parent_configuration() {
    for declaration in [
        "(export FLAG=true)",
        "f() { export FLAG=true; }",
        "if test x; then export FLAG=true; fi",
        "FLAG=true; if test x; then export FLAG; fi",
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
