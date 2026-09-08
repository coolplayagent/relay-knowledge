//! Windows account identity and private storage provisioning through a bounded
//! Windows PowerShell 5.1 process. No native unsafe code or executor-blocking I/O.

use std::path::Path;

use super::{PathError, PathErrorKind, PathPurpose, StorageDirectoryAccess};

pub(super) async fn current_sid() -> Result<String, PathError> {
    let sid = run_security_script("Get-RelayStorageSid").await?;
    validate_sid(&sid)?;
    Ok(sid)
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
        "Initialize-RelayPrivateStorage -DataPath '{}' -ExpectedSid '{sid}'{existing_only}; 'secured'",
        path.replace('\'', "''")
    );
    if run_security_script(&command).await? != "secured" {
        return Err(security_error(
            "unexpected Windows storage security response",
        ));
    }
    Ok(())
}

#[cfg(windows)]
async fn run_security_script(command: &str) -> Result<String, PathError> {
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
    bounded_command(&mut process, std::time::Duration::from_secs(10)).await
}

#[cfg(not(windows))]
async fn run_security_script(_command: &str) -> Result<String, PathError> {
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
