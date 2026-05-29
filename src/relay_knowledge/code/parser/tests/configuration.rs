use crate::domain::{CodeParseStatus, CodeRepositoryRegistration};

use super::*;

#[test]
fn configuration_tree_sitter_formats_extract_structured_facts() {
    let fixtures = [
        ConfigFixture {
            path: "README.md",
            source: b"# Runtime Guide\n\nConfigure retry_policy.\n",
            language_id: "markdown",
            symbol_name: "Runtime Guide",
            import_fragment: None,
        },
        ConfigFixture {
            path: "pom.xml",
            source: br#"<?xml version="1.0"?><project xsi:schemaLocation="urn:app schemas/app.xsd"><modelVersion>4.0.0</modelVersion></project>"#,
            language_id: "xml",
            symbol_name: "project",
            import_fragment: Some("schemas/app.xsd"),
        },
        ConfigFixture {
            path: "BUILD.bazel",
            source: br#"load(
    "//tools:defs.bzl",
    "rule",
)
# cc_library(name = "disabled_lib")
cc_library(name="runtime_lib", deps = [":core_lib"])"#,
            language_id: "starlark",
            symbol_name: "runtime_lib",
            import_fragment: Some("//tools:defs.bzl"),
        },
        ConfigFixture {
            path: "Makefile",
            source: b"include common.mk\nAPP = runtime\nbuild test: compile assets # generated locally\n\tFOO=bar ./script\n",
            language_id: "make",
            symbol_name: "build",
            import_fragment: Some("common.mk"),
        },
        ConfigFixture {
            path: "CMakeLists.txt",
            source: b"ADD_LIBRARY(\n  runtime_core\n  src/lib.cc\n)\nInclude(cmake/Flags.cmake)\ntarget_link_libraries(runtime_core PRIVATE core -pthread) # optional plugin\n",
            language_id: "cmake",
            symbol_name: "runtime_core",
            import_fragment: Some("cmake/Flags.cmake"),
        },
        ConfigFixture {
            path: "Dockerfile.dev",
            source: b"FROM --platform=$BUILDPLATFORM \\\n  rust:1.85 AS builder\n",
            language_id: "dockerfile",
            symbol_name: "builder",
            import_fragment: Some("rust:1.85"),
        },
        ConfigFixture {
            path: "app.properties",
            source: b"server.port 8080\n",
            language_id: "properties",
            symbol_name: "server.port",
            import_fragment: None,
        },
        ConfigFixture {
            path: "Cargo.toml",
            source: b"[package]\nname = \"relay-knowledge\"\n[[bin]]\nname = \"relay-knowledge\"\n",
            language_id: "toml",
            symbol_name: "package",
            import_fragment: None,
        },
        ConfigFixture {
            path: "settings.ini",
            source: b"[server]\nenabled=true\n",
            language_id: "ini",
            symbol_name: "server",
            import_fragment: None,
        },
        ConfigFixture {
            path: "config.yaml",
            source: b"server:\n  enabled: true\ncontainers:\n  - name: app\n",
            language_id: "yaml",
            symbol_name: "name",
            import_fragment: None,
        },
        ConfigFixture {
            path: "config.json",
            source: br#"{"enabled": true}"#,
            language_id: "json",
            symbol_name: "enabled",
            import_fragment: None,
        },
        ConfigFixture {
            path: "go.mod",
            source: b"module example.com/app\nrequire (\n\t// temporary pin\n\texample.com/lib v1.2.3\n)\n",
            language_id: "gomod",
            symbol_name: "example.com/app",
            import_fragment: None,
        },
        ConfigFixture {
            path: "build.ninja",
            source: b"rule cc\nbuild app.o: cc app.c\ninclude toolchain.ninja\n",
            language_id: "ninja",
            symbol_name: "app.o",
            import_fragment: Some("toolchain.ninja"),
        },
        ConfigFixture {
            path: "templates/page.html.j2",
            source: br#"{%- block body -%}{{ title }}{%- include "nav.html.j2" -%}{%- endblock -%}"#,
            language_id: "jinja2",
            symbol_name: "body",
            import_fragment: Some("nav.html.j2"),
        },
        ConfigFixture {
            path: "templates/deployment.yaml",
            source: br#"{{- define "app.labels" -}}app: relay{{- end -}}{{- include "app.labels" . -}}"#,
            language_id: "gotemplate",
            symbol_name: "app.labels",
            import_fragment: Some("app.labels"),
        },
    ];

    for fixture in fixtures {
        let snapshot = parse_source_snapshot(fixture.path, fixture.source);

        assert_eq!(
            snapshot.files[0].language_id, fixture.language_id,
            "{}",
            fixture.path
        );
        assert_eq!(
            snapshot.files[0].parse_status,
            CodeParseStatus::Parsed,
            "{} diagnostics: {:?}",
            fixture.path,
            snapshot.diagnostics
        );
        assert!(
            snapshot
                .symbols
                .iter()
                .any(|symbol| symbol.name == fixture.symbol_name),
            "{} should expose symbol {} in {:?}",
            fixture.path,
            fixture.symbol_name,
            snapshot.symbols
        );
        if let Some(fragment) = fixture.import_fragment {
            assert!(
                snapshot
                    .imports
                    .iter()
                    .any(|import| import.module.contains(fragment)),
                "{} should expose import {} in {:?}",
                fixture.path,
                fragment,
                snapshot.imports
            );
        }
    }
}

