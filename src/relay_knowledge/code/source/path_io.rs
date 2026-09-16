//! Classifies local source failures without swallowing storage or Git errors.

use std::io::{Error, ErrorKind};

use crate::{
    code::CodeIndexError,
    domain::{
        CodeFileDiagnostic, CodeParseStatus, CodePathIoDiagnostic, CodePathIoErrorKind,
        CodePathIoOperation, CodePathKind,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code) struct SkippedSourcePath {
    pub path: String,
    pub io: CodePathIoDiagnostic,
}

impl SkippedSourcePath {
    /// Empty paths represent the source root and must always fail the task.
    pub fn from_error(
        path: &str,
        path_kind: CodePathKind,
        operation: CodePathIoOperation,
        error: Error,
    ) -> Result<Self, CodeIndexError> {
        if path.is_empty() || path == "." {
            return Err(error.into());
        }
        let Some(error_kind) = classify(&error) else {
            return Err(error.into());
        };
        Ok(Self {
            path: path.to_owned(),
            io: CodePathIoDiagnostic {
                action: crate::domain::CodePathIoAction::Skipped,
                path_kind,
                operation,
                error_kind,
                raw_os_error: error.raw_os_error(),
            },
        })
    }

    /// A directory failure removes all inherited facts under that boundary.
    pub fn covers(&self, path: &str) -> bool {
        path == self.path
            || (self.io.path_kind == CodePathKind::Directory
                && path
                    .strip_prefix(&self.path)
                    .is_some_and(|rest| rest.starts_with('/')))
    }

    pub fn diagnostic(&self, repository_id: &str, source_scope: &str) -> CodeFileDiagnostic {
        CodeFileDiagnostic {
            repository_id: repository_id.to_owned(),
            source_scope: source_scope.to_owned(),
            path: self.path.clone(),
            parse_status: CodeParseStatus::Failed,
            message: format!(
                "source path skipped: {:?} failed ({:?}, os error {:?})",
                self.io.operation, self.io.error_kind, self.io.raw_os_error
            ),
            io: Some(self.io.clone()),
        }
    }

    /// Uses typed error evidence, never locale-dependent operating system messages.
    pub fn append_identity(&self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(b"source-path-skipped-v1\0");
        bytes.extend_from_slice(self.path.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(
            format!(
                "{:?}:{:?}:{:?}",
                self.io.path_kind, self.io.operation, self.io.error_kind
            )
            .as_bytes(),
        );
        bytes.push(0);
    }
}

fn classify(error: &Error) -> Option<CodePathIoErrorKind> {
    use CodePathIoErrorKind as Kind;
    #[cfg(windows)]
    match error.raw_os_error() {
        Some(1 | 50) => return Some(Kind::Unsupported),
        Some(2 | 3) => return Some(Kind::NotFound),
        Some(5) => return Some(Kind::PermissionDenied),
        Some(32 | 33) => return Some(Kind::SharingViolation),
        Some(123 | 206) => return Some(Kind::InvalidPath),
        _ => {}
    }
    match error.kind() {
        ErrorKind::NotFound => Some(Kind::NotFound),
        ErrorKind::PermissionDenied => Some(Kind::PermissionDenied),
        ErrorKind::Unsupported => Some(Kind::Unsupported),
        ErrorKind::InvalidInput | ErrorKind::NotADirectory | ErrorKind::IsADirectory => {
            Some(Kind::InvalidPath)
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "path_io_tests.rs"]
mod tests;
