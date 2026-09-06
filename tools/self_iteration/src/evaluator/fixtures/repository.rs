use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::{
    cases::{string_field, string_or},
    command::{CommandResult, CommandSpec},
};

use super::super::runtime::{concurrency::run_limited, contracts::EvalRuntime};
use super::{
    additional_languages::*, agent_workflow::*, c_and_cpp::*, common_languages::*,
    cross_language::*, nonstandard_layout::*, repository_maps::*, software_global::*,
    writer::write_fixture_file,
};

pub(in crate::evaluator) fn prepare_repository_path(
    runtime: &EvalRuntime,
    run_home: &Path,
    repo_name: &str,
    repo_config: &Value,
) -> Result<(PathBuf, Vec<CommandResult>), String> {
    let Some(fixture) = string_field(repo_config, "generated_fixture") else {
        return Ok((
            PathBuf::from(string_or(repo_config, "path", "")),
            Vec::new(),
        ));
    };
    let root = generated_repository_root(run_home, repo_name)?;
    create_generated_repository_files(&root, fixture)?;
    Ok((
        root.clone(),
        commit_generated_repository(runtime, repo_name, &root),
    ))
}

fn generated_repository_root(run_home: &Path, repo_name: &str) -> Result<PathBuf, String> {
    if repo_name.is_empty()
        || !repo_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(format!(
            "generated repository name must be a safe path component: {repo_name:?}"
        ));
    }
    Ok(run_home.join("generated-repositories").join(repo_name))
}

fn create_generated_repository_files(root: &Path, fixture: &str) -> Result<(), String> {
    if root.exists() {
        fs::remove_dir_all(root)
            .map_err(|error| format!("failed to remove {}: {error}", root.display()))?;
    }
    fs::create_dir_all(root)
        .map_err(|error| format!("failed to create {}: {error}", root.display()))?;
    if fixture == "grep_budget_v1" {
        return write_grep_budget_fixture(root);
    }
    if fixture == "index_performance_many_files_v1" {
        return write_index_performance_many_files_fixture(root);
    }
    if fixture == "index_performance_c_fragment_v1" {
        return write_index_performance_c_fragment_fixture(root);
    }
    if fixture == "repository_map_graph_v4" {
        return write_repository_map_graph_v4_fixture(root);
    }
    if fixture == "index_performance_wide_mixed_files_v1" {
        return write_index_performance_wide_mixed_files_fixture(root);
    }
    for (path, content) in generated_repository_files(fixture)? {
        write_fixture_file(&root.join(path), content)?;
    }
    Ok(())
}

