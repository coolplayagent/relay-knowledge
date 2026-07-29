use super::*;
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

#[test]
fn import_candidates_cover_common_package_roots() {
    let npm = import_match_candidates(
        "typescript",
        "import { createRoot } from \"@react-dom/client\"",
        None,
        "unresolved",
    );
    assert!(
        npm.iter()
            .any(|candidate| candidate.value == "@react-dom/client")
    );

    let rust = import_match_candidates("rust", "use serde_json::Value;", None, "unresolved");
    assert!(rust.iter().any(|candidate| candidate.value == "serde_json"));

    let python = import_match_candidates(
        "python",
        "from requests.sessions import Session",
        None,
        "unresolved",
    );
    assert!(python.iter().any(|candidate| candidate.value == "requests"));
}

#[test]
fn resolved_import_candidates_ignore_local_target_hints() {
    let candidates = import_match_candidates(
        "typescript",
        "import utils from \"./utils\";",
        Some("src/utils.ts"),
        "resolved",
    );

    assert!(candidates.is_empty());
}

#[test]
fn resolved_import_candidates_skip_local_module_text() {
    let candidates = import_match_candidates(
        "python",
        "from .requests import helper",
        Some("src/requests.py"),
        "resolved",
    );

    assert!(candidates.is_empty());
}

#[test]
fn resolved_python_import_candidates_keep_unresolved_external_parts() {
    let local_modules = local_python_modules(&["local_module"]);
    let candidates = import_match_candidates_with_python_locals(
        "python",
        "import requests, local_module",
        Some("import requests, local_module"),
        "resolved",
        Some(&local_modules),
    );

    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.value == "requests")
    );
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.value == "local_module")
    );
}

#[test]
fn resolved_python_import_candidates_skip_single_local_absolute_imports() {
    let candidates = import_match_candidates(
        "python",
        "import requests",
        Some("src/requests.py"),
        "resolved",
    );

    assert!(candidates.is_empty());
}

#[test]
fn python_file_paths_produce_import_module_keys() {
    assert_eq!(
        python::module_from_file_path("local_module.py").as_deref(),
        Some("local_module")
    );
    assert_eq!(
        python::module_from_file_path("internal/helpers.py").as_deref(),
        Some("internal.helpers")
    );
    assert_eq!(
        python::module_from_file_path("package/__init__.py").as_deref(),
        Some("package")
    );
}

#[test]
fn resolved_python_import_candidates_skip_local_modules_without_file_hints() {
    let local_modules = local_python_modules(&["internal.helpers"]);
    let candidates = import_match_candidates_with_python_locals(
        "python",
        "import requests, internal.helpers",
        Some("import requests, internal.helpers"),
        "resolved",
        Some(&local_modules),
    );

    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.value == "requests")
    );
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.value == "internal")
    );
}

#[test]
fn ambiguous_local_import_candidates_skip_local_module_text() {
    let candidates =
        import_match_candidates("python", "from .requests import helper", None, "ambiguous");

    assert!(candidates.is_empty());
}

#[test]
fn unresolved_local_import_candidates_skip_relative_python_modules() {
    let candidates =
        import_match_candidates("python", "from .requests import helper", None, "unresolved");

    assert!(candidates.is_empty());
}

#[test]
fn component_keys_cover_manifest_to_import_normalization() {
    let component = component(
        "component:serde-json",
        "cargo",
        "serde-json",
        "Cargo.toml",
        "rust",
    );

    let keys = component_match_keys(&component);
    assert!(keys.iter().any(|key| key.value == "serde_json"
        && key.confidence_basis_points == NORMALIZED_MATCH_CONFIDENCE));
}

#[test]
fn duplicate_match_keys_keep_highest_confidence() {
    let component = component("component:serde", "cargo", "serde", "Cargo.toml", "rust");

    let keys = component_match_keys(&component);
    assert!(
        keys.iter().any(
            |key| key.value == "serde" && key.confidence_basis_points == EXACT_MATCH_CONFIDENCE
        )
    );
}

#[test]
fn dependency_matches_prefer_nearest_manifest_owner() {
    let root = component("root-react", "npm", "react", "package.json", "javascript");
    let web = component(
        "web-react",
        "npm",
        "react",
        "apps/web/package.json",
        "javascript",
    );
    let admin = component(
        "admin-react",
        "npm",
        "react",
        "packages/admin/package.json",
        "javascript",
    );
    let components = vec![root, web, admin];
    let aliases = BTreeMap::new();
    let index = DependencyMatchIndex::new(&components, &aliases);

    let matches = index.matching_components("javascript", "react", "apps/web/src/app.js");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].component.component_id, "web-react");
}

