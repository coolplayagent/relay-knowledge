//! Native Windows identity, bounded path probes, and private ACL provisioning.

use std::{path::Path, time::Duration};

use super::{PathError, PathErrorKind, PathPurpose, StorageDirectoryAccess};

const SECURITY_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(windows)]
static PROBE_EXECUTABLE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Registers the immutable native worker executable during host bootstrap.
/// Embedded hosts must point to the relay-knowledge CLI, which implements the
/// worker dispatch. This avoids recursively launching an arbitrary library host.
#[cfg(windows)]
pub fn initialize_windows_probe_executable(
    executable: std::path::PathBuf,
) -> Result<(), PathError> {
    super::validate_path(PathPurpose::Runtime, &executable)?;
    if let Err(executable) = PROBE_EXECUTABLE.set(executable) {
        if PROBE_EXECUTABLE.get() != Some(&executable) {
            return Err(security_error(
                "Windows path probe executable is already configured",
            ));
        }
    }
    Ok(())
}

pub(super) fn current_sid() -> Result<String, PathError> {
    #[cfg(windows)]
    {
        use winsafe::{co, prelude::*};
        let token = winsafe::HPROCESS::GetCurrentProcess()
            .OpenProcessToken(co::TOKEN::QUERY)
            .map_err(|error| security_error(error.to_string()))?;
        let information = token
            .GetTokenInformation(co::TOKEN_INFORMATION_CLASS::User)
            .map_err(|error| security_error(error.to_string()))?;
        let winsafe::TokenInfo::User(user) = information else {
            return Err(security_error(
                "Windows returned unexpected token information",
            ));
        };
        let sid = winsafe::ConvertSidToStringSid(
            user.User
                .Sid()
                .ok_or_else(|| security_error("Windows token has no account SID"))?,
        )
        .map_err(|error| security_error(error.to_string()))?;
        validate_sid(&sid)?;
        Ok(sid)
    }
    #[cfg(not(windows))]
    Err(security_error(
        "automatic Windows storage requires a Windows host",
    ))
}

pub(super) fn validate_sid(sid: &str) -> Result<(), PathError> {
    let Some(tail) = sid.strip_prefix("S-1-") else {
        return Err(security_error("Windows returned an invalid account SID"));
    };
    let components: Vec<_> = tail.split('-').take(17).collect();
    let valid = (2..=16).contains(&components.len())
        && components.iter().enumerate().all(|(index, value)| {
            !value.is_empty()
                && value.bytes().all(|byte| byte.is_ascii_digit())
                && (value.len() == 1 || !value.starts_with('0'))
                && value.parse::<u64>().is_ok_and(|number| {
                    number
                        <= if index == 0 {
                            (1 << 48) - 1
                        } else {
                            u64::from(u32::MAX)
                        }
                })
        });
    if !valid {
        return Err(security_error("Windows returned an invalid account SID"));
    }
    Ok(())
}

pub(super) async fn prepare_private_directory(
    path: &Path,
    sid: &str,
    access: StorageDirectoryAccess,
    database_path: Option<&Path>,
) -> Result<(), PathError> {
    validate_sid(sid)?;
    let path = path
        .to_str()
        .filter(|value| !value.contains('\0') && value.len() <= 4096)
        .ok_or_else(|| {
            security_error("Windows data path must be valid Unicode and at most 4096 bytes")
        })?;
    let existing_only = if access == StorageDirectoryAccess::ExistingOnly {
        " -ExistingOnly"
    } else {
        ""
    };
    let command = format!(
        "Initialize-RelayPrivateStorage -DataPath '{}' -ExpectedSid '{sid}'{existing_only}{}; 'secured'",
        path.replace('\'', "''"),
        database_path
            .map(|path| {
                let path = path
                    .to_str()
                    .filter(|value| !value.contains('\0') && value.len() <= 4096)
                    .ok_or_else(|| {
                        security_error(
                            "Windows database path must be Unicode and at most 4096 bytes",
                        )
                    })?;
                Ok::<_, PathError>(format!(" -DatabasePath '{}'", path.replace('\'', "''")))
            })
            .transpose()?
            .unwrap_or_default()
    );
    if run_security_script(&command, SECURITY_COMMAND_TIMEOUT).await? != "secured" {
        return Err(security_error(
            "unexpected Windows storage security response",
        ));
    }
    Ok(())
}

