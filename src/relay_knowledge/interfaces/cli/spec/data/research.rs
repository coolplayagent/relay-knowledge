//! Explicit local research audit surfaces.
use super::super::{CliCommandSpec, CommandEffect, command_syntax, opt};

pub(super) fn command_specs() -> Vec<CliCommandSpec> {
    let mut commands = vec![command!(
        &["sources", "audit"],
        "relay-knowledge sources audit --root <repository> --input <catalog.json>",
        "Verify local original/extracted bytes and report independent access, coverage and review states.",
        "sources.audit",
        CommandEffect::ReadOnly,
        &[],
        &[
            opt(
                "--root",
                Some("path"),
                true,
                false,
                "Authorized repository root.",
                None,
                &[]
            ),
            opt(
                "--input",
                Some("path"),
                true,
                false,
                "Repository-relative relay-capture-v1 JSON catalog.",
                None,
                &[]
            ),
        ],
        &["relay-knowledge sources audit --root . --input sources/catalog.json --format json"],
        &[
            "Local audit never downloads, logs in, executes source content, modifies originals, or upgrades a review claim to verified approval.",
            "Raw bytes including CRLF and whitespace are hashed unchanged. Coverage is unknown without caller-declared sections; present sections do not prove full-body capture.",
            "Read budgets: 256 captures, 16 MiB per artifact, 256 MiB total, four workers and a 30-second deadline; paths reject symlinks and traversal.",
        ],
    )];
    for (name, usage, operation, effect, flags) in [
        (
            "validate",
            "relay-knowledge evidence validate --root <repository> --input <bundle.json> --scope <scope>",
            "evidence.validate",
            CommandEffect::ReadOnly,
            &["--root", "--input", "--scope"][..],
        ),
        (
            "import",
            "relay-knowledge evidence import --root <repository> --input <bundle.json> --scope <scope>",
            "evidence.import",
            CommandEffect::WritesGraph,
            &["--root", "--input", "--scope"][..],
        ),
        (
            "export",
            "relay-knowledge evidence export --id <id> --scope <scope> --revision <sha256>",
            "evidence.export",
            CommandEffect::ReadOnly,
            &["--id", "--scope", "--revision"][..],
        ),
        (
            "view",
            "relay-knowledge evidence view --root <repository> --input <bundle.json> --scope <scope> [--focus <node-id>]",
            "evidence.view",
            CommandEffect::ReadOnly,
            &["--root", "--input", "--scope", "--focus"][..],
        ),
        (
            "impact",
            "relay-knowledge evidence impact --root <repository> --input <bundle.json> --scope <scope> --node <id> --label <label>",
            "evidence.impact",
            CommandEffect::ReadOnly,
            &["--root", "--input", "--scope", "--node", "--label"][..],
        ),
    ] {
        let options = flags
            .iter()
            .map(|flag| {
                opt(
                    flag,
                    Some("value"),
                    *flag != "--focus",
                    false,
                    "Explicit authored-bundle selection; paths are repository-relative.",
                    None,
                    &[],
                )
            })
            .collect::<Vec<_>>();
        commands.push(command!(
            &["evidence", name], usage,
            "Validate, import, export or inspect a scoped authored evidence bundle.",
            operation, effect, &[], &options, &[],
            &[
                "Author-declared statuses never grant acceptance; imported evidence, relations and supersession claims are proposed.",
                "Views include JSON and escaped Mermaid. Impact returns a proposed revision with stable ids, aliases and explicit supersession; it does not edit originals.",
            ],
        ));
    }
    commands.push(command!(
        &["research", "status"],
        "relay-knowledge research status --root <repository> --delivery <archive|authored_graph|graphrag> [--catalog <path>] [--bundle <path> --scope <scope>] [--requirements <path>]",
        "Report navigation, captures, authored graph and matching index state independently.",
        "research.status", CommandEffect::ReadOnly, &[],
        &[
            opt("--root", Some("path"), true, false, "Authorized repository root.", None, &[]),
            opt("--delivery", Some("kind"), true, false, "Required delivery layer.", None, &["archive","authored_graph","graphrag"]),
            opt("--catalog", Some("path"), false, false, "Repository-relative capture catalog.", None, &[]),
            opt("--bundle", Some("path"), false, false, "Repository-relative authored bundle.", None, &[]),
            opt("--scope", Some("scope"), false, false, "Explicit bundle authorization scope.", None, &[]),
            opt("--requirements", Some("path"), false, false, "Repository-relative source-bound requirement manifest.", None, &[]),
        ], &[],
        &[
            "Archive and authored-graph deliveries do not require repository registration. GraphRAG requires the matching retrieval target.",
            "Freshness, parser integrity, author review and content completion are independent. File existence or matching hashes never prove a semantic requirement satisfied.",
        ],
    ));
    commands
}
