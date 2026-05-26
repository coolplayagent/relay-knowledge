use super::*;
use crate::domain::{CodeDependencyRecord, CodeRepositoryRegistration};

#[test]
fn extracts_cargo_and_npm_manifest_dependencies() {
    let cargo = collect(
        "Cargo.toml",
        "[dependencies]\nserde = \"1\"\nserde_alias = { package = \"serde\", version = \"1.0\" }\npathfinder = { git = \"https://example.invalid/pathfinder.git\", rev = \"abc123\" }\nsemver_alias = { package = \"semver\", cargo-version = \"ignored\", version = \"1.0\" }\nlocal_core = { path = \"../core\" }\nworkspace_dep = { workspace = true }\n[dev-dependencies]\ntokio = { version = \"1\" }\n[target.'cfg(unix)'.dependencies]\nnix = { rev = \"abc\" }\n",
    );
    assert_dependency(&cargo, "serde", "dependencies", Some("1"), None);
    assert_dependency(&cargo, "serde", "dependencies", Some("1.0"), None);
    assert_dependency(&cargo, "pathfinder", "dependencies", Some("abc123"), None);
    assert_dependency(&cargo, "semver", "dependencies", Some("1.0"), None);
    assert_dependency(&cargo, "tokio", "dev", Some("1"), None);
    assert_dependency(&cargo, "nix", "dependencies", Some("abc"), None);
    assert!(!cargo.iter().any(|dependency| matches!(
        dependency.package_name.as_str(),
        "serde_alias" | "local_core" | "workspace_dep"
    )));

    let package = collect(
        "package.json",
        r#"{
          "dependencies": {"react": "^18", "@workspace/internal": "workspace:*", "local-file": "file:../packages/local-file"},
          "devDependencies": {"vitest": "1.0.0", "local-link": "link:../packages/local-link"},
          "peerDependencies": {"typescript": ">=5"},
          "optionalDependencies": {"fsevents": "2.3.3"}
        }"#,
    );
    assert_dependency(&package, "react", "dependencies", Some("^18"), None);
    assert_dependency(&package, "vitest", "dev", Some("1.0.0"), None);
    assert_dependency(&package, "typescript", "peer", Some(">=5"), None);
    assert_dependency(&package, "fsevents", "optional", Some("2.3.3"), None);
    assert!(!package.iter().any(|dependency| matches!(
        dependency.package_name.as_str(),
        "@workspace/internal" | "local-file" | "local-link"
    )));
}

#[test]
fn cargo_lock_skips_local_workspace_packages() {
    let dependencies = collect(
        "Cargo.lock",
        r#"[[package]]
name = "workspace-root"
version = "0.1.0"

[[package]]
name = "serde"
version = "1.0.203"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "git-helper"
version = "0.2.0"
source = "git+https://example.invalid/helper.git#abcdef"

[[package]]
name = "sparse-helper"
version = "0.3.0"
source = "sparse+https://index.crates.io/"
"#,
    );

    assert_dependency(&dependencies, "serde", "locked", None, Some("1.0.203"));
    assert_dependency(&dependencies, "git-helper", "locked", None, Some("0.2.0"));
    assert_dependency(
        &dependencies,
        "sparse-helper",
        "locked",
        None,
        Some("0.3.0"),
    );
    assert!(
        !dependencies
            .iter()
            .any(|dependency| dependency.package_name == "workspace-root")
    );
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
                  "dependencies": {"bytes": {"version": "3.1.2"}, "local-nested": {"version": "file:../local-nested"}}
                },
                "local-linked": {"version": "file:../packages/local-linked"}
              }
            },
            "local-workspace": {"version": "workspace:packages/local-workspace"}
          }
        }"#,
    );

    assert_dependency(&dependencies, "express", "locked", None, Some("4.18.2"));
    assert_dependency(&dependencies, "accepts", "locked", None, Some("1.3.8"));
    assert_dependency(&dependencies, "body-parser", "locked", None, Some("1.20.1"));
    assert_dependency(&dependencies, "bytes", "locked", None, Some("3.1.2"));
    assert!(dependencies.iter().all(|dependency| dependency.is_lockfile));
    assert!(!dependencies.iter().any(|dependency| matches!(
        dependency.package_name.as_str(),
        "local-linked" | "local-nested" | "local-workspace"
    )));
}

