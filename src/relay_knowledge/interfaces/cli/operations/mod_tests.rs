//! Direct contracts for operational CLI parsing.

use super::*;

#[test]
fn service_lifecycle_defaults_to_dry_run_and_requires_explicit_execution() {
    let plan = parse_service(&[
        "plan".to_owned(),
        "upgrade".to_owned(),
        "--target-version".to_owned(),
        "1.2.3".to_owned(),
    ])
    .expect("service plan should parse");
    let execute = parse_service(&[
        "lifecycle".to_owned(),
        "upgrade".to_owned(),
        "--execute".to_owned(),
    ])
    .expect("explicit lifecycle execution should parse");

    assert!(matches!(
        plan,
        CliAction::ServicePlan(ServicePlanRequest {
            dry_run: true,
            execute: false,
            target_version: Some(version),
            ..
        }) if version == "1.2.3"
    ));
    assert!(matches!(
        execute,
        CliAction::ServicePlan(ServicePlanRequest {
            dry_run: false,
            execute: true,
            ..
        })
    ));
}

#[test]
fn service_run_parser_keeps_transport_and_web_flags_explicit() {
    assert_eq!(
        parse_service(&[
            "run".to_owned(),
            "--mcp".to_owned(),
            "streamable-http".to_owned(),
            "--web".to_owned(),
        ]),
        Ok(CliAction::ServiceRun {
            mcp: ServiceMcpTransport::StreamableHttp,
            web: true,
        })
    );
    assert_eq!(
        parse_service(&["run".to_owned(), "--mcp".to_owned(), "stdio".to_owned(),])
            .expect_err("unsupported foreground transport should fail"),
        CliError::UnexpectedArgument("stdio".to_owned())
    );
}
