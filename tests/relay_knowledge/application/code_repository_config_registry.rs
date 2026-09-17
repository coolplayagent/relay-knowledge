//! End-to-end acceptance of issues #389 and #394 through the normal index service.
use super::*;
use relay_knowledge::{api::CodeRepositoryRegisterRequest, domain::CodeConfigFilter};

#[tokio::test]
async fn portable_configuration_survives_real_indexing_source_filters_and_incremental_delete() {
    let repo = FixtureRepo::create("portable-config-lifecycle");
    let cases = [
        (
            "Reader.java",
            "java",
            "class Reader {String flag(){return System.getenv(\"FEATURE\");} void run(){String value=flag(); if(value!=null){}}}",
        ),
        (
            "reader.py",
            "python",
            "import os\ndef flag(): return os.getenv('FEATURE')\nvalue=flag()\nif value: pass\n",
        ),
        (
            "reader.js",
            "javascript",
            "function flag(){return process.env.FEATURE} const value=flag(); if(value) {}",
        ),
        (
            "reader.jsx",
            "jsx",
            "function flag(){return process.env.FEATURE} const value=flag(); if(value) {}",
        ),
        (
            "reader.ts",
            "typescript",
            "function flag(){return process.env.FEATURE} const value=flag(); if(value) {}",
        ),
        (
            "reader.tsx",
            "tsx",
            "function flag(){return process.env.FEATURE} const value=flag(); if(value) {}",
        ),
        (
            "reader.c",
            "c",
            "char *flag(void){return getenv(\"FEATURE\");} void run(){char *value=flag(); if(value){}}",
        ),
        (
            "reader.cpp",
            "cpp",
            "char *flag(){return std::getenv(\"FEATURE\");} void run(){auto value=flag(); if(value){}}",
        ),
        (
            "Reader.cs",
            "csharp",
            "class Reader {string flag(){return System.Environment.GetEnvironmentVariable(\"FEATURE\");} void run(){var value=flag(); if(value!=null){}}}",
        ),
        (
            "reader.rs",
            "rust",
            "fn flag()->Result<String,std::env::VarError>{std::env::var(\"FEATURE\")} fn run(){let value=flag(); if value.is_ok() {}}",
        ),
        (
            "reader.go",
            "go",
            "package demo\nfunc flag() string {return os.Getenv(\"FEATURE\")}\nfunc run(){value:=flag(); if value!=\"\"{}}",
        ),
        (
            "reader.kt",
            "kotlin",
            "fun flag(): String {return System.getenv(\"FEATURE\")}\nfun run(){val value=flag(); if(value!=null){}}",
        ),
        (
            "reader.scala",
            "scala",
            "def flag(): String = {return System.getenv(\"FEATURE\")}\ndef run(): Unit = {val value=flag(); if(value!=null){}}",
        ),
        (
            "reader.rb",
            "ruby",
            "def flag\n ENV.fetch('FEATURE')\nend\nvalue=flag()\nif value\nend\n",
        ),
        (
            "reader.php",
            "php",
            "<?php function flag(){return getenv('FEATURE');} $value=flag(); if($value){}",
        ),
        (
            "reader.swift",
            "swift",
            "func flag()->String? {return ProcessInfo.processInfo.environment[\"FEATURE\"]}\nfunc run(){let value=flag(); if value != nil {}}",
        ),
        (
            "reader.sh",
            "shell",
            "flag() { printf '%s' \"${FEATURE:-false}\"; }\nvalue=$(flag)\nif [ \"$value\" ]; then :; fi\n",
        ),
        (
            "reader.bzl",
            "starlark",
            "def flag(): return config.get('FEATURE')\nvalue=flag()\nif value: pass\n",
        ),
        (
            "reader.vue",
            "vue",
            "<script>function flag(){return process.env.FEATURE} const value=flag(); if(value) {}</script><template><div /></template>",
        ),
    ];
    for (path, _, source) in cases {
        repo.write(&format!("src/{path}"), source);
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "portable config"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-portable-config").await;
    for incremental in [false, true] {
        if incremental {
            repo.git(["rm", "-r", "src"]);
            repo.write("src/empty.rs", "fn empty() {}\n");
            repo.git(["add", "."]);
            repo.git(["commit", "-m", "remove configuration"]);
        }
        service
            .index_code_repository(
                CodeIndexRequest {
                    repository: selector("fixture", "HEAD"),
                    mode: if incremental {
                        CodeIndexMode::Incremental {
                            base_ref: "HEAD~1".into(),
                            head_ref: "HEAD".into(),
                        }
                    } else {
                        CodeIndexMode::Full
                    },
                    workspace_detection: Default::default(),
                    freshness_policy: FreshnessPolicy::WaitUntilFresh,
                    reuse_historical: false,
                },
                context("index-portable-config"),
            )
            .await
            .unwrap();
        let mut failures = Vec::new();
        for (_, source, _) in cases {
            let request = CodeFeatureFlagRequest::new(
                Some("FEATURE".into()),
                selector("fixture", "HEAD"),
                100,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap()
            .with_filters(CodeConfigFilter {
                source: Some(source.into()),
                ..Default::default()
            })
            .unwrap();
            let response = service
                .query_code_repository_feature_flags(request, context("portable-config-query"))
                .await
                .unwrap();
            if incremental {
                assert!(response.flags.is_empty(), "{source}: {:?}", response.flags);
                continue;
            }
            if !response.flags.iter().any(|flag| {
                flag.source_key == "FEATURE"
                    && flag
                        .usages
                        .iter()
                        .any(|u| u.edge_kind == "guards_code" && u.metadata.source_format == source)
            }) {
                failures.push(format!("{source}: {:?}", response.flags));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

#[tokio::test]
async fn configuration_registry_connects_formats_constants_getters_and_guards() {
    let repo = FixtureRepo::create("config-registry-acceptance");
    repo.write(
        "src/config.properties",
        "# @config domain=business hot-reload=true\nfeature_x=true\n",
    );
    repo.write("src/config.ini", "feature_x=false\n");
    repo.write("src/config.ctmpl", "feature_x={{ key \"feature_x\" }}\n");
    repo.write(
        "src/config.sh",
        "export FEATURE_ENV=true\necho \"$FEATURE_ENV\"\n",
    );
    repo.write(
        "src/FooConfig.java",
        "package demo; interface FooConfig { boolean getX(); }\n",
    );
    repo.write("src/DefaultFooConfig.java","package demo; class DefaultFooConfig implements FooConfig { public boolean getX() { return Boolean.parseBoolean(System.getProperty(\"feature_x\", \"false\")); } }\n");
    repo.write(
        "src/Keys.java",
        "package demo; class Keys { static final String Y=\"feature_y\"; }\n",
    );
    repo.write(
        "src/Reader.java",
        r#"package demo; class Reader { FooConfig field;
      void run(FooConfig config) {
        FooConfig local=config;
        System.getProperty("feature_x");
        if(Boolean.getBoolean("feature_x")){}
        if(field.getX()){} if(local.getX()){} if(config.getX()){}
        System.getProperty(Keys.Y);
      }}"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "configuration fixture"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".into(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context("config-register"),
        )
        .await
        .unwrap();
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("config-index"),
        )
        .await
        .unwrap();
    let request = CodeFeatureFlagRequest::new(
        Some("feature_x".into()),
        selector("fixture", "HEAD"),
        10,
        FreshnessPolicy::WaitUntilFresh,
    )
    .unwrap()
    .with_filters(CodeConfigFilter {
        domain: Some("business".into()),
        source: Some("properties".into()),
        hot_reload: Some(true),
        consistency: false,
    })
    .unwrap();
    let result = service
        .query_code_repository_feature_flags(request, context("config-query"))
        .await
        .unwrap();
    assert_eq!(result.freshness.state, CodeRepositoryFreshnessState::Fresh);
    assert_eq!(result.flags.len(), 1, "{:?}", result.flags);
    let flag = &result.flags[0];
    assert_eq!(flag.source_key, "feature_x");
    assert_eq!(
        flag.usages
            .iter()
            .filter(|u| u.edge_kind == "guards_code")
            .count(),
        4,
        "{:?}",
        flag.usages
    );
    for guard in flag.usages.iter().filter(|u| u.edge_kind == "guards_code") {
        assert!(flag.usages.iter().any(|read| Some(&read.usage_id)
            == guard.metadata.read_usage_id.as_ref()
            && read.edge_kind == "reads_config"));
    }
    for format in ["java", "properties", "ini", "ctmpl"] {
        assert!(
            flag.usages
                .iter()
                .any(|u| u.metadata.source_format == format),
            "{format}"
        );
    }
    let request = CodeFeatureFlagRequest::new(
        Some("feature_y".into()),
        selector("fixture", "HEAD"),
        10,
        FreshnessPolicy::WaitUntilFresh,
    )
    .unwrap()
    .with_filters(CodeConfigFilter {
        consistency: true,
        ..Default::default()
    })
    .unwrap();
    let result = service
        .query_code_repository_feature_flags(request, context("config-consistency"))
        .await
        .unwrap();
    let flag = &result.flags[0];
    assert_eq!(flag.source_key, "feature_y");
    assert!(flag.analysis_complete);
    assert!(
        flag.consistency_diagnostics
            .iter()
            .any(|d| d == "missing_from_format: ctmpl"),
        "{:?}",
        flag.consistency_diagnostics
    );
    assert!(
        flag.usages
            .iter()
            .any(|u| u.edge_kind == "declares_config_key")
    );
    assert!(flag.usages.iter().any(|u| u.edge_kind == "reads_config"));
    let request = CodeFeatureFlagRequest::new(
        Some("FEATURE_ENV".into()),
        selector("fixture", "HEAD"),
        10,
        FreshnessPolicy::WaitUntilFresh,
    )
    .unwrap();
    let result = service
        .query_code_repository_feature_flags(request, context("config-env"))
        .await
        .unwrap();
    assert_eq!(result.flags[0].source_kind, "env_var");
}
