use relay_knowledge::{
    api::{InterfaceKind, RequestContext},
    application::RelayKnowledgeService,
    env::{EnvironmentConfig, PlatformKind},
};

#[test]
fn cli_and_web_can_use_the_same_application_service() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
        ],
    )
    .expect("environment should parse");
    let service =
        RelayKnowledgeService::from_environment(&environment).expect("service should compose");
    let cli_context = RequestContext::with_ids(InterfaceKind::Cli, "req-cli", "trace-cli");
    let web_context = RequestContext::with_ids(InterfaceKind::Web, "req-web", "trace-web");

    let cli_response = service.project_status(cli_context);
    let web_response = service.project_status(web_context);

    assert_eq!(cli_response.project_name, "relay-knowledge");
    assert_eq!(web_response.project_name, "relay-knowledge");
    assert_eq!(cli_response.metadata.graph_version, 0);
    assert_eq!(web_response.metadata.graph_version, 0);
    assert_eq!(cli_response.metadata.trace_id, "trace-cli");
    assert_eq!(web_response.metadata.trace_id, "trace-web");
    assert_eq!(cli_response.runtime.config_dir, "/srv/relay/config");
    assert_eq!(web_response.runtime.http_bind, "127.0.0.1:8791");
}
