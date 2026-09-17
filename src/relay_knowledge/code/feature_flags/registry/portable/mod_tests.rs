use super::*;

#[test]
fn portable_rust_readers_respect_nearest_import_and_local_standard_names() {
    for (source, expected) in [
        (
            "mod std {pub mod env {}} use std::env; fn run(){env::var_os(\"KEY\");}",
            false,
        ),
        (
            "mod std {pub mod env {}} use ::std::env; fn run(){env::var_os(\"KEY\");}",
            true,
        ),
        (
            "use custom::env; fn run(){use std::env; env::var_os(\"KEY\");}",
            true,
        ),
        (
            "use std::env; fn run(){use custom::env; env::var_os(\"KEY\");}",
            false,
        ),
    ] {
        let (rows, syntax) = analyze("app.rs", source);
        assert_eq!(
            rows.iter()
                .any(|r| r.source_key == "KEY" && r.edge_kind == "reads_config"),
            expected,
            "{source}: {syntax} {rows:?}"
        );
    }
}

#[test]
fn portable_readers_handle_grouped_imports_and_multiple_write_targets() {
    for source in [
        "use std::*; fn f(){ let x=std::env::var_os(\"KEY\"); }",
        "use std as std; fn f(){ let x=std::env::var_os(\"KEY\"); }",
        "use std::fmt; fn f(){ let x=std::env::var_os(\"KEY\"); }",
        "use std::{env, path::Path}; fn f(){ let x=env::var_os(\"KEY\"); }",
        "pub use std :: /* module */ env; fn f(){ let x=env::var_os(\"KEY\"); }",
        "use std::env; mod unrelated {mod env {}} fn f(){let x=env::var_os(\"KEY\");}",
    ] {
        let (rows, ast) = analyze("reader.rs", source);
        assert!(
            rows.iter()
                .any(|row| row.source_key == "KEY" && row.metadata.flow_incomplete.is_none()),
            "{source}: {rows:?}\n{ast}"
        );
    }
    let (rows, ast) = analyze(
        "reader.rb",
        "ENV['A'], ENV['B'] = 'a', 'b'\nENV[ENV['KEY']] = 'x'\n",
    );
    assert!(
        !rows
            .iter()
            .any(|row| ["A", "B"].contains(&row.source_key.as_str())),
        "{rows:?}\n{ast}"
    );
    assert!(
        rows.iter().any(|row| row.source_key == "KEY"),
        "{rows:?}\n{ast}"
    );
    let (rows, ast) = analyze(
        "reader.py",
        "import os\nreader = os.environ.get\nvalue = os.environ['KEY']\n",
    );
    assert!(
        !rows.iter().any(|row| row.source_key == "get"),
        "{rows:?}\n{ast}"
    );
    assert!(
        rows.iter().any(|row| row.source_key == "KEY"),
        "{rows:?}\n{ast}"
    );
}

