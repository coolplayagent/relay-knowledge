use super::*;
use crate::interfaces::cli::OutputFormat;

#[tokio::test]
async fn process_free_help_and_version_actions_skip_runtime_configuration() {
    let help = run_command(CliCommand {
        action: CliAction::Help {
            path: vec!["repo".to_owned(), "query".to_owned()],
        },
        format: OutputFormat::Json,
        remote_base_url: None,
        help: true,
    })
    .await
    .expect("help should render");
    let version = run_command(CliCommand {
        action: CliAction::Version,
        format: OutputFormat::Text,
        remote_base_url: None,
        help: false,
    })
    .await
    .expect("version should render");
    let help: serde_json::Value = serde_json::from_str(help.trim()).expect("help should be JSON");

    assert_eq!(help["path"], serde_json::json!(["repo", "query"]));
    assert!(version.starts_with("relay-knowledge "));
}

#[test]
fn unsupported_remote_actions_return_a_stable_capability_error() {
    assert_eq!(
        remote_unsupported_error(),
        CliError::ApiFailed(
            "remote CLI mode supports repo list, repo index, repo scope preview, repo status, repo query, repo context, repo feature-flags, repo impact, repo report, repo software, and repo view"
                .to_owned()
        )
    );
}
