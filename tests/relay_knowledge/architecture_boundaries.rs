use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
struct ForbiddenToken {
    token: &'static str,
    reason: &'static str,
}

#[derive(Clone, Copy)]
struct BaselineAllowance {
    relative_path: &'static str,
    token: &'static str,
    max_count: usize,
}

const DOMAIN_FORBIDDEN_TOKENS: &[ForbiddenToken] = &[
    ForbiddenToken {
        token: "crate::api",
        reason: "domain must not depend on interface or wire DTO layers",
    },
    ForbiddenToken {
        token: "crate::application",
        reason: "domain must not depend on use-case orchestration",
    },
    ForbiddenToken {
        token: "crate::ports",
        reason: "domain must not depend on outer port contracts",
    },
    ForbiddenToken {
        token: "crate::adapters",
        reason: "domain must not depend on concrete adapters",
    },
    ForbiddenToken {
        token: "crate::interfaces",
        reason: "domain must not depend on CLI, Web, or agent interfaces",
    },
    ForbiddenToken {
        token: "crate::storage",
        reason: "domain must not depend on persistence contracts or implementations",
    },
    ForbiddenToken {
        token: "crate::code",
        reason: "domain code rules must live under domain, not depend on the legacy code facade",
    },
    ForbiddenToken {
        token: "crate::net",
        reason: "domain must not create or depend on network capabilities",
    },
    ForbiddenToken {
        token: "crate::env",
        reason: "domain must not read process environment",
    },
    ForbiddenToken {
        token: "crate::paths",
        reason: "domain must not resolve platform runtime paths",
    },
    ForbiddenToken {
        token: "crate::observability",
        reason: "domain must not depend on concrete observability runtime",
    },
    ForbiddenToken {
        token: "crate::retrieval",
        reason: "domain must not depend on outer retrieval services",
    },
    ForbiddenToken {
        token: "crate::indexing",
        reason: "domain must not depend on indexing service modules",
    },
    ForbiddenToken {
        token: "crate::model_provider",
        reason: "domain must not depend on model provider adapters",
    },
];

const PORTS_FORBIDDEN_TOKENS: &[ForbiddenToken] = &[
    ForbiddenToken {
        token: "crate::adapters",
        reason: "ports define contracts and must not depend on adapter implementations",
    },
    ForbiddenToken {
        token: "crate::storage::SqliteGraphStore",
        reason: "ports must not expose concrete SQLite store types",
    },
    ForbiddenToken {
        token: "crate::storage::PartitionedSqliteKnowledgeStore",
        reason: "ports must not expose concrete partitioned SQLite store types",
    },
    ForbiddenToken {
        token: "SqliteGraphStore",
        reason: "ports must not expose concrete SQLite store types",
    },
    ForbiddenToken {
        token: "PartitionedSqliteKnowledgeStore",
        reason: "ports must not expose concrete partitioned SQLite store types",
    },
    ForbiddenToken {
        token: "rusqlite",
        reason: "ports must use abstract storage errors instead of SQLite errors",
    },
    ForbiddenToken {
        token: "reqwest",
        reason: "ports must describe HTTP capability without binding to reqwest",
    },
    ForbiddenToken {
        token: "tree_sitter",
        reason: "ports must describe parser capability without binding to tree-sitter",
    },
    ForbiddenToken {
        token: "axum",
        reason: "ports must not depend on HTTP server adapter libraries",
    },
    ForbiddenToken {
        token: "tower_http",
        reason: "ports must not depend on HTTP middleware adapter libraries",
    },
    ForbiddenToken {
        token: "tokio::net",
        reason: "ports must not create sockets or listeners",
    },
];

const APPLICATION_FORBIDDEN_TOKENS: &[ForbiddenToken] = &[
    ForbiddenToken {
        token: "crate::adapters",
        reason: "application must depend on ports, not concrete adapters",
    },
    ForbiddenToken {
        token: "crate::net",
        reason: "application network work must go through an HTTP/network port",
    },
    ForbiddenToken {
        token: "crate::env",
        reason: "application must receive parsed configuration instead of reading env",
    },
    ForbiddenToken {
        token: "crate::paths",
        reason: "application must receive resolved paths instead of owning platform path policy",
    },
    ForbiddenToken {
        token: "SqliteGraphStore",
        reason: "application must not construct or name concrete SQLite stores",
    },
    ForbiddenToken {
        token: "PartitionedSqliteKnowledgeStore",
        reason: "application must not construct or name concrete partitioned SQLite stores",
    },
    ForbiddenToken {
        token: "rusqlite",
        reason: "application must not depend on SQLite adapter errors or APIs",
    },
    ForbiddenToken {
        token: "reqwest",
        reason: "application outbound HTTP must go through a port",
    },
    ForbiddenToken {
        token: "tree_sitter",
        reason: "application parser work must go through a parser port",
    },
    ForbiddenToken {
        token: "axum",
        reason: "application must not depend on HTTP server adapter libraries",
    },
    ForbiddenToken {
        token: "tower_http",
        reason: "application must not depend on HTTP middleware adapter libraries",
    },
    ForbiddenToken {
        token: "tokio::net",
        reason: "application must not create sockets or listeners",
    },
    ForbiddenToken {
        token: "std::env",
        reason: "application must not read process environment directly",
    },
];