fn generated_repository_files(fixture: &str) -> Result<Vec<(&'static str, &'static str)>, String> {
    match fixture {
        "c_syntax_v1" => Ok(vec![
            ("include/driver_ops.h", C_DRIVER_OPS_H),
            ("include/macros.h", C_MACROS_H),
            ("src/driver_ops.c", C_DRIVER_OPS_C),
            ("src/dispatch.c", C_DISPATCH_C),
            ("src/generated_table.c", C_GENERATED_TABLE_C),
            ("src/gcc_extension_policy.c", C_GCC_EXTENSION_POLICY_C),
            ("src/http_macro_module.c", C_HTTP_MACRO_MODULE_C),
            ("src/nginx_external_module.c", C_NGINX_EXTERNAL_MODULE_C),
            ("tests/fake_driver.c", C_FAKE_DRIVER_C),
        ]),
        "cpp_syntax_v1" => Ok(vec![
            ("include/store/cache.hpp", CPP_CACHE_HPP),
            ("include/store/exported_module.hpp", CPP_EXPORTED_MODULE_HPP),
            ("include/store/pipeline.hpp", CPP_PIPELINE_HPP),
            ("src/cache.cpp", CPP_CACHE_CPP),
            ("src/pipeline.cpp", CPP_PIPELINE_CPP),
            ("tests/fake_cache.cpp", CPP_FAKE_CACHE_CPP),
        ]),
        "cross_language_syntax_v1" => Ok(vec![
            (
                ".relay-knowledge-fixture-version",
                "cross_language_syntax_v1\n",
            ),
            ("include/rk_bridge.h", CROSS_LANGUAGE_BRIDGE_H),
            ("src/c_entry.c", CROSS_LANGUAGE_C_ENTRY),
            ("src/cpp_bridge.cpp", CROSS_LANGUAGE_CPP_BRIDGE),
            ("bridge/go_bridge.go", CROSS_LANGUAGE_GO_BRIDGE),
            ("crates/rust_bridge/src/lib.rs", CROSS_LANGUAGE_RUST_BRIDGE),
            ("tests/fake_bridge.c", CROSS_LANGUAGE_FAKE_BRIDGE),
        ]),
        "project_alias_v1" => Ok(vec![("src/lib.rs", PROJECT_ALIAS_LIB_RS)]),
        "python_syntax_v2" => Ok(vec![
            ("docs/operations.md", PYTHON_OPERATIONS_MD),
            ("syntax_service/__init__.py", PYTHON_INIT),
            ("syntax_service/decorators.py", PYTHON_DECORATORS),
            ("syntax_service/errors.py", PYTHON_ERRORS),
            ("syntax_service/service.py", PYTHON_SERVICE),
            ("tests/fake_service.py", PYTHON_FAKE_SERVICE),
        ]),
        "javascript_syntax_v2" => Ok(vec![
            ("src/runtime.js", JAVASCRIPT_RUNTIME),
            ("src/registry.js", JAVASCRIPT_REGISTRY),
            ("src/index.js", JAVASCRIPT_INDEX),
            ("tests/fakeRuntime.js", JAVASCRIPT_FAKE_RUNTIME),
        ]),
        "typescript_syntax_v2" => Ok(vec![
            ("src/protocol.ts", TYPESCRIPT_PROTOCOL),
            ("src/provider.ts", TYPESCRIPT_PROVIDER),
            ("src/component.tsx", TYPESCRIPT_COMPONENT),
            ("src/index.ts", TYPESCRIPT_INDEX),
            ("tests/fakeProvider.ts", TYPESCRIPT_FAKE_PROVIDER),
        ]),
        "go_syntax_v2" => Ok(vec![
            ("go.mod", GO_MOD),
            ("processor/worker.go", GO_WORKER),
            ("processor/pipeline.go", GO_PIPELINE),
            ("tests/fake_worker.go", GO_FAKE_WORKER),
        ]),
        "java_syntax_v2" => Ok(vec![
            (
                "src/main/java/example/ServiceContract.java",
                JAVA_SERVICE_CONTRACT,
            ),
            (
                "src/main/java/example/AnnotatedService.java",
                JAVA_ANNOTATED_SERVICE,
            ),
            (
                "src/main/java/example/ServiceFactory.java",
                JAVA_SERVICE_FACTORY,
            ),
            ("src/test/java/example/FakeService.java", JAVA_FAKE_SERVICE),
        ]),
        "rust_syntax_v2" => Ok(vec![
            ("src/lib.rs", RUST_LIB),
            ("src/service.rs", RUST_SERVICE),
            ("src/model.rs", RUST_MODEL),
            ("tests/fake_service.rs", RUST_FAKE_SERVICE),
        ]),
        "bash_syntax_v1" => Ok(vec![
            ("bin/install.sh", BASH_INSTALL),
            ("lib/runtime.sh", BASH_RUNTIME),
            ("tests/fake_runtime.sh", BASH_FAKE_RUNTIME),
        ]),
        "csharp_syntax_v2" => Ok(vec![
            ("src/Runtime/BufferPool.cs", CSHARP_BUFFER_POOL),
            ("src/Runtime/RuntimeService.cs", CSHARP_RUNTIME_SERVICE),
            ("tests/FakeRuntimeService.cs", CSHARP_FAKE_SERVICE),
        ]),
        "kotlin_syntax_v2" => Ok(vec![
            ("src/main/kotlin/example/Client.kt", KOTLIN_CLIENT),
            ("src/main/kotlin/example/Pipeline.kt", KOTLIN_PIPELINE),
            ("tests/FakeClient.kt", KOTLIN_FAKE_CLIENT),
        ]),
        "php_syntax_v2" => Ok(vec![
            ("src/App/Kernel.php", PHP_KERNEL),
            ("src/App/Contracts/Bootable.php", PHP_BOOTABLE),
            ("src/App/Providers/CacheProvider.php", PHP_CACHE_PROVIDER),
            ("tests/FakeKernel.php", PHP_FAKE_KERNEL),
        ]),
        "ruby_syntax_v2" => Ok(vec![
            ("lib/app/controller.rb", RUBY_CONTROLLER),
            ("lib/app/extensions.rb", RUBY_EXTENSIONS),
            ("lib/app/runtime.rb", RUBY_RUNTIME),
            ("tests/fake_controller.rb", RUBY_FAKE_CONTROLLER),
        ]),
        "scala_syntax_v2" => Ok(vec![
            ("src/main/scala/example/Pipeline.scala", SCALA_PIPELINE),
            ("src/main/scala/example/Runtime.scala", SCALA_RUNTIME),
            ("tests/FakePipeline.scala", SCALA_FAKE_PIPELINE),
        ]),
        "swift_syntax_v2" => Ok(vec![
            ("Sources/App/SessionClient.swift", SWIFT_SESSION_CLIENT),
            ("Sources/App/RequestPipeline.swift", SWIFT_REQUEST_PIPELINE),
            (
                "Tests/AppTests/FakeSessionClient.swift",
                SWIFT_FAKE_SESSION_CLIENT,
            ),
        ]),
        "config_document_syntax_v1" => Ok(vec![
            ("README.md", CONFIG_DOCUMENT_README_MD),
            ("docs/reference.md", CONFIG_DOCUMENT_REFERENCE_MD),
            ("config/service.conf", CONFIG_DOCUMENT_SERVICE_CONF),
            ("config/runtime.json", CONFIG_DOCUMENT_RUNTIME_JSON),
        ]),
        "nonstandard_layout_v1" => Ok(vec![
            (
                ".relay-knowledge-fixture-version",
                "nonstandard_layout_v1\n",
            ),
            (
                "external_deps/python_sdk/session_client.py",
                NONSTANDARD_PYTHON_SESSION_CLIENT,
            ),
            (
                "external_deps/ts_sdk/sessionClient.ts",
                NONSTANDARD_TYPESCRIPT_SESSION_CLIENT,
            ),
            (
                "plugins/example.com/nonstandard/session/client.go",
                NONSTANDARD_GO_SESSION_CLIENT,
            ),
            (
                "modules/java_sdk/src/main/java/example/ExternalJavaSessionClient.java",
                NONSTANDARD_JAVA_SESSION_CLIENT,
            ),
            (
                "external_deps/cpp_sdk/include/external_session_client.hpp",
                NONSTANDARD_CPP_SESSION_CLIENT_HPP,
            ),
            (
                "external_deps/cpp_sdk/session_client.cpp",
                NONSTANDARD_CPP_SESSION_CLIENT_CPP,
            ),
            (
                "Sources/SwiftSdk/ExternalSwiftSessionClient.swift",
                NONSTANDARD_SWIFT_SESSION_CLIENT,
            ),
            ("src/application.ts", NONSTANDARD_APPLICATION_TS),
            ("Cargo.toml", NONSTANDARD_CARGO_TOML),
            ("Cargo.lock", NONSTANDARD_CARGO_LOCK),
            ("web/package.json", NONSTANDARD_PACKAGE_JSON),
            ("web/package-lock.json", NONSTANDARD_PACKAGE_LOCK_JSON),
            ("go.mod", NONSTANDARD_GO_MOD),
            ("pyproject.toml", NONSTANDARD_PYPROJECT_TOML),
            ("modules/java_sdk/pom.xml", NONSTANDARD_POM_XML),
            (
                "modules/java_sdk/build.gradle.kts",
                NONSTANDARD_BUILD_GRADLE_KTS,
            ),
            (
                "external_deps/cpp_sdk/conanfile.txt",
                NONSTANDARD_CONANFILE_TXT,
            ),
            (
                "external_deps/cpp_sdk/conanfile.py",
                NONSTANDARD_CONANFILE_PY,
            ),
        ]),
        "software_global_v1" => Ok(vec![
            (".relay-knowledge-fixture-version", "software_global_v1\n"),
            ("Cargo.toml", SOFTWARE_GLOBAL_CARGO_TOML),
            ("Cargo.lock", SOFTWARE_GLOBAL_CARGO_LOCK),
            ("src/lib.rs", SOFTWARE_GLOBAL_LIB_RS),
            ("src/sdk_probe.c", SOFTWARE_GLOBAL_SDK_PROBE_C),
            ("package.json", SOFTWARE_GLOBAL_PACKAGE_JSON),
            ("web/app.js", SOFTWARE_GLOBAL_APP_JS),
            ("go.mod", SOFTWARE_GLOBAL_GO_MOD),
            ("CMakeLists.txt", SOFTWARE_GLOBAL_CMAKE),
            ("Makefile", SOFTWARE_GLOBAL_MAKEFILE),
            (".github/workflows/ci.yml", SOFTWARE_GLOBAL_WORKFLOW),
            ("Dockerfile", SOFTWARE_GLOBAL_DOCKERFILE),
            ("docker-compose.yml", SOFTWARE_GLOBAL_COMPOSE),
            ("deploy/app.yaml", SOFTWARE_GLOBAL_K8S),
            ("infra/main.tf", SOFTWARE_GLOBAL_TERRAFORM),
            ("service/relay-global.service", SOFTWARE_GLOBAL_SYSTEMD),
            ("api/openapi.yaml", SOFTWARE_GLOBAL_OPENAPI),
            ("docs/architecture.md", SOFTWARE_GLOBAL_ARCHITECTURE_MD),
            ("README.md", SOFTWARE_GLOBAL_README_MD),
            ("docs/catalog.md", SOFTWARE_GLOBAL_CATALOG_MD),
            (
                ".knowledge/knowledge-map.yaml",
                SOFTWARE_GLOBAL_KNOWLEDGE_MAP,
            ),
            ("config/flags.yaml", SOFTWARE_GLOBAL_FLAGS_YAML),
            ("tests/smoke.rs", SOFTWARE_GLOBAL_SMOKE_RS),
            ("templates/deployment.yaml.j2", SOFTWARE_GLOBAL_TEMPLATE),
        ]),
        "agent_workflow_v1" => Ok(vec![
            ("Cargo.toml", AGENT_WORKFLOW_CARGO_TOML),
            ("src/lib.rs", AGENT_WORKFLOW_CORE_LIB_RS),
            ("src/context.rs", AGENT_WORKFLOW_CORE_CONTEXT_RS),
            ("src/orchestrator.rs", AGENT_WORKFLOW_CORE_ORCHESTRATOR_RS),
            ("web/contextPacket.ts", AGENT_WORKFLOW_WEB_CONTEXT_TS),
            ("web/entry.ts", AGENT_WORKFLOW_WEB_ENTRY_TS),
            ("ops/policy_loader.py", AGENT_WORKFLOW_OPS_POLICY_PY),
            ("config/agent-eval.yaml", AGENT_WORKFLOW_CONFIG_YAML),
            ("docs/agent-workflow.md", AGENT_WORKFLOW_DOC_MD),
        ]),
        other => Err(format!("unknown generated repository fixture: {other}")),
    }
}

