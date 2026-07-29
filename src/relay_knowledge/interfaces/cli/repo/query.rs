use crate::domain::{
    CODEGRAPH_CONTEXT_DEFAULT_LIMIT, CODEGRAPH_CONTEXT_DEFAULT_MAX_BYTES, CodeQueryKind,
    FreshnessPolicy,
};

use super::{CliError, RepoCommand, value_after};

pub(super) fn parse_query(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    let mut query = None;
    let mut kind = CodeQueryKind::Hybrid;
    let mut limit = 10;
    let mut ref_selector = "HEAD".to_owned();
    let mut path_filters = Vec::new();
    let mut language_filters = Vec::new();
    let mut freshness = FreshnessPolicy::AllowStale;
    let mut exclude_generated = false;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--query" => {
                let (value, next_index) = collect_query_value(tokens, index, "--query")?;
                query = Some(value);
                index = next_index;
            }
            "--kind" => {
                kind = parse_query_kind(&value_after(tokens, index, "--kind")?)?;
                index += 2;
            }
            "--limit" => {
                limit = parse_limit(tokens, index, "--limit")?;
                index += 2;
            }
            "--ref" => {
                ref_selector = value_after(tokens, index, "--ref")?;
                index += 2;
            }
            "--path" => {
                path_filters.push(value_after(tokens, index, "--path")?);
                index += 2;
            }
            "--language" => {
                language_filters.push(value_after(tokens, index, "--language")?);
                index += 2;
            }
            "--freshness" => {
                freshness = super::parse_freshness(&value_after(tokens, index, "--freshness")?)?;
                index += 2;
            }
            "--exclude-generated" => {
                exclude_generated = true;
                index += 1;
            }
            other if !other.starts_with('-') && query.is_none() => {
                let (value, next_index) = collect_positional_query(tokens, index);
                query = Some(value);
                index = next_index;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoCommand::Query {
        alias,
        query: query.ok_or(CliError::MissingValue("--query"))?,
        kind,
        limit,
        ref_selector,
        path_filters,
        language_filters,
        freshness,
        exclude_generated,
    })
}

pub(super) fn parse_context(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    let mut query = None;
    let mut limit = CODEGRAPH_CONTEXT_DEFAULT_LIMIT;
    let mut ref_selector = "HEAD".to_owned();
    let mut path_filters = Vec::new();
    let mut language_filters = Vec::new();
    let mut freshness = FreshnessPolicy::AllowStale;
    let mut max_context_bytes = CODEGRAPH_CONTEXT_DEFAULT_MAX_BYTES;
    let mut include_code = true;
    let mut exclude_generated = false;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--query" => {
                let (value, next_index) = collect_query_value(tokens, index, "--query")?;
                query = Some(value);
                index = next_index;
            }
            "--limit" => {
                limit = parse_limit(tokens, index, "--limit")?;
                index += 2;
            }
            "--ref" => {
                ref_selector = value_after(tokens, index, "--ref")?;
                index += 2;
            }
            "--path" => {
                path_filters.push(value_after(tokens, index, "--path")?);
                index += 2;
            }
            "--language" => {
                language_filters.push(value_after(tokens, index, "--language")?);
                index += 2;
            }
            "--freshness" => {
                freshness = super::parse_freshness(&value_after(tokens, index, "--freshness")?)?;
                index += 2;
            }
            "--max-context-bytes" => {
                max_context_bytes = parse_limit(tokens, index, "--max-context-bytes")?;
                index += 2;
            }
            "--no-code" => {
                include_code = false;
                index += 1;
            }
            "--exclude-generated" => {
                exclude_generated = true;
                index += 1;
            }
            other if !other.starts_with('-') && query.is_none() => {
                let (value, next_index) = collect_positional_query(tokens, index);
                query = Some(value);
                index = next_index;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoCommand::Context {
        alias,
        query: query.ok_or(CliError::MissingValue("--query"))?,
        limit,
        ref_selector,
        path_filters,
        language_filters,
        freshness,
        max_context_bytes,
        include_code,
        exclude_generated,
    })
}

fn collect_query_value(
    tokens: &[String],
    index: usize,
    flag: &'static str,
) -> Result<(String, usize), CliError> {
    let first = value_after(tokens, index, flag)?;
    let mut values = vec![first];
    let mut cursor = index + 2;
    while cursor < tokens.len() && !tokens[cursor].starts_with("--") {
        values.push(tokens[cursor].clone());
        cursor += 1;
    }

    Ok((values.join(" "), cursor))
}

fn collect_positional_query(tokens: &[String], index: usize) -> (String, usize) {
    let mut values = Vec::new();
    let mut cursor = index;
    while cursor < tokens.len() && !tokens[cursor].starts_with("--") {
        values.push(tokens[cursor].clone());
        cursor += 1;
    }

    (values.join(" "), cursor)
}

fn parse_limit(tokens: &[String], index: usize, flag: &'static str) -> Result<usize, CliError> {
    let value = value_after(tokens, index, flag)?;
    value
        .parse::<usize>()
        .map_err(|_| CliError::InvalidLimit(value.clone()))
}

fn positional_alias(tokens: &[String]) -> Result<String, CliError> {
    tokens
        .first()
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .ok_or(CliError::MissingValue("<alias>"))
}

pub(super) fn parse_query_kind(value: &str) -> Result<CodeQueryKind, CliError> {
    match value {
        "hybrid" => Ok(CodeQueryKind::Hybrid),
        "symbol" | "symbols" => Ok(CodeQueryKind::Symbol),
        "definition" | "definitions" => Ok(CodeQueryKind::Definition),
        "reference" | "references" => Ok(CodeQueryKind::References),
        "caller" | "callers" => Ok(CodeQueryKind::Callers),
        "callee" | "callees" => Ok(CodeQueryKind::Callees),
        "import" | "imports" => Ok(CodeQueryKind::Imports),
        "sbom" => Ok(CodeQueryKind::Sbom),
        other => Err(CliError::InvalidCodeQueryKind(other.to_owned())),
    }
}