#[test]
fn extracts_package_lock_v2_packages() {
    let dependencies = collect(
        "package-lock.json",
        r#"{"packages":{"":{"name":"root"},"node_modules/react":{"version":"18.2.0"},"node_modules/@scope/pkg":{"name":"@scope/pkg","version":"1.2.3"},"node_modules/a/node_modules/b":{"version":"2.0.0"},"node_modules/a/node_modules/@scope/transitive":{"version":"3.0.0"},"node_modules/workspace-pkg":{"resolved":"packages/workspace-pkg","link":true},"node_modules/local-file":{"version":"1.0.0","resolved":"file:../packages/local-file"},"node_modules/local-workspace":{"version":"1.0.0","resolved":"workspace:packages/local-workspace"}}}"#,
    );

    assert_dependency(&dependencies, "react", "locked", None, Some("18.2.0"));
    assert_dependency(&dependencies, "@scope/pkg", "locked", None, Some("1.2.3"));
    assert_dependency(&dependencies, "b", "locked", None, Some("2.0.0"));
    assert_dependency(
        &dependencies,
        "@scope/transitive",
        "locked",
        None,
        Some("3.0.0"),
    );
    assert!(
        !dependencies
            .iter()
            .any(|dependency| dependency.package_name == "root")
    );
    assert!(
        !dependencies
            .iter()
            .any(|dependency| dependency.package_name == "a/node_modules/b")
    );
    assert!(!dependencies.iter().any(|dependency| matches!(
        dependency.package_name.as_str(),
        "workspace-pkg" | "local-file" | "local-workspace"
    )));
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
        "github.com/gin-gonic/gin v1.9.1 h1:abc\ngithub.com/gin-gonic/gin v1.9.1/go.mod h1:abcmod\ngolang.org/x/sync v0.7.0/go.mod h1:def\nlocalmodule v0.1.0 h1:skip\n",
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
    assert_eq!(
        go_sum
            .iter()
            .filter(
                |dependency| dependency.package_name == "github.com/gin-gonic/gin"
                    && dependency.resolved_version.as_deref() == Some("v1.9.1")
            )
            .count(),
        1
    );
}

#[test]
fn restricts_poetry_parsing_to_dependency_sections() {
    let dependencies = collect(
        "pyproject.toml",
        r#"[project]
	dependencies = [
	  "httpx>=0.27",
	  "colorama; platform_system == 'Windows'",
	  "compact-ref@https://example.invalid/compact-ref.whl",
	]
[project.optional-dependencies]
docs = ["mkdocs>=1"]
	[tool.poetry.dependencies]
	python = "^3.12"
	requests = "^2"
	rich = { markers = "python_version >= '3.12'", version = "^13" }
	local_shared = { path = "../shared", develop = true }
	[tool.poetry.group.test.dependencies]
	pytest = "^8"
[tool.poetry.scripts]
serve = "app.cli:main"
[tool.poetry.extras]
fast = ["uvloop"]
"#,
    );

    assert_dependency(&dependencies, "httpx", "dependencies", Some(">=0.27"), None);
    assert_dependency(&dependencies, "colorama", "dependencies", None, None);
    assert_dependency(
        &dependencies,
        "compact-ref",
        "dependencies",
        Some("@ https://example.invalid/compact-ref.whl"),
        None,
    );
    assert_dependency(&dependencies, "mkdocs", "docs", Some(">=1"), None);
    assert_dependency(&dependencies, "requests", "dependencies", Some("^2"), None);
    assert_dependency(&dependencies, "rich", "dependencies", Some("^13"), None);
    assert_dependency(&dependencies, "pytest", "test", Some("^8"), None);
    assert!(!dependencies.iter().any(|dependency| matches!(
        dependency.package_name.as_str(),
        "python" | "local_shared" | "serve" | "fast"
    )));
}

