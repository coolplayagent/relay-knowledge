//! Software file-role classification contracts.

use super::*;

#[test]
fn classifies_knowledge_map_and_documentation_before_generic_config() {
    assert_eq!(
        file_role(crate::project::KNOWLEDGE_MAP_RELATIVE_PATH, "yaml", false),
        "knowledge_map_manifest"
    );
    assert_eq!(
        file_role(".knowledge/topics/topic-deadbeef.yaml", "yaml", true),
        "knowledge_map_topic_shard"
    );
    assert_eq!(
        file_role(".knowledge/topics/topic-deadbeef.yaml", "yaml", false),
        "configuration"
    );
    assert_eq!(
        file_role("docs/runtime.md", "markdown", false),
        "documentation"
    );
}

#[test]
fn classifies_dependency_and_build_manifests_with_stable_precedence() {
    assert_eq!(
        file_role("requirements/dev.txt", "text", false),
        "dependency_manifest"
    );
    assert_eq!(
        file_role("build.gradle.kts", "kotlin", false),
        "dependency_manifest"
    );
    assert_eq!(
        file_role("CMakeLists.txt", "cmake", false),
        "dependency_manifest"
    );
    assert_eq!(
        file_role("BUILD.bazel", "starlark", false),
        "build_manifest"
    );
}

#[test]
fn limits_deployment_roles_to_deployment_scopes() {
    assert_eq!(
        file_role("Dockerfile.dev", "dockerfile", false),
        "deployment"
    );
    assert_eq!(
        file_role("systemd/relay.service", "ini", false),
        "deployment"
    );
    assert_eq!(
        file_role("k8s/deployment.yaml", "yaml", false),
        "deployment"
    );
    assert_eq!(file_role("src/k8s/client.rs", "rust", false), "source");
    assert_eq!(file_role("src/kubernetes/api.go", "go", false), "source");
}

#[test]
fn distinguishes_test_template_configuration_and_source_roles() {
    assert_eq!(file_role("tests/smoke.rs", "rust", false), "test");
    assert_eq!(
        file_role("templates/deployment.yaml.j2", "jinja2", false),
        "template"
    );
    assert_eq!(
        file_role("config/flags.yaml", "yaml", false),
        "configuration"
    );
    assert_eq!(file_role("src/lib.rs", "rust", false), "source");
}
