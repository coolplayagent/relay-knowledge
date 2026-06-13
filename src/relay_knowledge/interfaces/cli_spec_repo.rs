use super::{CliCommandSpec, CommandEffect, arg, command_syntax, opt};

pub(super) fn repo_register() -> CliCommandSpec {
    command!(
        &["repo", "register"],
        "relay-knowledge repo register <path> [--alias <name>] [--path <filter>]",
        "Register a code repository scope.",
        "code.repo.register",
        CommandEffect::WritesOperationalState,
        &[arg("path", true, false, "Repository root path.", None, &[])],
        &[
            opt(
                "--alias",
                Some("name"),
                false,
                false,
                "Stable repository alias used by later repo commands; defaults to the Git or filesystem root directory name.",
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
        ],
        &[
            "relay-knowledge repo register /path/to/relay-knowledge --format json",
            "relay-knowledge repo register /path/to/repo --alias core --path src --format json",
        ],
        &[
            "Stores repository registration metadata; indexing is a separate command. When `--alias` is omitted or blank, the resolved Git root or filesystem root directory name is used so later agent sessions can reuse the same repository. Registration rejects language filters so mixed-language repositories keep their full language surface; use repo query --language to narrow results. Registering the same repository root with another alias preserves existing aliases for that repository id."
        ],
    )
}

pub(super) fn repo_remove() -> CliCommandSpec {
    command!(
        &["repo", "remove"],
        "relay-knowledge repo remove <alias>",
        "Remove a registered code repository and its index state.",
        "code.repo.remove",
        CommandEffect::WritesOperationalState,
        &[arg(
            "alias",
            true,
            false,
            "Registered repository alias or repository id.",
            None,
            &[],
        )],
        &[],
        &["relay-knowledge repo remove core --format json"],
        &[
            "Deletes the repository registration, all aliases for that repository id, code index scopes, code-index tasks, repository-set membership, repository-set overlays, and software projection rows.",
            "Does not delete files from the source repository on disk.",
            "Removal is rejected while the repository has a running code-index task lease.",
        ],
    )
}

pub(super) fn repo_index() -> CliCommandSpec {
    command!(
        &["repo", "index"],
        "relay-knowledge repo index <alias> [--ref <ref>] [--dry-run|--reset]",
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
            opt(
                "--reset",
                None,
                false,
                false,
                "Reset unfinished code-index tasks for the repository.",
                None,
                &[],
            ),
        ],
        &[
            "relay-knowledge repo index core --ref HEAD --format json",
            "relay-knowledge repo index core --ref worktree --format json",
            "relay-knowledge repo index core --reset --format json",
        ],
        &[
            "`--dry-run` returns a scope preview instead of writing index state.",
            "`--ref worktree` indexes uncommitted and untracked files in the current Git worktree as a bounded overlay over the checked-out HEAD scope; queries that need those facts must also use `--ref worktree`.",
            "`--ref worktree` requires a matching checked-out HEAD base index; run `repo index <alias> --ref HEAD` before the first worktree overlay.",
            "`--ref worktree --dry-run` previews the checked-out HEAD scope used as the overlay base and does not write overlay index state.",
            "`--reset` clears stale task leases and retry state for unfinished repository tasks without deleting completed indexed scopes or reviving terminal dead-letter history.",
            "Cold full indexes return a durable task handle and the CLI runs one bounded worker attempt before returning; service mode continues unfinished background tasks and `repo status` reports checkpoints.",
        ],
    )
}

pub(super) fn repo_index_worker() -> CliCommandSpec {
    command!(
        @formats &["json", "streaming-json"],
        &["repo", "index-worker"],
        "relay-knowledge repo index-worker [--task-id <id>]",
        "Run one queued repository index task attempt.",
        "code.repo.index_worker",
        CommandEffect::WritesIndexes,
        &[],
        &[opt(
            "--task-id",
            Some("id"),
            false,
            false,
            "Specific code-index task to claim; omitted claims the next eligible task.",
            None,
            &[],
        )],
        &["relay-knowledge repo index-worker --task-id code-index-task:1 --format json"],
        &[
            "Use this single-shot worker in non-interactive agent sessions when a queued or retrying cold full index needs explicit progress without starting the foreground service.",
            "When no eligible task is claimed, JSON output reports `claimed=false` and `task=null`.",
            "`--format streaming-json` emits started, item, and completed events with the worker result in the item payload.",
            "The command respects durable task leases, retry backoff, checkpoints, and the single-writer indexing boundary.",
        ],
    )
}

