use super::{
    CliCommandSpec, CliOptionSpec, CommandEffect, arg, cli_spec_repo, cli_spec_repo_set,
    command_syntax, opt,
};

pub(super) fn command_specs() -> Vec<CliCommandSpec> {
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
        files_index(),
        files_query(),
        cli_spec_repo::repo_register(),
        cli_spec_repo::repo_index(),
        cli_spec_repo::repo_scope_preview(),
        cli_spec_repo::repo_update(),
        cli_spec_repo::repo_query(),
        cli_spec_repo::repo_feature_flags(),
        cli_spec_repo::repo_impact(),
        cli_spec_repo::repo_status(),
        cli_spec_repo::repo_report(),
        cli_spec_repo::repo_software(),
        cli_spec_repo_set::repo_set(),
        map_init(),
        map_show(),
        map_route(),
        map_source_add(),
        map_source_update(),
        map_source_remove(),
        map_validate(),
        map_agent_snippet(),
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
        worker(
            "status",
            "Read worker queue and lease status.",
            "worker.status",
            CommandEffect::ReadOnly,
        ),
        worker(
            "run-once",
            "Run one worker task attempt.",
            "worker.run_once",
            CommandEffect::WritesOperationalState,
        ),
        proposal_list(),
        proposal_show(),
        proposal_decision(
            "accept",
            "Accept and commit a proposal.",
            "proposal.accept",
            CommandEffect::WritesGraph,
        ),
        proposal_decision(
            "reject",
            "Reject a proposal without committing graph facts.",
            "proposal.reject",
            CommandEffect::WritesOperationalState,
        ),
        proposal_decision(
            "supersede",
            "Mark a proposal as superseded without committing graph facts.",
            "proposal.supersede",
            CommandEffect::WritesOperationalState,
        ),
        audit_query(),
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
        command!(
            &["service", "status"],
            "relay-knowledge service status",
            "Print installed service and operator status.",
            "service.status",
            CommandEffect::ReadOnly,
            &[],
            &[],
            &["relay-knowledge service status --format json"],
            &["`service doctor` is an alias for this command."],
        ),
        command!(
            &["service", "doctor"],
            "relay-knowledge service doctor",
            "Print service diagnostics.",
            "service.status",
            CommandEffect::ReadOnly,
            &[],
            &[],
            &["relay-knowledge service doctor --format json"],
            &["Alias for service status."],
        ),
        service_plan(),
        command!(
            &["service", "definition", "write"],
            "relay-knowledge service definition write",
            "Write the platform service definition file.",
            "service.definition.write",
            CommandEffect::WritesServiceDefinition,
            &[],
            &[],
            &["relay-knowledge service definition write --format json"],
            &["Does not perform privileged service installation."],
        ),
        service_operator(),
        service_run(),
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

fn map_init() -> CliCommandSpec {
    command!(
        &["map", "init"],
        "relay-knowledge map init",
        "Create the repository knowledge-map.yaml contract when missing.",
        "knowledge.map.init",
        CommandEffect::WritesOperationalState,
        &[],
        &[],
        &["relay-knowledge map init --format json"],
        &["Creates the default knowledge map and leaves AGENTS.md edits explicit."],
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
        &[],
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
        &[],
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
        &["Updates the default knowledge map and records a map history entry."],
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
        &["The source id is stable and cannot be changed by update."],
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
        &["Routes referencing the source are pruned during removal."],
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
        &["Checks the default knowledge map and the AGENTS.md reference."],
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

fn worker(
    action: &'static str,
    summary: &'static str,
    operation: &'static str,
    effect: CommandEffect,
) -> CliCommandSpec {
    command!(
        &["worker", action],
        "relay-knowledge worker status|run-once [--kind embedding|ocr|vision|extractor]",
        summary,
        operation,
        effect,
        &[],
        &[opt(
            "--kind",
            Some("kind"),
            false,
            false,
            "Worker kind to inspect or run.",
            None,
            &["embedding", "ocr", "vision", "extractor"],
        )],
        &["relay-knowledge worker status --format json"],
        &[],
    )
}

fn proposal_list() -> CliCommandSpec {
    command!(
        &["proposal", "list"],
        "relay-knowledge proposal list [--state <state>] [--limit <n>]",
        "List worker proposals.",
        "proposal.list",
        CommandEffect::ReadOnly,
        &[],
        &[
            opt(
                "--state",
                Some("state"),
                false,
                false,
                "Proposal state filter.",
                None,
                &["proposed", "accepted", "rejected", "superseded"],
            ),
            opt(
                "--limit",
                Some("n"),
                false,
                false,
                "Maximum proposal count.",
                Some("50"),
                &[],
            ),
        ],
        &["relay-knowledge proposal list --state proposed --format json"],
        &[],
    )
}

fn proposal_show() -> CliCommandSpec {
    command!(
        &["proposal", "show"],
        "relay-knowledge proposal show <id>",
        "Show one proposal and its conflicts.",
        "proposal.show",
        CommandEffect::ReadOnly,
        &[arg("id", true, false, "Proposal id.", None, &[])],
        &[],
        &["relay-knowledge proposal show proposal:1 --format json"],
        &[],
    )
}

fn proposal_decision(
    action: &'static str,
    summary: &'static str,
    operation: &'static str,
    effect: CommandEffect,
) -> CliCommandSpec {
    command!(
        &["proposal", action],
        "relay-knowledge proposal accept|reject|supersede <id> --by <actor> [--reason <text>]",
        summary,
        operation,
        effect,
        &[arg("id", true, false, "Proposal id.", None, &[])],
        &[
            opt(
                "--by",
                Some("actor"),
                true,
                false,
                "Human or automation identity making the decision.",
                None,
                &[],
            ),
            opt(
                "--reason",
                Some("text"),
                false,
                false,
                "Decision reason recorded in audit metadata.",
                None,
                &[],
            ),
        ],
        &[
            "relay-knowledge proposal accept proposal:1 --by reviewer --reason reviewed --format json",
        ],
        &[
            "`accept` can commit graph mutations; `reject` and `supersede` only update proposal state.",
        ],
    )
}

fn audit_query() -> CliCommandSpec {
    command!(
        &["audit", "query"],
        "relay-knowledge audit query [--operation <name>] [--limit <n>]",
        "Query persisted audit events.",
        "audit.query",
        CommandEffect::ReadOnly,
        &[],
        &[
            opt(
                "--operation",
                Some("name"),
                false,
                false,
                "Operation name filter.",
                None,
                &[],
            ),
            opt(
                "--limit",
                Some("n"),
                false,
                false,
                "Maximum audit event count.",
                Some("100"),
                &[],
            ),
        ],
        &["relay-knowledge audit query --limit 50 --format json"],
        &[],
    )
}

fn service_plan() -> CliCommandSpec {
    command!(
        &["service", "plan"],
        "relay-knowledge service plan install|uninstall",
        "Preview service-manager commands.",
        "service.plan",
        CommandEffect::ReadOnly,
        &[arg(
            "action",
            true,
            false,
            "Service manager action to plan.",
            None,
            &["install", "uninstall"],
        )],
        &[],
        &["relay-knowledge service plan install --format json"],
        &["Returns commands for the platform service manager without executing privileged steps."],
    )
}

fn files_index() -> CliCommandSpec {
    command!(
        &["files", "index"],
        "relay-knowledge files index [--root <path>] [--source <scope>]",
        "Scan authorized local file roots into the file-location index.",
        "files.index",
        CommandEffect::WritesIndexes,
        &[],
        &[
            opt(
                "--root",
                Some("path"),
                false,
                true,
                "Absolute root to scan. Omit to scan configured roots.",
                None,
                &[],
            ),
            opt(
                "--source",
                Some("scope"),
                false,
                false,
                "Source scope assigned to explicit roots.",
                Some("local-files"),
                &[],
            ),
        ],
        &["relay-knowledge files index --root /opt/docs --source local-files --format json"],
        &["Configured roots are absolute paths controlled by RELAY_KNOWLEDGE_FILE_INDEX_ROOTS."],
    )
}

fn files_query() -> CliCommandSpec {
    command!(
        &["files", "query"],
        "relay-knowledge files query <text> [--source <scope>] [--root <root-id>] [--limit <n>]",
        "Query the local file-location index.",
        "files.query",
        CommandEffect::ReadOnly,
        &[arg(
            "text",
            true,
            false,
            "File name, extension, directory, or path text.",
            None,
            &[],
        )],
        &[
            opt(
                "--source",
                Some("scope"),
                false,
                false,
                "Restricts results to one file source scope.",
                None,
                &[],
            ),
            opt(
                "--root",
                Some("root-id"),
                false,
                false,
                "Restricts results to one indexed root.",
                None,
                &[],
            ),
            opt(
                "--limit",
                Some("n"),
                false,
                false,
                "Maximum result count.",
                Some("20"),
                &[],
            ),
        ],
        &["relay-knowledge files query design pdf --source local-files --format json"],
        &["Queries are bounded by RELAY_KNOWLEDGE_FILE_QUERY_TIMEOUT_MS."],
    )
}

fn service_operator() -> CliCommandSpec {
    command!(
        &["service", "operator"],
        "relay-knowledge service operator status|pause|resume",
        "Read or change silent-update operator state.",
        "service.operator",
        CommandEffect::WritesOperationalState,
        &[arg(
            "action",
            true,
            false,
            "Operator action.",
            None,
            &["status", "pause", "resume"],
        )],
        &[],
        &["relay-knowledge service operator pause --format json"],
        &["`status` is read-only; `pause` and `resume` write operator state."],
    )
}

fn service_run() -> CliCommandSpec {
    command!(
        &["service", "run"],
        "relay-knowledge service run [--web] [--mcp streamable-http]",
        "Run the foreground service until shutdown.",
        "service.run",
        CommandEffect::RunsForegroundService,
        &[],
        &[
            opt(
                "--web",
                None,
                false,
                false,
                "Serve the Web workspace and Web API.",
                None,
                &[],
            ),
            opt(
                "--mcp",
                Some("transport"),
                false,
                false,
                "Enable an MCP transport for this process.",
                None,
                &["streamable-http"],
            ),
        ],
        &[
            "RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs relay-knowledge service run --web --mcp streamable-http",
        ],
        &["Long-running installed service operation should use the platform service manager."],
    )
}
