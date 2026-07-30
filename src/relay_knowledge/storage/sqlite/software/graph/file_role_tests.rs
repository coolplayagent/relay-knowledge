//! Software file-role classification contracts.

use super::*;

#[test]
fn classifies_knowledge_map_and_documentation_before_generic_config() {
    assert_eq!(
        file_role(crate::project::KNOWLEDGE_MAP_RELATIVE_PATH, "yaml"),
        "knowledge_map"
    );
    assert_eq!(file_role("docs/runtime.md", "markdown"), "documentation");
}

#[test]
fn classifies_dependency_and_build_manifests_with_stable_precedence() {
    assert_eq!(
        file_role("requirements/dev.txt", "text"),
        "dependency_manifest"
    );
    assert_eq!(
        file_role("build.gradle.kts", "kotlin"),
        "dependency_manifest"
    );
    assert_eq!(file_role("CMakeLists.txt", "cmake"), "dependency_manifest");
    assert_eq!(file_role("BUILD.bazel", "starlark"), "build_manifest");
}

#[test]
fn limits_deployment_roles_to_deployment_scopes() {
    assert_eq!(file_role("Dockerfile.dev", "dockerfile"), "deployment");
    assert_eq!(file_role("systemd/relay.service", "ini"), "deployment");
    assert_eq!(file_role("k8s/deployment.yaml", "yaml"), "deployment");
    assert_eq!(file_role("src/k8s/client.rs", "rust"), "source");
    assert_eq!(file_role("src/kubernetes/api.go", "go"), "source");
}

#[test]
fn distinguishes_test_template_configuration_and_source_roles() {
    assert_eq!(file_role("tests/smoke.rs", "rust"), "test");
    assert_eq!(
        file_role("templates/deployment.yaml.j2", "jinja2"),
        "template"
    );
    assert_eq!(file_role("config/flags.yaml", "yaml"), "configuration");
    assert_eq!(file_role("src/lib.rs", "rust"), "source");
}
