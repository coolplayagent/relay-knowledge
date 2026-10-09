//! Machine-readable feedback contracts and explicit publication effects.

use super::super::{CliCommandSpec, CommandEffect, arg, command_syntax, opt};

pub(super) fn command_specs() -> Vec<CliCommandSpec> {
    let mut commands = Vec::new();
    for (name, operation, summary) in [
        (
            "configure",
            "feedback.configure",
            "Persist explicit local publication policy; does not submit existing drafts.",
        ),
        (
            "report",
            "feedback.report",
            "Save feedback and, only when authorized by policy, publish an improvement issue.",
        ),
    ] {
        commands.push(command!(
            &["feedback", name], "relay-knowledge feedback configure|report --input <json-file>", summary,
            operation, CommandEffect::WritesOperationalState, &[],
            &[opt("--input", Some("json-file"), true, false, "Bounded versioned JSON document; report text is never executed.", None, &[])],
            &["relay-knowledge feedback report --input feedback.json --format json"],
            &["Report defaults to local-only. Auto-submit uses the persisted target, kind allowlist, quota and RELAY_KNOWLEDGE_FEEDBACK_GITHUB_TOKEN. Raw evidence remains local."],
        ));
    }
    commands.push(command!(
        &["feedback", "status"],
        "relay-knowledge feedback status [id]",
        "Read bounded feedback status and publication policy.",
        "feedback.status",
        CommandEffect::ReadOnly,
        &[arg(
            "id",
            false,
            false,
            "Local feedback identity; omit to list retained reports.",
            None,
            &[]
        )],
        &[],
        &["relay-knowledge feedback status --format json"],
        &[],
    ));
    for (name, operation, summary, effect) in [
        (
            "preview",
            "feedback.preview",
            "Preview the exact public payload and evidence bindings.",
            CommandEffect::ReadOnly,
        ),
        (
            "submit",
            "feedback.submit",
            "Run one authorized publication attempt.",
            CommandEffect::WritesOperationalState,
        ),
        (
            "retry",
            "feedback.retry",
            "Retry a safe failure or reconcile an uncertain send; never blindly repost.",
            CommandEffect::WritesOperationalState,
        ),
        (
            "track",
            "feedback.track",
            "Refresh remote issue status without marking a fix verified.",
            CommandEffect::WritesOperationalState,
        ),
    ] {
        commands.push(command!(
            &["feedback", name], "relay-knowledge feedback preview|submit|retry|track <id>", summary,
            operation, effect, &[arg("id", true, false, "Local feedback identity.", None, &[])], &[],
            &["relay-knowledge feedback preview <id> --format json"],
            &["submit/retry can create a remote GitHub issue only under auto-submit policy. track performs remote reads only. Ambiguous creation stays awaiting-reconciliation."],
        ));
    }
    for (name, operation, summary) in [
        (
            "link-fix",
            "feedback.link_fix",
            "Associate a reviewed fix reference and target version; await validation.",
        ),
        (
            "validate",
            "feedback.validate",
            "Record separately authorized runner evidence against the original scenario and expected result.",
        ),
    ] {
        commands.push(command!(
            &["feedback", name], "relay-knowledge feedback link-fix|validate <id> --input <json-file>", summary,
            operation, CommandEffect::WritesOperationalState,
            &[arg("id", true, false, "Local feedback identity.", None, &[])],
            &[opt("--input", Some("json-file"), true, false, "Version-bound fix or regression evidence JSON.", None, &[])],
            &["relay-knowledge feedback validate <id> --input validation.json --format json"],
            &["No scripts are executed. An issue closing, PR merging or release appearing cannot establish verified-fixed. Without a declared criterion, validation stays pending."],
        ));
    }
    commands
}

#[cfg(test)]
#[path = "feedback_tests.rs"]
mod tests;
