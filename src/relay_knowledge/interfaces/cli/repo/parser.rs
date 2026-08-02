use crate::domain::{FreshnessPolicy, SoftwareGlobalKind};

#[cfg(test)]
use super::query::parse_query_kind;
use super::{
    CliError, RepoCommand, parse_freshness,
    query::{parse_context, parse_query},
    value_after,
};

pub fn parse_repo(tokens: &[String]) -> Result<RepoCommand, CliError> {
    match tokens.first().map(String::as_str) {
        Some("register") => parse_register(&tokens[1..]),
        Some("remove") => parse_remove(&tokens[1..]),
        Some("index") => parse_index(&tokens[1..]),
        Some("index-worker") => parse_index_worker(&tokens[1..]),
        Some("scope") => parse_scope(&tokens[1..]),
        Some("update") => parse_update(&tokens[1..]),
        Some("query") => parse_query(&tokens[1..]),
        Some("context") => parse_context(&tokens[1..]),
        Some("feature-flags") => parse_feature_flags(&tokens[1..]),
        Some("impact") => parse_impact(&tokens[1..]),
        Some("status") => parse_status(&tokens[1..]),
        Some("report") => parse_report(&tokens[1..]),
        Some("software") => parse_software(&tokens[1..]),
        Some("view") => super::view::parse_view(&tokens[1..]).map(RepoCommand::View),
        Some(other) => Err(CliError::UnexpectedArgument(other.to_owned())),
        None => Err(CliError::UnexpectedArgument("repo".to_owned())),
    }
}

fn parse_register(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let root_path = tokens
        .first()
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .ok_or(CliError::MissingValue("<path>"))?;
    let mut alias = None;
    let mut path_filters = Vec::new();
    let mut language_filters = Vec::new();
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--alias" => {
                alias = Some(value_after(tokens, index, "--alias")?);
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
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoCommand::Register {
        root_path,
        alias: alias.unwrap_or_default(),
        path_filters,
        language_filters,
    })
}

fn parse_remove(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    if let Some(extra) = tokens.get(1) {
        return Err(CliError::UnexpectedArgument(extra.clone()));
    }

    Ok(RepoCommand::Remove { alias })
}

fn parse_index(tokens: &[String]) -> Result<RepoCommand, CliError> {
    if tokens.first().map(String::as_str) == Some("--reset") {
        let alias = tokens
            .get(1)
            .filter(|value| !value.starts_with('-'))
            .cloned()
            .ok_or(CliError::MissingValue("<alias>"))?;
        if let Some(extra) = tokens.get(2) {
            return Err(CliError::UnexpectedArgument(extra.clone()));
        }

        return Ok(RepoCommand::IndexReset { alias });
    }

    let alias = positional_alias(tokens)?;
    let mut ref_selector = "HEAD".to_owned();
    let mut ref_was_set = false;
    let mut dry_run = false;
    let mut reset = false;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--ref" => {
                ref_selector = value_after(tokens, index, "--ref")?;
                ref_was_set = true;
                index += 2;
            }
            "--dry-run" => {
                dry_run = true;
                index += 1;
            }
            "--reset" => {
                reset = true;
                index += 1;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }
    if reset {
        if dry_run {
            return Err(CliError::UnexpectedArgument("--dry-run".to_owned()));
        }
        if ref_was_set {
            return Err(CliError::UnexpectedArgument("--ref".to_owned()));
        }

        return Ok(RepoCommand::IndexReset { alias });
    }

    Ok(RepoCommand::Index {
        alias,
        ref_selector,
        dry_run,
    })
}

fn parse_index_worker(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let mut task_id = None;
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--task-id" => {
                task_id = Some(value_after(tokens, index, "--task-id")?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoCommand::IndexWorker { task_id })
}

fn parse_scope(tokens: &[String]) -> Result<RepoCommand, CliError> {
    if tokens.first().map(String::as_str) != Some("preview") {
        return Err(CliError::UnexpectedArgument(
            tokens
                .first()
                .cloned()
                .unwrap_or_else(|| "scope".to_owned()),
        ));
    }
    let alias = tokens
        .get(1)
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .ok_or(CliError::MissingValue("<alias>"))?;
    let mut ref_selector = "HEAD".to_owned();
    let mut index = 2;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--ref" => {
                ref_selector = value_after(tokens, index, "--ref")?;
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoCommand::ScopePreview {
        alias,
        ref_selector,
    })
}

fn parse_update(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    let (base_ref, head_ref, _) = parse_base_head_limit(tokens, 1, 50)?;

    Ok(RepoCommand::Update {
        alias,
        base_ref: base_ref.ok_or(CliError::MissingValue("--base"))?,
        head_ref: head_ref.ok_or(CliError::MissingValue("--head"))?,
    })
}

