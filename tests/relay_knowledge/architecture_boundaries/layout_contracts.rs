use super::*;
use syn::{Attribute, Item, TraitItem};

const MAX_TRACKED_FILE_LINES: usize = 1_000;
const FOUNDATIONAL_MODULES: &[&str] = &["clock", "env", "identity", "net", "paths"];
const LARGE_OWNER_BASELINE: &[(&str, usize)] = &[
    ("src/relay_knowledge/storage/sqlite/code/mod.rs", 915),
    (
        "src/relay_knowledge/storage/sqlite/schema/migration.rs",
        838,
    ),
    (
        "src/relay_knowledge/application/code_repository/indexing/mod.rs",
        79,
    ),
    (
        "src/relay_knowledge/storage/sqlite/code/snapshot/durable_clone/mod.rs",
        665,
    ),
    ("src/relay_knowledge/storage/contracts/code.rs", 314),
    ("src/relay_knowledge/storage/sqlite/schema/marker.rs", 970),
    ("src/relay_knowledge/storage/sqlite/business/mod.rs", 19),
    (
        "src/relay_knowledge/storage/sqlite/code/tasks/retention.rs",
        796,
    ),
    (
        "src/relay_knowledge/storage/sqlite/code/query/references/mod.rs",
        959,
    ),
    (
        "src/relay_knowledge/storage/partitioned/catalog/mod.rs",
        944,
    ),
    (
        "src/relay_knowledge/storage/sqlite/code/batch/finalize/search_documents/grouped.rs",
        937,
    ),
    ("src/relay_knowledge/application/knowledge/map/mod.rs", 599),
    (
        "src/relay_knowledge/storage/sqlite/code/batch/session/finalization.rs",
        626,
    ),
    ("src/relay_knowledge/storage/partitioned/mod.rs", 814),
];

const CODE_STORAGE_CAPABILITIES: &[(&str, &str)] = &[
    ("catalog.rs", "RepositoryCatalogStore"),
    ("projection.rs", "SoftwareProjectionStore"),
    ("publication.rs", "CodeIndexPublicationStore"),
    ("query.rs", "CodeQueryReadStore"),
    ("repository_set.rs", "CodeRepositorySetStore"),
    ("retention.rs", "CodeScopeRetentionStore"),
    ("source.rs", "CodeIndexSourceStore"),
    ("task.rs", "CodeIndexTaskStore"),
];

const MAX_CODE_STORAGE_CAPABILITY_METHODS: usize = 20;

