//! Repository registration, removal, status, and report command contracts.

use super::super::{CliCommandSpec, CommandEffect, arg, command_syntax, opt};

pub(in crate::interfaces::cli::spec) fn repo_register() -> CliCommandSpec {
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

pub(in crate::interfaces::cli::spec) fn repo_remove() -> CliCommandSpec {
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

pub(in crate::interfaces::cli::spec) fn repo_status() -> CliCommandSpec {
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

pub(in crate::interfaces::cli::spec) fn repo_report() -> CliCommandSpec {
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