#[test]
fn configuration_imports_resolve_project_files() {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        12,
        0,
    );

    parse_indexed_file(
        &mut build,
        "templates/page.html.j2",
        br#"{% include "partials/nav.html.j2" %}"#,
    )
    .expect("template should parse");
    parse_indexed_file(
        &mut build,
        "templates/partials/nav.html.j2",
        br#"{% block nav %}Home{% endblock %}"#,
    )
    .expect("included template should parse");
    parse_indexed_file(
        &mut build,
        "templates/pages/page.html.j2",
        br#"{% include "../base.html.j2" %}"#,
    )
    .expect("relative template should parse");
    parse_indexed_file(
        &mut build,
        "templates/base.html.j2",
        br#"{% block base %}Base{% endblock %}"#,
    )
    .expect("parent template should parse");
    parse_indexed_file(
        &mut build,
        "BUILD.bazel",
        br#"load("//tools:defs.bzl", "rule")
cc_binary(name="app", deps=["//libs:core"])"#,
    )
    .expect("build file should parse");
    parse_indexed_file(&mut build, "tools/defs.bzl", br#"def rule(): pass"#)
        .expect("starlark file should parse");
    parse_indexed_file(
        &mut build,
        "libs/BUILD.bazel",
        br#"cc_library(name="core")"#,
    )
    .expect("dependency build file should parse");
    parse_indexed_file(&mut build, "Makefile", b"include common.mk\n")
        .expect("makefile should parse");
    parse_indexed_file(&mut build, "common.mk", b"APP = relay\n")
        .expect("included makefile should parse");
    parse_indexed_file(
        &mut build,
        "CMakeLists.txt",
        b"include(cmake/Flags)\nadd_subdirectory(src)\n",
    )
    .expect("cmake root should parse");
    parse_indexed_file(&mut build, "cmake/Flags.cmake", b"set(FLAGS enabled)\n")
        .expect("cmake include should parse");
    parse_indexed_file(
        &mut build,
        "src/CMakeLists.txt",
        b"add_library(runtime src/lib.cc)\n",
    )
    .expect("cmake child should parse");
    parse_indexed_file(
        &mut build,
        "templates/deployment.yaml",
        br#"{{- template "app.labels" . -}}"#,
    )
    .expect("helm template caller should parse");
    parse_indexed_file(
        &mut build,
        "templates/_helpers.tpl",
        br#"{{- define "app.labels" -}}app: relay{{- end -}}"#,
    )
    .expect("helm template definition should parse");
    parse_indexed_file(
        &mut build,
        "charts/backend/templates/deployments/deployment.yaml",
        br#"{{- include "backend.labels" . -}}"#,
    )
    .expect("nested helm template caller should parse");
    parse_indexed_file(
        &mut build,
        "charts/backend/templates/_helpers.tpl",
        br#"{{- define "backend.labels" -}}app: backend{{- end -}}"#,
    )
    .expect("chart helper definition should parse");

    let snapshot = build.finish();
    assert_resolved_import(
        &snapshot,
        "partials/nav.html.j2",
        "templates/partials/nav.html.j2",
    );
    assert_resolved_import(&snapshot, "../base.html.j2", "templates/base.html.j2");
    assert_resolved_import(&snapshot, "//tools:defs.bzl", "tools/defs.bzl");
    assert_resolved_import(&snapshot, "common.mk", "common.mk");
    assert_resolved_import(&snapshot, "src/CMakeLists.txt", "src/CMakeLists.txt");
    assert_resolved_import(&snapshot, "cmake/Flags.cmake", "cmake/Flags.cmake");
    assert_resolved_import(&snapshot, "app.labels", "templates/_helpers.tpl");
    assert_resolved_import(
        &snapshot,
        "backend.labels",
        "charts/backend/templates/_helpers.tpl",
    );

    let target_reference = snapshot
        .references
        .iter()
        .find(|reference| reference.name == "//libs:core")
        .expect("absolute Starlark target label should be indexed");
    assert_eq!(target_reference.resolution_state, "resolved");
}

