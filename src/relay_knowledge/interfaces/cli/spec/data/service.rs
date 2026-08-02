//! Installed-service lifecycle, operator, worker, and foreground specifications.

use super::super::{CliCommandSpec, CliOptionSpec, CommandEffect, arg, command_syntax, opt};

pub(super) fn command_specs() -> Vec<CliCommandSpec> {
    vec![
        service_status(),
        service_doctor(),
        service_plan(),
        service_lifecycle(),
        service_definition_write(),
        service_operator(),
        service_worker(),
        service_run(),
    ]
}

fn service_status() -> CliCommandSpec {
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
    )
}

fn service_doctor() -> CliCommandSpec {
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
    )
}

fn service_plan() -> CliCommandSpec {
    command!(
        &["service", "plan"],
        "relay-knowledge service plan install|upgrade|rollback|uninstall [--target-version <version>] [--install-dir <path>]",
        "Preview service lifecycle commands.",
        "service.plan",
        CommandEffect::ReadOnly,
        &[arg(
            "action",
            true,
            false,
            "Service manager action to plan.",
            None,
            &["install", "upgrade", "rollback", "uninstall"],
        )],
        &service_lifecycle_options(false),
        &["relay-knowledge service plan upgrade --target-version 1.2.3 --format json"],
        &[
            "Returns dry-run lifecycle steps, permissions, runtime paths, rollback plan, and package manifest checks without executing platform commands."
        ],
    )
}

fn service_lifecycle() -> CliCommandSpec {
    command!(
        &["service", "lifecycle"],
        "relay-knowledge service lifecycle install|upgrade|rollback|uninstall [--dry-run|--execute] [--target-version <version>] [--install-dir <path>]",
        "Run or dry-run a staged service lifecycle plan.",
        "service.lifecycle",
        CommandEffect::WritesServiceDefinition,
        &[arg(
            "action",
            true,
            false,
            "Service lifecycle action to run or dry-run.",
            None,
            &["install", "upgrade", "rollback", "uninstall"],
        )],
        &service_lifecycle_options(true),
        &[
            "relay-knowledge service lifecycle install --dry-run --format json",
            "relay-knowledge service lifecycle upgrade --execute --target-version 1.2.3 --install-dir /opt/relay-knowledge --format json",
        ],
        &[
            "Defaults to dry-run. `--execute` runs local file steps and platform service-manager commands, rolling back completed steps if a later step fails."
        ],
    )
}

fn service_definition_write() -> CliCommandSpec {
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
    )
}

fn service_lifecycle_options(include_execute: bool) -> Vec<CliOptionSpec> {
    let mut options = vec![
        opt(
            "--target-version",
            Some("version"),
            false,
            false,
            "Version selected for install or upgrade planning.",
            None,
            &[],
        ),
        opt(
            "--install-dir",
            Some("path"),
            false,
            false,
            "Absolute binary install directory, kept separate from runtime state paths.",
            None,
            &[],
        ),
    ];
    if include_execute {
        options.insert(
            0,
            opt(
                "--dry-run",
                None,
                false,
                false,
                "Render the lifecycle plan without executing steps.",
                Some("true"),
                &[],
            ),
        );
        options.insert(
            1,
            opt(
                "--execute",
                None,
                false,
                false,
                "Execute staged local file steps and platform service-manager commands.",
                None,
                &[],
            ),
        );
    }
    options
}

fn service_worker() -> CliCommandSpec {
    command!(
        &["service", "worker", "run"],
        "relay-knowledge service worker run [--task-id <id>]",
        "Run one preview split-worker code-index task through durable leases.",
        "service.worker.run",
        CommandEffect::WritesIndexes,
        &[],
        &[opt(
            "--task-id",
            Some("id"),
            false,
            false,
            "Specific durable code-index task to claim.",
            None,
            &[],
        )],
        &["relay-knowledge service worker run --format json"],
        &[
            "Claims at most one queued code-index task and completes or fails it through the storage lease contract.",
        ],
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

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
