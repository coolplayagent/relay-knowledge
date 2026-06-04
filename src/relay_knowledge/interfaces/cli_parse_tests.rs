use super::*;

#[test]
fn parses_index_and_service_actions() {
    let index = CliCommand::parse(["index", "refresh", "--kind", "bm25"])
        .expect("index command should parse");
    let service = CliCommand::parse(["service", "doctor"]).expect("service command should parse");
    let run = CliCommand::parse(["service", "run", "--mcp", "streamable-http"])
        .expect("service run should parse");

    assert_eq!(
        index.action,
        CliAction::IndexRefresh {
            kinds: vec![IndexKind::Bm25],
        }
    );
    assert_eq!(service.action, CliAction::ServiceStatus);
    assert_eq!(
        run.action,
        CliAction::ServiceRun {
            mcp: ServiceMcpTransport::StreamableHttp,
            web: false,
        }
    );
}

#[test]
fn parses_service_run_with_web_and_mcp() {
    let web = CliCommand::parse(["service", "run", "--web"]).expect("web service should parse");
    let combined = CliCommand::parse(["service", "run", "--web", "--mcp", "streamable-http"])
        .expect("combined service should parse");

    assert_eq!(
        web.action,
        CliAction::ServiceRun {
            mcp: ServiceMcpTransport::Configured,
            web: true,
        }
    );
    assert_eq!(
        combined.action,
        CliAction::ServiceRun {
            mcp: ServiceMcpTransport::StreamableHttp,
            web: true,
        }
    );
}
