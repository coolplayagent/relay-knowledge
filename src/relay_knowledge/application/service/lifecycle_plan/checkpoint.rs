//! Lifecycle checkpoint, backup, restore, and local file operations.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::domain::{ServiceDefinitionPlan, ServiceManagerAction};

pub(super) fn write_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, contents).map_err(|error| error.to_string())
}

pub(super) fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
struct LifecycleCheckpoint {
    service_name: String,
    action: String,
    binary_path: String,
    definition_path: String,
    checksum: String,
    definition_backup_path: Option<String>,
    binary_backup_path: Option<String>,
    #[serde(default)]
    binary_cleanup_on_no_backup: bool,
}

pub(super) fn checkpoint_binary_restore_path(checkpoint_path: &Path) -> Option<PathBuf> {
    read_checkpoint_from_path(checkpoint_path)
        .ok()
        .and_then(|checkpoint| {
            if checkpoint.binary_backup_path.is_some() || checkpoint.binary_cleanup_on_no_backup {
                Some(PathBuf::from(checkpoint.binary_path))
            } else {
                None
            }
        })
}

pub(super) fn checkpoint_action_is_uninstall(checkpoint_path: &Path) -> bool {
    read_checkpoint_from_path(checkpoint_path)
        .is_ok_and(|checkpoint| checkpoint.action == ServiceManagerAction::Uninstall.as_str())
}

pub(super) fn capture_checkpoint(plan: &ServiceDefinitionPlan) -> Result<(), String> {
    let attempt_id = checkpoint_attempt_id();
    let definition_backup_path = backup_if_exists(
        Path::new(&plan.definition_path),
        CheckpointBackupKind::Definition,
        &attempt_id,
    )?;
    let binary_backup_path = if plan.install_dir.is_some() {
        backup_if_exists(
            Path::new(&plan.binary_path),
            CheckpointBackupKind::Binary,
            &attempt_id,
        )?
    } else {
        None
    };
    let binary_cleanup_on_no_backup = plan.install_dir.is_some() && binary_backup_path.is_none();
    let checkpoint = LifecycleCheckpoint {
        service_name: plan.service_name.clone(),
        action: plan.action.as_str().to_owned(),
        binary_path: plan.binary_path.clone(),
        definition_path: plan.definition_path.clone(),
        checksum: plan.checksum.clone(),
        definition_backup_path: definition_backup_path.map(|path| path.display().to_string()),
        binary_backup_path: binary_backup_path.map(|path| path.display().to_string()),
        binary_cleanup_on_no_backup,
    };
    write_checkpoint(Path::new(&plan.checkpoint_path), &checkpoint, &attempt_id)
}

fn write_checkpoint(
    path: &Path,
    checkpoint: &LifecycleCheckpoint,
    attempt_id: &str,
) -> Result<(), String> {
    let temporary_path = checkpoint_temporary_path(path, attempt_id);
    write_file(
        &temporary_path,
        serde_json::to_string_pretty(checkpoint)
            .map_err(|error| error.to_string())?
            .as_bytes(),
    )?;
    std::fs::rename(&temporary_path, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary_path);
        format!(
            "publish rollback checkpoint {} from {}: {error}",
            path.display(),
            temporary_path.display()
        )
    })
}

pub(super) fn validate_checkpoint(plan: &ServiceDefinitionPlan) -> Result<(), String> {
    let checkpoint = read_checkpoint(plan)?;
    if checkpoint.service_name != plan.service_name {
        return Err(format!(
            "rollback checkpoint service {} does not match {}",
            checkpoint.service_name, plan.service_name
        ));
    }
    validate_checkpoint_definition_backup(&checkpoint)?;
    if let Some(binary_backup_path) = checkpoint.binary_backup_path.as_deref() {
        validate_checkpoint_backup(Some(binary_backup_path), "binary")?;
    }
    Ok(())
}

fn validate_checkpoint_definition_backup(checkpoint: &LifecycleCheckpoint) -> Result<(), String> {
    if let Some(backup_path) = checkpoint.definition_backup_path.as_deref() {
        validate_checkpoint_backup(Some(backup_path), "service definition")?;
        return Ok(());
    }
    if checkpoint.action == ServiceManagerAction::Upgrade.as_str() {
        return Ok(());
    }
    Err("rollback checkpoint does not contain service definition backup path".to_owned())
}

fn read_checkpoint(plan: &ServiceDefinitionPlan) -> Result<LifecycleCheckpoint, String> {
    read_checkpoint_from_path(Path::new(&plan.checkpoint_path))
}