pub(super) async fn validate_service_database_path(
    path: &Path,
    access: StorageDirectoryAccess,
) -> Result<(), PathError> {
    let path = path
        .to_str()
        .filter(|value| !value.contains('\0') && value.len() <= 4096)
        .ok_or_else(|| {
            security_error("Windows database path must be Unicode and at most 4096 bytes")
        })?;
    let existing_only = if access == StorageDirectoryAccess::ExistingOnly {
        " -ExistingOnly"
    } else {
        ""
    };
    let command = format!(
        "Assert-RelayServiceDatabasePath -DatabasePath '{}'{}; 'secured'",
        path.replace('\'', "''"),
        existing_only
    );
    if run_security_script(&command, SECURITY_COMMAND_TIMEOUT).await? != "secured" {
        return Err(security_error(
            "unexpected service storage validation response",
        ));
    }
    Ok(())
}

#[cfg(windows)]
pub(super) async fn validate_service_inspection_tree(database: &Path) -> Result<(), PathError> {
    let database = database
        .to_str()
        .filter(|value| !value.contains('\0') && value.len() <= 4096)
        .ok_or_else(|| {
            security_error("Windows database path must be Unicode and at most 4096 bytes")
        })?;
    let command = format!(
        "$database = [System.IO.FileInfo]::new('{}'); Assert-RelayServiceDatabasePath $database.FullName -ExistingOnly; Assert-RelayStoragePayloadTree $database.Directory '' -ReparseOnly; 'secured'",
        database.replace('\'', "''")
    );
    if run_security_script(&command, SECURITY_COMMAND_TIMEOUT).await? != "secured" {
        return Err(security_error(
            "unexpected service inspection validation response",
        ));
    }
    Ok(())
}

#[cfg(windows)]
pub(super) async fn probe_path(path: &Path) -> Result<Option<bool>, PathError> {
    let path = path
        .to_str()
        .filter(|value| !value.contains('\0') && value.len() <= 4096)
        .ok_or_else(|| {
            security_error("Windows probe path must be Unicode and at most 4096 bytes")
        })?;
    // Reuse the trusted running binary as a disposable native filesystem worker.
    // It performs no runtime/service initialization and needs no script engine.
    #[cfg(not(test))]
    let executable = PROBE_EXECUTABLE.get().ok_or_else(|| {
        security_error(
            "host must initialize the Windows path probe executable before resolving storage",
        )
    })?;
    #[cfg(test)]
    let executable = std::env::current_exe().map_err(|error| security_error(error.to_string()))?;
    let mut command = tokio::process::Command::new(executable);
    command.env_clear();
    #[cfg(not(test))]
    command.args(["--internal-windows-storage-probe", path]);
    #[cfg(test)]
    command
        .args([
            "--exact",
            "paths::windows_storage::tests::windows_native_probe_worker",
            "--nocapture",
        ])
        .env("RELAY_TEST_WINDOWS_PROBE_PATH", path);
    let output = bounded_command(&mut command, super::DATA_DIRECTORY_PROBE_TIMEOUT).await?;
    let mut responses = output
        .lines()
        .filter_map(|line| line.strip_prefix("relay-storage-probe:"));
    let result = responses.next();
    if responses.next().is_some() {
        return Err(security_error("duplicate Windows path probe response"));
    }
    match result {
        Some("missing") => Ok(None),
        Some("directory" | "reparse") => Ok(Some(true)),
        Some("file") => Ok(Some(false)),
        _ => Err(security_error("unexpected Windows path probe response")),
    }
}

