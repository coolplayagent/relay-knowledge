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
        map_validate(),
        map_agent_snippet(),
    ]
}

fn map_init() -> CliCommandSpec {
    command!(
        &["map", "init"],
        "relay-knowledge map init",
        "Create or upgrade the repository knowledge-map.yaml contract and its software-model route.",
        "knowledge.map.init",
        CommandEffect::WritesOperationalState,
        &[],
        &[],
        &["relay-knowledge map init --format json"],
        &[
            "Creates the v2 root manifest and topic shard, or losslessly migrates a valid v1 single-file map while ensuring the repository code-map-backed software-model route.",
            "A conflicting reserved repository-software-model source is rejected rather than overwritten.",
            "The repository root is discovered from the process start directory by walking up to .git or .knowledge, with AGENTS.md as a compatibility fallback.",
        ],
    )
}

fn map_show() -> CliCommandSpec {
    command!(
        &["map", "show"],
        "relay-knowledge map show [--topic <id>]",
        "Read the repository knowledge map.",
        "knowledge.map.show",
        CommandEffect::ReadOnly,
        &[],
        &[opt(
            "--topic",
            Some("id"),
            false,
            false,
            "Restricts output to one knowledge topic.",
            None,
            &[],
        )],
        &["relay-knowledge map show --topic build --format json"],
        &[
            "The repository root is discovered from the process start directory before reading .knowledge/knowledge-map.yaml.",
            "The assembled view reads all current topic shards but returns only the bounded recent-history window; use map history for explicit history pages and map route for progressive one-topic loading."
        ],
    )
}

fn map_history() -> CliCommandSpec {
    command!(
        &["map", "history"],
        "relay-knowledge map history [--from <version>] [--limit <count>]",
        "Read one explicitly bounded page of Knowledge Map history.",
        "knowledge.map.history",
        CommandEffect::ReadOnly,
        &[],
        &[
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
        "relay-knowledge map route <topic>",
        "Return the ordered source route for a knowledge topic.",
        "knowledge.map.route",
        CommandEffect::ReadOnly,
        &[arg("topic", true, false, "Knowledge topic id.", None, &[],)],
        &[],
        &["relay-knowledge map route build --format json"],
        &["For a v2 map, reads only the root manifest and the requested topic shard."],
    )
}

fn map_source_add() -> CliCommandSpec {
    command!(
        &["map", "source", "add"],
        "relay-knowledge map source add --id <id> --topic <id> --kind <kind> --uri <uri> [--scope <source_scope>] [--description <text>]",
        "Add a knowledge source to the YAML contract.",
        "knowledge.map.source.add",
        CommandEffect::WritesOperationalState,
        &[],
        &map_source_options(true),
        &[
            "relay-knowledge map source add --id build-cargo --topic build --kind config --uri Cargo.toml --scope repo --format json"
        ],
        &[
            "Updates the default knowledge map and records a map history entry.",
            "The repository root is discovered from the process start directory before writing .knowledge/knowledge-map.yaml.",
        ],
    )
}

fn map_source_update() -> CliCommandSpec {
    command!(
        &["map", "source", "update"],
        "relay-knowledge map source update --id <id> [--topic <id>] [--kind <kind>] [--uri <uri>] [--scope <source_scope>] [--description <text>]",
        "Update a knowledge source in the YAML contract.",
        "knowledge.map.source.update",
        CommandEffect::WritesOperationalState,
        &[],
        &map_source_options(false),
        &[
            "relay-knowledge map source update --id build-cargo --description \"Cargo package manifest\" --format json"
        ],
        &[
            "The source id is stable and cannot be changed by update.",
            "The repository root is discovered from the process start directory before writing .knowledge/knowledge-map.yaml.",
        ],
    )
}

fn map_source_remove() -> CliCommandSpec {
    command!(
        &["map", "source", "remove"],
        "relay-knowledge map source remove --id <id>",
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
        &["relay-knowledge map source remove --id build-cargo --format json"],
        &[
            "Routes referencing the source are pruned during removal.",
            "The repository root is discovered from the process start directory before writing .knowledge/knowledge-map.yaml.",
        ],
    )
}

fn map_validate() -> CliCommandSpec {
    command!(
        &["map", "validate"],
        "relay-knowledge map validate",
        "Validate the YAML contract and AGENTS.md reference.",
        "knowledge.map.validate",
        CommandEffect::ReadOnly,
        &[],
        &[],
        &["relay-knowledge map validate --format json"],
        &[
            "Checks the root manifest, topic-shard and history-archive digests, history continuity, path confinement, and the AGENTS.md reference.",
            "The repository root is discovered from the process start directory before reading .knowledge/knowledge-map.yaml.",
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

#[cfg(test)]
#[path = "map_tests.rs"]
mod tests;