pub(super) fn repo_scope_preview() -> CliCommandSpec {
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

pub(super) fn repo_update() -> CliCommandSpec {
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

pub(super) fn repo_query() -> CliCommandSpec {
    command!(
        &["repo", "query"],
        "relay-knowledge repo query <alias> --query <text> [--kind <kind>] [--ref <ref>] [--path <filter>] [--language <id>] [--freshness <policy>] [--exclude-generated] [--limit <n>]",
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
                    "sbom",
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
            opt(
                "--exclude-generated",
                None,
                false,
                false,
                "Exclude generated files from query results.",
                None,
                &[],
            ),
        ],
        &["relay-knowledge repo query core --query retry_policy --kind definition --format json"],
        &[
            "The meaning of --kind is command-local; do not reuse index or worker kind values here.",
            "Generated files remain indexed for freshness and statistics; --exclude-generated only filters retrieval results.",
        ],
    )
}

pub(super) fn repo_context() -> CliCommandSpec {
    command!(
        &["repo", "context"],
        "relay-knowledge repo context <alias> --query <text> [--ref <ref>] [--path <filter>] [--language <id>] [--freshness <policy>] [--limit <n>] [--max-context-bytes <n>] [--no-code] [--exclude-generated]",
        "Build a one-call codegraph context pack for coding agents.",
        "code.repo.context",
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
                "Context request text; multiple unflagged words after --query are joined.",
                None,
                &[],
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
                "Restricts context to indexed path prefix.",
                None,
                &[],
            ),
            opt(
                "--language",
                Some("id"),
                false,
                true,
                "Restricts context to language id.",
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
                "Maximum entry, related-symbol, and graph-path count per group.",
                Some("8"),
                &[],
            ),
            opt(
                "--max-context-bytes",
                Some("n"),
                false,
                false,
                "Maximum serialized context pack size.",
                Some("65536"),
                &[],
            ),
            opt(
                "--no-code",
                None,
                false,
                false,
                "Omit code excerpts while keeping provenance and graph evidence.",
                None,
                &[],
            ),
            opt(
                "--exclude-generated",
                None,
                false,
                false,
                "Exclude generated files from context evidence.",
                None,
                &[],
            ),
        ],
        &[
            "relay-knowledge repo context core --query \"retry_policy callers imports\" --format json",
        ],
        &[
            "The command orchestrates existing code graph queries and does not trigger repository indexing or refresh.",
            "JSON includes entry_points, related_symbols, graph_paths, impact_hints, code_excerpts, freshness, budget, and truncation diagnostics.",
        ],
    )
}

pub(super) fn repo_feature_flags() -> CliCommandSpec {
    command!(
        &["repo", "feature-flags"],
        "relay-knowledge repo feature-flags <alias> [--query <text>] [--ref <ref>] [--path <filter>] [--language <id>] [--freshness <policy>] [--limit <n>]",
        "List configuration-driven feature flags and code relationships from a repository index.",
        "code.repo.feature_flags",
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
                false,
                false,
                "Optional filter over feature flag name, config key, path, or excerpt.",
                None,
                &[],
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
                "Maximum feature flag groups requested from the API.",
                Some("50"),
                &[],
            ),
        ],
        &["relay-knowledge repo feature-flags core --query checkout --format json"],
        &[
            "Feature flags are indexed facts; this command does not scan the repository at query time."
        ],
    )
}

pub(super) fn repo_impact() -> CliCommandSpec {
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

pub(super) fn repo_status() -> CliCommandSpec {
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
        &[
            "JSON status includes active code-index task, checkpoint counters, and scope retention when available."
        ],
    )
}

pub(super) fn repo_report() -> CliCommandSpec {
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

pub(super) fn repo_software() -> CliCommandSpec {
    command!(
        &["repo", "software"],
        "relay-knowledge repo software <alias> [--ref <ref>] [--kind dependencies|sdks|files|topics|relationships|build|iac|design|all] [--freshness <policy>] [--limit <n>]",
        "Read repository-scoped software dependency, SDK/API, file, topic, relationship, build, IaC, and design facts.",
        "code.repo.software",
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
                "--ref",
                Some("ref"),
                false,
                false,
                "Indexed Git ref or worktree selector.",
                Some("HEAD"),
                &[],
            ),
            opt(
                "--kind",
                Some("kind"),
                false,
                false,
                "Software global projection slice.",
                Some("all"),
                &[
                    "dependencies",
                    "sdks",
                    "files",
                    "topics",
                    "relationships",
                    "build",
                    "iac",
                    "design",
                    "all",
                ],
            ),
            opt(
                "--freshness",
                Some("policy"),
                false,
                false,
                "Controls projection freshness.",
                Some("allow-stale"),
                &["allow-stale", "wait-until-fresh", "graph-only"],
            ),
            opt(
                "--limit",
                Some("n"),
                false,
                false,
                "Maximum rows per returned projection slice.",
                Some("100"),
                &[],
            ),
        ],
        &["relay-knowledge repo software core --kind all --format json"],
        &[
            "The projection is built from authorized repository index facts: dependency manifests, lockfiles, unresolved import/include targets, build manifests, IaC files, and design documentation."
        ],
    )
}
