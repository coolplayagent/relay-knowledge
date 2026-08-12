use std::path::{Path, PathBuf};

use crate::{
    domain::ServicePermissionRequirement,
    env::{
        RELAY_KNOWLEDGE_DATA_DIR, RELAY_KNOWLEDGE_WATCHER_COMMIT_RECONCILE_INTERVAL_MS,
        RELAY_KNOWLEDGE_WATCHER_ENABLED,
    },
    project::{
        LINUX_SERVICE_DEFINITION_FILE_NAME, MACOS_SERVICE_DEFINITION_FILE_NAME, PROJECT_NAME,
        WINDOWS_SERVICE_DEFINITION_FILE_NAME,
    },
    watcher::WatcherConfig,
};

pub(super) fn current_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

pub(super) fn binary_path(
    platform: &str,
    install_dir: Option<&Path>,
    current_exe: &Path,
) -> PathBuf {
    match install_dir {
        Some(dir) => dir.join(binary_filename(platform)),
        None => current_exe.to_path_buf(),
    }
}

fn binary_filename(platform: &str) -> &'static str {
    if platform == "windows" {
        "relay-knowledge.exe"
    } else {
        PROJECT_NAME
    }
}

pub(super) fn service_definition_filename(platform: &str) -> &'static str {
    match platform {
        "windows" => WINDOWS_SERVICE_DEFINITION_FILE_NAME,
        "macos" => MACOS_SERVICE_DEFINITION_FILE_NAME,
        _ => LINUX_SERVICE_DEFINITION_FILE_NAME,
    }
}

pub(super) fn permission_requirements(platform: &str) -> Vec<ServicePermissionRequirement> {
    match platform {
        "windows" => vec![ServicePermissionRequirement {
            scope: "administrator".to_owned(),
            reason: "Windows Service registration and removal require an elevated PowerShell session."
                .to_owned(),
        }],
        "macos" => vec![ServicePermissionRequirement {
            scope: "launchd-user-domain".to_owned(),
            reason: "launchctl must register, start, stop, and unload the user launchd service."
                .to_owned(),
        }],
        _ => vec![ServicePermissionRequirement {
            scope: "systemd-user-manager".to_owned(),
            reason: "systemctl --user manages the resident service without an unmanaged background loop."
                .to_owned(),
        }],
    }
}

pub(super) fn render_definition(platform: &str, executable: &str, data_dir: &str) -> String {
    let reconcile_interval_ms = WatcherConfig::DEFAULT_COMMIT_RECONCILE_INTERVAL_MS;
    match platform {
        "windows" => format!(
            "<service><id>{name}</id><name>{name}</name><executable>{executable}</executable><arguments>service run --web --mcp streamable-http</arguments><env name=\"{data_dir_name}\" value=\"{data_dir}\"/><env name=\"{watcher_enabled_name}\" value=\"true\"/><env name=\"{reconcile_name}\" value=\"{reconcile_interval_ms}\"/></service>\n",
            name = PROJECT_NAME,
            executable = xml_escape(executable),
            data_dir_name = RELAY_KNOWLEDGE_DATA_DIR,
            data_dir = xml_escape(data_dir),
            watcher_enabled_name = RELAY_KNOWLEDGE_WATCHER_ENABLED,
            reconcile_name = RELAY_KNOWLEDGE_WATCHER_COMMIT_RECONCILE_INTERVAL_MS,
        ),
        "macos" => format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>Label</key><string>{label}</string><key>ProgramArguments</key><array><string>{executable}</string><string>service</string><string>run</string><string>--web</string><string>--mcp</string><string>streamable-http</string></array><key>EnvironmentVariables</key><dict><key>{data_dir_name}</key><string>{data_dir}</string><key>{watcher_enabled_name}</key><string>true</string><key>{reconcile_name}</key><string>{reconcile_interval_ms}</string></dict><key>RunAtLoad</key><true/></dict></plist>\n",
            label = launchd_label(),
            executable = xml_escape(executable),
            data_dir_name = RELAY_KNOWLEDGE_DATA_DIR,
            data_dir = xml_escape(data_dir),
            watcher_enabled_name = RELAY_KNOWLEDGE_WATCHER_ENABLED,
            reconcile_name = RELAY_KNOWLEDGE_WATCHER_COMMIT_RECONCILE_INTERVAL_MS,
        ),
        _ => {
            let data_environment = format!("{RELAY_KNOWLEDGE_DATA_DIR}={data_dir}");
            let watcher_environment = format!("{RELAY_KNOWLEDGE_WATCHER_ENABLED}=true");
            let reconcile_environment = format!(
                "{RELAY_KNOWLEDGE_WATCHER_COMMIT_RECONCILE_INTERVAL_MS}={reconcile_interval_ms}"
            );
            format!(
                "[Unit]\nDescription=relay-knowledge background service\nAfter=network-online.target\n\n[Service]\nType=simple\nExecStart={executable} service run --web --mcp streamable-http\nEnvironment={data_environment}\nEnvironment={watcher_environment}\nEnvironment={reconcile_environment}\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
                executable = systemd_quote(executable),
                data_environment = systemd_quote(&data_environment),
                watcher_environment = systemd_quote(&watcher_environment),
                reconcile_environment = systemd_quote(&reconcile_environment),
            )
        }
    }
}

