//! Full-parser recovery is exercised only after a proven native syntax error.
use super::*;
use crate::domain::{CodeParseStatus, CodeRepositoryRegistration};

const COMPLEX_TEMPLATE: &str = "# 模板\r\n{{/* ignored {{ key \"fake\" }}\r\n */}}\r\n  {{- key\r\n \"feature_x\" -}}\r\n{{ keyOrDefault\r\n \"quoted\" \"a}}b\\\"c\" }}\r\n{{ keyOrDefault\r\n \"raw\" `x}}y\r\nz` }}\r\n{{ env\r\n \"ENV_SWITCH\" }}\r\n";

#[test]
fn quoted_go_delimiters_recover_but_unclosed_actions_remain_partial() {
    assert_recovery_status(COMPLEX_TEMPLATE, CodeParseStatus::Parsed);
    assert_recovery_status("{{ key \"unterminated }}", CodeParseStatus::Partial);
    assert_recovery_status(
        &format!("{COMPLEX_TEMPLATE}{{{{/* unfinished }}}}"),
        CodeParseStatus::Partial,
    );
}

#[test]
fn malformed_go_actions_cannot_use_quoted_delimiter_recovery() {
    for action in [
        "{{ key \"x\", \"y\" }}",
        "{{ define \"x\" \"extra\" }}body{{ end }}",
        "{{ block \"x\" }}body{{ end }}",
        "{{ $missing }}",
        "{{ \"x\" | \"y\" }}",
        "{{-key \"x\"}}",
    ] {
        assert_recovery_status(
            &format!("{COMPLEX_TEMPLATE}{action}"),
            CodeParseStatus::Partial,
        );
    }
}

fn assert_recovery_status(source: &str, expected: CodeParseStatus) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_jinja2::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(
        tree.root_node().has_error(),
        "fixture must reach manual recovery, not native Parsed: {source}"
    );
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .unwrap();
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    parse_indexed_file(&mut build, "config.ctmpl", source.as_bytes()).unwrap();
    assert_eq!(build.finish().files[0].parse_status, expected, "{source}");
}

#[test]
fn malformed_numeric_go_action_keeps_native_syntax_diagnostic() {
    for word in ["123abc", "18446744073709551616", "0x10000000000000000"] {
        assert_recovery_status(
            &format!("{COMPLEX_TEMPLATE}{{{{ {word} }}}}"),
            CodeParseStatus::Partial,
        );
    }
}

#[test]
fn declared_go_variables_recover_without_accepting_unbound_uses() {
    for (action, status) in [
        ("{{ $flag := key \"feature_x\" }}", CodeParseStatus::Parsed),
        ("{{ $1bad := key \"feature_x\" }}", CodeParseStatus::Parsed),
        ("{{ $missing }}", CodeParseStatus::Partial),
    ] {
        assert_recovery_status(&format!("{COMPLEX_TEMPLATE}{action}"), status);
    }
}
