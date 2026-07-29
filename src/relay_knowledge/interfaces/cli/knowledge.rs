use crate::domain::{FreshnessPolicy, IndexKind};

use super::{CliAction, CliError, parse_freshness, value_after};

pub(super) fn parse_ingest(tokens: &[String]) -> Result<CliAction, CliError> {
    let mut source_scope = None;
    let mut content = None;
    let mut entity_labels = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        match tokens[index].as_str() {
            "--source" => {
                source_scope = Some(value_after(tokens, index, "--source")?);
                index += 2;
            }
            "--content" => {
                content = Some(value_after(tokens, index, "--content")?);
                index += 2;
            }
            "--entity" => {
                entity_labels.push(value_after(tokens, index, "--entity")?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(CliAction::Ingest {
        source_scope: source_scope.ok_or(CliError::MissingValue("--source"))?,
        content: content.ok_or(CliError::MissingValue("--content"))?,
        entity_labels,
    })
}

pub(super) fn parse_query(tokens: &[String]) -> Result<CliAction, CliError> {
    let mut query = None;
    let mut source_scope = None;
    let mut limit = 10;
    let mut freshness = FreshnessPolicy::default();
    let mut index = 0;

    while index < tokens.len() {
        match tokens[index].as_str() {
            "--" if query.is_none() => {
                query = Some(value_after(tokens, index, "query")?);
                index += 2;
            }
            "--source" => {
                source_scope = Some(value_after(tokens, index, "--source")?);
                index += 2;
            }
            "--limit" => {
                let value = value_after(tokens, index, "--limit")?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_| CliError::InvalidLimit(value.clone()))?;
                index += 2;
            }
            "--freshness" => {
                freshness = parse_freshness(&value_after(tokens, index, "--freshness")?)?;
                index += 2;
            }
            other if !other.starts_with('-') && query.is_none() => {
                let mut values = vec![other.to_owned()];
                index += 1;
                while index < tokens.len() && !tokens[index].starts_with('-') {
                    values.push(tokens[index].clone());
                    index += 1;
                }
                query = Some(values.join(" "));
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(CliAction::Query {
        query: query.ok_or(CliError::MissingValue("query"))?,
        source_scope,
        limit,
        freshness,
    })
}

pub(super) fn parse_graph(tokens: &[String]) -> Result<CliAction, CliError> {
    if tokens == ["inspect"] {
        return Ok(CliAction::GraphInspect);
    }

    Err(CliError::UnexpectedArgument(
        tokens
            .first()
            .cloned()
            .unwrap_or_else(|| "graph".to_owned()),
    ))
}

pub(super) fn parse_index(tokens: &[String]) -> Result<CliAction, CliError> {
    if tokens.first().map(String::as_str) != Some("refresh") {
        return Err(CliError::UnexpectedArgument(
            tokens
                .first()
                .cloned()
                .unwrap_or_else(|| "index".to_owned()),
        ));
    }

    let mut kinds = Vec::new();
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--kind" => {
                kinds.push(parse_index_kind(&value_after(tokens, index, "--kind")?)?);
                index += 2;
            }
            other => return Err(CliError::UnexpectedArgument(other.to_owned())),
        }
    }

    Ok(CliAction::IndexRefresh { kinds })
}

fn parse_index_kind(value: &str) -> Result<IndexKind, CliError> {
    match value {
        "bm25" => Ok(IndexKind::Bm25),
        "semantic" => Ok(IndexKind::Semantic),
        "vector" => Ok(IndexKind::Vector),
        other => Err(CliError::InvalidIndexKind(other.to_owned())),
    }
}
