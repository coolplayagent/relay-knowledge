//! Generated Repository Map v4 fixture used by index-backed acceptance cases.

use std::path::Path;

use sha2::{Digest, Sha256};

use super::writer::write_fixture_file;

struct MapSourceSpec {
    id: &'static str,
    kind: &'static str,
    uri: &'static str,
    source_scope: &'static str,
    description: &'static str,
}

struct MapTopicSpec {
    id: &'static str,
    title: &'static str,
    description: &'static str,
    sources: &'static [MapSourceSpec],
}

const ARCHITECTURE_SOURCES: &[MapSourceSpec] = &[
    MapSourceSpec {
        id: "architecture-guide",
        kind: "doc",
        uri: "docs/architecture.md",
        source_scope: "docs",
        description: "Reviewed architecture boundaries and graph ownership.",
    },
    MapSourceSpec {
        id: "architecture-runtime-config",
        kind: "config",
        uri: "config/runtime.yaml",
        source_scope: "repo",
        description: "Runtime configuration connected to the architecture view.",
    },
];

const BUSINESS_SOURCES: &[MapSourceSpec] = &[MapSourceSpec {
    id: "repository-business-glossary",
    kind: "file",
    uri: "knowledge/glossary/business-glossary.yaml",
    source_scope: "repo",
    description: "Authored business terms for the indexed repository scope.",
}];

const BUILD_SOURCES: &[MapSourceSpec] = &[MapSourceSpec {
    id: "build-manifest",
    kind: "config",
    uri: "Cargo.toml",
    source_scope: "repo",
    description: "Build and dependency manifest evidence.",
}];

const DEPLOYMENT_SOURCES: &[MapSourceSpec] = &[MapSourceSpec {
    id: "deployment-manifest",
    kind: "config",
    uri: "k8s/app.yaml",
    source_scope: "repo",
    description: "Deployment topology and runtime resource evidence.",
}];

const OPERATIONS_SOURCES: &[MapSourceSpec] = &[MapSourceSpec {
    id: "operations-runbook",
    kind: "doc",
    uri: "docs/operations.md",
    source_scope: "docs",
    description: "Operational recovery and observability guidance.",
}];

const RUNTIME_SOURCES: &[MapSourceSpec] = &[MapSourceSpec {
    id: "runtime-service",
    kind: "runtime",
    uri: "systemd/relay-map.service",
    source_scope: "repo",
    description: "Managed background service definition.",
}];

const SECURITY_SOURCES: &[MapSourceSpec] = &[MapSourceSpec {
    id: "security-model",
    kind: "doc",
    uri: "docs/security.md",
    source_scope: "docs",
    description: "Authorization and source-boundary guidance.",
}];

const SOFTWARE_MODEL_SOURCES: &[MapSourceSpec] = &[MapSourceSpec {
    id: "repository-software-model",
    kind: "repo",
    uri: ".",
    source_scope: "repo",
    description: "Code-map-backed software model entry point.",
}];

const MAP_TOPICS: &[MapTopicSpec] = &[
    MapTopicSpec {
        id: "architecture",
        title: "Architecture",
        description: "Architecture boundaries and runtime configuration evidence.",
        sources: ARCHITECTURE_SOURCES,
    },
    MapTopicSpec {
        id: "business-knowledge",
        title: "Business knowledge",
        description: "Authored terminology tied to technical graph evidence.",
        sources: BUSINESS_SOURCES,
    },
    MapTopicSpec {
        id: "build",
        title: "Build",
        description: "Build definitions and dependency evidence.",
        sources: BUILD_SOURCES,
    },
    MapTopicSpec {
        id: "deployment",
        title: "Deployment",
        description: "Deployment resources and service topology.",
        sources: DEPLOYMENT_SOURCES,
    },
    MapTopicSpec {
        id: "operations",
        title: "Operations",
        description: "Recovery diagnostics and observable workflows.",
        sources: OPERATIONS_SOURCES,
    },
    MapTopicSpec {
        id: "runtime",
        title: "Runtime",
        description: "Managed background service behavior.",
        sources: RUNTIME_SOURCES,
    },
    MapTopicSpec {
        id: "security",
        title: "Security",
        description: "Authorization and evidence boundary policy.",
        sources: SECURITY_SOURCES,
    },
    MapTopicSpec {
        id: "software-model",
        title: "Whole-software model",
        description: "Repository code graph and software projection entry point.",
        sources: SOFTWARE_MODEL_SOURCES,
    },
];

