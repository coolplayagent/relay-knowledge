use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

const ALIAS: &str = "knowledge-loop-fixture";

#[test]
fn business_bootstrap_explains_uncommitted_empty_and_filtered_snapshots() {
    let fixture = AcceptanceFixture::create();
    let initialized = fixture.cli(["map", "init", "--format", "json"]);
    assert_eq!(
        selected_map(&initialized, "knowledge")["business_bootstrap"]["schema_version"],
        1
    );
    assert!(
        selected_map(&initialized, "codespec")
            .get("business_bootstrap")
            .is_none()
    );
    fixture.cli([
        "repo",
        "register",
        fixture.repository_text(),
        "--alias",
        ALIAS,
        "--path",
        "src",
        "--format",
        "json",
    ]);
    fixture.cli(["repo", "index", ALIAS, "--ref", "HEAD", "--format", "json"]);
    let absent = fixture.cli([
        "repo", "business", ALIAS, "--kind", "all", "--ref", "HEAD", "--format", "json",
    ]);
    assert_eq!(absent["diagnostics"]["reason"], "no_business_sources");
    assert_eq!(absent["result"]["status"], "unavailable");
    assert_eq!(absent["knowledge"]["state"], "no_sources");
    assert_eq!(absent["knowledge"]["source_count"], 0);
    assert_eq!(absent["knowledge"]["stale"], false);
    assert_eq!(absent["scope"]["resolved_commit_sha"], fixture.commit);

    git(&fixture.repository, ["add", "codespec", "knowledge"]);
    git(
        &fixture.repository,
        ["commit", "-m", "commit empty glossary"],
    );
    fixture.cli(["repo", "index", ALIAS, "--ref", "HEAD", "--format", "json"]);
    let empty = fixture.cli([
        "repo", "business", ALIAS, "--kind", "all", "--ref", "HEAD", "--format", "json",
    ]);
    assert_eq!(empty["diagnostics"]["reason"], "empty_glossary");
    assert_eq!(empty["result"]["status"], "no_match");
    assert_eq!(empty["knowledge"]["state"], "empty_glossary");
    assert_eq!(empty["knowledge"]["source_count"], 1);
    assert_eq!(empty["knowledge"]["term_count"], 0);
    assert_eq!(empty["knowledge"]["stale"], false);

    let glossary_path = "knowledge/glossary/business-glossary.yaml";
    let example = empty["diagnostics"]["bootstrap"]["schema_example"]
        .as_str()
        .unwrap()
        .replace("src/billing.rs", "src/lib.rs");
    write(&fixture.repository, glossary_path, &example);
    // Even re-indexing HEAD must ignore an authored but uncommitted glossary.
    fixture.cli(["repo", "index", ALIAS, "--ref", "HEAD", "--format", "json"]);
    let uncommitted = fixture.cli([
        "repo", "business", ALIAS, "--ref", "HEAD", "--format", "json",
    ]);
    assert_eq!(uncommitted["diagnostics"]["reason"], "empty_glossary");
    git(&fixture.repository, ["add", glossary_path]);
    git(
        &fixture.repository,
        ["commit", "-m", "author mapped business term"],
    );
    fixture.cli(["repo", "index", ALIAS, "--ref", "HEAD", "--format", "json"]);
    let populated = fixture.cli([
        "repo", "business", ALIAS, "--ref", "HEAD", "--format", "json",
    ]);
    assert!(populated.get("diagnostics").is_none());
    assert_eq!(populated["knowledge"]["term_count"], 1);
    assert_eq!(populated["knowledge"]["mapping_count"], 1);
    assert_eq!(
        populated["terms"][0]["mappings"][0]["resolution_state"],
        "resolved"
    );
    assert!(populated.get("resolution").is_none());
    assert!(populated.get("status").is_none());
    assert_eq!(populated["request"]["mode"], "list");
    assert_eq!(populated["result"]["status"], "matched");
    assert_eq!(populated["knowledge"]["state"], "mapped");
    let exact = fixture.cli([
        "repo", "business", ALIAS, "--query", "MRR", "--kind", "terms", "--format", "json",
    ]);
    assert_eq!(exact["request"]["mode"], "search");
    assert_eq!(exact["result"]["status"], "matched");
    assert_eq!(exact["result"]["match_type"], "exact");
    assert_eq!(exact["result"]["returned_term_count"], 1);
    assert_eq!(exact["result"]["returned_mapping_count"], 0);
    assert_eq!(exact["knowledge"]["mapping_count"], 1);
    assert!(exact.get("diagnostics").is_none());
    for filter in ["--query", "--domain"] {
        let unmatched = fixture.cli([
            "repo",
            "business",
            ALIAS,
            filter,
            "absent-value",
            "--ref",
            "HEAD",
            "--format",
            "json",
        ]);
        assert_eq!(unmatched["diagnostics"]["reason"], "no_match");
        assert_eq!(unmatched["result"]["status"], "no_match");
        assert_eq!(unmatched["knowledge"]["state"], "mapped");
        assert_eq!(unmatched["knowledge"]["term_count"], 1);
    }
    let graph_only = fixture.cli([
        "repo",
        "business",
        ALIAS,
        "--freshness",
        "graph-only",
        "--format",
        "json",
    ]);
    assert_eq!(graph_only["diagnostics"]["reason"], "graph_only");
    assert_eq!(graph_only["knowledge"]["state"], "unknown");
    assert_eq!(graph_only["result"]["status"], "unavailable");

    let without_mappings = example.split("    mappings:").next().unwrap();
    write(&fixture.repository, glossary_path, without_mappings);
    git(&fixture.repository, ["add", glossary_path]);
    git(
        &fixture.repository,
        ["commit", "-m", "retain term without mappings"],
    );
    fixture.cli(["repo", "index", ALIAS, "--ref", "HEAD", "--format", "json"]);
    let no_mappings = fixture.cli([
        "repo", "business", ALIAS, "--kind", "mappings", "--format", "json",
    ]);
    assert_eq!(no_mappings["diagnostics"]["reason"], "no_mappings");
    assert_eq!(no_mappings["knowledge"]["state"], "terms_only");
    assert_eq!(no_mappings["result"]["status"], "no_match");
    let terms = fixture.cli([
        "repo", "business", ALIAS, "--kind", "terms", "--format", "json",
    ]);
    assert!(terms.get("diagnostics").is_none());
    assert_eq!(terms["knowledge"]["state"], "terms_only");
    assert_eq!(terms["result"]["status"], "matched");
    assert_eq!(terms["terms"].as_array().unwrap().len(), 1);
}

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
    assert_eq!(business["knowledge"]["source_scope"], indexed_scope);
    assert_eq!(business["knowledge"]["resolved_commit_sha"], fixture.commit);
    assert_eq!(business["result"]["status"], "matched");
    assert_eq!(business["result"]["match_type"], "exact");
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

