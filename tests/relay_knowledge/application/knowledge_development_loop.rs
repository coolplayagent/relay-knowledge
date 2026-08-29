use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

const ALIAS: &str = "knowledge-loop-fixture";

#[test]
fn bootstrap_binds_business_software_and_context_to_one_indexed_commit() {
    let mut fixture = AcceptanceFixture::create();

    let initialized = fixture.cli(["map", "init", "--format", "json"]);
    let initial_version = selected_map(&initialized, "knowledge")["map_version"]
        .as_u64()
        .expect("map init should report its version");
    let repeated = fixture.cli(["map", "init", "--format", "json"]);
    assert_eq!(
        selected_map(&repeated, "knowledge")["map_version"],
        initial_version
    );
    write(
        &fixture.repository,
        "knowledge/glossary/business-glossary.yaml",
        r#"schema_version: 1
domains:
  - id: revenue
    name: Revenue
    description: Subscription revenue concepts.
terms:
  - id: monthly-recurring-revenue
    domain: revenue
    canonical_name: Monthly Recurring Revenue
    definition: Recurring subscription revenue normalized to one month.
    language: en
    aliases:
      - value: MRR
        kind: abbreviation
        language: en
    semantics:
      aggregation: sum
      unit: USD
      grain: subscription
      time_basis: month
    mappings:
      - relation: calculated_from
        target_kind: file
        target: src/lib.rs
"#,
    );
    git(&fixture.repository, ["add", "codespec", "knowledge"]);
    git(
        &fixture.repository,
        ["commit", "-m", "author business glossary"],
    );
    fixture.commit = git_text(&fixture.repository, ["rev-parse", "HEAD"]);

    let route = fixture.cli([
        "map",
        "route",
        "software-model",
        "--type",
        "knowledge",
        "--format",
        "json",
    ]);
    assert_eq!(
        route["route"]["source_order"],
        serde_json::json!(["repository-software-model"])
    );
    assert_eq!(route["sources"][0]["kind"], "repo");
    assert_eq!(route["sources"][0]["uri"], ".");
    assert_eq!(route["sources"][0]["source_scope"], "repo");
    let business_route = fixture.cli([
        "map",
        "route",
        "business-knowledge",
        "--type",
        "knowledge",
        "--format",
        "json",
    ]);
    assert_eq!(
        business_route["route"]["source_order"],
        serde_json::json!(["repository-business-glossary"])
    );

    fixture.cli([
        "repo",
        "register",
        fixture.repository_text(),
        "--alias",
        ALIAS,
        "--format",
        "json",
    ]);
    let indexed = fixture.cli(["repo", "index", ALIAS, "--ref", "HEAD", "--format", "json"]);
    assert_eq!(indexed["scope"]["resolved_commit_sha"], fixture.commit);
    assert_eq!(indexed["status"]["stale"], false);
    assert_eq!(indexed["checkpoint"]["state"], "completed");

    let software = fixture.cli([
        "repo",
        "software",
        ALIAS,
        "--kind",
        "all",
        "--ref",
        &fixture.commit,
        "--freshness",
        "wait-until-fresh",
        "--format",
        "json",
    ]);
    let architecture = fixture.cli([
        "repo",
        "view",
        ALIAS,
        "--kind",
        "architecture-layers",
        "--ref",
        &fixture.commit,
        "--freshness",
        "wait-until-fresh",
        "--format",
        "json",
    ]);
    let business = fixture.cli([
        "repo",
        "business",
        ALIAS,
        "--kind",
        "all",
        "--query",
        "MRR",
        "--ref",
        &fixture.commit,
        "--freshness",
        "wait-until-fresh",
        "--format",
        "json",
    ]);
    let business_domains = fixture.cli([
        "repo",
        "view",
        ALIAS,
        "--kind",
        "business-domains",
        "--ref",
        &fixture.commit,
        "--freshness",
        "wait-until-fresh",
        "--format",
        "json",
    ]);
    let context = fixture.cli([
        "repo",
        "context",
        ALIAS,
        "--query",
        "MRR",
        "--ref",
        &fixture.commit,
        "--freshness",
        "wait-until-fresh",
        "--format",
        "json",
    ]);

    let indexed_scope = indexed["scope"]["scope_id"]
        .as_str()
        .expect("index should report a source scope");
    for response in [&software, &architecture, &business, &business_domains] {
        assert_eq!(response["scope"]["scope_id"], indexed_scope);
        assert_eq!(response["scope"]["resolved_commit_sha"], fixture.commit);
        assert_eq!(response["scope"]["stale"], false);
    }
    assert_eq!(context["repository_scope"]["scope_id"], indexed_scope);
    assert_eq!(
        context["repository_scope"]["resolved_commit_sha"],
        fixture.commit
    );
    assert_eq!(context["repository_scope"]["stale"], false);
    assert_eq!(software["status"]["source_scope"], indexed_scope);
    assert_eq!(software["status"]["stale"], false);
    assert_eq!(business["status"]["source_scope"], indexed_scope);
    assert_eq!(business["status"]["resolved_commit_sha"], fixture.commit);
    assert_eq!(business["resolution"], "exact");
    assert_eq!(
        business["terms"][0]["canonical_name"],
        "Monthly Recurring Revenue"
    );
    assert_eq!(
        business["terms"][0]["mappings"][0]["resolution_state"],
        "resolved"
    );
    assert_eq!(
        business["terms"][0]["definitions"][0]["evidence"]["resolved_commit_sha"],
        fixture.commit
    );
    assert_eq!(
        context["business_context"][0]["id"],
        "monthly-recurring-revenue"
    );
    assert!(
        business_domains["evidence"]
            .as_array()
            .is_some_and(|evidence| {
                evidence
                    .iter()
                    .any(|item| item["evidence_kind"] == "business_glossary")
            })
    );
    assert!(
        software["files"]
            .as_array()
            .is_some_and(|files| !files.is_empty())
    );
    assert!(
        software["sdk_usages"].as_array().is_some_and(|usages| {
            usages.iter().any(|usage| {
                usage["resolution_state"] == "unresolved"
                    && usage["target_hint"]
                        .as_str()
                        .is_some_and(|hint| hint.contains("vendor_sdk"))
                    && usage["evidence_path"] == "src/lib.rs"
            })
        }),
        "software projection should retain unresolved SDK metadata: {}",
        software["sdk_usages"]
    );
    assert!(
        architecture["evidence"]
            .as_array()
            .is_some_and(|evidence| !evidence.is_empty())
    );

    let validated = fixture.cli(["map", "validate", "--format", "json"]);
    for map_type in ["codespec", "knowledge"] {
        let result = selected_map(&validated, map_type);
        assert_eq!(
            result["valid"], true,
            "final {map_type} map validation should succeed: {validated}"
        );
        assert_eq!(result["diagnostics"], serde_json::json!([]));
    }
}

