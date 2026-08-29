//! Knowledge-map command specifications.

use super::super::{CliCommandSpec, CliOptionSpec, CommandEffect, arg, command_syntax, opt};

pub(super) fn command_specs() -> Vec<CliCommandSpec> {
    vec![
        map_init(),
        map_show(),
        map_history(),
        map_route(),
        map_source_add(),
        map_source_update(),
        map_source_remove(),
        map_directory_add(),
        map_directory_update(),
        map_directory_remove(),
        map_migrate(),
        map_validate(),
        map_agent_snippet(),
    ]
}

fn map_init() -> CliCommandSpec {
    command!(
        &["map", "init"],
        "relay-knowledge map init [--type <knowledge|codespec|all>]",
        "Create or upgrade the repository CodeSpec and Knowledge map contracts.",
        "knowledge.map.init",
        CommandEffect::WritesOperationalState,
        &[],
        &[map_type_option(false)],
        &["relay-knowledge map init --format json"],
        &[
            "Defaults to all and creates v3 typed directory roots; Knowledge migration preserves valid v1/v2 content while ensuring built-in routes.",
            "A conflicting reserved repository-software-model source is rejected rather than overwritten.",
            "The repository root is discovered from .git, exact map files, or AGENTS.md compatibility fallback.",
        ],
    )
}

fn map_show() -> CliCommandSpec {
    command!(
        &["map", "show"],
        "relay-knowledge map show [--type <knowledge|codespec|all>] [--topic <id>] [--directory <path>]",
        "Read repository CodeSpec and Knowledge maps.",
        "knowledge.map.show",
        CommandEffect::ReadOnly,
        &[],
        &[
            map_type_option(false),
            opt(
                "--topic",
                Some("id"),
                false,
                false,
                "Restricts output to one knowledge topic.",
                None,
                &[],
            ),
            opt(
                "--directory",
                Some("path"),
                false,
                false,
                "Restricts output to one governed directory.",
                None,
                &[],
            )
        ],
        &["relay-knowledge map show --topic build --format json"],
        &[
            "The repository root is discovered before reading the selected visible map files.",
            "The assembled view reads all current topic shards but returns only the bounded recent-history window; use map history for explicit history pages and map route for progressive one-topic loading."
        ],
    )
}

fn map_history() -> CliCommandSpec {
    command!(
        &["map", "history"],
        "relay-knowledge map history [--type <knowledge|codespec|all>] [--from <version>] [--limit <count>]",
        "Read one explicitly bounded page of Knowledge Map history.",
        "knowledge.map.history",
        CommandEffect::ReadOnly,
        &[],
        &[
            map_type_option(false),
            opt(
                "--from",
                Some("version"),
                false,
                false,
                "First history version to return. Defaults to 1.",
                None,
                &[],
            ),
            opt(
                "--limit",
                Some("count"),
                false,
                false,
                "Maximum entries to return. Defaults to 64 and cannot exceed 256.",
                None,
                &[],
            ),
        ],
        &["relay-knowledge map history --from 17 --limit 32 --format json"],
        &["History pages verify the referenced archive chain before returning entries."],
    )
}

fn map_route() -> CliCommandSpec {
    command!(
        &["map", "route"],
        "relay-knowledge map route <topic> --type knowledge",
        "Return the ordered source route for a knowledge topic.",
        "knowledge.map.route",
        CommandEffect::ReadOnly,
        &[arg("topic", true, false, "Knowledge topic id.", None, &[],)],
        &[map_type_option(true)],
        &["relay-knowledge map route build --type knowledge --format json"],
        &["For a v2 map, reads only the root manifest and the requested topic shard."],
    )
}

fn map_source_add() -> CliCommandSpec {
    command!(
        &["map", "source", "add"],
        "relay-knowledge map source add --type knowledge --id <id> --topic <id> --kind <kind> --uri <uri> [--scope <source_scope>] [--description <text>]",
        "Add a knowledge source to the YAML contract.",
        "knowledge.map.source.add",
        CommandEffect::WritesOperationalState,
        &[],
        &map_source_options(true),
        &[
            "relay-knowledge map source add --type knowledge --id build-cargo --topic build --kind config --uri Cargo.toml --scope repo --format json"
        ],
        &[
            "Updates the default knowledge map and records a map history entry.",
            "The repository root is discovered from the process start directory before writing knowledge/knowledge-map.yaml.",
        ],
    )
}

