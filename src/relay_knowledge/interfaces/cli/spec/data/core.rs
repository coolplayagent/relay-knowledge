//! Core knowledge, graph, diagnostics, setup, version, and help specifications.

use super::super::{CliCommandSpec, CommandEffect, arg, command_syntax, opt};

pub(super) fn knowledge_commands() -> Vec<CliCommandSpec> {
    vec![
        command!(
            &["status"],
            "relay-knowledge status [--format text|json|markdown|streaming-json]",
            "Print project and runtime status.",
            "project.status",
            CommandEffect::ReadOnly,
            &[],
            &[],
            &["relay-knowledge status --format json"],
            &["No command is equivalent to status."],
        ),
        command!(
            &["ingest"],
            "relay-knowledge ingest --source <scope> --content <text> [--entity <label>]",
            "Ingest one evidence item and optional entity labels.",
            "knowledge.ingest",
            CommandEffect::WritesGraph,
            &[],
            &[
                opt(
                    "--source",
                    Some("scope"),
                    true,
                    false,
                    "Source scope for evidence and graph versioning.",
                    None,
                    &[],
                ),
                opt(
                    "--content",
                    Some("text"),
                    true,
                    false,
                    "Evidence content to store and index.",
                    None,
                    &[],
                ),
                opt(
                    "--entity",
                    Some("label"),
                    false,
                    true,
                    "Entity label grounded by this evidence.",
                    None,
                    &[],
                ),
            ],
            &[
                "relay-knowledge ingest --source docs --content \"Rust async\" --entity Rust --format json",
            ],
            &["Writes graph state and schedules derived index refresh work."],
        ),
        command!(
            &["query"],
            "relay-knowledge query <text> [--source <scope>] [--limit <n>] [--freshness <policy>]",
            "Retrieve hybrid GraphRAG context for a query.",
            "knowledge.retrieve_context",
            CommandEffect::ReadOnly,
            &[arg(
                "text",
                true,
                false,
                "Query text. Use `-- <text>` when it starts with a dash.",
                None,
                &[],
            )],
            &[
                opt(
                    "--source",
                    Some("scope"),
                    false,
                    false,
                    "Restricts retrieval to one source scope.",
                    None,
                    &[],
                ),
                opt(
                    "--limit",
                    Some("n"),
                    false,
                    false,
                    "Maximum result count requested from the API.",
                    Some("10"),
                    &[],
                ),
                opt(
                    "--freshness",
                    Some("policy"),
                    false,
                    false,
                    "Controls derived-index freshness requirements.",
                    Some("allow-stale"),
                    &["allow-stale", "wait-until-fresh", "graph-only"],
                ),
            ],
            &["relay-knowledge query SQLite --freshness wait-until-fresh --format json"],
            &["`graph-only` bypasses derived indexes and reads graph facts only."],
        ),
    ]
}

pub(super) fn graph_commands() -> Vec<CliCommandSpec> {
    vec![
        command!(
            &["graph", "inspect"],
            "relay-knowledge graph inspect",
            "Inspect graph and repository totals.",
            "graph.inspect",
            CommandEffect::ReadOnly,
            &[],
            &[],
            &["relay-knowledge graph inspect --format json"],
            &[],
        ),
        command!(
            &["index", "refresh"],
            "relay-knowledge index refresh [--kind bm25|semantic|vector]",
            "Refresh one or more derived retrieval indexes.",
            "index.refresh",
            CommandEffect::WritesIndexes,
            &[],
            &[opt(
                "--kind",
                Some("kind"),
                false,
                true,
                "Index family to refresh.",
                None,
                &["bm25", "semantic", "vector"],
            )],
            &["relay-knowledge index refresh --kind semantic --kind vector --format json"],
            &["Without --kind, all supported index families are requested."],
        ),
    ]
}

pub(super) fn diagnostic_commands() -> Vec<CliCommandSpec> {
    vec![
        command!(
            &["provider", "probe"],
            "relay-knowledge provider probe",
            "Probe the configured embedding provider.",
            "provider.embedding.probe",
            CommandEffect::ReadOnly,
            &[],
            &[],
            &["relay-knowledge provider probe --format json"],
            &["Secrets are redacted by the env boundary."],
        ),
        command!(
            &["health"],
            "relay-knowledge health",
            "Print service health diagnostics.",
            "service.health",
            CommandEffect::ReadOnly,
            &[],
            &[],
            &["relay-knowledge health --format json"],
            &[],
        ),
    ]
}

pub(super) fn setup_and_meta_commands() -> Vec<CliCommandSpec> {
    vec![
        command!(
            &["setup", "doctor"],
            "relay-knowledge setup doctor",
            "Check local runtime readiness and print concrete remediation commands.",
            "setup.doctor",
            CommandEffect::ReadOnly,
            &[],
            &[],
            &["relay-knowledge setup doctor --format json"],
            &["Aggregates status, health, index freshness, service, MCP, and worker diagnostics."],
        ),
        command!(
            &["setup", "profile"],
            "relay-knowledge setup profile <local|agent-readonly|service|external-embedding>",
            "Print recommended environment variables and commands for a setup profile.",
            "setup.profile",
            CommandEffect::ReadOnly,
            &[arg(
                "profile",
                true,
                false,
                "Named setup profile to render.",
                None,
                &["local", "agent-readonly", "service", "external-embedding"],
            )],
            &[],
            &["relay-knowledge setup profile agent-readonly --format json"],
            &[
                "Profiles are recommendations only; they do not write environment files or install services."
            ],
        ),
        command!(
            @formats &["text", "json", "markdown"],
            &["version"],
            "relay-knowledge version [--format text|json|markdown]",
            "Print binary version.",
            "version",
            CommandEffect::ReadOnly,
            &[],
            &[],
            &["relay-knowledge version --format json"],
            &["Does not load runtime configuration."],
        ),
        command!(
            @formats &["text", "json", "markdown"],
            &["version", "check"],
            "relay-knowledge version check [--format text|json|markdown]",
            "Check configured release sources for a newer stable version.",
            "version.check",
            CommandEffect::ReadOnly,
            &[],
            &[],
            &["relay-knowledge version check --format json"],
            &[
                "Reads GitHub Releases and crates.io through the network boundary and caches diagnostics under the runtime cache directory."
            ],
        ),
        command!(
            &["help"],
            "relay-knowledge help [command...] [--format text|json]",
            "Print human or machine-readable CLI metadata.",
            "cli.help",
            CommandEffect::ReadOnly,
            &[arg(
                "command",
                false,
                true,
                "Optional command path to describe.",
                None,
                &[],
            )],
            &[],
            &["relay-knowledge help repo query --format json"],
            &["This command is intended for scripts, skills, and LLM tools."],
        ),
    ]
}

#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;