fn parse_feature_flags(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    let mut query = None;
    let mut limit = 50;
    let mut ref_selector = "HEAD".to_owned();
    let mut path_filters = Vec::new();
    let mut language_filters = Vec::new();
    let mut freshness = FreshnessPolicy::AllowStale;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--query" => {
                let (value, next_index) = collect_query_value(tokens, index, "--query")?;
                query = Some(value);
                index = next_index;
            }
            "--limit" => {
                let value = value_after(tokens, index, "--limit")?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_| CliError::InvalidLimit(value.clone()))?;
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
                freshness = parse_freshness(&value_after(tokens, index, "--freshness")?)?;
                index += 2;
            }
            other if !other.starts_with('-') && query.is_none() => {
                let (value, next_index) = collect_positional_query(tokens, index);
                query = Some(value);
                index = next_index;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoCommand::FeatureFlags {
        alias,
        query,
        limit,
        ref_selector,
        path_filters,
        language_filters,
        freshness,
    })
}

fn parse_impact(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    let (base_ref, head_ref, limit) = parse_base_head_limit(tokens, 1, 100)?;

    Ok(RepoCommand::Impact {
        alias,
        base_ref: base_ref.ok_or(CliError::MissingValue("--base"))?,
        head_ref: head_ref.ok_or(CliError::MissingValue("--head"))?,
        limit,
    })
}

fn parse_status(tokens: &[String]) -> Result<RepoCommand, CliError> {
    Ok(RepoCommand::Status {
        alias: positional_alias(tokens)?,
    })
}

fn parse_report(tokens: &[String]) -> Result<RepoCommand, CliError> {
    Ok(RepoCommand::Report {
        alias: positional_alias(tokens)?,
    })
}

fn parse_software(tokens: &[String]) -> Result<RepoCommand, CliError> {
    let alias = positional_alias(tokens)?;
    let mut ref_selector = "HEAD".to_owned();
    let mut kind = SoftwareGlobalKind::All;
    let mut freshness = FreshnessPolicy::AllowStale;
    let mut limit = 100;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--ref" => {
                ref_selector = value_after(tokens, index, "--ref")?;
                index += 2;
            }
            "--kind" => {
                kind = parse_software_kind(&value_after(tokens, index, "--kind")?)?;
                index += 2;
            }
            "--freshness" => {
                freshness = parse_freshness(&value_after(tokens, index, "--freshness")?)?;
                index += 2;
            }
            "--limit" => {
                let value = value_after(tokens, index, "--limit")?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_| CliError::InvalidLimit(value.clone()))?;
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(RepoCommand::Software {
        alias,
        ref_selector,
        kind,
        freshness,
        limit,
    })
}

fn collect_query_value(
    tokens: &[String],
    index: usize,
    flag: &'static str,
) -> Result<(String, usize), CliError> {
    let first = value_after(tokens, index, flag)?;
    let mut values = vec![first];
    let mut next = index + 2;
    while next < tokens.len() && !tokens[next].starts_with('-') {
        values.push(tokens[next].clone());
        next += 1;
    }

    Ok((values.join(" "), next))
}

fn collect_positional_query(tokens: &[String], index: usize) -> (String, usize) {
    let mut values = Vec::new();
    let mut next = index;
    while next < tokens.len() && !tokens[next].starts_with('-') {
        values.push(tokens[next].clone());
        next += 1;
    }

    (values.join(" "), next)
}

fn parse_base_head_limit(
    tokens: &[String],
    start_index: usize,
    default_limit: usize,
) -> Result<(Option<String>, Option<String>, usize), CliError> {
    let mut base_ref = None;
    let mut head_ref = None;
    let mut limit = default_limit;
    let mut index = start_index;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--base" => {
                base_ref = Some(value_after(tokens, index, "--base")?);
                index += 2;
            }
            "--head" => {
                head_ref = Some(value_after(tokens, index, "--head")?);
                index += 2;
            }
            "--limit" => {
                let value = value_after(tokens, index, "--limit")?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_| CliError::InvalidLimit(value.clone()))?;
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok((base_ref, head_ref, limit))
}

fn positional_alias(tokens: &[String]) -> Result<String, CliError> {
    tokens
        .first()
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .ok_or(CliError::MissingValue("<alias>"))
}

fn parse_software_kind(value: &str) -> Result<SoftwareGlobalKind, CliError> {
    match value {
        "dependencies" => Ok(SoftwareGlobalKind::Dependencies),
        "sdks" => Ok(SoftwareGlobalKind::Sdks),
        "files" => Ok(SoftwareGlobalKind::Files),
        "topics" => Ok(SoftwareGlobalKind::Topics),
        "relationships" => Ok(SoftwareGlobalKind::Relationships),
        "build" => Ok(SoftwareGlobalKind::Build),
        "iac" => Ok(SoftwareGlobalKind::Iac),
        "design" => Ok(SoftwareGlobalKind::Design),
        "all" => Ok(SoftwareGlobalKind::All),
        other => Err(CliError::InvalidSoftwareKind(other.to_owned())),
    }
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod parser_tests;