pub(super) fn install_command(
    platform: &str,
    definition_path: &Path,
    binary_path: &Path,
) -> Vec<String> {
    match platform {
        "windows" => vec![
            "powershell".to_owned(),
            "-NoProfile".to_owned(),
            "-ExecutionPolicy".to_owned(),
            "Bypass".to_owned(),
            "-Command".to_owned(),
            windows_install_service_script(binary_path),
        ],
        "macos" => vec![
            "launchctl".to_owned(),
            "load".to_owned(),
            definition_path.display().to_string(),
        ],
        _ => vec![
            "systemctl".to_owned(),
            "--user".to_owned(),
            "enable".to_owned(),
            definition_path.display().to_string(),
        ],
    }
}

fn windows_install_service_script(binary_path: &Path) -> String {
    let binary_path_name = format!(
        "\"{}\" service run --web --mcp streamable-http",
        binary_path.display()
    );
    format!(
        "$ErrorActionPreference = 'Stop'; New-Service -Name {name} -BinaryPathName {binary_path_name} -DisplayName {name} -StartupType Automatic -ErrorAction Stop",
        name = powershell_quote(PROJECT_NAME),
        binary_path_name = powershell_quote(&binary_path_name)
    )
}

pub(super) fn windows_configure_environment_command(definition_path: &Path) -> Vec<String> {
    vec![
        "powershell".to_owned(),
        "-NoProfile".to_owned(),
        "-ExecutionPolicy".to_owned(),
        "Bypass".to_owned(),
        "-Command".to_owned(),
        windows_configure_environment_script(definition_path),
    ]
}

fn windows_configure_environment_script(definition_path: &Path) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'; [xml]$definition = Get-Content -Raw -Path {definition_path}; $serviceEnvironment = @($definition.service.env | ForEach-Object {{ \"$($_.name)=$($_.value)\" }}); if ($serviceEnvironment.Count -eq 0) {{ throw 'service definition contains no environment values' }}; New-ItemProperty -Path {registry_path} -Name Environment -PropertyType MultiString -Value $serviceEnvironment -Force -ErrorAction Stop | Out-Null",
        definition_path = powershell_quote(&definition_path.display().to_string()),
        registry_path = powershell_quote(&format!(
            "HKLM:\\SYSTEM\\CurrentControlSet\\Services\\{}",
            PROJECT_NAME
        ))
    )
}

pub(super) fn windows_refresh_registration_command(definition_path: &Path) -> Vec<String> {
    vec![
        "powershell".to_owned(),
        "-NoProfile".to_owned(),
        "-ExecutionPolicy".to_owned(),
        "Bypass".to_owned(),
        "-Command".to_owned(),
        windows_refresh_registration_script(definition_path),
    ]
}

fn windows_refresh_registration_script(definition_path: &Path) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'; [xml]$definition = Get-Content -Raw -Path {definition_path}; $binaryPathName = '\"' + $definition.service.executable + '\" ' + $definition.service.arguments; & sc.exe config {name} binPath= $binaryPathName; if ($LASTEXITCODE -ne 0) {{ throw \"sc.exe config failed with exit code $LASTEXITCODE\" }}; {environment_script}",
        definition_path = powershell_quote(&definition_path.display().to_string()),
        name = powershell_quote(PROJECT_NAME),
        environment_script = windows_configure_environment_script(definition_path)
    )
}

fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn systemd_quote(value: &str) -> String {
    format!("\"{}\"", systemd_escape(value))
}

fn systemd_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
        .replace('$', "$$")
}

pub(super) fn uninstall_command(platform: &str, definition_path: &Path) -> Vec<String> {
    match platform {
        "windows" => vec![
            "sc.exe".to_owned(),
            "delete".to_owned(),
            PROJECT_NAME.to_owned(),
        ],
        "macos" => vec![
            "launchctl".to_owned(),
            "unload".to_owned(),
            definition_path.display().to_string(),
        ],
        _ => vec![
            "systemctl".to_owned(),
            "--user".to_owned(),
            "disable".to_owned(),
            "--now".to_owned(),
            LINUX_SERVICE_DEFINITION_FILE_NAME.to_owned(),
        ],
    }
}

pub(super) fn start_command(platform: &str) -> Vec<String> {
    match platform {
        "windows" => vec![
            "powershell".to_owned(),
            "-NoProfile".to_owned(),
            "-Command".to_owned(),
            format!(
                "$ErrorActionPreference = 'Stop'; Start-Service -Name '{}' -ErrorAction Stop",
                PROJECT_NAME
            ),
        ],
        "macos" => vec!["launchctl".to_owned(), "start".to_owned(), launchd_label()],
        _ => vec![
            "systemctl".to_owned(),
            "--user".to_owned(),
            "start".to_owned(),
            LINUX_SERVICE_DEFINITION_FILE_NAME.to_owned(),
        ],
    }
}

pub(super) fn stop_command(platform: &str) -> Vec<String> {
    match platform {
        "windows" => vec![
            "powershell".to_owned(),
            "-NoProfile".to_owned(),
            "-Command".to_owned(),
            format!(
                "$ErrorActionPreference = 'Stop'; Stop-Service -Name '{}' -ErrorAction Stop",
                PROJECT_NAME
            ),
        ],
        "macos" => vec!["launchctl".to_owned(), "stop".to_owned(), launchd_label()],
        _ => vec![
            "systemctl".to_owned(),
            "--user".to_owned(),
            "stop".to_owned(),
            LINUX_SERVICE_DEFINITION_FILE_NAME.to_owned(),
        ],
    }
}

fn launchd_label() -> String {
    MACOS_SERVICE_DEFINITION_FILE_NAME
        .strip_suffix(".plist")
        .unwrap_or(MACOS_SERVICE_DEFINITION_FILE_NAME)
        .to_owned()
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
#[path = "platform_service_tests.rs"]
mod tests;