fn map_source_update() -> CliCommandSpec {
    command!(
        &["map", "source", "update"],
        "relay-knowledge map source update --type knowledge --id <id> [--topic <id>] [--kind <kind>] [--uri <uri>] [--scope <source_scope>] [--description <text>]",
        "Update a knowledge source in the YAML contract.",
        "knowledge.map.source.update",
        CommandEffect::WritesOperationalState,
        &[],
        &map_source_options(false),
        &[
            "relay-knowledge map source update --type knowledge --id build-cargo --description \"Cargo package manifest\" --format json"
        ],
        &[
            "The source id is stable and cannot be changed by update.",
            "The repository root is discovered from the process start directory before writing knowledge/knowledge-map.yaml.",
        ],
    )
}

fn map_source_remove() -> CliCommandSpec {
    command!(
        &["map", "source", "remove"],
        "relay-knowledge map source remove --type knowledge --id <id>",
        "Remove a knowledge source from the YAML contract.",
        "knowledge.map.source.remove",
        CommandEffect::WritesOperationalState,
        &[],
        &[opt(
            "--id",
            Some("id"),
            true,
            false,
            "Knowledge source id.",
            None,
            &[],
        )],
        &["relay-knowledge map source remove --type knowledge --id build-cargo --format json"],
        &[
            "Routes referencing the source are pruned during removal.",
            "The repository root is discovered from the process start directory before writing knowledge/knowledge-map.yaml.",
        ],
    )
}

fn map_validate() -> CliCommandSpec {
    command!(
        &["map", "validate"],
        "relay-knowledge map validate [--type <knowledge|codespec|all>]",
        "Validate repository map contracts and AGENTS.md references.",
        "knowledge.map.validate",
        CommandEffect::ReadOnly,
        &[],
        &[map_type_option(false)],
        &["relay-knowledge map validate --format json"],
        &[
            "Checks the root manifest, topic-shard and history-archive digests, history continuity, path confinement, and the AGENTS.md reference.",
            "The repository root is discovered before reading the selected visible map files.",
        ],
    )
}

fn map_agent_snippet() -> CliCommandSpec {
    command!(
        &["map", "agent-snippet"],
        "relay-knowledge map agent-snippet",
        "Print the AGENTS.md knowledge map reference snippet.",
        "knowledge.map.agent_snippet",
        CommandEffect::ReadOnly,
        &[],
        &[],
        &["relay-knowledge map agent-snippet --format text"],
        &[],
    )
}

fn map_source_options(add: bool) -> Vec<CliOptionSpec> {
    vec![
        map_type_option(true),
        opt(
            "--id",
            Some("id"),
            true,
            false,
            "Knowledge source id.",
            None,
            &[],
        ),
        opt(
            "--topic",
            Some("id"),
            add,
            false,
            "Knowledge topic id.",
            None,
            &[],
        ),
        opt(
            "--kind",
            Some("kind"),
            add,
            false,
            "Knowledge source category.",
            None,
            &[
                "repo",
                "file",
                "doc",
                "config",
                "db",
                "ci",
                "runtime",
                "wiki",
                "monitoring",
            ],
        ),
        opt(
            "--uri",
            Some("uri"),
            add,
            false,
            "Authoritative source location.",
            None,
            &[],
        ),
        opt(
            "--scope",
            Some("source_scope"),
            false,
            false,
            "Optional relay-knowledge source scope tied to this source.",
            None,
            &[],
        ),
        opt(
            "--description",
            Some("text"),
            false,
            false,
            "Human-readable source description.",
            None,
            &[],
        ),
    ]
}

fn map_directory_add() -> CliCommandSpec {
    directory_command(
        "add",
        "Add a governed directory to one repository map.",
        "repository.map.directory.add",
        true,
    )
}

