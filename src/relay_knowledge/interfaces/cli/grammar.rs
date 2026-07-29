use super::{CliDiagnostic, CliError, OutputFormat, cli_spec};

pub(super) fn diagnose(tokens: &[String], error: CliError, format: OutputFormat) -> CliError {
    if matches!(
        error,
        CliError::RuntimeConfigFailed(_) | CliError::ApiFailed(_) | CliError::ApiError { .. }
    ) {
        return error;
    }

    let grammar = CliGrammar::new();
    let invocation = grammar.parse_context(tokens);
    let unexpected_token = unexpected_token(&error);
    let suggestion = suggestion_for(
        &grammar.spec.commands,
        invocation.command,
        &invocation.matched_path,
        tokens,
        &error,
    );
    let message = message_for(
        invocation.command,
        &invocation.matched_path,
        tokens,
        &error,
        suggestion.as_deref(),
    );

    CliError::Diagnostic(Box::new(CliDiagnostic::new(
        message,
        invocation.usage,
        suggestion,
        invocation.matched_path,
        unexpected_token,
        invocation.expected,
        format,
    )))
}

struct CliGrammar {
    spec: cli_spec::CliSpec,
}

impl CliGrammar {
    fn new() -> Self {
        Self {
            spec: cli_spec::cli_spec(),
        }
    }

    fn parse_context(&self, tokens: &[String]) -> ParsedInvocation<'_> {
        let matched_path = matched_path(&self.spec.commands, tokens);
        let command = command_for_path(&self.spec.commands, &matched_path);
        let usage = command
            .map(|command| command.usage.to_owned())
            .or_else(|| namespace_usage(&matched_path));
        let expected = expected_terms(&self.spec.commands, command, &matched_path);

        ParsedInvocation {
            matched_path,
            command,
            usage,
            expected,
        }
    }
}

struct ParsedInvocation<'a> {
    matched_path: Vec<String>,
    command: Option<&'a cli_spec::CliCommandSpec>,
    usage: Option<String>,
    expected: Vec<String>,
}

fn matched_path(commands: &[cli_spec::CliCommandSpec], tokens: &[String]) -> Vec<String> {
    let words = command_like_tokens(tokens);
    commands
        .iter()
        .map(|command| common_prefix(command.path.as_slice(), &words))
        .max_by_key(Vec::len)
        .unwrap_or_default()
}

fn command_like_tokens(tokens: &[String]) -> Vec<&str> {
    let mut words = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index].as_str();
        if token == "--" {
            break;
        }
        if token.starts_with('-') {
            index += option_value_width(token, tokens.get(index + 1).map(String::as_str));
            continue;
        }
        words.push(token);
        index += 1;
    }

    words
}

fn option_value_width(option: &str, next: Option<&str>) -> usize {
    if next.is_none() {
        return 1;
    }
    if matches!(
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
            | "--query"
            | "--description"
            | "--id"
            | "--mcp"
            | "--state"
            | "--by"
            | "--reason"
            | "--operation"
            | "--input"
            | "--root"
            | "--scope"
            | "--topic"
            | "--uri"
    ) {
        2
    } else {
        1
    }
}

fn common_prefix(path: &[&'static str], words: &[&str]) -> Vec<String> {
    path.iter()
        .zip(words.iter())
        .take_while(|(left, right)| left == right)
        .map(|(segment, _)| (*segment).to_owned())
        .collect()
}

fn command_for_path<'a>(
    commands: &'a [cli_spec::CliCommandSpec],
    path: &[String],
) -> Option<&'a cli_spec::CliCommandSpec> {
    let path = path.iter().map(String::as_str).collect::<Vec<_>>();
    commands
        .iter()
        .find(|command| command.path.as_slice() == path.as_slice())
}

fn namespace_usage(path: &[String]) -> Option<String> {
    if path.is_empty() {
        None
    } else {
        Some(format!("relay-knowledge {} <subcommand>", path.join(" ")))
    }
}