#[test]
fn central_requirements_files_own_source_imports() {
    let mut django = component(
        "django",
        "python",
        "Django",
        "requirements/base.txt",
        "python",
    );
    django.source_kind = "requirements.txt".to_owned();
    let components = vec![django];
    let aliases = BTreeMap::new();
    let index = DependencyMatchIndex::new(&components, &aliases);

    let matches = index.matching_components("python", "django", "src/app.py");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].component.component_id, "django");
}

#[test]
fn cargo_aliases_match_local_import_names() {
    let serde = component("serde", "cargo", "serde", "Cargo.toml", "rust");
    let mut aliases = BTreeMap::new();
    aliases.insert(
        component_evidence_key(&serde),
        cargo_alias_match_keys(
            "serde",
            "serde_alias = { package = \"serde\", version = \"1\" }",
        ),
    );
    let components = vec![serde];
    let index = DependencyMatchIndex::new(&components, &aliases);

    let matches = index.matching_components("rust", "serde_alias", "src/lib.rs");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].component.component_id, "serde");
}

#[test]
fn jvm_import_candidates_do_not_emit_segment_only_keys() {
    let candidates = import_match_candidates(
        "java",
        "import com.mycompany.core.User;",
        None,
        "unresolved",
    );

    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.value == "com.mycompany.core.user")
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.value == "com.mycompany.core")
    );
    assert!(!candidates.iter().any(|candidate| candidate.value == "core"));
}

#[test]
fn jvm_dependency_coordinates_match_import_prefixes() {
    let slf4j = component("slf4j", "maven", "org.slf4j:slf4j-api", "pom.xml", "java");
    let components = vec![slf4j];
    let aliases = BTreeMap::new();
    let index = DependencyMatchIndex::new(&components, &aliases);
    let candidates =
        import_match_candidates("java", "import org.slf4j.Logger;", None, "unresolved");
    let matches = candidates
        .iter()
        .flat_map(|candidate| {
            index.matching_components("java", &candidate.value, "src/main/java/App.java")
        })
        .collect::<Vec<_>>();

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].component.component_id, "slf4j");
}

#[test]
fn jvm_unique_group_fallback_ignores_language_duplicate_rows() {
    let java = component(
        "slf4j-java",
        "maven",
        "org.slf4j:slf4j-api",
        "pom.xml",
        "java",
    );
    let kotlin = component(
        "slf4j-kotlin",
        "maven",
        "org.slf4j:slf4j-api",
        "pom.xml",
        "kotlin",
    );
    let scala = component(
        "slf4j-scala",
        "maven",
        "org.slf4j:slf4j-api",
        "pom.xml",
        "scala",
    );
    let components = vec![java, kotlin, scala];
    let aliases = BTreeMap::new();
    let index = DependencyMatchIndex::new(&components, &aliases);
    let candidates =
        import_match_candidates("java", "import org.slf4j.Logger;", None, "unresolved");
    let matches = candidates
        .iter()
        .flat_map(|candidate| {
            index.matching_components("java", &candidate.value, "src/main/java/App.java")
        })
        .collect::<Vec<_>>();

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].component.component_id, "slf4j-java");
}

#[test]
fn jvm_unique_group_fallback_is_scoped_to_manifest_owner() {
    let api = component(
        "slf4j-api",
        "maven",
        "org.slf4j:slf4j-api",
        "apps/a/pom.xml",
        "java",
    );
    let simple = component(
        "slf4j-simple",
        "maven",
        "org.slf4j:slf4j-simple",
        "apps/b/pom.xml",
        "java",
    );
    let components = vec![api, simple];
    let aliases = BTreeMap::new();
    let index = DependencyMatchIndex::new(&components, &aliases);
    let candidates =
        import_match_candidates("java", "import org.slf4j.Logger;", None, "unresolved");
    let matches = candidates
        .iter()
        .flat_map(|candidate| {
            index.matching_components("java", &candidate.value, "apps/a/src/main/java/App.java")
        })
        .collect::<Vec<_>>();

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].component.component_id, "slf4j-api");
}

