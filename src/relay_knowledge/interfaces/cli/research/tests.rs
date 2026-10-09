use super::*;

fn args(value: &str) -> Vec<String> {
    value.split_whitespace().map(str::to_owned).collect()
}

#[test]
fn parses_every_explicit_workflow_and_rejects_missing_or_duplicate_authority() {
    for (namespace, command, runtime) in [
        ("sources", "audit --root . --input sources.json", false),
        (
            "evidence",
            "validate --root . --input bundle.json --scope research",
            false,
        ),
        (
            "evidence",
            "import --root . --input bundle.json --scope research",
            true,
        ),
        (
            "evidence",
            "export --id study --scope research --revision abc",
            true,
        ),
        (
            "evidence",
            "view --root . --input bundle.json --scope research --focus a",
            false,
        ),
        (
            "evidence",
            "impact --root . --input bundle.json --scope research --node a --label New",
            false,
        ),
        (
            "research",
            "status --root . --delivery archive --catalog sources.json --requirements requirements.json",
            true,
        ),
        (
            "research",
            "status --root . --delivery authored_graph --bundle b.json --scope research",
            true,
        ),
        ("research", "status --root . --delivery graphrag", true),
    ] {
        let CliAction::Research(parsed) = parse(namespace, &args(command)).unwrap() else {
            panic!("wrong command");
        };
        assert_eq!(parsed.needs_runtime(), runtime);
    }
    for (namespace, command) in [
        ("sources", "audit --input a.json"),
        ("sources", "audit --root . --root . --input a.json"),
        ("sources", "audit --root . --input"),
        ("sources", "audit --root . --input a --guess true"),
        ("evidence", "validate --root . --input a"),
        ("evidence", "erase"),
        ("research", "status --root . --delivery complete"),
        ("unknown", ""),
    ] {
        assert!(
            parse(namespace, &args(command)).is_err(),
            "{namespace} {command}"
        );
    }
}

#[tokio::test]
async fn graph_commands_require_the_runtime_and_offline_commands_report_input_errors() {
    for (namespace, command) in [
        (
            "evidence",
            "import --root . --input bundle.json --scope research",
        ),
        (
            "evidence",
            "export --id study --scope research --revision abc",
        ),
        ("research", "status --root . --delivery archive"),
        ("sources", "audit --root /missing-research-root --input a"),
        (
            "evidence",
            "validate --root /missing-research-root --input a --scope research",
        ),
        (
            "evidence",
            "view --root /missing-research-root --input a --scope research",
        ),
        (
            "evidence",
            "impact --root /missing-research-root --input a --scope research --node a --label New",
        ),
    ] {
        let CliAction::Research(command) = parse(namespace, &args(command)).unwrap() else {
            panic!()
        };
        assert!(
            run(
                command,
                None,
                RequestContext::for_interface(crate::api::InterfaceKind::Cli),
                OutputFormat::Json
            )
            .await
            .is_err()
        );
    }
}
