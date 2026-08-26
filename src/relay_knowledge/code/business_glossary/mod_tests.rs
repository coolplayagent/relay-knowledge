use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::domain::CodeRepositoryRegistration;

use super::load_business_knowledge_projection;

#[test]
fn loads_only_route_authorized_glossary_from_fixed_commit() {
    let repository = git_repository(
        r#"schema_version: 1
map_version: 1
updated_at: "2026-08-26T00:00:00Z"
topics:
  - id: business-knowledge
    title: Business knowledge
    description: Authored glossary
sources:
  - id: repository-business-glossary
    topic: business-knowledge
    kind: file
    uri: .knowledge/business-glossary.yaml
    read_policy: direct
    write_policy: manual-review
    status: active
    version: 1
    source_scope: repo
routes:
  - topic: business-knowledge
    source_order:
      - repository-business-glossary
history:
  - version: 1
    action: map.init
    actor: test
    summary: Initialized business knowledge
"#,
        r#"schema_version: 1
domains:
  - id: sales
    name: Sales
  - id: support
    name: Support
terms:
  - id: conversion
    domain: sales
    canonical_name: Conversion
    definition: Completed purchase ratio
    aliases:
      - value: CVR
        kind: abbreviation
    mappings:
      - relation: represented_by
        target_kind: file
        target: src/sales.rs
  - id: conversion
    domain: support
    canonical_name: Conversion
    definition: Ticket converted to escalation
"#,
    );
    let commit = git(&repository, &["rev-parse", "HEAD"]);
    let registration = registration(&repository);

    let projection = load_business_knowledge_projection(&registration, "scope-1", &commit)
        .expect("projection should load");

    assert_eq!(projection.resolved_commit_sha, commit);
    assert_eq!(projection.sources.len(), 1);
    assert_eq!(projection.sources[0].glossary.terms.len(), 2);
    assert_eq!(
        projection.sources[0].glossary.terms[0].aliases[0].value,
        "CVR"
    );
}

#[test]
fn rejects_route_path_escape_before_reading_source() {
    let repository = git_repository(
        r#"schema_version: 1
map_version: 1
updated_at: "2026-08-26T00:00:00Z"
topics:
  - id: business-knowledge
    title: Business knowledge
    description: Authored glossary
sources:
  - id: repository-business-glossary
    topic: business-knowledge
    kind: file
    uri: ../business-glossary.yaml
    read_policy: direct
    write_policy: manual-review
    status: active
    version: 1
    source_scope: repo
routes:
  - topic: business-knowledge
    source_order: [repository-business-glossary]
history:
  - version: 1
    action: map.init
    actor: test
    summary: Initialized business knowledge
"#,
        "schema_version: 1\ndomains: []\nterms: []\n",
    );
    let commit = git(&repository, &["rev-parse", "HEAD"]);

    let error = load_business_knowledge_projection(&registration(&repository), "scope-1", &commit)
        .expect_err("unsafe source must fail");

    let message = error.to_string();
    assert!(message.contains("reserved source 'repository-business-glossary'"));
    assert!(message.contains("uri '.knowledge/business-glossary.yaml'"));
}

fn git_repository(map: &str, glossary: &str) -> TestRepository {
    let repository = TestRepository::new();
    fs::create_dir_all(repository.path().join(".knowledge")).expect("knowledge directory");
    fs::write(repository.path().join(".knowledge/knowledge-map.yaml"), map).expect("write map");
    fs::write(
        repository.path().join(".knowledge/business-glossary.yaml"),
        glossary,
    )
    .expect("write glossary");
    git(&repository, &["init"]);
    git(
        &repository,
        &["config", "user.email", "tests@example.invalid"],
    );
    git(
        &repository,
        &["config", "user.name", "Relay Knowledge Tests"],
    );
    git(&repository, &["add", ".knowledge"]);
    git(&repository, &["commit", "-m", "Add business knowledge"]);
    repository
}

fn registration(repository: &TestRepository) -> CodeRepositoryRegistration {
    CodeRepositoryRegistration::new(
        "repository-1",
        "fixture",
        repository.path().to_string_lossy(),
        Vec::new(),
        Vec::new(),
    )
    .expect("registration")
}

fn git(repository: &TestRepository, arguments: &[&str]) -> String {
    git_path(repository.path(), arguments)
}

struct TestRepository(PathBuf);

impl TestRepository {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "relay-knowledge-business-glossary-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temporary repository");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRepository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git_path(repository: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .output()
        .expect("git command");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        arguments,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("UTF-8 git output")
        .trim()
        .to_owned()
}