#[test]
fn map_graph_dimensions_survive_incremental_removal_and_pinned_replay() {
    let fixture = AcceptanceFixture::create();
    fixture.cli(["map", "init", "--format", "json"]);
    let dimensions = [
        "architecture",
        "build",
        "deployment",
        "operations",
        "runtime",
        "security",
    ];
    for dimension in dimensions {
        let uri = format!("docs/{dimension}.md");
        write(
            &fixture.repository,
            &uri,
            &format!("# {dimension} evidence\n\nReviewed {dimension} graph boundary.\n"),
        );
        fixture.cli([
            "map",
            "source",
            "add",
            "--type",
            "knowledge",
            "--id",
            dimension,
            "--topic",
            dimension,
            "--kind",
            "doc",
            "--uri",
            &uri,
            "--scope",
            "docs",
            "--description",
            "Reviewed graph evidence",
            "--format",
            "json",
        ]);
        let route = fixture.cli([
            "map",
            "route",
            dimension,
            "--type",
            "knowledge",
            "--format",
            "json",
        ]);
        assert_eq!(route["sources"][0]["uri"], uri);
    }
    write(
        &fixture.repository,
        "k8s/app.yaml",
        "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: graph-fixture\n",
    );
    git(&fixture.repository, ["add", "."]);
    git(
        &fixture.repository,
        ["commit", "-m", "connect graph dimensions"],
    );
    let base = git_text(&fixture.repository, ["rev-parse", "HEAD"]);
    fixture.cli([
        "repo",
        "register",
        fixture.repository_text(),
        "--alias",
        ALIAS,
        "--format",
        "json",
    ]);
    let indexed = fixture.cli(["repo", "index", ALIAS, "--ref", &base, "--format", "json"]);
    assert_eq!(indexed["checkpoint"]["state"], "completed");
    assert_eq!(indexed["status"]["stale"], false);
    let first = map_graph_projection(&fixture, &base);
    let topic_names = map_topic_names(&first);
    for name in dimensions
        .into_iter()
        .chain(["business-knowledge", "software-model"])
    {
        assert!(
            topic_names.contains(name),
            "missing dimension {name}: {topic_names:?}"
        );
        assert!(first["relationships"].as_array().unwrap().iter().any(|edge|
            edge["relationship_kind"] == "documents" && edge["target_hint"] == name));
    }
    for field in [
        "components",
        "sdk_usages",
        "files",
        "topics",
        "relationships",
        "build_targets",
        "iac_resources",
        "design_elements",
        "entities",
        "statements",
    ] {
        assert!(
            !first[field].as_array().unwrap().is_empty(),
            "missing software dimension {field}"
        );
    }
    assert_eq!(
        first["status"]["projection_schema_version"],
        relay_knowledge::domain::SOFTWARE_PROJECTION_SCHEMA_VERSION
    );
    assert_eq!(first["status"]["completeness_basis_points"], 10000);
    assert_map_graph_endpoints(&first);

    fixture.cli([
        "map",
        "source",
        "remove",
        "--type",
        "knowledge",
        "--id",
        "security",
        "--format",
        "json",
    ]);
    git(&fixture.repository, ["add", "."]);
    git(
        &fixture.repository,
        ["commit", "-m", "retire security map route"],
    );
    let head = git_text(&fixture.repository, ["rev-parse", "HEAD"]);
    let updated = fixture.cli([
        "repo", "update", ALIAS, "--base", &base, "--head", &head, "--format", "json",
    ]);
    assert_eq!(updated["checkpoint"]["state"], "completed", "{updated}");
    let latest = map_graph_projection(&fixture, &head);
    // Removing the final source preserves the authored topic and an empty route.
    // Its new content-addressed shard must replace the old shard in the graph.
    let route = fixture.cli([
        "map",
        "route",
        "security",
        "--type",
        "knowledge",
        "--format",
        "json",
    ]);
    assert_eq!(route["sources"], serde_json::json!([]));
    assert_eq!(route["route"]["source_order"], serde_json::json!([]));
    let old_topic = first["topics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|topic| topic["topic_kind"] == "knowledge_map_topic" && topic["name"] == "security")
        .unwrap();
    let new_topic = latest["topics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|topic| topic["topic_kind"] == "knowledge_map_topic" && topic["name"] == "security")
        .unwrap();
    assert_ne!(old_topic["source_path"], new_topic["source_path"]);
    assert!(
        !latest["relationships"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["relationship_kind"] == "documents"
                && edge["evidence_path"] == old_topic["source_path"]),
        "retired shards must not contribute graph edges"
    );
    assert_map_graph_endpoints(&latest);
    let replay = map_graph_projection(&fixture, &base);
    assert_eq!(replay["topics"], first["topics"]);
    assert_eq!(
        replay["relationships"], first["relationships"],
        "pinned graph edges must retain IDs and evidence"
    );

    let validated = fixture.cli(["map", "validate", "--format", "json"]);
    for kind in ["codespec", "knowledge"] {
        assert_eq!(selected_map(&validated, kind)["valid"], true);
    }
    let status = fixture.cli(["repo", "status", ALIAS, "--format", "json"]);
    assert_eq!(status["status"]["last_indexed_commit"], head);
    assert_eq!(status["status"]["stale"], false);
}

fn map_graph_projection(fixture: &AcceptanceFixture, commit: &str) -> Value {
    let response = fixture.cli([
        "repo",
        "software",
        ALIAS,
        "--kind",
        "all",
        "--ref",
        commit,
        "--freshness",
        "wait-until-fresh",
        "--limit",
        "500",
        "--format",
        "json",
    ]);
    assert_eq!(response["scope"]["resolved_commit_sha"], commit);
    assert_eq!(response["scope"]["stale"], false);
    assert_eq!(response["status"]["stale"], false);
    assert_eq!(
        response["status"]["source_scope"],
        response["scope"]["scope_id"]
    );
    response
}

fn map_topic_names(response: &Value) -> std::collections::BTreeSet<&str> {
    response["topics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|topic| topic["topic_kind"] == "knowledge_map_topic")
        .map(|topic| topic["name"].as_str().unwrap())
        .collect()
}

fn assert_map_graph_endpoints(response: &Value) {
    let files = response["files"].as_array().unwrap();
    let entities = response["entities"].as_array().unwrap();
    for edge in response["relationships"].as_array().unwrap() {
        assert_eq!(edge["source_scope"], response["scope"]["scope_id"]);
        assert!(
            files
                .iter()
                .any(|file| file["software_file_id"] == edge["source_id"]),
            "missing source: {edge}"
        );
        let (collection, key) = match edge["target_kind"].as_str().unwrap() {
            "topic" => ("topics", "topic_id"),
            "component" => ("components", "component_id"),
            "sdk_usage" => ("sdk_usages", "usage_id"),
            "configuration" => {
                assert!(
                    entities
                        .iter()
                        .any(|entity| entity["attributes"]["legacy_projection_id"]
                            == edge["target_id"]),
                    "missing configuration: {edge}"
                );
                continue;
            }
            other => panic!("unexpected edge target {other}"),
        };
        assert!(
            response[collection]
                .as_array()
                .unwrap()
                .iter()
                .any(|target| target[key] == edge["target_id"]),
            "missing target: {edge}"
        );
    }
    for statement in response["statements"].as_array().unwrap() {
        assert_eq!(statement["source_scope"], response["scope"]["scope_id"]);
        assert!(
            entities
                .iter()
                .any(|entity| entity["entity_key"] == statement["subject_id"]),
            "missing statement subject: {statement}"
        );
        if let Some(object) = statement["object_id"].as_str() {
            assert!(
                entities.iter().any(|entity| entity["entity_key"] == object),
                "missing statement object: {statement}"
            );
        }
    }
}
