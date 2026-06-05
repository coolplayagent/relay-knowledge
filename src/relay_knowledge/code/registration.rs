use std::path::Path;

use crate::domain::CodeRepositoryRegistration;

use super::{CodeIndexError, source::registration_source};

pub(crate) const REGISTRATION_LANGUAGE_FILTER_ERROR: &str = concat!(
    "registration language filters are not supported; ",
    "register the full language surface and use query-time --language filters to narrow results"
);

/// Validates a code source root and creates a stable repository registration.
pub fn register_repository(
    path: impl AsRef<Path>,
    alias: impl Into<String>,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
) -> Result<CodeRepositoryRegistration, CodeIndexError> {
    if !language_filters.is_empty() {
        return Err(CodeIndexError::InvalidInput(
            REGISTRATION_LANGUAGE_FILTER_ERROR.to_owned(),
        ));
    }
    let source = registration_source(path.as_ref())?;
    let root_identity = source.root.display().to_string();
    let repository_id = source.identity;
    let alias = explicit_or_project_alias(alias, &source.root)?;

    CodeRepositoryRegistration::new(
        repository_id,
        alias,
        root_identity,
        path_filters,
        language_filters,
    )
    .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))
}

fn explicit_or_project_alias(
    alias: impl Into<String>,
    root: &Path,
) -> Result<String, CodeIndexError> {
    let alias = alias.into();
    if !alias.trim().is_empty() {
        return Ok(alias);
    }

    root.file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            CodeIndexError::InvalidInput(
                "repository alias is empty and Git root has no project directory name".to_owned(),
            )
        })
}