#[test]
fn portable_reader_provenance_writes_and_parentheses_are_respected() {
    for (path, source) in [
        ("reader.rb", "ENV['ONLY_WRITE'] = # note\n '1'\n"),
        ("reader.js", "process.env.ONLY_WRITE = /* note */ '1';"),
    ] {
        let (rows, ast) = analyze(path, source);
        assert!(
            !rows.iter().any(|row| row.source_key == "ONLY_WRITE"),
            "{rows:?}\n{ast}"
        );
    }
    let (rows, ast) = analyze(
        "reader.rb",
        "ENV['ONLY_WRITE'] = ENV['ACTUAL_READ']\nENV['READ_WRITE'] ||= '1'\n",
    );
    assert!(
        !rows.iter().any(|row| row.source_key == "ONLY_WRITE"),
        "{rows:?}\n{ast}"
    );
    for key in ["ACTUAL_READ", "READ_WRITE"] {
        assert!(
            rows.iter().any(|row| row.source_key == key),
            "{rows:?}\n{ast}"
        );
    }
    for source in [
        "mod env {pub fn var_os(_: &str)->bool{false}} fn f(){let v=env::var_os(\"LOCAL_ONLY\");}",
        "use custom::{env}; fn f(){let v=env::var_os(\"LOCAL_ONLY\");}",
    ] {
        let (rows, ast) = analyze("reader.rs", source);
        assert!(
            !rows.iter().any(|row| row.source_key == "LOCAL_ONLY"),
            "{rows:?}\n{ast}"
        );
    }
    let (rows, ast) = analyze(
        "reader.py",
        "import os\nvalue = (os.environ.get)('ENABLED')\n",
    );
    assert!(
        rows.iter().any(|row| row.source_key == "ENABLED"),
        "{rows:?}\n{ast}"
    );
    assert!(!rows.iter().any(|row| row.source_key == "get"), "{rows:?}");
    let (rows, ast) = analyze(
        "reader.ts",
        "const exists = (process.env.hasOwnProperty)('ENABLED');",
    );
    assert!(
        !rows.iter().any(|row| row.source_key == "hasOwnProperty"),
        "{rows:?}\n{ast}"
    );
    let (rows, ast) = analyze(
        "reader.bzl",
        "def enabled(ctx):\n    return ctx.getenv('ENABLED', '0')\n",
    );
    assert!(
        rows.iter().any(|row| row.source_key == "ENABLED"
            && row.metadata.default_value.as_deref() == Some("0")
            && row.metadata.flow_incomplete.as_deref() == Some("unproven_environment_receiver")),
        "{rows:?}\n{ast}"
    );
}

#[test]
fn portable_environment_readers_preserve_keys_without_method_name_artifacts() {
    for (path, source, key) in [
        (
            "reader.py",
            "import os\nvalue = os.environ.get('ENABLED')\n",
            "ENABLED",
        ),
        ("reader.rb", "return unless ENV[\"ENABLED\"]\n", "ENABLED"),
        (
            "reader.rs",
            "use std::env; fn flag() { let v = env::var_os(\"ENABLED\"); }",
            "ENABLED",
        ),
        (
            "reader.ts",
            "import { ref } from 'vue'; const url = import.meta.env.BASE_URL;",
            "BASE_URL",
        ),
        (
            "reader.bzl",
            "KEY = 'ENABLED'\ndef enabled(ctx):\n    return ctx.getenv(KEY) == '1'\n",
            "ENABLED",
        ),
    ] {
        let (rows, ast) = analyze(path, source);
        assert!(
            rows.iter()
                .any(|row| row.source_kind == "env_var" && row.source_key == key),
            "{path}: {rows:?}\n{ast}"
        );
        assert!(
            !rows
                .iter()
                .any(|row| row.source_kind == "env_var" && row.source_key == "get"),
            "{rows:?}"
        );
    }
    let (rows, ast) = analyze(
        "reader.ts",
        "const exists = process.env.hasOwnProperty('ENABLED');",
    );
    assert!(
        !rows
            .iter()
            .any(|row| row.source_kind == "env_var" && row.source_key == "hasOwnProperty"),
        "{rows:?}\n{ast}"
    );
}

#[test]
fn portable_mutated_methods_and_properties_do_not_prove_local_guards() {
    for (path, source) in [
        (
            "reader.js",
            "class C {flag(){return process.env.FEATURE} run(){if(this.flag()) {}}} C.prototype.flag=()=>false; new C().run();",
        ),
        (
            "reader.js",
            "class C {get flag(){return process.env.FEATURE} run(){if(this.flag) {}}} C.prototype.flag=false; new C().run();",
        ),
        (
            "reader.py",
            "import os\nclass C:\n @property\n def flag(self): return os.getenv('FEATURE')\n def run(self):\n  if self.flag: pass\nC.flag=False\nC().run()\n",
        ),
    ] {
        let (rows, ast) = analyze(path, source);
        assert!(rows.iter().any(|r| r.source_key == "FEATURE"), "{rows:?}");
        assert!(
            !rows.iter().any(|r| r.source_key == "FEATURE"
                && (r.edge_kind == "guards_code" || r.metadata.declared_getter.is_some())),
            "{rows:?}\n{ast}"
        );
    }
}