fn expected_terms(
    commands: &[cli_spec::CliCommandSpec],
    command: Option<&cli_spec::CliCommandSpec>,
    matched_path: &[String],
) -> Vec<String> {
    if let Some(command) = command {
        let mut terms = command
            .arguments
            .iter()
            .map(|argument| format!("<{}>", argument.name))
            .collect::<Vec<_>>();
        terms.extend(command.options.iter().map(|option| option.flag.to_owned()));
        return terms;
    }

    let prefix = matched_path.iter().map(String::as_str).collect::<Vec<_>>();
    commands
        .iter()
        .filter(|command| command_path_starts_with(command, &prefix))
        .filter_map(|command| command.path.get(prefix.len()).copied())
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn command_path_starts_with(command: &cli_spec::CliCommandSpec, prefix: &[&str]) -> bool {
    command.path.starts_with(prefix)
}

fn unexpected_token(error: &CliError) -> Option<String> {
    match error {
        CliError::UnexpectedArgument(token) => Some(token.clone()),
        CliError::InvalidFormat(value)
        | CliError::InvalidCodeQueryKind(value)
        | CliError::InvalidFreshness(value)
        | CliError::InvalidIndexKind(value)
        | CliError::InvalidMapSourceKind(value)
        | CliError::InvalidWorkerKind(value)
        | CliError::InvalidProposalState(value)
        | CliError::InvalidServiceAction(value)
        | CliError::InvalidLimit(value) => Some(value.clone()),
        CliError::MissingValue(value) => Some((*value).to_owned()),
        _ => None,
    }
}

fn suggestion_for(
    commands: &[cli_spec::CliCommandSpec],
    command: Option<&cli_spec::CliCommandSpec>,
    matched_path: &[String],
    tokens: &[String],
    error: &CliError,
) -> Option<String> {
    match error {
        CliError::UnexpectedArgument(token) if token.starts_with("--") => {
            positional_query_suggestion(command, token, tokens)
                .or_else(|| flag_style_command_suggestion(commands, token))
                .or_else(|| local_option_suggestion(command, token))
        }
        CliError::UnexpectedArgument(token) => command_suggestion(commands, matched_path, token)
            .or_else(|| command.map(|command| command.usage.to_owned())),
        CliError::MissingValue(_) => command.map(|command| command.usage.to_owned()),
        CliError::InvalidCodeQueryKind(value)
        | CliError::InvalidFreshness(value)
        | CliError::InvalidIndexKind(value)
        | CliError::InvalidMapSourceKind(value)
        | CliError::InvalidWorkerKind(value)
        | CliError::InvalidProposalState(value)
        | CliError::InvalidServiceAction(value) => enum_value_suggestion(command, value)
            .or_else(|| command.map(|command| command.usage.to_owned())),
        CliError::InvalidLimit(_) => command.map(|command| command.usage.to_owned()),
        _ => None,
    }
}

fn flag_style_command_suggestion(
    commands: &[cli_spec::CliCommandSpec],
    token: &str,
) -> Option<String> {
    let command_name = token.trim_start_matches('-');
    commands
        .iter()
        .filter(|command| command.path.len() == 1)
        .min_by_key(|command| edit_distance(command.path[0], command_name))
        .filter(|command| edit_distance(command.path[0], command_name) <= 2)
        .map(|command| command.usage.to_owned())
}

fn local_option_suggestion(
    command: Option<&cli_spec::CliCommandSpec>,
    token: &str,
) -> Option<String> {
    command.and_then(|command| {
        command
            .options
            .iter()
            .min_by_key(|option| edit_distance(option.flag, token))
            .filter(|option| edit_distance(option.flag, token) <= 3)
            .map(|option| format!("{} {}", command.path.join(" "), option.flag))
    })
}

fn positional_query_suggestion(
    command: Option<&cli_spec::CliCommandSpec>,
    token: &str,
    tokens: &[String],
) -> Option<String> {
    let command = command?;
    if command.path.as_slice() != ["query"] || token != "--query" {
        return None;
    }

    tokens
        .iter()
        .position(|value| value == token)
        .and_then(|index| tokens.get(index + 1))
        .map(|query| format!("relay-knowledge query {query}"))
        .or_else(|| Some("relay-knowledge query <text>".to_owned()))
}

fn command_suggestion(
    commands: &[cli_spec::CliCommandSpec],
    matched_path: &[String],
    token: &str,
) -> Option<String> {
    let prefix = matched_path.iter().map(String::as_str).collect::<Vec<_>>();
    commands
        .iter()
        .filter(|command| command.path.starts_with(prefix.as_slice()))
        .filter_map(|command| {
            command
                .path
                .get(prefix.len())
                .copied()
                .map(|segment| (command, segment))
        })
        .min_by_key(|(_, segment)| edit_distance(segment, token))
        .filter(|(_, segment)| edit_distance(segment, token) <= 2)
        .map(|(command, _)| command.usage.to_owned())
}

fn enum_value_suggestion(
    command: Option<&cli_spec::CliCommandSpec>,
    value: &str,
) -> Option<String> {
    let command = command?;
    command
        .options
        .iter()
        .flat_map(|option| option.allowed_values.iter())
        .min_by_key(|allowed| edit_distance(allowed, value))
        .filter(|allowed| edit_distance(allowed, value) <= 3)
        .map(|allowed| allowed.to_string())
}

fn message_for(
    command: Option<&cli_spec::CliCommandSpec>,
    matched_path: &[String],
    tokens: &[String],
    error: &CliError,
    suggestion: Option<&str>,
) -> String {
    match error {
        CliError::UnexpectedArgument(token)
            if token.starts_with("--") && matched_path.is_empty() =>
        {
            format!("unknown option '{token}'; commands are positional")
        }
        CliError::UnexpectedArgument(token)
            if command.map(|command| command.path.as_slice()) == Some(&["query"][..])
                && token == "--query" =>
        {
            "unexpected option '--query' for 'query'; query text is positional".to_owned()
        }
        CliError::UnexpectedArgument(token) if command.is_none() => {
            let attempted = attempted_command(matched_path, token, tokens);
            if let Some(suggestion) = suggestion {
                let command_name = suggested_command_name(suggestion);
                format!("unknown command '{attempted}'; did you mean '{command_name}'?")
            } else {
                format!("unknown command '{attempted}'")
            }
        }
        CliError::UnexpectedArgument(token) if token.starts_with("--") => {
            format!(
                "unexpected option '{token}' for '{}'",
                matched_path.join(" ")
            )
        }
        CliError::UnexpectedArgument(token) if !matched_path.is_empty() => {
            format!(
                "unexpected argument '{token}' for '{}'",
                matched_path.join(" ")
            )
        }
        CliError::MissingValue(value) => format!("missing value for {value}"),
        CliError::InvalidCodeQueryKind(_)
        | CliError::InvalidFreshness(_)
        | CliError::InvalidIndexKind(_)
        | CliError::InvalidWorkerKind(_)
        | CliError::InvalidProposalState(_)
        | CliError::InvalidServiceAction(_)
        | CliError::InvalidLimit(_) => error.to_string(),
        _ => error.to_string(),
    }
}

fn attempted_command(matched_path: &[String], token: &str, tokens: &[String]) -> String {
    if matched_path.is_empty() {
        return token.to_owned();
    }

    let mut attempted = matched_path.to_vec();
    if tokens.iter().any(|candidate| candidate == token) {
        attempted.push(token.to_owned());
    }
    attempted.join(" ")
}

fn suggested_command_name(suggestion: &str) -> String {
    suggestion
        .strip_prefix("relay-knowledge ")
        .unwrap_or(suggestion)
        .split_whitespace()
        .take_while(|segment| !segment.starts_with('<') && !segment.starts_with('['))
        .collect::<Vec<_>>()
        .join(" ")
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right_len = right.chars().count();
    let mut previous = (0..=right_len).collect::<Vec<_>>();
    let mut current = vec![0; right_len + 1];

    for (left_index, left_char) in left.chars().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right.chars().enumerate() {
            let insertion = current[right_index] + 1;
            let deletion = previous[right_index + 1] + 1;
            let substitution = previous[right_index] + usize::from(left_char != right_char);
            current[right_index + 1] = insertion.min(deletion).min(substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[right_len]
}