#[test]
fn foundational_capabilities_have_one_physical_owner() {
    let root = source_root();
    let mut violations = Vec::new();

    for module in FOUNDATIONAL_MODULES {
        let directory = root.join(module);
        let facade = directory.join("mod.rs");
        if !directory.is_dir() || !facade.is_file() {
            violations.push(format!(
                "{module} must be a physical module directory with a mod.rs facade"
            ));
        }

        let flat_alias = root.join(format!("{module}.rs"));
        if flat_alias.exists() {
            violations.push(format!(
                "{} duplicates the foundational `{module}` owner",
                relative_source_path(&flat_alias, &root)
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "foundational ownership violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn production_modules_are_real_bounded_owners() {
    let root = source_root();
    let mut violations = Vec::new();

    for path in production_rust_files(&root) {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let relative = relative_source_path(&path, &root);
        let line_count = source.lines().count();

        if source.trim().is_empty() {
            violations.push(format!("{relative} is empty"));
        }
        let line_budget = LARGE_OWNER_BASELINE
            .iter()
            .find(|(path, _)| *path == relative)
            .map_or(MAX_TRACKED_FILE_LINES, |(_, budget)| *budget);
        if line_count > line_budget {
            violations.push(format!(
                "{relative} has {line_count} lines; its non-increasing owner budget is {line_budget}"
            ));
        }
        for line in production_path_redirect_lines(&source, &relative) {
            violations.push(format!(
                "{relative}:{line} uses a production path redirect; move the owner into a real module"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "production module ownership violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn all_tracked_text_files_stay_within_line_budget() {
    let repository_root = source_root()
        .parent()
        .and_then(Path::parent)
        .expect("source root has repository ancestors")
        .to_path_buf();
    let output = Command::new("git")
        .args([
            "-C",
            repository_root
                .to_str()
                .expect("repository path should be valid UTF-8"),
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .output()
        .expect("git ls-files should execute");
    assert!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut violations = Vec::new();
    for path_bytes in output.stdout.split(|byte| *byte == 0) {
        if path_bytes.is_empty() {
            continue;
        }
        let relative = String::from_utf8(path_bytes.to_vec())
            .expect("tracked repository paths should be valid UTF-8");
        if relative == "Cargo.lock" {
            continue;
        }
        let bytes = fs::read(repository_root.join(&relative))
            .unwrap_or_else(|error| panic!("read tracked file {relative}: {error}"));
        if bytes.contains(&0) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let line_count = text.lines().count();
        if line_count > MAX_TRACKED_FILE_LINES {
            violations.push(format!(
                "{relative} has {line_count} lines; tracked text files may have at most {MAX_TRACKED_FILE_LINES}"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "tracked text file line-budget violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn code_repository_storage_contract_stays_capability_partitioned() {
    let root = source_root().join("storage/contracts");
    let facade = parse_rust_file(&root.join("code.rs"));
    let aggregate = facade
        .items
        .iter()
        .find_map(|item| match item {
            Item::Trait(contract) if contract.ident == "CodeRepositoryStore" => Some(contract),
            _ => None,
        })
        .expect("CodeRepositoryStore compatibility facade should exist");
    assert!(
        aggregate.items.is_empty(),
        "CodeRepositoryStore must remain a method-free compatibility facade"
    );

    for (file_name, trait_name) in CODE_STORAGE_CAPABILITIES {
        let syntax = parse_rust_file(&root.join("code").join(file_name));
        let contract = syntax
            .items
            .iter()
            .find_map(|item| match item {
                Item::Trait(contract) if contract.ident == *trait_name => Some(contract),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{file_name} must own {trait_name}"));
        let method_count = contract
            .items
            .iter()
            .filter(|item| matches!(item, TraitItem::Fn(_)))
            .count();
        assert!(
            method_count <= MAX_CODE_STORAGE_CAPABILITY_METHODS,
            "{trait_name} has {method_count} methods; split it before exceeding {MAX_CODE_STORAGE_CAPABILITY_METHODS}"
        );
    }
}

#[test]
fn application_uses_injected_process_runtime_and_narrow_repository_helpers() {
    let application_root = source_root().join("application");
    let mut process_access = Vec::new();
    for path in production_rust_files(&application_root) {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        if source.contains("std::env") || source.contains("from_process_environment") {
            process_access.push(relative_source_path(&path, &source_root()));
        }
    }
    assert!(
        process_access.is_empty(),
        "application must consume bootstrap-injected runtime snapshots; direct process access remains in:\n{}",
        process_access.join("\n")
    );

    let repository_helpers =
        fs::read_to_string(application_root.join("code_repository/repository/status.rs"))
            .expect("repository status helper should be readable");
    assert!(
        repository_helpers.contains("impl RepositoryCatalogStore")
            && repository_helpers.contains("impl CodeIndexPublicationStore")
            && !repository_helpers.contains("KnowledgeStore"),
        "high-fan-in repository helpers must retain narrow capability parameters"
    );
}

fn parse_rust_file(path: &Path) -> syn::File {
    let source =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    syn::parse_file(&source).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

fn production_path_redirect_lines(source: &str, relative_path: &str) -> Vec<usize> {
    let syntax = syn::parse_file(source)
        .unwrap_or_else(|error| panic!("parse Rust syntax from {relative_path}: {error}"));
    let mut lines = Vec::new();
    collect_path_redirects(&syntax.items, &mut lines);
    lines
}

fn collect_path_redirects(items: &[Item], lines: &mut Vec<usize>) {
    for item in items {
        let Item::Mod(module) = item else {
            continue;
        };
        if has_test_configuration(&module.attrs) {
            continue;
        }
        if let Some(attribute) = module
            .attrs
            .iter()
            .find(|attribute| attribute.path().is_ident("path"))
        {
            lines.push(attribute.pound_token.spans[0].start().line.max(1));
        }
        if let Some((_, nested)) = &module.content {
            collect_path_redirects(nested, lines);
        }
    }
}

fn has_test_configuration(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && match &attribute.meta {
                syn::Meta::List(list) => list.tokens.to_string().split_whitespace().any(|token| {
                    token.trim_matches(|character: char| {
                        !character.is_alphanumeric() && character != '_'
                    }) == "test"
                }),
                _ => false,
            }
    })
}