const APPLICATION_MIGRATION_BASELINE: &[BaselineAllowance] = &[
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/code_repository/support.rs",
        token: "std::env",
        max_count: 1,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/code_repository/repository_test_support.rs",
        token: "SqliteGraphStore",
        max_count: 3,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/code_repository/repository_test_support.rs",
        token: "std::env",
        max_count: 3,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/knowledge/file_index.rs",
        token: "SqliteGraphStore",
        max_count: 2,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/knowledge/file_index.rs",
        token: "std::env",
        max_count: 1,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/knowledge/map.rs",
        token: "std::env",
        max_count: 2,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/service/mod.rs",
        token: "crate::net",
        max_count: 1,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/service/lifecycle_plan.rs",
        token: "std::env",
        max_count: 1,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/service/lifecycle_plan/execution.rs",
        token: "std::env",
        max_count: 1,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/service/storage_provider.rs",
        token: "PartitionedSqliteKnowledgeStore",
        max_count: 5,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/service/storage_provider.rs",
        token: "SqliteGraphStore",
        max_count: 2,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/service/storage_provider.rs",
        token: "std::env",
        max_count: 1,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/update/mod.rs",
        token: "reqwest",
        max_count: 9,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/worker/operations.rs",
        token: "crate::net",
        max_count: 1,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/worker/operations.rs",
        token: "std::env",
        max_count: 1,
    },
];

#[test]
fn domain_does_not_reference_outer_layers() {
    let source_root = source_root();
    let violations = token_violations(
        &source_root.join("domain"),
        &source_root,
        DOMAIN_FORBIDDEN_TOKENS,
        &[],
    );

    assert_no_violations("domain onion boundary", violations);
}

#[test]
fn ports_do_not_reference_concrete_adapter_libraries() {
    let source_root = source_root();
    let ports_root = source_root.join("ports");
    if !ports_root.exists() {
        return;
    }

    let violations = token_violations(&ports_root, &source_root, PORTS_FORBIDDEN_TOKENS, &[]);

    assert_no_violations("ports adapter-library boundary", violations);
}

#[test]
fn application_infrastructure_references_do_not_exceed_migration_baseline() {
    let source_root = source_root();
    let violations = token_violations(
        &source_root.join("application"),
        &source_root,
        APPLICATION_FORBIDDEN_TOKENS,
        APPLICATION_MIGRATION_BASELINE,
    );

    assert_no_violations("application infrastructure boundary", violations);
}

fn token_violations(
    scan_root: &Path,
    source_root: &Path,
    forbidden_tokens: &[ForbiddenToken],
    baseline: &[BaselineAllowance],
) -> Vec<String> {
    rust_files(scan_root)
        .into_iter()
        .flat_map(|path| {
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            let relative_path = relative_source_path(&path, source_root);
            forbidden_tokens.iter().filter_map(move |forbidden| {
                let count = source.matches(forbidden.token).count();
                if count == 0 {
                    return None;
                }
                let allowed = baseline_count(baseline, &relative_path, forbidden.token);
                if count <= allowed {
                    return None;
                }
                Some(format!(
                    "{relative_path}: `{}` appears {count} time(s), allowed {allowed}. {}",
                    forbidden.token, forbidden.reason
                ))
            })
        })
        .collect()
}

fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_files(root, &mut files);
    files.sort();
    files
}

fn collect_rust_files(path: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(path)
        .unwrap_or_else(|error| panic!("read directory {}: {error}", path.display()))
    {
        let entry = entry.unwrap_or_else(|error| panic!("read directory entry: {error}"));
        let entry_path = entry.path();
        if entry_path.is_dir() {
            collect_rust_files(&entry_path, files);
        } else if entry_path
            .extension()
            .is_some_and(|extension| extension == "rs")
            && !is_test_source_file(&entry_path)
        {
            files.push(entry_path);
        }
    }
}

fn is_test_source_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "tests.rs" || name.ends_with("_tests.rs"))
}

fn source_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/relay_knowledge")
}

fn relative_source_path(path: &Path, source_root: &Path) -> String {
    let repository_root = source_root
        .parent()
        .and_then(Path::parent)
        .expect("source root has repository ancestors");
    let relative = path.strip_prefix(repository_root).unwrap_or(path);
    relative.to_string_lossy().replace('\\', "/")
}

fn baseline_count(baseline: &[BaselineAllowance], relative_path: &str, token: &str) -> usize {
    baseline
        .iter()
        .find(|allowance| allowance.relative_path == relative_path && allowance.token == token)
        .map_or(0, |allowance| allowance.max_count)
}

fn assert_no_violations(rule_name: &str, violations: Vec<String>) {
    assert!(
        violations.is_empty(),
        "{rule_name} violations:\n{}\nMove new references behind domain/application/ports/adapters/bootstrap boundaries or reduce the migration baseline.",
        violations.join("\n")
    );
}