fn map_directory_update() -> CliCommandSpec {
    directory_command(
        "update",
        "Update a governed directory without renaming it.",
        "repository.map.directory.update",
        false,
    )
}

fn map_directory_remove() -> CliCommandSpec {
    command!(
        &["map", "directory", "remove"],
        "relay-knowledge map directory remove --type <knowledge|codespec> --directory <path>",
        "Remove a custom governed directory.",
        "repository.map.directory.remove",
        CommandEffect::WritesOperationalState,
        &[],
        &[
            map_type_option(true),
            opt(
                "--directory",
                Some("path"),
                true,
                false,
                "Directory identity.",
                None,
                &[]
            )
        ],
        &[
            "relay-knowledge map directory remove --type knowledge --directory integrations --format json"
        ],
        &["The five baseline directories for each map cannot be removed."],
    )
}

fn directory_command(
    action: &'static str,
    summary: &'static str,
    operation_id: &'static str,
    require_fields: bool,
) -> CliCommandSpec {
    let syntax = if require_fields {
        "relay-knowledge map directory add --type <knowledge|codespec> --directory <path> --purpose <text> --content-scope <glob> --load-hint <hint> --update-rule <rule> [--key-file <path>] [--relation <kind=target>]"
    } else {
        "relay-knowledge map directory update --type <knowledge|codespec> --directory <path> [directory fields]"
    };
    command!(
        &["map", "directory", action],
        syntax,
        summary,
        operation_id,
        CommandEffect::WritesOperationalState,
        &[],
        &[
            map_type_option(true),
            opt(
                "--directory",
                Some("path"),
                true,
                false,
                "Stable governed directory identity.",
                None,
                &[]
            ),
            opt(
                "--purpose",
                Some("text"),
                require_fields,
                false,
                "Directory purpose.",
                None,
                &[]
            ),
            opt(
                "--content-scope",
                Some("glob"),
                require_fields,
                true,
                "Confined repository-relative content glob.",
                None,
                &[]
            ),
            opt(
                "--key-file",
                Some("path"),
                false,
                true,
                "Confined repository-relative key file.",
                None,
                &[]
            ),
            opt(
                "--load-hint",
                Some("hint"),
                require_fields,
                false,
                "Agent loading policy.",
                None,
                &["always", "task_match", "on_demand"]
            ),
            opt(
                "--relation",
                Some("kind=target"),
                false,
                true,
                "Typed qualified directory relationship.",
                None,
                &[]
            ),
            opt(
                "--update-rule",
                Some("rule"),
                require_fields,
                false,
                "Content update policy.",
                None,
                &["reviewed", "generated", "external_sync"]
            ),
        ],
        &[],
        &["All mutations require one concrete --type; generated map artifacts remain CLI-managed."],
    )
}

fn map_migrate() -> CliCommandSpec {
    command!(
        &["map", "migrate"],
        "relay-knowledge map migrate --type knowledge <--to-v3|--rollback>",
        "Migrate a v1/v2 Knowledge Map to v3 or restore its retained v2 root.",
        "repository.map.migrate",
        CommandEffect::WritesOperationalState,
        &[],
        &[
            map_type_option(true),
            opt(
                "--to-v3",
                None,
                false,
                false,
                "Publish the visible v3 root and legacy redirect.",
                None,
                &[]
            ),
            opt(
                "--rollback",
                None,
                false,
                false,
                "Restore the retained v2 root.",
                None,
                &[]
            ),
        ],
        &["relay-knowledge map migrate --type knowledge --to-v3 --format json"],
        &["Exactly one migration action is required."],
    )
}

fn map_type_option(concrete: bool) -> CliOptionSpec {
    opt(
        "--type",
        Some("map_type"),
        concrete,
        false,
        if concrete {
            "Concrete repository map type."
        } else {
            "Repository map type; defaults to all."
        },
        None,
        if concrete {
            &["knowledge", "codespec"]
        } else {
            &["knowledge", "codespec", "all"]
        },
    )
}

#[cfg(test)]
#[path = "map_tests.rs"]
mod tests;
