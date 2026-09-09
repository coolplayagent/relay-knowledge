use super::*;

#[test]
fn recovery_accepts_go_literals_comments_pipelines_and_balanced_control_actions() {
    for content in [
        "{{ keyOrDefault \"flag\" \"a}}b\" }}",
        "{{/* ignored }} {{ \" */}}{{ keyOrDefault `raw` `a}}b\r\nc` }}",
        "{{- if .Values.enabled -}}ready{{- else if .Values.other -}}other{{- else -}}none{{- end -}}",
        "{{ with (key \"flag\") }}{{ . }}{{ end }}",
        "{{ range .Values }}{{ break }}{{ else }}none{{ end }}",
        "{{ define \"name\" }}body{{ end }}{{ template \"name\" . }}",
        "{{ block \"name\" . }}body{{ end }}",
        "{{ keyOrDefault \"bytes\" \"\\xff\" }}",
        "plain static text",
    ] {
        assert!(balanced(content), "{content}");
    }
}

#[test]
fn recovery_rejects_incomplete_literals_comments_commands_and_control_blocks() {
    for content in [
        "{{ key \"unterminated }}",
        "{{ key `unterminated }}",
        "{{/* unfinished }}",
        "{{ (key \"flag\" }}",
        "{{ key \"flag\" | }}",
        "{{ key \"flag\" || key \"x\" }}",
        "{{ if true }}",
        "{{ if | key \"flag\" }}{{ end }}",
        "{{ end }}",
        "{{ if true }}{{ else }}{{ else }}{{ end }}",
        "{{ range . }}{{ else if true }}{{ end }}",
        "{{ key \"\\q\" }}",
        "{{ @@@ }}",
        "{{ }}",
        "text }}",
        "{{ key /* comment */ \"flag\" }}",
        "{{ value := \"not_variable\" }}",
        "{{ key \"x\", \"y\" }}",
        "{{ define \"x\" \"extra\" }}body{{ end }}",
        "{{ block \"x\" }}body{{ end }}",
        "{{ $missing }}",
        "{{ $x = \"x\" }}",
        "{{ \"x\" | \"y\" }}",
        "{{-key \"x\"}}",
        "{{ break }}",
    ] {
        assert!(!balanced(content), "{content}");
    }
}

#[test]
fn recovery_work_and_nesting_bounds_fail_closed() {
    let oversized = format!("{{{{ key \"flag\" {} }}}}", " ".repeat(65_536));
    assert!(!balanced(&oversized));
    assert!(!balanced(&format!(
        "{}{}",
        "{{ if true }}".repeat(129),
        "{{ end }}".repeat(129)
    )));
    assert!(!balanced(&format!(
        "{{{{ {}\"value\"{} }}}}",
        "(".repeat(129),
        ")".repeat(129)
    )));
}