#[test]
fn portable_nonnullable_conversions_do_not_activate_nullish_fallbacks() {
    for source in [
        "const value=Boolean(process.env.FEATURE) ?? true;",
        "const value=Number(process.env.FEATURE) ?? 1;",
    ] {
        let (rows, ast) = analyze("reader.js", source);
        assert!(rows.iter().any(|row| row.source_key == "FEATURE"));
        assert!(
            rows.iter()
                .filter(|row| row.source_key == "FEATURE")
                .all(|row| row.metadata.default_value.is_none()),
            "{rows:?}\n{ast}"
        );
    }
}

#[test]
fn native_getter_parameters_shadow_outer_functions() {
    for (path, source) in [
        (
            "reader.kt",
            "fun flag(): String {return System.getenv(\"FEATURE\")}\nfun run(flag: () -> String) {if(flag()!=\"\") {}}",
        ),
        (
            "reader.swift",
            "func flag()->String? {return ProcessInfo.processInfo.environment[\"FEATURE\"]}\nfunc run(flag: () -> Bool) {if flag() {}}",
        ),
        (
            "reader.scala",
            "def flag(): String = {return System.getenv(\"FEATURE\")}\ndef run(flag: () => Boolean): Unit = {if(flag()) {}}",
        ),
        (
            "reader.php",
            "<?php function flag(){return getenv('FEATURE');} function run($flag){if($flag()) {}}",
        ),
    ] {
        let (rows, ast) = analyze(path, source);
        assert!(
            !rows
                .iter()
                .any(|row| row.edge_kind == "guards_code" && row.source_key == "FEATURE"),
            "{path}: {rows:?}\n{ast}"
        );
    }
}

#[test]
fn absence_fallback_defaults_follow_language_operators() {
    for (path, source) in [
        ("reader.js", "const value=process.env.FEATURE ?? 'false';"),
        ("reader.ts", "const value=process.env.FEATURE ?? 'false';"),
        (
            "reader.py",
            "import os\nvalue=os.getenv('FEATURE') or 'false'\n",
        ),
        (
            "reader.kt",
            "val value=System.getenv(\"FEATURE\") ?: \"false\"",
        ),
        (
            "reader.swift",
            "let value=ProcessInfo.processInfo.environment[\"FEATURE\"] ?? \"false\"",
        ),
        (
            "reader.rs",
            "fn run(){let value=std::env::var(\"FEATURE\").unwrap_or(\"false\".to_owned());}",
        ),
    ] {
        let (rows, ast) = analyze(path, source);
        assert!(
            rows.iter().any(|r| r.source_key == "FEATURE"
                && r.metadata.default_value.as_deref() == Some("false")),
            "{path}: {rows:?}\n{ast}"
        );
    }
}

#[test]
fn imported_platform_aliases_and_local_names_do_not_invoke_property_getters() {
    for (path, source) in [
        (
            "reader.py",
            "import fake as os\nvalue=os.getenv('FEATURE')\n",
        ),
        (
            "reader.js",
            "import process from './fake.js'; const value=process.env.FEATURE;",
        ),
    ] {
        let (rows, _) = analyze(path, source);
        assert!(
            !rows
                .iter()
                .any(|r| r.source_key == "FEATURE" && r.edge_kind == "reads_config"),
            "{rows:?}"
        );
    }
    for (path, source) in [
        (
            "reader.js",
            "class C { get flag(){return process.env.FEATURE} run(){const flag=false; if(flag) {}} }",
        ),
        (
            "reader.js",
            "class C { get flag(){return process.env.FEATURE} run(){if(flag) {}} }",
        ),
        (
            "reader.py",
            "import os\nclass C:\n    @property\n    def flag(self): return os.getenv('FEATURE')\n    def run(self):\n        if flag: pass\n",
        ),
    ] {
        let (rows, _) = analyze(path, source);
        assert!(
            !rows.iter().any(|r| r.edge_kind == "guards_code"),
            "{rows:?}"
        );
    }
}