struct AcceptanceFixture {
    repository: PathBuf,
    runtime: PathBuf,
    commit: String,
}

impl AcceptanceFixture {
    fn create() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after the epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("relay-knowledge-loop-{nonce}"));
        let repository = root.join("repository");
        let runtime = root.join("runtime");
        fs::create_dir_all(repository.join("src")).expect("source directory should exist");
        fs::create_dir_all(repository.join("docs")).expect("docs directory should exist");
        fs::create_dir_all(&runtime).expect("runtime directory should exist");

        write(
            &repository,
            "Cargo.toml",
            "[package]\nname = \"knowledge-loop-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nserde = \"1\"\n",
        );
        write(
            &repository,
            "src/lib.rs",
            "use vendor_sdk::Client;\n\npub fn client() -> Option<Client> { None }\n",
        );
        write(
            &repository,
            "docs/architecture.md",
            "# Architecture\n\nThe application layer consumes an external SDK boundary.\n",
        );
        write(
            &repository,
            "AGENTS.md",
            "CodeSpec map: codespec/codespec-map.yaml\nKnowledge map: knowledge/knowledge-map.yaml\n",
        );
        git(&repository, ["init"]);
        git(
            &repository,
            ["config", "user.email", "relay@example.invalid"],
        );
        git(&repository, ["config", "user.name", "Relay Test"]);
        git(&repository, ["add", "."]);
        git(&repository, ["commit", "-m", "initial knowledge fixture"]);
        let commit = git_text(&repository, ["rev-parse", "HEAD"]);

        Self {
            repository,
            runtime,
            commit,
        }
    }

    fn repository_text(&self) -> &str {
        self.repository
            .to_str()
            .expect("fixture repository path should be UTF-8")
    }

    fn cli<const N: usize>(&self, args: [&str; N]) -> Value {
        let mut command = Command::new(env!("CARGO_BIN_EXE_relay-knowledge"));
        command
            .current_dir(&self.repository)
            .env_clear()
            .env("HOME", self.runtime.join("home"))
            .env("TMPDIR", self.runtime.join("tmp"))
            .env("RELAY_KNOWLEDGE_HOME", self.runtime.join("relay"))
            .env("RELAY_KNOWLEDGE_SEMANTIC_BACKEND", "local")
            .env("RELAY_KNOWLEDGE_VECTOR_BACKEND", "local")
            .args(args);
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
        let output = command.output().expect("relay-knowledge should run");
        assert!(
            output.status.success(),
            "relay-knowledge failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("relay-knowledge stdout should be JSON")
    }
}

fn selected_map<'a>(response: &'a Value, map_type: &str) -> &'a Value {
    response["results"]
        .as_array()
        .and_then(|results| results.iter().find(|result| result["map_type"] == map_type))
        .unwrap_or_else(|| panic!("response should contain {map_type} map result: {response}"))
}

impl Drop for AcceptanceFixture {
    fn drop(&mut self) {
        let root = self
            .repository
            .parent()
            .expect("fixture repository should have a root");
        let _ = fs::remove_dir_all(root);
    }
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("fixture parent directory should exist");
    }
    fs::write(path, content).expect("fixture content should be written");
}

fn git<const N: usize>(repository: &Path, args: [&str; N]) {
    let output = Command::new("git")
        .current_dir(repository)
        .args(args)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_text<const N: usize>(repository: &Path, args: [&str; N]) -> String {
    let output = Command::new("git")
        .current_dir(repository)
        .args(args)
        .output()
        .expect("git should run");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("git output should be UTF-8")
        .trim()
        .to_owned()
}
