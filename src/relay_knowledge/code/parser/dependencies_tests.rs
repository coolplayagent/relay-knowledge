use super::*;
use crate::domain::{CodeDependencyRecord, CodeRepositoryRegistration};

#[test]
fn extracts_cargo_and_npm_manifest_dependencies() {
    let cargo = collect(
        "Cargo.toml",
        "[dependencies]\nserde = \"1\"\n[dev-dependencies]\ntokio = { version = \"1\" }\n[target.'cfg(unix)'.dependencies]\nnix = { rev = \"abc\" }\n",
    );
    assert_dependency(&cargo, "serde", "dependencies", Some("1"), None);
    assert_dependency(&cargo, "tokio", "dev", Some("1"), None);
    assert_dependency(&cargo, "nix", "dependencies", Some("abc"), None);

    let package = collect(
        "package.json",
        r#"{
          "dependencies": {"react": "^18"},
          "devDependencies": {"vitest": "1.0.0"},
          "peerDependencies": {"typescript": ">=5"},
          "optionalDependencies": {"fsevents": "2.3.3"}
        }"#,
    );
    assert_dependency(&package, "react", "dependencies", Some("^18"), None);
    assert_dependency(&package, "vitest", "dev", Some("1.0.0"), None);
    assert_dependency(&package, "typescript", "peer", Some(">=5"), None);
    assert_dependency(&package, "fsevents", "optional", Some("2.3.3"), None);
}

#[test]
fn recurses_package_lock_v1_dependencies() {
    let dependencies = collect(
        "package-lock.json",
        r#"{
          "lockfileVersion": 1,
          "dependencies": {
            "express": {
              "version": "4.18.2",
              "dependencies": {
                "accepts": {"version": "1.3.8"},
                "body-parser": {
                  "version": "1.20.1",
                  "dependencies": {"bytes": {"version": "3.1.2"}}
                }
              }
            }
          }
        }"#,
    );

    assert_dependency(&dependencies, "express", "locked", None, Some("4.18.2"));
    assert_dependency(&dependencies, "accepts", "locked", None, Some("1.3.8"));
    assert_dependency(&dependencies, "body-parser", "locked", None, Some("1.20.1"));
    assert_dependency(&dependencies, "bytes", "locked", None, Some("3.1.2"));
    assert!(dependencies.iter().all(|dependency| dependency.is_lockfile));
}

#[test]
fn extracts_package_lock_v2_packages() {
    let dependencies = collect(
        "package-lock.json",
        r#"{"packages":{"":{"name":"root"},"node_modules/react":{"version":"18.2.0"},"node_modules/@scope/pkg":{"name":"@scope/pkg","version":"1.2.3"}}}"#,
    );

    assert_dependency(&dependencies, "react", "locked", None, Some("18.2.0"));
    assert_dependency(&dependencies, "@scope/pkg", "locked", None, Some("1.2.3"));
    assert!(
        !dependencies
            .iter()
            .any(|dependency| dependency.package_name == "root")
    );
}

#[test]
fn extracts_go_manifest_and_sum_dependencies() {
    let go_mod = collect(
        "go.mod",
        "module example.test/app\nrequire (\n  github.com/gin-gonic/gin v1.9.1\n  golang.org/x/sync v0.7.0 // indirect\n)\nrequire example.test/direct v1.2.3\n",
    );
    assert_dependency(
        &go_mod,
        "github.com/gin-gonic/gin",
        "require",
        Some("v1.9.1"),
        None,
    );
    assert_dependency(
        &go_mod,
        "golang.org/x/sync",
        "require",
        Some("v0.7.0"),
        None,
    );
    assert_dependency(
        &go_mod,
        "example.test/direct",
        "require",
        Some("v1.2.3"),
        None,
    );

    let go_sum = collect(
        "go.sum",
        "github.com/gin-gonic/gin v1.9.1 h1:abc\ngolang.org/x/sync v0.7.0/go.mod h1:def\nlocalmodule v0.1.0 h1:skip\n",
    );
    assert_dependency(
        &go_sum,
        "github.com/gin-gonic/gin",
        "locked",
        None,
        Some("v1.9.1"),
    );
    assert_dependency(&go_sum, "golang.org/x/sync", "locked", None, Some("v0.7.0"));
    assert!(
        !go_sum
            .iter()
            .any(|dependency| dependency.package_name == "localmodule")
    );
}

#[test]
fn restricts_poetry_parsing_to_dependency_sections() {
    let dependencies = collect(
        "pyproject.toml",
        r#"[project]
dependencies = [
  "httpx>=0.27",
]
[project.optional-dependencies]
docs = ["mkdocs>=1"]
[tool.poetry.dependencies]
python = "^3.12"
requests = "^2"
[tool.poetry.group.test.dependencies]
pytest = "^8"
[tool.poetry.scripts]
serve = "app.cli:main"
[tool.poetry.extras]
fast = ["uvloop"]
"#,
    );

    assert_dependency(&dependencies, "httpx", "dependencies", Some(">=0.27"), None);
    assert_dependency(&dependencies, "mkdocs", "docs", Some(">=1"), None);
    assert_dependency(&dependencies, "requests", "dependencies", Some("^2"), None);
    assert_dependency(&dependencies, "pytest", "test", Some("^8"), None);
    assert!(!dependencies.iter().any(|dependency| matches!(
        dependency.package_name.as_str(),
        "python" | "serve" | "fast"
    )));
}

