//! Direct unit contract for platform service definitions and manager commands.

use std::path::Path;

use super::*;

#[test]
fn platform_service_specs_keep_manager_commands_and_definition_formats_distinct() {
    let definition_path = Path::new("/tmp/relay knowledge.service");
    let binary_path = Path::new("/opt/relay knowledge/relay-knowledge");

    let linux = install_command("linux", definition_path, binary_path);
    let macos = install_command("macos", definition_path, binary_path);
    let windows = install_command("windows", definition_path, binary_path);

    assert_eq!(linux.first().map(String::as_str), Some("systemctl"));
    assert_eq!(macos.first().map(String::as_str), Some("launchctl"));
    assert_eq!(windows.first().map(String::as_str), Some("powershell"));
    assert!(
        render_definition("linux", "/opt/relay knowledge", "/tmp/data")
            .contains("ExecStart=\"/opt/relay knowledge\"")
    );
    assert!(render_definition("macos", "/opt/relay", "/tmp/data").contains("<plist"));
    assert!(render_definition("windows", "C:\\relay.exe", "C:\\data").contains("<service>"));
}
