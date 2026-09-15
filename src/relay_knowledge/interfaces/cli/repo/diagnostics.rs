//! CLI parsing for bounded snapshot diagnostic reads.
use super::{CliError, RepoCommand, value_after};
use crate::domain::{CodeDiagnosticsRequest, CodeRepositorySelector};

pub(super) fn parse(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = tokens
        .first()
        .filter(|s| !s.starts_with('-'))
        .ok_or(CliError::MissingValue("<alias>"))?;
    let mut ref_selector = "HEAD".to_owned();
    let mut paths = Vec::new();
    let mut limit = 50;
    let mut cursor = None;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--ref" => ref_selector = value_after(tokens, index, "--ref")?,
            "--path" => paths.push(value_after(tokens, index, "--path")?),
            "--cursor" => cursor = Some(value_after(tokens, index, "--cursor")?),
            "--limit" => {
                let value = value_after(tokens, index, "--limit")?;
                limit = value.parse().map_err(|_| CliError::InvalidLimit(value))?;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
        index += 2;
    }
    let repository = CodeRepositorySelector::new(alias.clone(), ref_selector, paths, Vec::new())
        .map_err(|e| CliError::UnexpectedArgument(e.to_string()))?;
    let request = CodeDiagnosticsRequest {
        repository,
        limit,
        cursor,
    };
    request.validate().map_err(CliError::UnexpectedArgument)?;
    Ok(RepoCommand::Diagnostics(request))
}

#[cfg(test)]
mod mod_tests;
