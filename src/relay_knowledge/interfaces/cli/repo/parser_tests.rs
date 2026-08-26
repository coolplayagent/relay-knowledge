use super::*;

#[test]
fn parses_business_query_with_domain_and_fixed_ref() {
    assert_eq!(
        parse_repo(&[
            "business".to_owned(),
            "repo".to_owned(),
            "--kind".to_owned(),
            "mappings".to_owned(),
            "--query".to_owned(),
            "monthly revenue".to_owned(),
            "--domain".to_owned(),
            "sales".to_owned(),
            "--ref".to_owned(),
            "abc123".to_owned(),
        ])
        .expect("business command"),
        RepoCommand::Business {
            alias: "repo".to_owned(),
            ref_selector: "abc123".to_owned(),
            domain: Some("sales".to_owned()),
            query: Some("monthly revenue".to_owned()),
            kind: BusinessKnowledgeQueryKind::Mappings,
            freshness: FreshnessPolicy::AllowStale,
            limit: 100,
        }
    );
}
use crate::domain::CodeQueryKind;

#[test]
fn parses_snapshot_scoped_repository_graph() {
    let command = parse_repo(&[
        "graph".to_owned(),
        "core".to_owned(),
        "--focus".to_owned(),
        "knowledge/research/rates.md".to_owned(),
        "--path".to_owned(),
        "knowledge/research".to_owned(),
        "--ref".to_owned(),
        "main".to_owned(),
        "--depth".to_owned(),
        "2".to_owned(),
        "--node-limit".to_owned(),
        "40".to_owned(),
        "--edge-limit".to_owned(),
        "80".to_owned(),
    ])
    .expect("repo graph should parse");

    assert_eq!(
        command,
        RepoCommand::Graph {
            alias: "core".to_owned(),
            focus_path: "knowledge/research/rates.md".to_owned(),
            depth: 2,
            ref_selector: "main".to_owned(),
            path_filters: vec!["knowledge/research".to_owned()],
            node_limit: 40,
            edge_limit: 80,
        }
    );
}

#[test]
fn parses_repo_query_with_kind_filters_and_freshness() {
    let command = parse_repo(&[
        "query".to_owned(),
        "core".to_owned(),
        "--query".to_owned(),
        "RetryPolicy".to_owned(),
        "--kind".to_owned(),
        "references".to_owned(),
        "--path".to_owned(),
        "src".to_owned(),
        "--language".to_owned(),
        "rust".to_owned(),
        "--freshness".to_owned(),
        "wait-until-fresh".to_owned(),
    ])
    .expect("repo query should parse");

    assert_eq!(
        command,
        RepoCommand::Query {
            alias: "core".to_owned(),
            query: "RetryPolicy".to_owned(),
            kind: CodeQueryKind::References,
            limit: 10,
            ref_selector: "HEAD".to_owned(),
            path_filters: vec!["src".to_owned()],
            language_filters: vec!["rust".to_owned()],
            freshness: FreshnessPolicy::WaitUntilFresh,
            exclude_generated: false,
        }
    );
}

#[test]
fn parses_repo_query_exclude_generated_flag() {
    let command = parse_repo(&[
        "query".to_owned(),
        "core".to_owned(),
        "--query".to_owned(),
        "RetryPolicy".to_owned(),
        "--exclude-generated".to_owned(),
    ])
    .expect("repo query should parse generated exclusion");

    assert!(matches!(
        command,
        RepoCommand::Query {
            exclude_generated: true,
            ..
        }
    ));
}

#[test]
fn parses_repo_context_with_budget_and_code_controls() {
    let command = parse_repo(&[
        "context".to_owned(),
        "core".to_owned(),
        "--query".to_owned(),
        "RetryPolicy".to_owned(),
        "callers".to_owned(),
        "--ref".to_owned(),
        "main".to_owned(),
        "--path".to_owned(),
        "src".to_owned(),
        "--language".to_owned(),
        "rust".to_owned(),
        "--freshness".to_owned(),
        "wait-until-fresh".to_owned(),
        "--limit".to_owned(),
        "7".to_owned(),
        "--max-context-bytes".to_owned(),
        "4096".to_owned(),
        "--no-code".to_owned(),
        "--exclude-generated".to_owned(),
    ])
    .expect("repo context should parse");

    assert_eq!(
        command,
        RepoCommand::Context {
            alias: "core".to_owned(),
            query: "RetryPolicy callers".to_owned(),
            limit: 7,
            ref_selector: "main".to_owned(),
            path_filters: vec!["src".to_owned()],
            language_filters: vec!["rust".to_owned()],
            freshness: FreshnessPolicy::WaitUntilFresh,
            max_context_bytes: 4096,
            include_code: false,
            exclude_generated: true,
        }
    );
}