#[test]
fn extracts_requirements_dependencies_without_options() {
    let dependencies = collect(
        "requirements-dev.txt",
        "# install set\n-r base.txt\nrequests[socks]>=2.32\nuvicorn==0.29.0 # server\n",
    );

    assert_dependency(
        &dependencies,
        "requests",
        "requirements",
        Some(">=2.32"),
        None,
    );
    assert_dependency(
        &dependencies,
        "uvicorn",
        "requirements",
        Some("==0.29.0"),
        None,
    );
    assert_eq!(dependencies.len(), 2);
}

#[test]
fn extracts_maven_bom_and_gradle_external_dependencies() {
    let pom = collect(
        "pom.xml",
        "<dependencyManagement>
  <dependencies>
    <dependency>
      <groupId>org.springframework.boot</groupId>
      <artifactId>spring-boot-dependencies</artifactId>
      <version>3.2.0</version>
      <type>pom</type>
      <scope>import</scope>
    </dependency>
  </dependencies>
</dependencyManagement>
<dependencies>
  <dependency>
    <groupId>org.slf4j</groupId>
    <artifactId>slf4j-api</artifactId>
    <version>2.0.9</version>
    <scope>runtime</scope>
  </dependency>
</dependencies>",
    );
    assert_dependency(
        &pom,
        "org.springframework.boot:spring-boot-dependencies",
        "bom",
        Some("3.2.0"),
        None,
    );
    assert_dependency(&pom, "org.slf4j:slf4j-api", "runtime", Some("2.0.9"), None);

    let gradle = collect(
        "build.gradle",
        "plugins { id 'java' }\nimplementation platform('org.springframework.boot:spring-boot-dependencies:3.2.0')\nimplementation 'org.slf4j:slf4j-api:2.0.9'\nimplementation(project(':core'))\n",
    );
    assert_dependency(
        &gradle,
        "org.springframework.boot:spring-boot-dependencies",
        "bom",
        Some("3.2.0"),
        None,
    );
    assert_dependency(
        &gradle,
        "org.slf4j:slf4j-api",
        "implementation",
        Some("2.0.9"),
        None,
    );
    assert!(
        !gradle
            .iter()
            .any(|dependency| dependency.package_name == ":core")
    );
}

#[test]
fn extracts_conan_txt_and_python_dependencies() {
    let txt = collect(
        "conanfile.txt",
        "[requires]\nzlib/1.2.13\n[tool_requires]\ncmake/3.28.0\n[generators]\nCMakeDeps\n",
    );
    assert_dependency(&txt, "zlib", "requires", Some("1.2.13"), None);
    assert_dependency(&txt, "cmake", "tool_requires", Some("3.28.0"), None);

    let py = collect(
        "conanfile.py",
        "class Recipe:\n    requires = \"openssl/3.2.1\"\n    def build_requirements(self):\n        self.tool_requires(\"ninja/1.11.1\")\n",
    );
    assert_dependency(&py, "openssl", "requires", Some("3.2.1"), None);
    assert_dependency(&py, "ninja", "build_requires", Some("1.11.1"), None);
}

#[test]
fn ignores_invalid_or_unsupported_dependency_files() {
    assert!(collect("package.json", "{not json").is_empty());
    assert!(collect("src/lib.rs", "serde = \"1\"").is_empty());
    assert!(collect("requirements.txt", "--index-url https://example.invalid\n").is_empty());
}

fn collect(path: &str, content: &str) -> Vec<CodeDependencyRecord> {
    collect_dependencies(&test_build(), path, "file", content)
        .expect("dependency parsing should not fail")
}

fn assert_dependency(
    dependencies: &[CodeDependencyRecord],
    package_name: &str,
    group: &str,
    requirement: Option<&str>,
    resolved: Option<&str>,
) {
    assert!(
        dependencies.iter().any(|dependency| {
            dependency.package_name == package_name
                && dependency.dependency_group == group
                && dependency.requirement.as_deref() == requirement
                && dependency.resolved_version.as_deref() == resolved
        }),
        "missing dependency {package_name} group={group} requirement={requirement:?} resolved={resolved:?}; got {:?}",
        dependencies
            .iter()
            .map(|dependency| (
                dependency.package_name.as_str(),
                dependency.dependency_group.as_str(),
                dependency.requirement.as_deref(),
                dependency.resolved_version.as_deref()
            ))
            .collect::<Vec<_>>()
    );
}

fn test_build() -> SnapshotBuild {
    let registration = CodeRepositoryRegistration {
        repository_id: "repo".to_owned(),
        root_path: "/tmp/repo".to_owned(),
        alias: "repo".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
    };
    SnapshotBuild::new(
        &registration,
        "HEAD".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    )
}