#[test]
fn configuration_import_resolution_prefers_importer_directory() {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        3,
        0,
    );

    parse_indexed_file(
        &mut build,
        "templates/page.html.j2",
        br#"{% include "partials/nav.html.j2" %}"#,
    )
    .expect("template should parse");
    parse_indexed_file(
        &mut build,
        "templates/partials/nav.html.j2",
        br#"{% block nav %}Scoped{% endblock %}"#,
    )
    .expect("scoped included template should parse");
    parse_indexed_file(
        &mut build,
        "partials/nav.html.j2",
        br#"{% block nav %}Root{% endblock %}"#,
    )
    .expect("root included template should parse");

    let snapshot = build.finish();
    let import = snapshot
        .imports
        .iter()
        .find(|import| import.module.contains("partials/nav.html.j2"))
        .expect("template include should be indexed");

    assert_eq!(import.resolution_state, "resolved");
    assert_eq!(
        import.target_hint.as_deref(),
        Some("templates/partials/nav.html.j2")
    );
}

#[test]
fn malformed_strict_configuration_files_remain_partial() {
    let snapshot = parse_source_snapshot("config.json", br#"{"enabled": true"#);

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Partial);
    assert!(
        snapshot
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("error nodes"))
    );
}

#[test]
fn configuration_extractors_avoid_spurious_references() {
    let go_mod = parse_source_snapshot(
        "go.mod",
        b"module example.com/app\nrequire (\n\t// temporary pin\n\texample.com/lib v1.2.3\n)\nreplace example.com/old v1.2.3 => example.com/new v1.4.5\nreplace example.com/local => ../local\nreplace (\n\texample.com/block-old v1.0.0 => example.com/block-new v1.1.0\n)\n",
    );
    assert!(
        !go_mod
            .references
            .iter()
            .any(|reference| reference.name == "//"),
        "go.mod comments must not become dependency references: {:?}",
        go_mod.references
    );
    assert!(
        !go_mod
            .references
            .iter()
            .any(|reference| reference.name == "v1.2.3"),
        "go.mod replace versions must not become dependency references: {:?}",
        go_mod.references
    );
    assert!(
        go_mod
            .references
            .iter()
            .any(|reference| reference.name == "example.com/new"),
        "go.mod replace target modules should be indexed: {:?}",
        go_mod.references
    );
    assert!(
        go_mod
            .references
            .iter()
            .any(|reference| reference.name == "example.com/block-new"),
        "go.mod replace block target modules should be indexed: {:?}",
        go_mod.references
    );
    assert!(
        !go_mod
            .references
            .iter()
            .any(|reference| reference.name == "../local"),
        "go.mod local replace paths must not become dependency references: {:?}",
        go_mod.references
    );

    let ninja = parse_source_snapshot(
        "build.ninja",
        b"rule cc\nbuild app.o: cc $\n  app.c # generated locally\nbuilddir = out\ndepfile = ${builddir}/app.d\n",
    );
    assert!(
        !ninja
            .references
            .iter()
            .any(|reference| reference.name == "cc" && reference.kind == "target"),
        "ninja rule names must not become input target references: {:?}",
        ninja.references
    );
    assert!(
        ninja
            .references
            .iter()
            .any(|reference| { reference.name == "app.c" && reference.kind == "target" }),
        "ninja inputs should still be indexed: {:?}",
        ninja.references
    );
    assert!(
        !ninja
            .references
            .iter()
            .any(|reference| { matches!(reference.name.as_str(), "generated" | "locally") }),
        "ninja inline comments must not become target references: {:?}",
        ninja.references
    );
    assert!(
        ninja
            .references
            .iter()
            .any(|reference| reference.name == "builddir" && reference.kind == "variable"),
        "Ninja braced variable references should be indexed: {:?}",
        ninja.references
    );

    let starlark = parse_source_snapshot(
        "BUILD.bazel",
        br#"load(
    # "//old:defs.bzl"
    "//tools:defs.bzl",
    "rule",
)
# cc_library(name = "disabled_lib")
cc_library(
    name="runtime_lib",
    deps = select({"//conditions:default": [":real"]}) + ["//libs:remote"],
    visibility = ["//visibility:public"],
) # ":old_target""#,
    );
    assert!(
        !starlark
            .symbols
            .iter()
            .any(|symbol| symbol.name == "disabled_lib"),
        "commented Starlark targets must not be indexed: {:?}",
        starlark.symbols
    );
    assert!(
        starlark
            .symbols
            .iter()
            .any(|symbol| symbol.name == "runtime_lib"),
        "Starlark name= assignments should be indexed: {:?}",
        starlark.symbols
    );
    assert!(
        !starlark
            .references
            .iter()
            .any(|reference| reference.name == ":old_target"),
        "Starlark inline comments must not become target references: {:?}",
        starlark.references
    );
    assert!(
        starlark
            .references
            .iter()
            .any(|reference| reference.name == "//:real")
            && starlark
                .references
                .iter()
                .any(|reference| reference.name == "//libs:remote"),
        "Starlark target labels should be normalized: {:?}",
        starlark.references
    );
    assert!(
        !starlark.references.iter().any(|reference| {
            matches!(
                reference.name.as_str(),
                ":real" | "remote" | "//tools:defs.bzl"
            )
        }),
        "Starlark load labels and raw local labels must not become target references: {:?}",
        starlark.references
    );
    assert!(
        !starlark.references.iter().any(|reference| {
            matches!(
                reference.name.as_str(),
                "//visibility:public" | "//conditions:default"
            )
        }),
        "Bazel pseudo-labels must not become target references: {:?}",
        starlark.references
    );
    assert!(
        starlark
            .imports
            .iter()
            .any(|import| import.module == "//tools:defs.bzl")
            && !starlark
                .imports
                .iter()
                .any(|import| import.module == "//old:defs.bzl"),
        "Starlark load imports should ignore commented labels: {:?}",
        starlark.imports
    );
    let starlark_assignment = parse_source_snapshot(
        "tools/macros.bzl",
        br#"cc_library(
    srcs = glob(["*.cc"]),
    name = "nested_app",
)

