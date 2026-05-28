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
                "Stable repository alias used by later repo commands; defaults to the Git root directory name.",
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
            "Stores repository registration metadata; indexing is a separate command. When `--alias` is omitted or blank, the resolved Git root directory name is used so later agent sessions can reuse the same repository. Registration rejects language filters so mixed-language repositories keep their full language surface; use repo query --language to narrow results. Registering the same repository root with another alias preserves existing aliases for that repository id."
        ],
    )
}

pub(super) fn repo_index() -> CliCommandSpec {
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
        &[
            "`--dry-run` returns a scope preview instead of writing index state.",
            "Cold full indexes return a durable task handle and the CLI runs one bounded worker attempt before returning; service mode continues unfinished background tasks and `repo status` reports checkpoints.",
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
        ],
        &["relay-knowledge repo query core --query retry_policy --kind definition --format json"],
        &["The meaning of --kind is command-local; do not reuse index or worker kind values here."],
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
        "relay-knowledge repo software <alias> [--ref <ref>] [--kind dependencies|sdks|all] [--freshness <policy>] [--limit <n>]",
        "Read repository-scoped software dependency and SDK/API facts.",
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
                &["dependencies", "sdks", "all"],
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
            "The projection is built from authorized repository index facts: dependency manifests, lockfiles, and unresolved import/include targets."
        ],
    )
}