#[test]
fn shadowed_getters_and_rebound_imports_do_not_reuse_previous_evidence() {
    for source in [
        "import os\ndef flag():\n    return os.getenv('FEATURE')\ndef run(flag):\n    if flag(): pass\n",
        "import os\ndef flag():\n    return os.getenv('FEATURE')\nflag = unknown_callable\nif flag(): pass\n",
        "import os\nfrom settings import flag\ndef run(flag):\n    if flag(): pass\n",
    ] {
        let (rows, ast) = analyze("reader.py", source);
        assert!(
            !rows.iter().any(|row| row.edge_kind == "guards_code"),
            "{rows:?}\n{ast}"
        );
    }
    for source in [
        "import os\nfrom settings import KEY\nKEY = unknown()\nvalue = os.getenv(KEY)\n",
        "import os\nfrom settings import KEY\ndef run(KEY):\n    return os.getenv(KEY)\n",
    ] {
        let (rows, _) = analyze("reader.py", source);
        let read = rows.iter().find(|r| r.edge_kind == "reads_config").unwrap();
        assert_ne!(
            read.metadata.reference.as_deref(),
            Some("python|settings||KEY")
        );
        assert_eq!(
            read.metadata.flow_incomplete.as_deref(),
            Some("unresolved_configuration_key")
        );
    }
}

#[test]
fn reader_parameters_have_api_specific_default_semantics() {
    for (path, source) in [
        ("reader.php", "<?php $value = getenv('FEATURE', true);"),
        (
            "reader.cs",
            "class Reader { string Read() { return System.Environment.GetEnvironmentVariable(\"FEATURE\", EnvironmentVariableTarget.User); } }",
        ),
    ] {
        let (rows, ast) = analyze(path, source);
        let read = rows
            .iter()
            .find(|r| r.source_key == "FEATURE" && r.edge_kind == "reads_config")
            .unwrap_or_else(|| panic!("{rows:?}\n{ast}"));
        assert_eq!(read.metadata.default_value, None, "{rows:?}");
    }
    let (rows, _) = analyze(
        "reader.py",
        "import os\nvalue = os.getenv('FEATURE', unknown())\n",
    );
    let read = rows.iter().find(|r| r.source_key == "FEATURE").unwrap();
    assert_eq!(
        read.metadata.flow_incomplete.as_deref(),
        Some("unevaluated_explicit_default")
    );
}