def helper():
    name = "debug"

cc_library(
    name = "real_target",
)"#,
    );
    assert!(
        !starlark_assignment
            .symbols
            .iter()
            .any(|symbol| symbol.name == "debug"),
        "standalone Starlark name assignments must not become targets: {:?}",
        starlark_assignment.symbols
    );
    assert!(
        starlark_assignment
            .symbols
            .iter()
            .any(|symbol| symbol.name == "real_target"),
        "Starlark rule-call name arguments should still become targets: {:?}",
        starlark_assignment.symbols
    );
    assert!(
        starlark_assignment
            .symbols
            .iter()
            .any(|symbol| symbol.name == "nested_app"),
        "Starlark nested calls before name= must not close the rule call: {:?}",
        starlark_assignment.symbols
    );

    let make = parse_source_snapshot(
        "Makefile",
        b"OBJECTS = main.o\nall test: build # generated locally\ninstall:: build\n%.o: %.c\napp: \\\n  $(OBJECTS)\napp: CFLAGS += -O2\nbuild:\n\tFOO=bar ./script\n\techo: hi\n",
    );
    assert!(
        !make.symbols.iter().any(|symbol| symbol.name == "FOO"),
        "recipe assignments must not become Make variables: {:?}",
        make.symbols
    );
    assert!(
        !make.symbols.iter().any(|symbol| symbol.name == "echo"),
        "recipe commands must not become Make targets: {:?}",
        make.symbols
    );
    assert!(
        make.symbols.iter().any(|symbol| symbol.name == "all")
            && make.symbols.iter().any(|symbol| symbol.name == "test"),
        "multiple Make targets should be indexed independently: {:?}",
        make.symbols
    );
    assert!(
        !make.references.iter().any(|reference| {
            matches!(
                reference.name.as_str(),
                "generated" | "locally" | "CFLAGS" | "-O2"
            )
        }),
        "Make inline comments must not become prerequisites: {:?}",
        make.references
    );
    assert!(
        make.symbols.iter().any(|symbol| symbol.name == "%.o")
            && make
                .references
                .iter()
                .any(|reference| reference.name == "%.c" && reference.kind == "target"),
        "Make pattern rules should index pattern targets and prerequisites: {:?} {:?}",
        make.symbols,
        make.references
    );
    assert!(
        make.references
            .iter()
            .any(|reference| reference.name == "OBJECTS" && reference.kind == "variable"),
        "Make prerequisite variables should become variable references: {:?}",
        make.references
    );
    assert!(
        !make
            .references
            .iter()
            .any(|reference| reference.name == ":"),
        "Make double-colon rules must not emit ':' prerequisites: {:?}",
        make.references
    );

    let cmake = parse_source_snapshot(
        "CMakeLists.txt",
        b"include(cmake/Flags)\nset(SRC_DIR src)\nset(OUT ${SRC_DIR}/app)\nadd_library(app src.cc)\ntarget_link_libraries(app PRIVATE core /usr/lib/libssl.a -pthread) # optional plugin\n",
    );
    assert!(
        !cmake
            .references
            .iter()
            .any(|reference| matches!(reference.name.as_str(), "optional" | "plugin")),
        "CMake comments must not become target references: {:?}",
        cmake.references
    );
    assert!(
        !cmake
            .references
            .iter()
            .any(|reference| reference.name == "-pthread"),
        "CMake link flags must not become target references: {:?}",
        cmake.references
    );
    assert!(
        !cmake
            .references
            .iter()
            .any(|reference| reference.name == "/usr/lib/libssl.a"),
        "CMake linked library file paths must not become target references: {:?}",
        cmake.references
    );
    assert!(
        cmake
            .references
            .iter()
            .any(|reference| reference.name == "SRC_DIR" && reference.kind == "variable"),
        "CMake ${{...}} variable references should be indexed: {:?}",
        cmake.references
    );
    assert!(
        cmake
            .symbols
            .iter()
            .any(|symbol| symbol.name == "SRC_DIR" && symbol.kind == "variable"),
        "CMake set() variables should become variable symbols: {:?}",
        cmake.symbols
    );
    assert!(
        cmake
            .imports
            .iter()
            .any(|import| import.module == "cmake/Flags.cmake"),
        "CMake includes without suffix should target .cmake files: {:?}",
        cmake.imports
    );

    let dockerfile = parse_source_snapshot(
        "Dockerfile",
        b"FROM --platform=$TARGETPLATFORM \\\n  rust:1.85 AS builder\nFROM builder AS final\n",
    );
    assert!(
        dockerfile
            .imports
            .iter()
            .any(|import| import.module == "rust:1.85")
            && !dockerfile
                .imports
                .iter()
                .any(|import| import.module == "builder"),
        "Dockerfile prior stages must not become external image imports: {:?}",
        dockerfile.imports
    );

    let xml = parse_source_snapshot(
        "pom.xml",
        br#"<?xml version="1.0"?><project>
<!--
<disabled>
-->
<modelVersion>4.0.0</modelVersion></project>"#,
    );
    assert!(
        !xml.symbols.iter().any(|symbol| symbol.name == "?xml"),
        "XML processing instructions must not become elements: {:?}",
        xml.symbols
    );
    assert!(
        !xml.symbols.iter().any(|symbol| symbol.name == "disabled"),
        "XML elements inside multi-line comments must not become symbols: {:?}",
        xml.symbols
    );
    let xml_schema = parse_source_snapshot(
        "pom.xml",
        br#"<project xsi:noNamespaceSchemaLocation="schemas/app.xsd"></project>"#,
    );
    assert!(
        xml_schema
            .imports
            .iter()
            .any(|import| import.module == "schemas/app.xsd"),
        "XML no-namespace schema locations should become imports: {:?}",
        xml_schema.imports
    );
    let xml_import_comments = parse_source_snapshot(
        "pom.xml",
        br#"<project><!-- <xi:include href="old.xml"/> --><xi:include href="current.xml"/><xi:include href="second.xml"/></project>"#,
    );
    assert!(
        !xml_import_comments
            .imports
            .iter()
            .any(|import| import.module == "old.xml"),
        "XML imports inside comments must not be indexed: {:?}",
        xml_import_comments.imports
    );
    assert!(
        xml_import_comments
            .imports
            .iter()
            .any(|import| import.module == "current.xml"),
        "XML imports outside comments should still be indexed: {:?}",
        xml_import_comments.imports
    );
    assert!(
        xml_import_comments
            .imports
            .iter()
            .any(|import| import.module == "second.xml"),
        "every XML import attribute on a line should be indexed: {:?}",
        xml_import_comments.imports
    );

    let static_template =
        parse_source_snapshot("templates/config.yaml", b"server:\n  port: 8080\n");
    assert_eq!(static_template.files[0].language_id, "gotemplate");
    assert!(
        static_template
            .symbols
            .iter()
            .any(|symbol| symbol.name == "port" && symbol.kind == "config_key"),
        "static YAML files under templates/ should still expose config keys: {:?}",
        static_template.symbols
    );

    let jinja = parse_source_snapshot(
        "templates/page.html.j2",
        br#"{# {% include "old.html.j2" %} #}{% include "current.html.j2" %}{% include "footer.html.j2" %}"#,
    );
    assert!(
        !jinja
            .imports
            .iter()
            .any(|import| import.module == "old.html.j2"),
        "Jinja tags inside comments must not become imports: {:?}",
        jinja.imports
    );
    assert!(
        jinja
            .imports
            .iter()
            .any(|import| import.module == "current.html.j2"),
        "Jinja tags outside comments should still become imports: {:?}",
        jinja.imports
    );
    assert!(
        jinja
            .imports
            .iter()
            .any(|import| import.module == "footer.html.j2"),
        "every Jinja include action on a line should be indexed: {:?}",
        jinja.imports
    );

    let go_template = parse_source_snapshot(
        "templates/deployment.yaml",
        br#"{{/* {{ include "old.tpl" . }} */}}{{- includeIf "debug.tpl" . -}}{{- include "app.labels" . -}}-{{ include "app.version" . }}"#,
    );
    assert!(
        !go_template
            .imports
            .iter()
            .any(|import| import.module == "debug.tpl"),
        "Go-template action prefixes must not become imports: {:?}",
        go_template.imports
    );
    assert!(
        !go_template
            .imports
            .iter()
            .any(|import| import.module == "old.tpl"),
        "Go-template comments must not become imports: {:?}",
        go_template.imports
    );
    assert!(
        go_template
            .imports
            .iter()
            .any(|import| import.module == "app.labels"),
        "Go-template include actions should still be indexed: {:?}",
        go_template.imports
    );
    assert!(
        go_template
            .imports
            .iter()
            .any(|import| import.module == "app.version"),
        "every Go-template action on a line should be indexed: {:?}",
        go_template.imports
    );

    let json = parse_source_snapshot(
        "package.json",
        br#"{"scripts":{"test":"cargo test"},"dependencies":{"vite":"latest"}}"#,
    );
    assert!(
        json.symbols.iter().any(|symbol| symbol.name == "scripts")
            && json
                .symbols
                .iter()
                .any(|symbol| symbol.name == "dependencies"),
        "minified JSON object keys should all become config symbols: {:?}",
        json.symbols
    );
}