fn write_grep_budget_fixture(root: &Path) -> Result<(), String> {
    for index in 0..300 {
        write_fixture_file(
            &root.join("src").join(format!("noise_{index:03}.c")),
            &format!("int noise_{index:03}(void) {{ return {index}; }}\n"),
        )?;
    }
    write_fixture_file(
        &root.join("zzz").join("late_target.c"),
        r#"// RK_LATE_BUDGET_NOTE must remain reachable after broad candidate selection.
int late_budget_target(void)
{
    return 7;
}
"#,
    )
}

fn write_index_performance_many_files_fixture(root: &Path) -> Result<(), String> {
    for index in 0..1024 {
        let shard = index / 64;
        write_fixture_file(
            &root
                .join("src")
                .join(format!("shard_{shard:03}"))
                .join(format!("file_{index:04}.rs")),
            &format!("pub fn rk_perf_target_{index:04}(input: u64) -> u64 {{ input + {index} }}\n"),
        )?;
    }

    write_fixture_file(
        &root.join("external_deps/rust_sdk/lib.rs"),
        "pub fn rk_perf_discovered_dependency(input: u64) -> u64 { input + 1 }\n",
    )?;

    Ok(())
}

fn write_index_performance_c_fragment_fixture(root: &Path) -> Result<(), String> {
    let mut fragment = String::new();
    for index in 0..256 {
        fragment.push_str(&format!(
            "{{ .flags = RK_PERF_ENTRY_VALID, .family = 6, .model = {index}, .data = 1 }},\n"
        ));
    }
    write_fixture_file(&root.join("include/perf_initializer_fragment.h"), &fragment)
}