#[test]
fn parses_repo_view_with_filters_and_changed_paths() {
    let command = parse_repo(&[
        "view".to_owned(),
        "core".to_owned(),
        "--kind".to_owned(),
        "affected-scope".to_owned(),
        "--ref".to_owned(),
        "worktree".to_owned(),
        "--path".to_owned(),
        "src".to_owned(),
        "--language".to_owned(),
        "rust".to_owned(),
        "--freshness".to_owned(),
        "wait-until-fresh".to_owned(),
        "--limit".to_owned(),
        "12".to_owned(),
        "--changed-path".to_owned(),
        "src/lib.rs".to_owned(),
    ])
    .expect("repo view should parse");

    let RepoCommand::View(command) = command else {
        panic!("expected view command");
    };
    assert_eq!(command.alias, "core");
    assert_eq!(command.kind, crate::domain::CodebaseViewKind::AffectedScope);
    assert_eq!(command.ref_selector, "worktree");
    assert_eq!(command.path_filters, ["src"]);
    assert_eq!(command.language_filters, ["rust"]);
    assert_eq!(command.freshness, FreshnessPolicy::WaitUntilFresh);
    assert_eq!(command.limit, 12);
    assert_eq!(command.changed_paths, ["src/lib.rs"]);
}

#[test]
fn parses_repo_feature_flags_with_optional_filter_and_scope() {
    let command = parse_repo(&[
        "feature-flags".to_owned(),
        "core".to_owned(),
        "--query".to_owned(),
        "checkout".to_owned(),
        "--ref".to_owned(),
        "HEAD".to_owned(),
        "--path".to_owned(),
        "src".to_owned(),
        "--language".to_owned(),
        "rust".to_owned(),
        "--freshness".to_owned(),
        "wait-until-fresh".to_owned(),
        "--limit".to_owned(),
        "20".to_owned(),
    ])
    .expect("feature flags command should parse");

    assert_eq!(
        command,
        RepoCommand::FeatureFlags {
            alias: "core".to_owned(),
            query: Some("checkout".to_owned()),
            limit: 20,
            ref_selector: "HEAD".to_owned(),
            path_filters: vec!["src".to_owned()],
            language_filters: vec!["rust".to_owned()],
            freshness: FreshnessPolicy::WaitUntilFresh,
        }
    );
}