#[test]
fn configuration_references_do_not_resolve_to_unrelated_code_symbols() {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        2,
        0,
    );

    parse_indexed_file(&mut build, "Makefile", b"app: test\n").expect("makefile should parse");
    parse_indexed_file(&mut build, "src/lib.rs", b"fn test() {}\n")
        .expect("rust file should parse");

    let snapshot = build.finish();
    let reference = snapshot
        .references
        .iter()
        .find(|reference| reference.path == "Makefile" && reference.name == "test")
        .expect("Make target reference should be indexed");

    assert_eq!(reference.resolution_state, "unresolved");
    assert!(reference.target_symbol_snapshot_id.is_none());
}

#[test]
fn configuration_import_resolution_does_not_use_source_root_aliases() {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        2,
        0,
    );

    parse_indexed_file(&mut build, "Makefile", b"include common.mk\n")
        .expect("makefile should parse");
    parse_indexed_file(&mut build, "src/common.mk", b"APP = relay\n")
        .expect("source-root makefile should parse");

    let snapshot = build.finish();
    let import = snapshot
        .imports
        .iter()
        .find(|import| import.module == "common.mk")
        .expect("Make include should be indexed");

    assert_eq!(import.resolution_state, "unresolved");
    assert_eq!(import.target_hint.as_deref(), Some("common.mk"));
}

struct ConfigFixture {
    path: &'static str,
    source: &'static [u8],
    language_id: &'static str,
    symbol_name: &'static str,
    import_fragment: Option<&'static str>,
}

fn parse_source_snapshot(path: &str, source: &[u8]) -> crate::domain::CodeIndexSnapshot {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );

    parse_indexed_file(&mut build, path, source).expect("file should parse");

    build.finish()
}

fn assert_resolved_import(
    snapshot: &crate::domain::CodeIndexSnapshot,
    module_fragment: &str,
    target_hint: &str,
) {
    let import = snapshot
        .imports
        .iter()
        .find(|import| import.module.contains(module_fragment))
        .unwrap_or_else(|| panic!("import {module_fragment} should be indexed"));

    assert_eq!(import.resolution_state, "resolved", "{import:?}");
    assert_eq!(import.target_hint.as_deref(), Some(target_hint));
}
