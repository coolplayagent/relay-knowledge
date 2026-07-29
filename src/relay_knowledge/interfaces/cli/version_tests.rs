use super::*;

#[test]
fn version_notice_skips_machine_readable_and_version_commands() {
    assert!(!should_check_after_command(&CliCommand {
        action: CliAction::Version,
        format: OutputFormat::Text,
        remote_base_url: None,
        help: false,
    }));
    assert!(!should_check_after_command(&CliCommand {
        action: CliAction::Status,
        format: OutputFormat::Json,
        remote_base_url: None,
        help: false,
    }));
    assert!(should_check_after_command(&CliCommand {
        action: CliAction::Status,
        format: OutputFormat::Text,
        remote_base_url: None,
        help: false,
    }));
}