#[test]
fn jvm_group_fallback_does_not_match_sibling_artifacts() {
    let spring_core = component(
        "spring-core",
        "maven",
        "org.springframework:spring-core",
        "pom.xml",
        "java",
    );
    let spring_web = component(
        "spring-web",
        "maven",
        "org.springframework:spring-web",
        "pom.xml",
        "java",
    );
    let components = vec![spring_core, spring_web];
    let aliases = BTreeMap::new();
    let index = DependencyMatchIndex::new(&components, &aliases);
    let candidates = import_match_candidates(
        "java",
        "import org.springframework.web.client.RestTemplate;",
        None,
        "unresolved",
    );
    let matches = candidates
        .iter()
        .flat_map(|candidate| {
            index.matching_components("java", &candidate.value, "src/main/java/App.java")
        })
        .collect::<Vec<_>>();

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].component.component_id, "spring-web");
}

#[test]
fn go_component_keys_do_not_widen_manifest_identity_prefixes() {
    let component = component(
        "component:aws-sdk",
        "go",
        "github.com/aws/aws-sdk-go-v2",
        "go.mod",
        "go",
    );

    let keys = component_match_keys(&component);
    assert!(
        keys.iter()
            .any(|key| key.value == "github.com/aws/aws-sdk-go-v2")
    );
    assert!(!keys.iter().any(|key| key.value == "github.com/aws"));
}

#[test]
fn go_import_candidates_strip_alias_tokens() {
    let candidates =
        import_match_candidates("go", "k8s k8s.io/client-go/informers", None, "unresolved");

    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.value == "k8s.io/client-go")
    );
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.value == "k8s k8s.io/client-go")
    );
}

#[test]
fn schema_creation_marks_existing_projection_statuses_stale() {
    let connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "
            CREATE TABLE software_global_status (
                source_scope TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL,
                projected_graph_version INTEGER NOT NULL,
                stale INTEGER NOT NULL,
                component_count INTEGER NOT NULL,
                sdk_usage_count INTEGER NOT NULL,
                last_error TEXT
            );
            INSERT INTO software_global_status (
                source_scope, repository_id, projected_graph_version, stale,
                component_count, sdk_usage_count, last_error
            ) VALUES ('scope', 'repo', 7, 0, 1, 1, NULL);
            ",
        )
        .expect("status should seed");

    initialize_schema(&connection).expect("dependency usage schema should initialize");

    let (stale, last_error) = connection
        .query_row(
            "SELECT stale, last_error FROM software_global_status WHERE source_scope = 'scope'",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .expect("status should load");
    assert_eq!(stale, 1);
    assert_eq!(
        last_error.as_deref(),
        Some("software dependency usage projection requires refresh")
    );
}

#[test]
fn bom_components_do_not_match_import_usage() {
    let mut bom = component(
        "spring-boot-bom",
        "maven",
        "org.springframework.boot:spring-boot-dependencies",
        "pom.xml",
        "java",
    );
    bom.dependency_group = "bom".to_owned();
    let components = vec![bom];
    let aliases = BTreeMap::new();
    let index = DependencyMatchIndex::new(&components, &aliases);
    let candidates = import_match_candidates(
        "java",
        "import org.springframework.boot.SpringApplication;",
        None,
        "unresolved",
    );
    let matches = candidates
        .iter()
        .flat_map(|candidate| {
            index.matching_components("java", &candidate.value, "src/main/java/App.java")
        })
        .collect::<Vec<_>>();

    assert!(matches.is_empty());
}

fn local_python_modules(modules: &[&str]) -> BTreeSet<String> {
    modules.iter().map(|module| normalize_key(module)).collect()
}

fn component(
    component_id: &str,
    ecosystem: &str,
    name: &str,
    evidence_path: &str,
    language_id: &str,
) -> SoftwareComponent {
    SoftwareComponent {
        component_id: component_id.to_owned(),
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        ecosystem: ecosystem.to_owned(),
        name: name.to_owned(),
        requirement: Some("1".to_owned()),
        resolved_version: None,
        dependency_group: "dependencies".to_owned(),
        source_kind: evidence_path
            .rsplit_once('/')
            .map_or(evidence_path, |(_, file_name)| file_name)
            .to_owned(),
        relationship_state: "declared".to_owned(),
        language_id: language_id.to_owned(),
        evidence_path: evidence_path.to_owned(),
        evidence_line_range: RepositoryCodeRange { start: 7, end: 7 },
        confidence_basis_points: 10000,
        created_graph_version: GraphVersion::new(1),
    }
}