fn write_index_performance_wide_mixed_files_fixture(root: &Path) -> Result<(), String> {
    write_fixture_file(
        &root.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/perf_core"]
resolver = "2"
"#,
    )?;
    write_fixture_file(
        &root.join("crates/perf_core/Cargo.toml"),
        r#"[package]
name = "rk-perf-core"
version = "0.1.0"
edition = "2021"
"#,
    )?;

    let mut lib_rs = String::from("pub mod bridge;\n");
    for shard in 0..32 {
        lib_rs.push_str(&format!("pub mod shard_{shard:03};\n"));
        let mut mod_rs = String::new();
        for offset in 0..64 {
            let index = shard * 64 + offset;
            mod_rs.push_str(&format!("pub mod file_{index:04};\n"));
            write_fixture_file(
                &root
                    .join("crates/perf_core/src")
                    .join(format!("shard_{shard:03}"))
                    .join(format!("file_{index:04}.rs")),
                &format!(
                    "pub struct RkWideRecord{index:04} {{ pub value: u64 }}\n\
                     pub fn rk_wide_target_{index:04}(input: u64) -> u64 {{\n\
                         input.wrapping_add({index}).rotate_left({rotate})\n\
                     }}\n\
                     pub fn rk_wide_map_{index:04}(items: &[u64]) -> u64 {{\n\
                         items.iter().copied().map(rk_wide_target_{index:04}).sum()\n\
                     }}\n",
                    rotate = index % 32
                ),
            )?;
        }
        write_fixture_file(
            &root
                .join("crates/perf_core/src")
                .join(format!("shard_{shard:03}"))
                .join("mod.rs"),
            &mod_rs,
        )?;
    }
    write_fixture_file(&root.join("crates/perf_core/src/lib.rs"), &lib_rs)?;
    write_fixture_file(
        &root.join("crates/perf_core/src/bridge.rs"),
        r#"use crate::shard_000::file_0000::rk_wide_target_0000;
use crate::shard_015::file_1023::rk_wide_target_1023;
use crate::shard_031::file_2047::rk_wide_target_2047;

pub fn rk_wide_bridge_dispatch(input: u64) -> u64 {
    let early = rk_wide_target_0000(input);
    let middle = rk_wide_target_1023(early);
    let late = rk_wide_target_2047(middle);
    rk_wide_target_2047(late)
}

pub fn rk_wide_cross_shard_pipeline(values: &[u64]) -> u64 {
    values
        .iter()
        .copied()
        .map(rk_wide_bridge_dispatch)
        .sum()
}
"#,
    )?;

    Ok(())
}

