use super::{CliCommandSpec, CommandEffect, arg, command_syntax, opt};

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
        repo_register(),
        repo_index(),
        repo_scope_preview(),
        repo_update(),
        repo_query(),
        repo_impact(),
        repo_status(),
        repo_report(),
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

fn repo_register() -> CliCommandSpec {
    command!(
        &["repo", "register"],
        "relay-knowledge repo register <path> --alias <name> [--path <filter>] [--language <id>]",
        "Register a code repository scope.",
        "code.repo.register",
        CommandEffect::WritesOperationalState,
        &[arg("path", true, false, "Repository root path.", None, &[])],
        &[
            opt(
                "--alias",
                Some("name"),
                true,
                false,
                "Stable repository alias used by later repo commands.",
                None,
                &[],
            ),
            opt(
                "--path",
                Some("filter"),
                false,
                true,
                "Path prefix included in indexing.",
                None,
                &[],
            ),
            opt(
                "--language",
                Some("id"),
                false,
                true,
                "Language id included in indexing.",
                None,
                &[],
            ),
        ],
        &[
            "relay-knowledge repo register /path/to/repo --alias core --path src --language rust --format json",
        ],
        &[
            "Stores repository registration metadata; indexing is a separate command. Registering the same repository root with another alias preserves existing aliases for that repository id."
        ],
    )
}

fn repo_index() -> CliCommandSpec {
    command!(
        &["repo", "index"],
        "relay-knowledge repo index <alias> [--ref <ref>] [--dry-run]",
        "Index a registered repository ref.",
        "code.repo.index",
        CommandEffect::WritesIndexes,
        &[arg(
            "alias",
            true,
            false,
            "Registered repository alias.",
            None,
            &[],
        )],
        &[
            opt(
                "--ref",
                Some("ref"),
                false,
                false,
                "Git ref, commit, or worktree selector.",
                Some("HEAD"),
                &[],
            ),
            opt(
                "--dry-run",
                None,
                false,
                false,
                "Preview scope without committing index data.",
                None,
                &[],
            ),
        ],
        &["relay-knowledge repo index core --ref HEAD --format json"],
        &["`--dry-run` returns a scope preview instead of writing index state."],
    )
}

fn repo_scope_preview() -> CliCommandSpec {
    command!(
        &["repo", "scope", "preview"],
        "relay-knowledge repo scope preview <alias> [--ref <ref>]",
        "Preview repository indexing scope.",
        "code.repo.scope_preview",
        CommandEffect::ReadOnly,
        &[arg(
            "alias",
            true,
            false,
            "Registered repository alias.",
            None,
            &[],
        )],
        &[opt(
            "--ref",
            Some("ref"),
            false,
            false,
            "Git ref, commit, or worktree selector.",
            Some("HEAD"),
            &[],
        )],
        &["relay-knowledge repo scope preview core --ref HEAD --format json"],
        &[],
    )
}

fn repo_update() -> CliCommandSpec {
    command!(
        &["repo", "update"],
        "relay-knowledge repo update <alias> --base <ref> --head <ref>",
        "Incrementally update repository index from base to head.",
        "code.repo.update",
        CommandEffect::WritesIndexes,
        &[arg(
            "alias",
            true,
            false,
            "Registered repository alias.",
            None,
            &[],
        )],
        &[
            opt(
                "--base",
                Some("ref"),
                true,
                false,
                "Previously indexed base ref.",
                None,
                &[],
            ),
            opt(
                "--head",
                Some("ref"),
                true,
                false,
                "Target ref to index.",
                None,
                &[],
            ),
        ],
        &["relay-knowledge repo update core --base main --head HEAD --format json"],
        &[
            "`--base` may refer to any persisted matching indexed scope for the repository and filters; it does not need to be the currently active repository status."
        ],
    )
}

fn repo_query() -> CliCommandSpec {
    command!(
        &["repo", "query"],
        "relay-knowledge repo query <alias> --query <text> [--kind <kind>] [--ref <ref>] [--path <filter>] [--language <id>] [--freshness <policy>] [--limit <n>]",
        "Retrieve code symbols, references, and chunks from a repository index.",
        "code.repo.query",
        CommandEffect::ReadOnly,
        &[arg(
            "alias",
            true,
            false,
            "Registered repository alias.",
            None,
            &[],
        )],
        &[
            opt(
                "--query",
                Some("text"),
                true,
                false,
                "Code search text; multiple unflagged words after --query are joined.",
                None,
                &[],
            ),
            opt(
                "--kind",
                Some("kind"),
                false,
                false,
                "Code retrieval mode.",
                Some("hybrid"),
                &[
                    "hybrid",
                    "symbol",
                    "definition",
                    "references",
                    "callers",
                    "callees",
                    "imports",
                ],
            ),
            opt(
                "--ref",
                Some("ref"),
                false,
                false,
                "Indexed Git ref or worktree selector.",
                Some("HEAD"),
                &[],
            ),
            opt(
                "--path",
                Some("filter"),
                false,
                true,
                "Restricts query to indexed path prefix.",
                None,
                &[],
            ),
            opt(
                "--language",
                Some("id"),
                false,
                true,
                "Restricts query to language id.",
                None,
                &[],
            ),
            opt(
                "--freshness",
                Some("policy"),
                false,
                false,
                "Controls index freshness.",
                Some("allow-stale"),
                &["allow-stale", "wait-until-fresh", "graph-only"],
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
        ],
        &["relay-knowledge repo query core --query retry_policy --kind definition --format json"],
        &["The meaning of --kind is command-local; do not reuse index or worker kind values here."],
    )
}

fn repo_impact() -> CliCommandSpec {
    command!(
        &["repo", "impact"],
        "relay-knowledge repo impact <alias> --base <ref> --head <ref> [--limit <n>]",
        "Analyze code impact between two refs.",
        "code.repo.impact",
        CommandEffect::ReadOnly,
        &[arg(
            "alias",
            true,
            false,
            "Registered repository alias.",
            None,
            &[],
        )],
        &[
            opt(
                "--base",
                Some("ref"),
                true,
                false,
                "Base ref for diff analysis.",
                None,
                &[],
            ),
            opt(
                "--head",
                Some("ref"),
                true,
                false,
                "Head ref for diff analysis.",
                None,
                &[],
            ),
            opt(
                "--limit",
                Some("n"),
                false,
                false,
                "Maximum impact result count.",
                Some("100"),
                &[],
            ),
        ],
        &["relay-knowledge repo impact core --base main --head HEAD --format json"],
        &[],
    )
}

fn repo_status() -> CliCommandSpec {
    command!(
        &["repo", "status"],
        "relay-knowledge repo status <alias>",
        "Read repository index status.",
        "code.repo.status",
        CommandEffect::ReadOnly,
        &[arg(
            "alias",
            true,
            false,
            "Registered repository alias.",
            None,
            &[],
        )],
        &[],
        &["relay-knowledge repo status core --format json"],
        &[],
    )
}

fn repo_report() -> CliCommandSpec {
    command!(
        &["repo", "report"],
        "relay-knowledge repo report <alias> [--format markdown|json]",
        "Render repository report.",
        "code.repo.report",
        CommandEffect::ReadOnly,
        &[arg(
            "alias",
            true,
            false,
            "Registered repository alias.",
            None,
            &[],
        )],
        &[],
        &["relay-knowledge repo report core --format markdown"],
        &[],
    )
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
