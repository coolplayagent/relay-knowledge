use super::{CliError, OutputFormat};
use crate::project::PROJECT_NAME;

macro_rules! command {
    (
        @formats $formats:expr,
        $path:expr,
        $usage:expr,
        $summary:expr,
        $operation:expr,
        $effect:expr,
        $arguments:expr,
        $options:expr,
        $examples:expr,
        $notes:expr $(,)?
    ) => {
        CliCommandSpec {
            path: $path.to_vec(),
            usage: $usage,
            summary: $summary,
            operation: $operation,
            effect: $effect,
            arguments: $arguments.to_vec(),
            options: $options.to_vec(),
            output_formats: $formats.to_vec(),
            examples: $examples.to_vec(),
            notes: $notes.to_vec(),
        }
    };
    (
        $path:expr,
        $usage:expr,
        $summary:expr,
        $operation:expr,
        $effect:expr,
        $arguments:expr,
        $options:expr,
        $examples:expr,
        $notes:expr $(,)?
    ) => {
        command!(
            @formats &["text", "json", "markdown", "streaming-json"],
            $path,
            $usage,
            $summary,
            $operation,
            $effect,
            $arguments,
            $options,
            $examples,
            $notes,
        )
    };
}

#[path = "cli_spec_data.rs"]
mod cli_spec_data;

const CLI_SPEC_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct CliSpec {
    schema_version: u16,
    binary: &'static str,
    version: &'static str,
    global_options: Vec<CliOptionSpec>,
    commands: Vec<CliCommandSpec>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct CliCommandSpec {
    path: Vec<&'static str>,
    usage: &'static str,
    summary: &'static str,
    operation: &'static str,
    effect: CommandEffect,
    arguments: Vec<CliArgumentSpec>,
    options: Vec<CliOptionSpec>,
    output_formats: Vec<&'static str>,
    examples: Vec<&'static str>,
    notes: Vec<&'static str>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct CliArgumentSpec {
    name: &'static str,
    required: bool,
    repeatable: bool,
    meaning: &'static str,
    default: Option<&'static str>,
    allowed_values: Vec<&'static str>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct CliOptionSpec {
    flag: &'static str,
    value_name: Option<&'static str>,
    required: bool,
    repeatable: bool,
    meaning: &'static str,
    default: Option<&'static str>,
    allowed_values: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum CommandEffect {
    ReadOnly,
    WritesGraph,
    WritesIndexes,
    WritesOperationalState,
    WritesServiceDefinition,
    RunsForegroundService,
}

pub(super) fn render_help(path: &[String], format: OutputFormat) -> Result<String, CliError> {
    let spec = cli_spec();
    match format {
        OutputFormat::Json => super::serialize_line(&select_spec(&spec, path)?),
        OutputFormat::StreamingJson => super::serialize_line(&select_spec(&spec, path)?),
        OutputFormat::Text | OutputFormat::Markdown => render_help_text(&spec, path),
    }
}

pub(super) fn cli_spec() -> CliSpec {
    CliSpec {
        schema_version: CLI_SPEC_SCHEMA_VERSION,
        binary: PROJECT_NAME,
        version: env!("CARGO_PKG_VERSION"),
        global_options: vec![
            opt(
                "--format",
                Some("text|json|markdown|streaming-json"),
                false,
                false,
                "Selects the output protocol. Use json for scripts and skills.",
                Some("text"),
                &["text", "json", "markdown", "streaming-json"],
            ),
            opt(
                "--help",
                None,
                false,
                false,
                "Prints command help. With --format json, prints machine-readable command metadata.",
                None,
                &[],
            ),
            opt(
                "--version",
                None,
                false,
                false,
                "Prints the binary version without loading runtime configuration.",
                None,
                &[],
            ),
        ],
        commands: cli_spec_data::command_specs(),
    }
}

fn select_spec(spec: &CliSpec, path: &[String]) -> Result<serde_json::Value, CliError> {
    if path.is_empty() {
        return serde_json::to_value(spec)
            .map_err(|error| CliError::RenderFailed(error.to_string()));
    }
    if let Some(command) = find_command(&spec.commands, path) {
        return serde_json::to_value(command)
            .map_err(|error| CliError::RenderFailed(error.to_string()));
    }
    let namespace = find_namespace(&spec.commands, path)
        .ok_or_else(|| CliError::UnknownHelpTopic(path.join(" ")))?;

    Ok(serde_json::json!({
        "schema_version": spec.schema_version,
        "binary": spec.binary,
        "version": spec.version,
        "kind": "namespace",
        "path": path,
        "commands": namespace,
    }))
}

fn render_help_text(spec: &CliSpec, path: &[String]) -> Result<String, CliError> {
    if path.is_empty() {
        let mut output = String::new();
        output.push_str("Usage: relay-knowledge <command> [options] [--format text|json|markdown|streaming-json]\n\n");
        output.push_str("Use `relay-knowledge help <command> --format json` for machine-readable parameter metadata.\n\n");
        output.push_str("Commands:\n");
        for command in &spec.commands {
            output.push_str(&format!(
                "  {:<42} {}\n",
                command.path.join(" "),
                command.summary
            ));
        }
        output.push_str("\nGlobal options:\n");
        append_options(&mut output, &spec.global_options);
        return Ok(output);
    }

    if let Some(command) = find_command(&spec.commands, path) {
        return render_command_help_text(command);
    }

    let namespace = find_namespace(&spec.commands, path)
        .ok_or_else(|| CliError::UnknownHelpTopic(path.join(" ")))?;
    let mut output = String::new();
    output.push_str(&format!(
        "Usage: relay-knowledge {} <subcommand>\n\n",
        path.join(" ")
    ));
    output.push_str("Subcommands:\n");
    for command in namespace {
        output.push_str(&format!(
            "  {:<42} {}\n",
            command.path.join(" "),
            command.summary
        ));
    }

    Ok(output)
}

fn render_command_help_text(command: &CliCommandSpec) -> Result<String, CliError> {
    let mut output = String::new();
    output.push_str(&format!("Usage: {}\n\n", command.usage));
    output.push_str(command.summary);
    output.push('\n');
    output.push_str(&format!("Operation: {}\n", command.operation));
    output.push_str(&format!("Effect: {:?}\n", command.effect));
    if !command.arguments.is_empty() {
        output.push_str("\nArguments:\n");
        for argument in &command.arguments {
            output.push_str(&format!(
                "  {:<14} {}{}\n",
                argument.name,
                required_label(argument.required),
                argument.meaning
            ));
        }
    }
    if !command.options.is_empty() {
        output.push_str("\nOptions:\n");
        append_options(&mut output, &command.options);
    }
    if !command.examples.is_empty() {
        output.push_str("\nExamples:\n");
        for example in &command.examples {
            output.push_str(&format!("  {example}\n"));
        }
    }
    if !command.notes.is_empty() {
        output.push_str("\nNotes:\n");
        for note in &command.notes {
            output.push_str(&format!("  - {note}\n"));
        }
    }

    Ok(output)
}

fn append_options(output: &mut String, options: &[CliOptionSpec]) {
    for option in options {
        let value = option.value_name.unwrap_or("");
        let default = option
            .default
            .map(|value| format!(" Default: {value}."))
            .unwrap_or_default();
        let values = if option.allowed_values.is_empty() {
            String::new()
        } else {
            format!(" Values: {}.", option.allowed_values.join(", "))
        };
        output.push_str(&format!(
            "  {:<14} {:<34} {}{}{}\n",
            option.flag, value, option.meaning, default, values
        ));
    }
}

fn find_command<'a>(commands: &'a [CliCommandSpec], path: &[String]) -> Option<&'a CliCommandSpec> {
    let requested = path.iter().map(String::as_str).collect::<Vec<_>>();
    if let Some(command) = commands
        .iter()
        .find(|command| command.path.as_slice() == requested.as_slice())
    {
        return Some(command);
    }

    commands
        .iter()
        .filter(|command| requested.starts_with(command.path.as_slice()))
        .max_by_key(|command| command.path.len())
}