fn commit_generated_repository(
    runtime: &EvalRuntime,
    repo_name: &str,
    root: &Path,
) -> Vec<CommandResult> {
    let env = generated_git_env(&runtime.env);
    let commands = [
        vec!["git", "init", "-q"],
        vec![
            "git",
            "config",
            "user.email",
            "self-iteration@example.invalid",
        ],
        vec![
            "git",
            "config",
            "user.name",
            "relay-knowledge self-iteration",
        ],
        vec!["git", "add", "."],
        vec![
            "git",
            "commit",
            "--no-gpg-sign",
            "-q",
            "-m",
            "Generate relay-knowledge syntax fixture",
        ],
    ];
    commands
        .into_iter()
        .enumerate()
        .map(|(index, command)| {
            run_limited(
                &runtime.limiter,
                CommandSpec::new(
                    format!("{repo_name}_generated_fixture_git_{index}"),
                    command.into_iter().map(ToOwned::to_owned).collect(),
                    root,
                    Some(env.clone()),
                    runtime.timeout.min(30),
                ),
            )
        })
        .collect()
}

pub(in crate::evaluator) fn generated_git_env(
    env: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut scoped = env.clone();
    scoped.insert(
        "GIT_AUTHOR_DATE".to_owned(),
        "2026-05-20T00:00:00Z".to_owned(),
    );
    scoped.insert(
        "GIT_COMMITTER_DATE".to_owned(),
        "2026-05-20T00:00:00Z".to_owned(),
    );
    scoped
}

#[cfg(test)]
#[path = "repository_tests.rs"]
mod repository_tests;