#[test]
fn extracts_requirements_dependencies_without_options() {
    let dependencies = collect(
        "requirements-dev.txt",
        "# install set\n-r base.txt\n../shared-lib\n./vendor/pkg\nlocal_path @ file:../local-path\nrequests[socks]>=2.32\nuvicorn==0.29.0 # server\ncolorama; platform_system == \"Windows\"\nwatchfiles @ https://example.invalid/watchfiles.whl#sha256=abc ; python_version >= \"3.11\"\ncompact-ref@https://example.invalid/compact-ref.whl#sha256=def\n-e git+https://example.invalid/editable.git#egg=editable_pkg\n--editable git+ssh://git@example.invalid/org/other.git#egg=other-pkg\n",
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
    assert_dependency(&dependencies, "colorama", "requirements", None, None);
    assert_dependency(
        &dependencies,
        "watchfiles",
        "requirements",
        Some("@ https://example.invalid/watchfiles.whl#sha256=abc"),
        None,
    );
    assert_dependency(
        &dependencies,
        "compact-ref",
        "requirements",
        Some("@ https://example.invalid/compact-ref.whl#sha256=def"),
        None,
    );
    assert_dependency(
        &dependencies,
        "editable_pkg",
        "requirements",
        Some("@ git+https://example.invalid/editable.git#egg=editable_pkg"),
        None,
    );
    assert_dependency(
        &dependencies,
        "other-pkg",
        "requirements",
        Some("@ git+ssh://git@example.invalid/org/other.git#egg=other-pkg"),
        None,
    );
    assert_eq!(dependencies.len(), 7);
    assert!(!dependencies.iter().any(|dependency| matches!(
        dependency.package_name.as_str(),
        "../shared-lib" | "./vendor/pkg" | "local_path"
    )));
}

#[test]
fn extracts_nested_requirements_and_constraints_files() {
    let nested = collect("requirements/base.txt", "Django>=5.0\n-r local-dev.txt\n");
    let constraints = collect("constraints.txt", "urllib3==2.2.1\n");

    assert_dependency(&nested, "Django", "requirements", Some(">=5.0"), None);
    assert_dependency(
        &constraints,
        "urllib3",
        "requirements",
        Some("==2.2.1"),
        None,
    );
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
	    <dependency>
	      <groupId>org.junit</groupId>
	      <artifactId>junit-bom</artifactId>
	      <version>5.10.1</version>
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
    assert!(
        !pom.iter()
            .any(|dependency| dependency.package_name == "org.junit:junit-bom")
    );

    let gradle = collect(
        "build.gradle",
        "plugins { id 'java' }\nimplementation platform('org.springframework.boot:spring-boot-dependencies:3.2.0')\nimplementation 'org.slf4j:slf4j-api:2.0.9'\nruntimeOnly group: 'ch.qos.logback', name: 'logback-classic', version: '1.4.14'\ntestImplementation(group = \"org.junit.jupiter\", name = \"junit-jupiter-api\", version = \"5.10.1\")\nimplementation(project(':core'))\n",
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
    assert_dependency(
        &gradle,
        "ch.qos.logback:logback-classic",
        "runtimeOnly",
        Some("1.4.14"),
        None,
    );
    assert_dependency(
        &gradle,
        "org.junit.jupiter:junit-jupiter-api",
        "testImplementation",
        Some("5.10.1"),
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
fn preserves_shared_manifest_language_scope_for_dependency_records() {
    let package = collect_with_languages(
        "package.json",
        r#"{"dependencies":{"react":"^18"}}"#,
        &["typescript"],
    );
    assert_dependency_language(&package, "react", "typescript");
    assert_dependency_language(
        &collect("package.json", r#"{"dependencies":{"react":"^18"}}"#),
        "react",
        "tsx",
    );

    let package_lock = collect_with_languages(
        "package-lock.json",
        r#"{"packages":{"node_modules/react":{"version":"18.2.0"}}}"#,
        &["jsx"],
    );
    assert_dependency_language(&package_lock, "react", "jsx");

    let pom = collect_with_languages(
        "pom.xml",
        "<dependencies><dependency><groupId>org.slf4j</groupId><artifactId>slf4j-api</artifactId><version>2.0.9</version></dependency></dependencies>",
        &["scala"],
    );
    assert_dependency_language(&pom, "org.slf4j:slf4j-api", "scala");

    let gradle = collect_with_languages(
        "build.gradle.kts",
        r#"implementation("org.slf4j:slf4j-api:2.0.9")"#,
        &["kotlin"],
    );
    assert_dependency_language(&gradle, "org.slf4j:slf4j-api", "kotlin");

    let conan = collect_with_languages("conanfile.txt", "[requires]\nzlib/1.2.13\n", &["c"]);
    assert_dependency_language(&conan, "zlib", "c");
}

#[test]
fn ignores_invalid_or_unsupported_dependency_files() {
    assert!(collect("package.json", "{not json").is_empty());
    assert!(collect("src/lib.rs", "serde = \"1\"").is_empty());
    assert!(collect("requirements.txt", "--index-url https://example.invalid\n").is_empty());
}

fn collect(path: &str, content: &str) -> Vec<CodeDependencyRecord> {
    collect_with_languages(path, content, &[])
}

fn collect_with_languages(
    path: &str,
    content: &str,
    language_filters: &[&str],
) -> Vec<CodeDependencyRecord> {
    collect_dependencies(&test_build(language_filters), path, "file", content)
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

fn assert_dependency_language(
    dependencies: &[CodeDependencyRecord],
    package_name: &str,
    language_id: &str,
) {
    assert!(
        dependencies.iter().any(|dependency| {
            dependency.package_name == package_name && dependency.language_id == language_id
        }),
        "missing dependency {package_name} language={language_id}; got {:?}",
        dependencies
            .iter()
            .map(|dependency| (
                dependency.package_name.as_str(),
                dependency.language_id.as_str()
            ))
            .collect::<Vec<_>>()
    );
}

fn test_build(language_filters: &[&str]) -> SnapshotBuild {
    let registration = CodeRepositoryRegistration {
        repository_id: "repo".to_owned(),
        root_path: "/tmp/repo".to_owned(),
        alias: "repo".to_owned(),
        path_filters: Vec::new(),
        language_filters: language_filters
            .iter()
            .map(|language| (*language).to_owned())
            .collect(),
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