fn read_checkpoint_from_path(path: &Path) -> Result<LifecycleCheckpoint, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("read rollback checkpoint {}: {error}", path.display()))?;
    serde_json::from_str(&contents)
        .map_err(|error| format!("parse rollback checkpoint {}: {error}", path.display()))
}

pub(super) fn copy_current_binary(plan: &ServiceDefinitionPlan) -> Result<(), String> {
    let source = std::env::current_exe().map_err(|error| error.to_string())?;
    let target = Path::new(&plan.binary_path);
    if source == target {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::copy(source, target)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(super) fn verify_install_binary_target(plan: &ServiceDefinitionPlan) -> Result<(), String> {
    let target = Path::new(&plan.binary_path);
    if target.exists() {
        return Err(format!(
            "install target binary already exists at {}; run service lifecycle upgrade --install-dir to replace it",
            target.display()
        ));
    }
    Ok(())
}

pub(super) fn verify_service_definition_target(plan: &ServiceDefinitionPlan) -> Result<(), String> {
    let target = Path::new(&plan.definition_path);
    if target.exists() {
        return Err(format!(
            "service definition already exists at {}; run service lifecycle upgrade to replace it",
            target.display()
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum CheckpointBackupKind {
    Definition,
    Binary,
}

impl CheckpointBackupKind {
    const fn suffix(self) -> &'static str {
        match self {
            Self::Definition => "definition",
            Self::Binary => "binary",
        }
    }
}

fn backup_if_exists(
    path: &Path,
    kind: CheckpointBackupKind,
    attempt_id: &str,
) -> Result<Option<PathBuf>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let backup = backup_path(path, kind, attempt_id);
    std::fs::copy(path, &backup)
        .map(|_| Some(backup))
        .map_err(|error| error.to_string())
}

pub(super) fn restore_checkpoint_binary(plan: &ServiceDefinitionPlan) -> Result<String, String> {
    let checkpoint = read_checkpoint(plan)?;
    let binary_path = Path::new(&checkpoint.binary_path);
    if let Some(backup_path) = checkpoint.binary_backup_path.as_deref() {
        restore_checkpoint_backup(binary_path, Some(backup_path), "binary")?;
        return Ok(format!("restored {}", checkpoint.binary_path));
    }
    if checkpoint.binary_cleanup_on_no_backup {
        remove_file_if_exists(binary_path)?;
        return Ok(format!("removed {}", checkpoint.binary_path));
    }
    Err("rollback checkpoint does not contain binary backup path".to_owned())
}

pub(super) fn restore_checkpoint_definition(
    plan: &ServiceDefinitionPlan,
) -> Result<String, String> {
    let checkpoint = read_checkpoint(plan)?;
    let definition_path = Path::new(&checkpoint.definition_path);
    if let Some(backup_path) = checkpoint.definition_backup_path.as_deref() {
        restore_checkpoint_backup(definition_path, Some(backup_path), "service definition")?;
        return Ok(format!("restored {}", checkpoint.definition_path));
    }
    if checkpoint.action == ServiceManagerAction::Upgrade.as_str() {
        remove_file_if_exists(definition_path)?;
        return Ok(format!("removed {}", checkpoint.definition_path));
    }
    Err("rollback checkpoint does not contain service definition backup path".to_owned())
}

fn restore_checkpoint_backup(
    path: &Path,
    backup_path: Option<&str>,
    label: &str,
) -> Result<(), String> {
    let backup = validate_checkpoint_backup(backup_path, label)?;
    std::fs::copy(&backup, path)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn validate_checkpoint_backup(backup_path: Option<&str>, label: &str) -> Result<PathBuf, String> {
    let backup = backup_path
        .map(PathBuf::from)
        .ok_or_else(|| format!("rollback checkpoint does not contain {label} backup path"))?;
    if !backup.exists() {
        return Err(format!("missing rollback backup {}", backup.display()));
    }
    Ok(backup)
}

fn backup_path(path: &Path, kind: CheckpointBackupKind, attempt_id: &str) -> PathBuf {
    let mut file_name = path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("checkpoint"));
    file_name.push(format!(".{}.{attempt_id}.rollback", kind.suffix()));
    path.with_file_name(file_name)
}

fn checkpoint_temporary_path(path: &Path, attempt_id: &str) -> PathBuf {
    let mut file_name = path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("checkpoint"));
    file_name.push(format!(".{attempt_id}.tmp"));
    path.with_file_name(file_name)
}

fn checkpoint_attempt_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{timestamp}", std::process::id())
}

#[cfg(test)]
#[path = "checkpoint_tests.rs"]
mod tests;
