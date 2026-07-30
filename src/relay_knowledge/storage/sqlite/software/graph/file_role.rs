//! Deterministic software-projection file role classification.

use crate::project::KNOWLEDGE_MAP_RELATIVE_PATH;

pub(super) fn file_role(path: &str, language_id: &str) -> &'static str {
    if path == KNOWLEDGE_MAP_RELATIVE_PATH {
        return "knowledge_map";
    }
    let file_name = path.rsplit('/').next().unwrap_or(path);
    if language_id == "markdown" {
        return "documentation";
    }
    if dependency_manifest_path(path, file_name) {
        return "dependency_manifest";
    }
    if build_manifest_path(file_name, language_id) {
        return "build_manifest";
    }
    if deployment_path(path, file_name, language_id) {
        return "deployment";
    }
    if test_path(path, file_name) {
        return "test";
    }
    if template_language(language_id) {
        return "template";
    }
    if config_language(language_id) {
        return "configuration";
    }

    "source"
}

fn dependency_manifest_path(path: &str, file_name: &str) -> bool {
    matches!(
        file_name,
        "Cargo.toml"
            | "Cargo.lock"
            | "package.json"
            | "package-lock.json"
            | "go.mod"
            | "go.sum"
            | "requirements.txt"
            | "pyproject.toml"
            | "uv.lock"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "gradle.lockfile"
            | "conanfile.txt"
            | "conanfile.py"
            | "CMakeLists.txt"
    ) || python_requirements_path(path, file_name)
}

fn python_requirements_path(path: &str, file_name: &str) -> bool {
    file_name.ends_with(".txt")
        && (file_name.starts_with("requirements")
            || file_name.starts_with("constraints")
            || path.split('/').any(|segment| segment == "requirements"))
}

fn build_manifest_path(file_name: &str, language_id: &str) -> bool {
    matches!(
        file_name,
        "BUILD"
            | "BUILD.bazel"
            | "WORKSPACE"
            | "WORKSPACE.bazel"
            | "MODULE.bazel"
            | "Makefile"
            | "GNUmakefile"
            | "BSDmakefile"
            | "CMakeLists.txt"
            | "build.ninja"
    ) || matches!(language_id, "cmake" | "make" | "ninja" | "starlark")
}

fn deployment_path(path: &str, file_name: &str, language_id: &str) -> bool {
    file_name.starts_with("Dockerfile")
        || file_name.starts_with("Containerfile")
        || matches!(language_id, "dockerfile")
        || (deployment_service_path(path)
            && (deployment_manifest_language(language_id) || service_manager_file_name(file_name)))
        || (kubernetes_manifest_path(path) && deployment_manifest_language(language_id))
}

fn deployment_service_path(path: &str) -> bool {
    path.starts_with("systemd/")
        || path.starts_with("launchd/")
        || path.contains("/systemd/")
        || path.contains("/launchd/")
}

fn kubernetes_manifest_path(path: &str) -> bool {
    path.starts_with("k8s/")
        || path.starts_with("kubernetes/")
        || path.contains("/k8s/")
        || path.contains("/kubernetes/")
}

fn deployment_manifest_language(language_id: &str) -> bool {
    config_language(language_id) || template_language(language_id)
}

fn service_manager_file_name(file_name: &str) -> bool {
    file_name.ends_with(".service")
        || file_name.ends_with(".socket")
        || file_name.ends_with(".timer")
        || file_name.ends_with(".target")
        || file_name.ends_with(".plist")
}

fn test_path(path: &str, file_name: &str) -> bool {
    path.starts_with("test/")
        || path.starts_with("tests/")
        || path.contains("/test/")
        || path.contains("/tests/")
        || file_name.contains("_test.")
        || file_name.contains(".test.")
        || file_name.contains(".spec.")
}

fn template_language(language_id: &str) -> bool {
    matches!(language_id, "jinja2" | "gotemplate")
}

fn config_language(language_id: &str) -> bool {
    matches!(
        language_id,
        "json" | "toml" | "yaml" | "ini" | "properties" | "xml" | "jinja2" | "gotemplate"
    )
}

#[cfg(test)]
#[path = "file_role_tests.rs"]
mod tests;
