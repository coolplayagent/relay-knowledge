use super::{CliCommandSpec, CommandEffect, arg, command_syntax, opt};

pub(super) fn files_index() -> CliCommandSpec {
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

pub(super) fn files_query() -> CliCommandSpec {
    command!(
        &["files", "query"],
        "relay-knowledge files query <text> [--source <scope>] [--root <root-id>] [--freshness <policy>] [--limit <n>]",
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
                "--freshness",
                Some("policy"),
                false,
                false,
                "Controls file-index freshness.",
                Some("allow-stale"),
                &["allow-stale", "wait-until-fresh", "graph-only"],
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

pub(super) fn files_content() -> CliCommandSpec {
    command!(
        &["files", "content"],
        "relay-knowledge files content <text> [--source <scope>] [--root <root-id>] [--freshness <policy>] [--limit <n>]",
        "Query the local file-content read model.",
        "files.content",
        CommandEffect::ReadOnly,
        &[arg(
            "text",
            true,
            false,
            "Text to search inside bounded, indexed file-content chunks.",
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
                "--freshness",
                Some("policy"),
                false,
                false,
                "Controls file-index freshness.",
                Some("allow-stale"),
                &["allow-stale", "wait-until-fresh", "graph-only"],
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
        &["relay-knowledge files content database runbook --source local-files --format json"],
        &[
            "Content hits are returned as content_role=user_source with source path, span, fingerprint, and stale read-model cursors.",
        ],
    )
}
