//! Repository indexing, worker, scope-preview, and update command contracts.

use super::super::{CliCommandSpec, CommandEffect, arg, command_syntax, opt};

pub(in crate::interfaces::cli::spec) fn repo_index() -> CliCommandSpec {
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

pub(in crate::interfaces::cli::spec) fn repo_index_worker() -> CliCommandSpec {
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

pub(in crate::interfaces::cli::spec) fn repo_scope_preview() -> CliCommandSpec {
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

pub(in crate::interfaces::cli::spec) fn repo_update() -> CliCommandSpec {
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