const ORPHAN_TOPIC: MapTopicSpec = MapTopicSpec {
    id: "orphan-shadow-route",
    title: "Orphan shadow route",
    description: "A locally valid shard that the root manifest does not authorize.",
    sources: &[MapSourceSpec {
        id: "orphan-shadow-source",
        kind: "doc",
        uri: "docs/orphan.md",
        source_scope: "docs",
        description: "Unreferenced evidence that must not enter the software map.",
    }],
};

pub(super) fn write_repository_map_graph_v4_fixture(root: &Path) -> Result<(), String> {
    for (path, content) in repository_evidence_files() {
        write_fixture_file(&root.join(path), content)?;
    }
    write_fixture_file(&root.join("codespec/codespec-map.yaml"), CODESPEC_MAP)?;

    let mut topic_refs = String::new();
    for topic in MAP_TOPICS {
        let shard = topic_shard(topic);
        let digest = sha256(shard.as_bytes());
        let relative = format!(
            "topics/topic-{}-{digest}.yaml",
            &sha256(topic.id.as_bytes())[..16]
        );
        write_fixture_file(&root.join("knowledge").join(&relative), &shard)?;
        topic_refs.push_str(&topic_root_ref(topic, &relative, &digest));
    }

    let orphan_shard = topic_shard(&ORPHAN_TOPIC);
    let orphan_digest = sha256(orphan_shard.as_bytes());
    let orphan_relative = format!(
        "topics/topic-{}-{orphan_digest}.yaml",
        &sha256(ORPHAN_TOPIC.id.as_bytes())[..16]
    );
    write_fixture_file(&root.join("knowledge").join(orphan_relative), &orphan_shard)?;

    let knowledge_map = format!("{KNOWLEDGE_MAP_HEADER}{topic_refs}{KNOWLEDGE_MAP_HISTORY}");
    write_fixture_file(&root.join("knowledge/knowledge-map.yaml"), &knowledge_map)
}

fn topic_shard(topic: &MapTopicSpec) -> String {
    let mut sources = String::new();
    let mut source_order = String::new();
    for source in topic.sources {
        sources.push_str(&format!(
            "- id: {}\n  topic: {}\n  kind: {}\n  uri: {}\n  source_scope: {}\n  read_policy: direct\n  write_policy: manual-review\n  status: active\n  version: 1\n  description: {}\n",
            source.id,
            topic.id,
            source.kind,
            source.uri,
            source.source_scope,
            source.description
        ));
        source_order.push_str(&format!("  - {}\n", source.id));
    }
    format!(
        "schema_version: 4\ntopic:\n  id: {}\n  title: {}\n  description: {}\nsources:\n{}route:\n  topic: {}\n  source_order:\n{}  fallback: bounded-search\n",
        topic.id, topic.title, topic.description, sources, topic.id, source_order
    )
}

fn topic_root_ref(topic: &MapTopicSpec, relative: &str, digest: &str) -> String {
    let source_ids = topic
        .sources
        .iter()
        .map(|source| format!("  - {}\n", source.id))
        .collect::<String>();
    format!(
        "- id: {}\n  title: {}\n  description: {}\n  source_ids:\n{}  ref: {}\n  digest: {}\n",
        topic.id, topic.title, topic.description, source_ids, relative, digest
    )
}