#[test]
fn parses_repo_command_forms_and_validation_errors() {
    assert_eq!(
        parse_repo(&["list".to_owned()]).expect("list command should parse"),
        RepoCommand::List
    );
    assert_eq!(
        parse_repo(&["list".to_owned(), "extra".to_owned()])
            .expect_err("list command should reject arguments"),
        CliError::UnexpectedArgument("extra".to_owned())
    );
    let register = parse_repo(&[
        "register".to_owned(),
        "/work/repo".to_owned(),
        "--alias".to_owned(),
        "core".to_owned(),
        "--path".to_owned(),
        "src".to_owned(),
    ])
    .expect("register command should parse");
    assert_eq!(
        register,
        RepoCommand::Register {
            root_path: "/work/repo".to_owned(),
            alias: "core".to_owned(),
            path_filters: vec!["src".to_owned()],
            language_filters: Vec::new(),
        }
    );
    assert_eq!(
        parse_repo(&["register".to_owned(), "/work/repo".to_owned()])
            .expect("register without alias should parse"),
        RepoCommand::Register {
            root_path: "/work/repo".to_owned(),
            alias: String::new(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
        }
    );
    assert_eq!(
        parse_repo(&["remove".to_owned(), "core".to_owned()]).expect("remove command should parse"),
        RepoCommand::Remove {
            alias: "core".to_owned()
        }
    );

    assert_eq!(
        parse_repo(&["index".to_owned(), "core".to_owned()]).expect("index command should parse"),
        RepoCommand::Index {
            alias: "core".to_owned(),
            ref_selector: "HEAD".to_owned(),
            dry_run: false,
            reuse_historical: false,
        }
    );
    assert_eq!(
        parse_repo(&["index".to_owned(), "core".to_owned(), "--reset".to_owned()])
            .expect("index reset flag should parse"),
        RepoCommand::IndexReset {
            alias: "core".to_owned(),
        }
    );
    assert_eq!(
        parse_repo(&["index".to_owned(), "--reset".to_owned(), "core".to_owned()])
            .expect("index reset prefix should parse"),
        RepoCommand::IndexReset {
            alias: "core".to_owned(),
        }
    );
    assert_eq!(
        parse_repo(&[
            "index".to_owned(),
            "core".to_owned(),
            "--ref".to_owned(),
            "HEAD".to_owned(),
            "--reset".to_owned(),
        ])
        .expect_err("reset should reject explicit refs"),
        CliError::UnexpectedArgument("--ref".to_owned())
    );
    assert_eq!(
        parse_repo(&[
            "index".to_owned(),
            "core".to_owned(),
            "--dry-run".to_owned(),
            "--reset".to_owned(),
        ])
        .expect_err("reset should reject dry-run"),
        CliError::UnexpectedArgument("--dry-run".to_owned())
    );
    assert_eq!(
        parse_repo(&["index-worker".to_owned()]).expect("index worker should parse"),
        RepoCommand::IndexWorker { task_id: None }
    );
    assert_eq!(
        parse_repo(&[
            "index-worker".to_owned(),
            "--task-id".to_owned(),
            "code-index-task:1".to_owned(),
        ])
        .expect("task-specific index worker should parse"),
        RepoCommand::IndexWorker {
            task_id: Some("code-index-task:1".to_owned())
        }
    );
    assert_eq!(
        parse_repo(&[
            "index".to_owned(),
            "core".to_owned(),
            "--dry-run".to_owned(),
            "--ref".to_owned(),
            "main".to_owned(),
        ])
        .expect("dry-run index command should parse"),
        RepoCommand::Index {
            alias: "core".to_owned(),
            ref_selector: "main".to_owned(),
            dry_run: true,
            reuse_historical: false,
        }
    );
    assert_eq!(
        parse_repo(&[
            "index".to_owned(),
            "core".to_owned(),
            "--reuse-historical".to_owned(),
        ])
        .expect("reuse-historical flag should parse"),
        RepoCommand::Index {
            alias: "core".to_owned(),
            ref_selector: "HEAD".to_owned(),
            dry_run: false,
            reuse_historical: true,
        }
    );
    assert_eq!(
        parse_repo(&[
            "index".to_owned(),
            "core".to_owned(),
            "--reuse-historical".to_owned(),
            "--reset".to_owned(),
        ])
        .expect_err("reset should reject reuse-historical"),
        CliError::UnexpectedArgument("--reuse-historical".to_owned())
    );
    assert_eq!(
        parse_repo(&[
            "scope".to_owned(),
            "preview".to_owned(),
            "core".to_owned(),
            "--ref".to_owned(),
            "main".to_owned(),
        ])
        .expect("scope preview should parse"),
        RepoCommand::ScopePreview {
            alias: "core".to_owned(),
            ref_selector: "main".to_owned(),
        }
    );
    assert_eq!(
        parse_repo(&["report".to_owned(), "core".to_owned()]).expect("report command should parse"),
        RepoCommand::Report {
            alias: "core".to_owned()
        }
    );
    assert_eq!(
        parse_repo(&[
            "update".to_owned(),
            "core".to_owned(),
            "--base".to_owned(),
            "main".to_owned(),
            "--head".to_owned(),
            "feature".to_owned(),
        ])
        .expect("update command should parse"),
        RepoCommand::Update {
            alias: "core".to_owned(),
            base_ref: Some("main".to_owned()),
            head_ref: Some("feature".to_owned()),
        }
    );
    assert_eq!(
        parse_repo(&["update".to_owned(), "core".to_owned()])
            .expect("update defaults should parse"),
        RepoCommand::Update {
            alias: "core".to_owned(),
            base_ref: None,
            head_ref: None,
        }
    );
    assert_eq!(
        parse_repo(&[
            "impact".to_owned(),
            "core".to_owned(),
            "--base".to_owned(),
            "main".to_owned(),
            "--head".to_owned(),
            "feature".to_owned(),
            "--limit".to_owned(),
            "7".to_owned(),
        ])
        .expect("impact command should parse"),
        RepoCommand::Impact {
            alias: "core".to_owned(),
            base_ref: "main".to_owned(),
            head_ref: "feature".to_owned(),
            limit: 7,
        }
    );
    assert_eq!(
        parse_repo(&["status".to_owned(), "core".to_owned()]).expect("status command should parse"),
        RepoCommand::Status {
            alias: "core".to_owned()
        }
    );

    assert_eq!(parse_query_kind("hybrid").unwrap(), CodeQueryKind::Hybrid);
    assert_eq!(parse_query_kind("symbol").unwrap(), CodeQueryKind::Symbol);
    assert_eq!(
        parse_query_kind("definition").unwrap(),
        CodeQueryKind::Definition
    );
    assert_eq!(parse_query_kind("callers").unwrap(), CodeQueryKind::Callers);
    assert_eq!(parse_query_kind("callees").unwrap(), CodeQueryKind::Callees);
    assert_eq!(parse_query_kind("imports").unwrap(), CodeQueryKind::Imports);
    assert_eq!(parse_query_kind("sbom").unwrap(), CodeQueryKind::Sbom);
    assert_eq!(
        parse_query_kind("impact").unwrap_err(),
        CliError::InvalidCodeQueryKind("impact".to_owned())
    );

    let positional_query = parse_repo(&[
        "query".to_owned(),
        "core".to_owned(),
        "RetryPolicy".to_owned(),
        "budget".to_owned(),
        "--kind".to_owned(),
        "symbol".to_owned(),
    ])
    .expect("positional query should parse");
    assert!(matches!(
        &positional_query,
        RepoCommand::Query {
            kind: CodeQueryKind::Symbol,
            ..
        }
    ));
    assert!(matches!(
        positional_query,
        RepoCommand::Query { query, .. } if query == "RetryPolicy budget"
    ));

    assert_eq!(
        parse_repo(&[]).expect_err("empty repo command should fail"),
        CliError::UnexpectedArgument("repo".to_owned())
    );
    assert_eq!(
        parse_repo(&["remove".to_owned()]).expect_err("missing remove alias should fail"),
        CliError::MissingValue("<alias>")
    );
    assert_eq!(
        parse_repo(&["remove".to_owned(), "core".to_owned(), "extra".to_owned(),])
            .expect_err("remove extra argument should fail"),
        CliError::UnexpectedArgument("extra".to_owned())
    );
    assert_eq!(
        parse_repo(&["query".to_owned(), "core".to_owned()])
            .expect_err("missing query should fail"),
        CliError::MissingValue("--query")
    );
    assert_eq!(
        parse_repo(&[
            "impact".to_owned(),
            "core".to_owned(),
            "--base".to_owned(),
            "main".to_owned(),
        ])
        .expect_err("missing head should fail"),
        CliError::MissingValue("--head")
    );
    assert_eq!(
        parse_repo(&[
            "query".to_owned(),
            "core".to_owned(),
            "--query".to_owned(),
            "RetryPolicy".to_owned(),
            "--kind".to_owned(),
            "unknown".to_owned(),
        ])
        .expect_err("unknown query kind should fail"),
        CliError::InvalidCodeQueryKind("unknown".to_owned())
    );
    assert_eq!(
        parse_repo(&[
            "impact".to_owned(),
            "core".to_owned(),
            "--base".to_owned(),
            "main".to_owned(),
            "--head".to_owned(),
            "feature".to_owned(),
            "--limit".to_owned(),
            "many".to_owned(),
        ])
        .expect_err("bad limit should fail"),
        CliError::InvalidLimit("many".to_owned())
    );
    assert_eq!(
        parse_repo(&["unknown".to_owned()]).expect_err("unknown subcommand should fail"),
        CliError::UnexpectedArgument("unknown".to_owned())
    );
}

#[test]
fn update_parser_rejects_impact_only_and_duplicate_flags() {
    let limit = parse_repo(&[
        "update".to_owned(),
        "core".to_owned(),
        "--limit".to_owned(),
        "5".to_owned(),
    ])
    .expect_err("update must not silently accept impact flags");
    assert_eq!(limit, CliError::UnexpectedArgument("--limit".to_owned()));

    let duplicate = parse_repo(&[
        "update".to_owned(),
        "core".to_owned(),
        "--head".to_owned(),
        "one".to_owned(),
        "--head".to_owned(),
        "two".to_owned(),
    ])
    .expect_err("duplicate refs should fail closed");
    assert_eq!(duplicate, CliError::UnexpectedArgument("--head".to_owned()));
}
