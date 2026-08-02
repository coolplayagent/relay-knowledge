//! Worker, proposal, and audit command specifications.

use super::super::{CliCommandSpec, CommandEffect, arg, command_syntax, opt};

pub(super) fn command_specs() -> Vec<CliCommandSpec> {
    vec![
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

#[cfg(test)]
#[path = "operations_tests.rs"]
mod tests;