fn sha256(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

fn repository_evidence_files() -> [(&'static str, &'static str); 22] {
    [
        (
            "AGENTS.md",
            "CodeSpec map: codespec/codespec-map.yaml\nKnowledge map: knowledge/knowledge-map.yaml\n",
        ),
        (
            ".relay-knowledge-fixture-version",
            "repository_map_graph_v4\n",
        ),
        (
            "Cargo.toml",
            "[package]\nname = \"repository-map-graph-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nserde = \"1\"\n",
        ),
        (
            "src/lib.rs",
            "pub fn indexed_graph_version() -> u64 { 3 }\n",
        ),
        (
            "docs/architecture.md",
            "# Architecture graph\n\nThe map joins authored routes to indexed code evidence.\n",
        ),
        (
            "docs/operations.md",
            "# Operations recovery\n\nDurable workers resume from bounded checkpoints.\n",
        ),
        (
            "docs/security.md",
            "# Security boundaries\n\nOnly root-authorized topic shards enter the software map.\n",
        ),
        (
            "docs/orphan.md",
            "# Orphan evidence\n\nThis document is present but its shard is not authorized.\n",
        ),
        (
            "config/runtime.yaml",
            "runtime:\n  queue_capacity: 32\n  timeout_ms: 5000\n",
        ),
        (
            "k8s/app.yaml",
            "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: repository-map-graph\n",
        ),
        (
            "systemd/relay-map.service",
            "[Unit]\nDescription=Repository map graph fixture\n\n[Service]\nExecStart=/usr/bin/relay-map service run\n",
        ),
        (
            "knowledge/glossary/business-glossary.yaml",
            "schema_version: 1\ndomains: []\nterms: []\n",
        ),
        ("knowledge/domain/README.md", "# Domain\n"),
        ("knowledge/guides/README.md", "# Guides\n"),
        ("knowledge/ops/README.md", "# Operations\n"),
        ("knowledge/glossary/README.md", "# Glossary\n"),
        ("knowledge/best-practices/README.md", "# Best practices\n"),
        ("codespec/requirements/README.md", "# Requirements\n"),
        ("codespec/design/README.md", "# Design\n"),
        ("codespec/api/README.md", "# API\n"),
        ("codespec/test/README.md", "# Test\n"),
        ("codespec/decisions/README.md", "# Decisions\n"),
    ]
}

const KNOWLEDGE_MAP_HEADER: &str = r#"schema_version: 4
artifact_kind: map
map_type: knowledge
map_version: 1
updated_at: unix:0
directories:
- directory: domain
  purpose: Domain concepts and rules.
  content_scope: [knowledge/domain/**]
  key_files: [knowledge/domain/README.md]
  load_hint: on_demand
  relations: []
  update_rule: reviewed
- directory: guides
  purpose: Task-oriented repository guides.
  content_scope: [knowledge/guides/**]
  key_files: [knowledge/guides/README.md]
  load_hint: on_demand
  relations: []
  update_rule: reviewed
- directory: ops
  purpose: Operational diagnostics and recovery.
  content_scope: [knowledge/ops/**]
  key_files: [knowledge/ops/README.md]
  load_hint: on_demand
  relations: []
  update_rule: reviewed
- directory: glossary
  purpose: Business terminology and mappings.
  content_scope: [knowledge/glossary/**]
  key_files: [knowledge/glossary/README.md]
  load_hint: on_demand
  relations: []
  update_rule: reviewed
- directory: best-practices
  purpose: Reviewed engineering practices.
  content_scope: [knowledge/best-practices/**]
  key_files: [knowledge/best-practices/README.md]
  load_hint: on_demand
  relations: []
  update_rule: reviewed
topics:
"#;

const KNOWLEDGE_MAP_HISTORY: &str = r#"history:
  omitted_through: 0
  recent:
  - version: 1
    action: init
    actor: fixture
    summary: Created the high-dimensional repository map fixture.
"#;

const CODESPEC_MAP: &str = r#"schema_version: 4
artifact_kind: map
map_type: codespec
map_version: 1
updated_at: unix:0
directories:
- directory: requirements
  purpose: Product requirements and acceptance criteria.
  content_scope: [codespec/requirements/**]
  key_files: [codespec/requirements/README.md]
  load_hint: on_demand
  relations: []
  update_rule: reviewed
- directory: design
  purpose: Architecture and implementation designs.
  content_scope: [codespec/design/**]
  key_files: [codespec/design/README.md]
  load_hint: on_demand
  relations: []
  update_rule: reviewed
- directory: api
  purpose: Public interface contracts.
  content_scope: [codespec/api/**]
  key_files: [codespec/api/README.md]
  load_hint: on_demand
  relations: []
  update_rule: reviewed
- directory: test
  purpose: Verification strategy and evidence.
  content_scope: [codespec/test/**]
  key_files: [codespec/test/README.md]
  load_hint: on_demand
  relations: []
  update_rule: reviewed
- directory: decisions
  purpose: Durable architecture decisions.
  content_scope: [codespec/decisions/**]
  key_files: [codespec/decisions/README.md]
  load_hint: on_demand
  relations: []
  update_rule: reviewed
topics: []
history:
  omitted_through: 0
  recent:
  - version: 1
    action: init
    actor: fixture
    summary: Created the CodeSpec map fixture.
"#;