/// Handles the disposable native path worker before CLI/runtime configuration.
/// The caller must exit after printing the result. Blocking filesystem access
/// is confined to this child, which its parent kills on timeout or cancellation.
#[cfg(windows)]
pub fn windows_probe_worker(args: &[String]) -> Option<Result<String, PathError>> {
    use winsafe::{co, prelude::*};
    if args.first().map(String::as_str) != Some("--internal-windows-storage-probe") {
        return None;
    }
    Some((|| {
        let path = args
            .get(1)
            .filter(|path| args.len() == 2 && !path.contains('\0') && path.len() <= 4096)
            .ok_or_else(|| {
                security_error("Windows probe requires one path of at most 4096 bytes")
            })?;
        let kind = match winsafe::GetFileAttributes(path) {
            Ok(attributes) if attributes.has(co::FILE_ATTRIBUTE::REPARSE_POINT) => "reparse",
            Ok(attributes) if attributes.has(co::FILE_ATTRIBUTE::DIRECTORY) => "directory",
            Ok(_) => "file",
            Err(co::ERROR::FILE_NOT_FOUND | co::ERROR::PATH_NOT_FOUND) => "missing",
            Err(error) => return Err(security_error(error.to_string())),
        };
        Ok(format!("relay-storage-probe:{kind}"))
    })())
}

#[cfg(windows)]
async fn run_security_script(command: &str, timeout: Duration) -> Result<String, PathError> {
    let system_directory = std::path::PathBuf::from(
        winsafe::GetSystemDirectory().map_err(|error| security_error(error.to_string()))?,
    );
    let windows_directory = system_directory
        .parent()
        .filter(|_| system_directory.is_absolute())
        .ok_or_else(|| security_error("Windows returned an invalid system directory"))?;
    let shell_directory = system_directory.join("WindowsPowerShell/v1.0");
    let program = shell_directory.join("powershell.exe");
    let script = format!(
        "{}\ntry {{ {command} }} catch {{ [Console]::WriteLine($_.Exception.Message); exit 1 }}",
        include_str!("windows_storage.ps1")
    );
    let mut process = tokio::process::Command::new(program);
    // No inherited CLR profiler, module-search, or launcher environment may
    // influence this privileged token/ACL boundary.
    process
        .env_clear()
        .env("SystemRoot", windows_directory)
        .env("WINDIR", windows_directory)
        .env("PSModulePath", shell_directory.join("Modules"));
    process.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &script,
    ]);
    bounded_command(&mut process, timeout).await
}

#[cfg(not(windows))]
async fn run_security_script(_command: &str, _timeout: Duration) -> Result<String, PathError> {
    Err(security_error(
        "automatic Windows storage requires a Windows host",
    ))
}

#[cfg(any(windows, test))]
async fn bounded_command(
    command: &mut tokio::process::Command,
    timeout: std::time::Duration,
) -> Result<String, PathError> {
    use std::process::Stdio;
    use tokio::io::AsyncReadExt;

    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| security_error(error.to_string()))?;
    let result = tokio::time::timeout(timeout, async {
        let mut output = Vec::new();
        child
            .stdout
            .take()
            .expect("piped stdout")
            .take(4097)
            .read_to_end(&mut output)
            .await
            .map_err(|error| security_error(error.to_string()))?;
        if output.len() > 4096 {
            return Err(security_error(
                "Windows security response exceeds 4096 bytes",
            ));
        }
        let status = child
            .wait()
            .await
            .map_err(|error| security_error(error.to_string()))?;
        let text = String::from_utf8(output)
            .map_err(|_| security_error("Windows security response is not UTF-8"))?;
        if !status.success() {
            return Err(security_error(format!(
                "Windows security command failed ({status}): {}",
                text.trim()
            )));
        }
        Ok(text.trim().to_owned())
    })
    .await;
    // Dropping a cancelled/timed-out future also drops and kills its child.
    result.unwrap_or_else(|_| Err(security_error("Windows security command timed out")))
}

fn security_error(reason: impl Into<String>) -> PathError {
    PathError {
        purpose: PathPurpose::Data,
        kind: PathErrorKind::WindowsStorageSecurity {
            reason: reason.into(),
        },
    }
}

#[cfg(test)]
#[path = "windows_security_tests.rs"]
mod tests;
