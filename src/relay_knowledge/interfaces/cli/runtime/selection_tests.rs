use super::*;
use crate::interfaces::cli::CliCommand;

#[test]
fn environment_urls_only_select_remote_capable_commands() {
    let environment_url = Some("http://127.0.0.1:8791".to_owned());
    let status = CliCommand::parse(["status"]).expect("status should parse");
    let repo_status =
        CliCommand::parse(["repo", "status", "org/repo"]).expect("repo status should parse");
    let repo_software =
        CliCommand::parse(["repo", "software", "org/repo", "--kind", "relationships"])
            .expect("repo software should parse");
    let repo_reset = CliCommand::parse(["repo", "index", "--reset", "org/repo"])
        .expect("repo reset should parse");

    assert!(!remote_environment_needed(&status));
    assert!(remote_environment_needed(&repo_status));
    assert!(remote_environment_needed(&repo_software));
    assert!(remote_environment_needed(&repo_reset));
    assert_eq!(
        select_remote_base_url(&status, environment_url.clone()),
        None
    );
    assert_eq!(
        select_remote_base_url(&repo_status, environment_url.clone()),
        environment_url
    );
}

#[test]
fn explicit_remote_urls_take_precedence_for_every_command() {
    let command = CliCommand::parse(["--remote", "http://127.0.0.1:9000", "status"])
        .expect("explicit remote status should parse");

    assert!(remote_environment_needed(&command));
    assert_eq!(
        select_remote_base_url(&command, Some("http://127.0.0.1:8791".to_owned())).as_deref(),
        Some("http://127.0.0.1:9000")
    );
}