fn find_namespace<'a>(
    commands: &'a [CliCommandSpec],
    path: &[String],
) -> Option<Vec<&'a CliCommandSpec>> {
    let requested = path.iter().map(String::as_str).collect::<Vec<_>>();
    let matches = commands
        .iter()
        .filter(|command| command.path.starts_with(requested.as_slice()))
        .collect::<Vec<_>>();
    if matches.is_empty() {
        None
    } else {
        Some(matches)
    }
}

fn required_label(required: bool) -> &'static str {
    if required { "required. " } else { "optional. " }
}

fn arg(
    name: &'static str,
    required: bool,
    repeatable: bool,
    meaning: &'static str,
    default: Option<&'static str>,
    allowed_values: &[&'static str],
) -> CliArgumentSpec {
    CliArgumentSpec {
        name,
        required,
        repeatable,
        meaning,
        default,
        allowed_values: allowed_values.to_vec(),
    }
}

fn opt(
    flag: &'static str,
    value_name: Option<&'static str>,
    required: bool,
    repeatable: bool,
    meaning: &'static str,
    default: Option<&'static str>,
    allowed_values: &[&'static str],
) -> CliOptionSpec {
    CliOptionSpec {
        flag,
        value_name,
        required,
        repeatable,
        meaning,
        default,
        allowed_values: allowed_values.to_vec(),
    }
}
