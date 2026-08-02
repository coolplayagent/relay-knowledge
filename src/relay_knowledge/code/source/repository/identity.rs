use std::{fs, path::Path};

use super::super::{
    CodeIndexError,
    git::{git_optional, resolve_git_root},
    ids::stable_id,
};
use super::RegistrationSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::code) enum RepositorySourceKind {
    Git,
    FileSystem,
}

impl RepositorySourceKind {
    pub(in crate::code) const fn is_filesystem(self) -> bool {
        matches!(self, Self::FileSystem)
    }
}

pub(in crate::code) fn registration_source(
    path: &Path,
) -> Result<RegistrationSource, CodeIndexError> {
    match resolve_git_root(path) {
        Ok(root) => {
            let root_identity = root.display().to_string();
            let origin = git_optional(&root, ["config", "--get", "remote.origin.url"])?
                .unwrap_or_else(|| root_identity.clone());
            Ok(RegistrationSource {
                root,
                identity: stable_id("repo", [origin.as_str(), root_identity.as_str()]),
            })
        }
        Err(git_error) => {
            if !git_error_is_not_repository(&git_error) || path_or_parent_has_git_metadata(path)? {
                return Err(git_error);
            }
            let root = path.canonicalize().map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => git_error,
                _ => CodeIndexError::Io(error),
            })?;
            if !root.is_dir() {
                return Err(CodeIndexError::InvalidInput(format!(
                    "code repository root '{}' is not a directory",
                    root.display()
                )));
            }
            Ok(RegistrationSource {
                identity: filesystem_registration_identity_for_root(&root),
                root,
            })
        }
    }
}

pub(in crate::code) fn filesystem_registration_identity(
    path: &Path,
) -> Result<String, CodeIndexError> {
    let root = path.canonicalize()?;
    if !root.is_dir() {
        return Err(CodeIndexError::InvalidInput(format!(
            "code repository root '{}' is not a directory",
            root.display()
        )));
    }

    Ok(filesystem_registration_identity_for_root(&root))
}

fn filesystem_registration_identity_for_root(root: &Path) -> String {
    let root_identity = root.display().to_string();
    stable_id("repo", ["filesystem", root_identity.as_str()])
}

fn git_error_is_not_repository(error: &CodeIndexError) -> bool {
    matches!(error, CodeIndexError::Git { message, .. } if message.contains("not a git repository"))
}

fn path_or_parent_has_git_metadata(path: &Path) -> Result<bool, CodeIndexError> {
    let Ok(mut current) = path.canonicalize() else {
        return Ok(false);
    };
    if current.is_file() {
        current.pop();
    }
    loop {
        match fs::symlink_metadata(current.join(".git")) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if !current.pop() {
            return Ok(false);
        }
    }
}

pub(in crate::code) fn source_kind(root: &Path) -> Result<RepositorySourceKind, CodeIndexError> {
    match resolve_git_root(root) {
        Ok(_) => Ok(RepositorySourceKind::Git),
        Err(error)
            if git_error_is_not_repository(&error) && !path_or_parent_has_git_metadata(root)? =>
        {
            Ok(RepositorySourceKind::FileSystem)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