#[test]
fn syntax_records_literal_and_constant_environment_keys() {
    let mut failures = Vec::new();
    for (path, source) in [
        (
            "sample.py",
            "import os\nKEY = \"FEATURE\"\nvalue = os.getenv(KEY, \"false\")\nif value:\n    pass\n",
        ),
        (
            "sample.js",
            "const KEY = \"FEATURE\"; const value = process.env[KEY]; if (value) {}",
        ),
        (
            "sample.jsx",
            "const KEY = \"FEATURE\"; const value = process.env[KEY]; if (value) {}",
        ),
        (
            "sample.ts",
            "const KEY = \"FEATURE\"; const value = process.env[KEY]; if (value) {}",
        ),
        (
            "sample.tsx",
            "const KEY = \"FEATURE\"; const value = process.env[KEY]; if (value) {}",
        ),
        (
            "sample.rs",
            "fn demo() { let key = \"FEATURE\"; let value = std::env::var(key); if value.is_ok() {} }",
        ),
        (
            "sample.c",
            "void demo() { const char *KEY = \"FEATURE\"; char *value = getenv(KEY); if (value) {} }",
        ),
        (
            "sample.cpp",
            "void demo() { const char *KEY = \"FEATURE\"; auto value = std::getenv(KEY); if (value) {} }",
        ),
        (
            "sample.go",
            "package demo\nfunc demo() { const KEY = \"FEATURE\"; value := os.Getenv(KEY); if value != \"\" {} }",
        ),
        (
            "sample.cs",
            "class Demo { void Run() { const string KEY = \"FEATURE\"; var value = System.Environment.GetEnvironmentVariable(KEY); if (value != null) {} } }",
        ),
        (
            "sample.kt",
            "fun demo() { val KEY = \"FEATURE\"; val value = System.getenv(KEY); if (value != null) {} }",
        ),
        (
            "sample.scala",
            "def demo(): Unit = { val KEY = \"FEATURE\"; val value = System.getenv(KEY); if (value != null) {} }",
        ),
        (
            "sample.rb",
            "KEY = \"FEATURE\"\nvalue = ENV.fetch(KEY, \"false\")\nif value\nend\n",
        ),
        (
            "sample.php",
            "<?php $KEY = \"FEATURE\"; $value = getenv($KEY); if ($value) {}",
        ),
        (
            "sample.swift",
            "func demo() { let KEY = \"FEATURE\"; let value = ProcessInfo.processInfo.environment[KEY]; if value != nil {} }",
        ),
        ("sample.bzl", "KEY = \"FEATURE\"\nvalue = config.get(KEY)\n"),
    ] {
        let language = crate::code::languages::detect_language(path).unwrap();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&(language.language)()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let input = FeatureFlagFileInput {
            line_index: Default::default(),
            syntax_root: Some(tree.root_node()),
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path,
            language_id: language.id,
            content: source,
            config_facts: &[],
        };
        let records = extract(&input).unwrap();
        if !records
            .iter()
            .any(|r| r.source_key == "FEATURE" && r.edge_kind == "reads_config")
        {
            failures.push(format!(
                "{path}: {records:?}\n{}",
                tree.root_node().to_sexp()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn analyze(path: &str, source: &str) -> (Vec<CodeFeatureFlagRecord>, String) {
    let language = crate::code::languages::detect_language(path).unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&(language.language)()).unwrap();
    let tree = parser.parse(source, None).unwrap();
    let input = FeatureFlagFileInput {
        line_index: Default::default(),
        syntax_root: Some(tree.root_node()),
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path,
        language_id: language.id,
        content: source,
        config_facts: &[],
    };
    (extract(&input).unwrap(), tree.root_node().to_sexp())
}

#[test]
fn zero_argument_getters_retain_read_and_condition_evidence_across_languages() {
    let mut failures = Vec::new();
    for (path, source) in [
        (
            "sample.py",
            "import os\ndef flag():\n    return os.getenv(\"FEATURE\", \"false\")\nvalue = flag()\nif value:\n    pass\n",
        ),
        (
            "sample.js",
            "function flag() { return process.env.FEATURE; } const value = flag(); if (value) {}",
        ),
        (
            "sample.jsx",
            "function flag() { return process.env.FEATURE; } const value = flag(); if (value) {}",
        ),
        (
            "sample.ts",
            "function flag() { return process.env.FEATURE; } const value = flag(); if (value) {}",
        ),
        (
            "sample.tsx",
            "function flag() { return process.env.FEATURE; } const value = flag(); if (value) {}",
        ),
        (
            "sample.rs",
            "fn flag() -> Result<String, std::env::VarError> { std::env::var(\"FEATURE\") } fn run() { let value = flag(); if value.is_ok() {} }",
        ),
        (
            "sample.c",
            "char *flag(void) { return getenv(\"FEATURE\"); } void run() { char *value = flag(); if (value) {} }",
        ),
        (
            "sample.cpp",
            "char *flag() { return std::getenv(\"FEATURE\"); } void run() { auto value = flag(); if (value) {} }",
        ),
        (
            "sample.go",
            "package demo\nfunc flag() string { return os.Getenv(\"FEATURE\") }\nfunc run() { value := flag(); if value != \"\" {} }",
        ),
        (
            "sample.cs",
            "class Demo { static string flag() { return System.Environment.GetEnvironmentVariable(\"FEATURE\"); } void Run() { var value = flag(); if (value != null) {} } }",
        ),
        (
            "sample.kt",
            "fun flag(): String { return System.getenv(\"FEATURE\") }\nfun run() { val value = flag(); if (value != null) {} }",
        ),
        (
            "sample.scala",
            "def flag(): String = { return System.getenv(\"FEATURE\") }\ndef run(): Unit = { val value = flag(); if (value != null) {} }",
        ),
        (
            "sample.rb",
            "def flag\n  ENV.fetch(\"FEATURE\", \"false\")\nend\nvalue = flag()\nif value\nend\n",
        ),
        (
            "sample.php",
            "<?php function flag() { return getenv(\"FEATURE\"); } $value = flag(); if ($value) {}",
        ),
        (
            "sample.swift",
            "func flag() -> String? { return ProcessInfo.processInfo.environment[\"FEATURE\"] }\nfunc run() { let value = flag(); if value != nil {} }",
        ),
        (
            "sample.bzl",
            "def flag():\n    return config.get(\"FEATURE\")\nvalue = flag()\nif value:\n    pass\n",
        ),
    ] {
        let (records, syntax) = analyze(path, source);
        if !records
            .iter()
            .any(|r| r.source_key == "FEATURE" && r.metadata.declared_getter.is_some())
            || !records.iter().any(|r| {
                r.source_key == "FEATURE"
                    && r.edge_kind == "guards_code"
                    && r.metadata.read_usage_id.is_some()
            })
        {
            failures.push(format!("{path}: {syntax}\n{records:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn mutations_shadowing_and_dynamic_keys_are_not_constant_facts() {
    for source in [
        "import os\nKEY = 'FEATURE'\nKEY = unknown()\nos.getenv(KEY)\n",
        "import os\nKEY = 'FEATURE'\ndef flag(KEY):\n    return os.getenv(KEY)\n",
        "import os\ndef a():\n    KEY = 'FEATURE'\ndef b():\n    return os.getenv(KEY)\n",
        "import os\nKEY = 'FEATURE'\ndef flag():\n    return os.getenv(KEY)\nKEY = unknown()\n",
        "import os\nKEY = 'FEATURE'\nchange(KEY)\nos.getenv(KEY)\n",
    ] {
        let (records, _) = analyze("sample.py", source);
        assert!(
            !records
                .iter()
                .any(|r| r.source_key == "FEATURE" && r.edge_kind == "reads_config"),
            "{source}: {records:?}"
        );
        assert!(
            records
                .iter()
                .any(|r| r.edge_kind == "reads_config" && r.metadata.flow_incomplete.is_some()),
            "{source}"
        );
    }
    let (records, _) = analyze(
        "sample.py",
        "def run(os):\n    return os.getenv('FEATURE')\n",
    );
    assert!(
        !records
            .iter()
            .any(|row| matches!(row.edge_kind.as_str(), "reads_config" | "guards_code"))
    );
}

#[test]
fn property_getters_preserve_configuration_guards() {
    let mut failures = Vec::new();
    for (path, source) in [
        (
            "sample.py",
            "import os\nclass Settings:\n    @property\n    def enabled(self):\n        return os.getenv('FEATURE')\n    def run(self):\n        if self.enabled:\n            pass\n",
        ),
        (
            "sample.js",
            "class Settings { get enabled() { return process.env.FEATURE; } run() { if(this.enabled) {} } }",
        ),
        (
            "sample.ts",
            "class Settings { get enabled() { return process.env.FEATURE; } run() { if(this.enabled) {} } }",
        ),
        (
            "sample.cs",
            "class Settings { string enabled { get { return System.Environment.GetEnvironmentVariable(\"FEATURE\"); } } void run() { if(enabled!=null) {} } }",
        ),
        (
            "sample.kt",
            "class Settings { val enabled: String get() = System.getenv(\"FEATURE\")\n fun run() { if(enabled!=null) {} }\n }",
        ),
        (
            "sample.swift",
            "class Settings { var enabled: String? { return ProcessInfo.processInfo.environment[\"FEATURE\"] }; func run() { if enabled != nil {} } }",
        ),
    ] {
        let (rows, syntax) = analyze(path, source);
        if !rows.iter().any(|r| r.metadata.declared_getter.is_some())
            || !rows
                .iter()
                .any(|r| r.edge_kind == "guards_code" && r.source_key == "FEATURE")
        {
            failures.push(format!("{path}: {syntax}\n{rows:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
