use super::super::{
    CliAction, CliCommand, CliError, OutputFormat, files, grammar, knowledge, map, operations,
    repo, repo_set, setup,
};

#[cfg(test)]
#[path = "parse_tests.rs"]
mod tests;

impl OutputFormat {
    /// Parses a CLI output format value.
    pub fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            "markdown" => Ok(Self::Markdown),
            "streaming-json" => Ok(Self::StreamingJson),
            other => Err(CliError::InvalidFormat(other.to_owned())),
        }
    }
}

impl CliCommand {
    /// Parses the CLI arguments after the binary name.
    pub fn parse<I, S>(args: I) -> Result<Self, CliError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let tokens = args.into_iter().map(Into::into).collect::<Vec<_>>();
        let mut action_tokens = Vec::new();
        let mut format = OutputFormat::default();
        let mut remote_base_url = None;
        let mut help = false;
        let mut version = false;
        let mut command_seen = false;
        let mut delimiter_value = false;
        let mut index = 0;

        while index < tokens.len() {
            let arg = &tokens[index];
            if delimiter_value {
                action_tokens.push(arg.clone());
                delimiter_value = false;
                index += 1;
            } else if arg == "--format" {
                let value = tokens
                    .get(index + 1)
                    .ok_or(CliError::MissingFormatValue)?
                    .clone();
                format = OutputFormat::parse(&value)?;
                index += 2;
            } else if let Some(value) = arg.strip_prefix("--format=") {
                format = OutputFormat::parse(value)?;
                index += 1;
            } else if arg == "--remote" {
                remote_base_url = Some(
                    tokens
                        .get(index + 1)
                        .ok_or(CliError::MissingValue("--remote"))?
                        .clone(),
                );
                index += 2;
            } else if let Some(value) = arg.strip_prefix("--remote=") {
                if value.trim().is_empty() {
                    return Err(CliError::MissingValue("--remote"));
                }
                remote_base_url = Some(value.to_owned());
                index += 1;
            } else if arg == "--help" || arg == "-h" {
                help = true;
                index += 1;
            } else if arg == "--version" && !command_seen {
                version = true;
                index += 1;
            } else if arg == "--" {
                action_tokens.push(arg.clone());
                delimiter_value = true;
                index += 1;
            } else if option_consumes_value(arg) {
                action_tokens.push(arg.clone());
                if let Some(value) = tokens.get(index + 1) {
                    action_tokens.push(value.clone());
                    index += 2;
                } else {
                    index += 1;
                }
            } else {
                command_seen |= is_command_word(arg);
                action_tokens.push(arg.clone());
                index += 1;
            }
        }

        let action = if help {
            CliAction::Help {
                path: help_path(action_tokens),
            }
        } else if version {
            if let Some(token) = action_tokens.first() {
                let error = CliError::UnexpectedArgument(token.clone());
                return Err(grammar::diagnose(&action_tokens, error, format));
            }
            CliAction::Version
        } else {
            match parse_action(action_tokens.clone()) {
                Ok(action) => action,
                Err(error) => return Err(grammar::diagnose(&action_tokens, error, format)),
            }
        };

        Ok(Self {
            action,
            format,
            remote_base_url,
            help,
        })
    }
}

fn option_consumes_value(option: &str) -> bool {
    matches!(
        option,
        "--source"
            | "--content"
            | "--entity"
            | "--limit"
            | "--freshness"
            | "--kind"
            | "--alias"
            | "--path"
            | "--language"
            | "--ref"
            | "--base"
            | "--head"
            | "--changed-path"
            | "--query"
            | "--description"
            | "--id"
            | "--priority"
            | "--mcp"
            | "--state"
            | "--by"
            | "--reason"
            | "--operation"
            | "--task-id"
            | "--input"
            | "--root"
            | "--scope"
            | "--topic"
            | "--uri"
            | "--target-version"
            | "--install-dir"
    )
}

fn is_command_word(token: &str) -> bool {
    matches!(
        token,
        "status"
            | "ingest"
            | "query"
            | "repo"
            | "repo-set"
            | "files"
            | "map"
            | "graph"
            | "index"
            | "worker"
            | "proposal"
            | "audit"
            | "provider"
            | "health"
            | "service"
            | "setup"
            | "version"
            | "help"
    )
}

fn parse_action(tokens: Vec<String>) -> Result<CliAction, CliError> {
    if tokens.is_empty() || tokens == ["status"] {
        return Ok(CliAction::Status);
    }

    match tokens[0].as_str() {
        "status" => Err(CliError::UnexpectedArgument(
            tokens
                .get(1)
                .cloned()
                .unwrap_or_else(|| "status".to_owned()),
        )),
        "ingest" => knowledge::parse_ingest(&tokens[1..]),
        "query" => knowledge::parse_query(&tokens[1..]),
        "files" => files::parse_files(&tokens[1..]),
        "map" => map::parse_map(&tokens[1..]),
        "repo" => repo::parse_repo(&tokens[1..]).map(CliAction::Repo),
        "repo-set" => repo_set::parse_repo_set(&tokens[1..]).map(CliAction::RepoSet),
        "graph" => knowledge::parse_graph(&tokens[1..]),
        "index" => knowledge::parse_index(&tokens[1..]),
        "worker" => operations::parse_worker(&tokens[1..]),
        "proposal" => operations::parse_proposal(&tokens[1..]),
        "audit" => operations::parse_audit(&tokens[1..]),
        "provider" => parse_provider(&tokens[1..]),
        "health" if tokens.len() == 1 => Ok(CliAction::Health),
        "service" => operations::parse_service(&tokens[1..]),
        "setup" => setup::parse_setup(&tokens[1..]),
        "version" if tokens.len() == 1 => Ok(CliAction::Version),
        "version" if tokens == ["version", "check"] => Ok(CliAction::VersionCheck),
        "help" => Ok(CliAction::Help {
            path: help_path(tokens[1..].to_vec()),
        }),
        other => Err(CliError::UnexpectedArgument(other.to_owned())),
    }
}

fn help_path(tokens: Vec<String>) -> Vec<String> {
    tokens
        .into_iter()
        .filter(|token| token != "--")
        .filter(|token| !token.starts_with('-'))
        .collect()
}

fn parse_provider(tokens: &[String]) -> Result<CliAction, CliError> {
    if tokens == ["probe"] {
        return Ok(CliAction::ProviderProbe);
    }

    Err(CliError::UnexpectedArgument(
        tokens
            .first()
            .cloned()
            .unwrap_or_else(|| "provider".to_owned()),
    ))
}
