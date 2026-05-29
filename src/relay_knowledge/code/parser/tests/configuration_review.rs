use crate::domain::{CodeIndexSnapshot, CodeParseStatus, CodeRepositoryRegistration};

use super::*;

#[test]
fn configuration_review_regressions_skip_structural_noise() {
    let starlark = parse_source_snapshot(
        "BUILD.bazel",
        br#"load(
    # )
    "//tools:defs.bzl",
    "rule",
)
cc_binary(name = "app", deps = ["//libs"])
"#,
    );
    assert!(
        starlark
            .imports
            .iter()
            .any(|import| import.module == "//tools:defs.bzl"),
        "Starlark load comments must not close multiline calls: {:?}",
        starlark.imports
    );
    assert!(
        starlark
            .references
            .iter()
            .any(|reference| reference.name == "//libs:libs"),
        "Bazel package-only labels should normalize to default target labels: {:?}",
        starlark.references
    );
    assert!(
        !starlark
            .references
            .iter()
            .any(|reference| reference.name == "//libs"),
        "Bazel package-only labels should not keep the unresolvable package literal: {:?}",
        starlark.references
    );
    let starlark_string_parens = parse_source_snapshot(
        "BUILD.bazel",
        br#"load(
    "//tools:defs.bzl",
    "macro(",
)
load("//more:defs.bzl", "other")
"#,
    );
    assert!(
        starlark_string_parens
            .imports
            .iter()
            .any(|import| import.module == "//tools:defs.bzl")
            && starlark_string_parens
                .imports
                .iter()
                .any(|import| import.module == "//more:defs.bzl"),
        "Starlark string parentheses must not swallow later calls: {:?}",
        starlark_string_parens.imports
    );
    let starlark_scope = parse_sources_snapshot(&[
        (
            "app/BUILD",
            br#"cc_binary(name = "app", deps = [":core"])"# as &[u8],
        ),
        ("libs/BUILD", br#"cc_library(name = "core")"# as &[u8]),
    ]);
    assert!(
        starlark_scope.references.iter().any(|reference| {
            reference.path == "app/BUILD"
                && reference.name == "//app:core"
                && reference.target_symbol_snapshot_id.is_none()
        }),
        "local Bazel labels should stay scoped to their BUILD package: {:?}",
        starlark_scope.references
    );

    let go_mod = parse_source_snapshot(
        "go.mod",
        b"module example.com/app\nrequire (\n\texample.com/lib v1.2.3\n) // end require\nreplace example.com/old => example.com/new\n",
    );
    assert!(
        !go_mod
            .references
            .iter()
            .any(|reference| reference.name == "replace"),
        "go.mod trailing block comments must close require blocks before replace lines: {:?}",
        go_mod.references
    );
    assert!(
        go_mod
            .references
            .iter()
            .any(|reference| reference.name == "example.com/new"),
        "go.mod replace lines after commented block closes should still be indexed: {:?}",
        go_mod.references
    );

    let yaml = parse_source_snapshot(
        "config.yaml",
        b"script: |\n  echo: hello\nmirrors:\n  - http://example.com\nserver:\n  port: 8080\n",
    );
    assert!(
        !yaml.symbols.iter().any(|symbol| symbol.name == "echo"),
        "YAML block scalar content must not become config keys: {:?}",
        yaml.symbols
    );
    assert!(
        !yaml.symbols.iter().any(|symbol| symbol.name == "http"),
        "YAML scalar list URLs must not become config keys: {:?}",
        yaml.symbols
    );
    assert!(
        yaml.chunks.iter().any(|chunk| {
            chunk.symbol_snapshot_id.is_none() && chunk.content.contains("http://example.com")
        }),
        "structured config files with symbols should keep file chunks for scalar values: {:?}",
        yaml.chunks
    );
    assert!(
        yaml.symbols.iter().any(|symbol| symbol.name == "port"),
        "YAML mappings after a block scalar should still be indexed: {:?}",
        yaml.symbols
    );
    let yaml_sequence_scalar =
        parse_source_snapshot("config.yaml", b"scripts:\n  - |\n    echo: hi\nname: app\n");
    assert!(
        !yaml_sequence_scalar
            .symbols
            .iter()
            .any(|symbol| symbol.name == "echo"),
        "YAML sequence block scalar content must not become config keys: {:?}",
        yaml_sequence_scalar.symbols
    );
    assert!(
        yaml_sequence_scalar
            .symbols
            .iter()
            .any(|symbol| symbol.name == "name"),
        "YAML mappings after sequence block scalars should still be indexed: {:?}",
        yaml_sequence_scalar.symbols
    );
    let yaml_quoted_scalar =
        parse_source_snapshot("config.yaml", b"commands:\n  - \"echo: hi\"\nname: app\n");
    assert!(
        !yaml_quoted_scalar
            .symbols
            .iter()
            .any(|symbol| symbol.name == "echo"),
        "YAML quoted scalar colons must not become config keys: {:?}",
        yaml_quoted_scalar.symbols
    );
    assert!(
        yaml_quoted_scalar
            .symbols
            .iter()
            .any(|symbol| symbol.name == "name"),
        "YAML mappings after quoted scalar lists should still be indexed: {:?}",
        yaml_quoted_scalar.symbols
    );

    let cmake = parse_source_snapshot(
        "CMakeLists.txt",
        b"add_executable(${PROJECT_NAME} main.cc)\nadd_library(real src.cc)\nadd_custom_target(generated_headers)\nadd_dependencies(real generated_headers)\ntarget_link_libraries(real LINK_INTERFACE_LIBRARIES core)\n",
    );
    assert!(
        !cmake
            .symbols
            .iter()
            .any(|symbol| symbol.name.contains("PROJECT_NAME")),
        "CMake variable-expanded target names must not become symbols: {:?}",
        cmake.symbols
    );
    assert!(
        cmake.symbols.iter().any(|symbol| symbol.name == "real"),
        "literal CMake targets should still be indexed: {:?}",
        cmake.symbols
    );
    assert!(
        cmake
            .references
            .iter()
            .any(|reference| reference.name == "generated_headers" && reference.kind == "target"),
        "CMake add_dependencies target arguments should be indexed as target references: {:?}",
        cmake.references
    );
    assert!(
        !cmake
            .references
            .iter()
            .any(|reference| reference.name == "LINK_INTERFACE_LIBRARIES"),
        "legacy CMake link-interface keywords must not become target references: {:?}",
        cmake.references
    );
    let cmake_include = parse_sources_snapshot(&[
        (
            "cmake/Config.cmake",
            br#"include("${CMAKE_CURRENT_LIST_DIR}/Targets.cmake")"# as &[u8],
        ),
        (
            "cmake/Targets.cmake",
            b"add_library(pkg INTERFACE)\n" as &[u8],
        ),
        (
            "cmake/cmake/Targets.cmake",
            b"add_library(wrong INTERFACE)\n" as &[u8],
        ),
    ]);
    let cmake_include_import = cmake_include
        .imports
        .iter()
        .find(|import| import.module == "cmake/Targets.cmake")
        .expect("CMAKE_CURRENT_LIST_DIR include should be indexed");
    assert_eq!(
        cmake_include_import.resolution_state, "resolved",
        "CMAKE_CURRENT_LIST_DIR includes should resolve to sibling files: {:?}",
        cmake_include.imports
    );
    assert_eq!(
        cmake_include_import.target_hint.as_deref(),
        Some("cmake/Targets.cmake"),
        "already-resolved CMAKE_CURRENT_LIST_DIR includes must not prepend the parent twice: {:?}",
        cmake_include.imports
    );
    let cmake_nested_include = parse_sources_snapshot(&[
        (
            "src/CMakeLists.txt",
            b"include(cmake/Flags.cmake)\n" as &[u8],
        ),
        ("cmake/Flags.cmake", b"set(FLAGS enabled)\n" as &[u8]),
    ]);
    let nested_cmake_import = cmake_nested_include
        .imports
        .iter()
        .find(|import| import.path == "src/CMakeLists.txt" && import.module == "cmake/Flags.cmake")
        .expect("nested CMake include should be indexed");
    assert_eq!(
        nested_cmake_import.resolution_state, "unresolved",
        "relative CMake includes must not fall back to the repository root: {:?}",
        cmake_nested_include.imports
    );
    let cmake_quoted_parens = parse_source_snapshot(
        "CMakeLists.txt",
        b"set(PATTERN \"(\")\nset(BRACKET [=[(]=])\nadd_library(after src.cc)\n",
    );
    assert!(
        cmake_quoted_parens
            .symbols
            .iter()
            .any(|symbol| symbol.name == "after" && symbol.kind == "target"),
        "CMake quoted/bracket argument parentheses must not swallow later calls: {:?}",
        cmake_quoted_parens.symbols
    );
    let cmake_variable_scope = parse_sources_snapshot(&[
        ("app/CMakeLists.txt", b"set(SRC_DIR src)\n" as &[u8]),
        (
            "tools/CMakeLists.txt",
            b"set(OUT ${SRC_DIR}/tool)\n" as &[u8],
        ),
    ]);
    let cross_file_cmake_var = cmake_variable_scope
        .references
        .iter()
        .find(|reference| {
            reference.path == "tools/CMakeLists.txt"
                && reference.name == "SRC_DIR"
                && reference.kind == "variable"
        })
        .expect("tools CMake file should record SRC_DIR reference");
    assert_eq!(
        cross_file_cmake_var.target_symbol_snapshot_id, None,
        "CMake variable references should not resolve to unrelated CMake files: {:?}",
        cmake_variable_scope.references
    );
    assert_eq!(cross_file_cmake_var.resolution_state, "unresolved");
    let cmake_subdir_scope = parse_sources_snapshot(&[
        ("src/CMakeLists.txt", b"add_subdirectory(lib)\n" as &[u8]),
        (
            "lib/CMakeLists.txt",
            b"add_library(root_level src.cc)\n" as &[u8],
        ),
    ]);
    let src_subdir_import = cmake_subdir_scope
        .imports
        .iter()
        .find(|import| {
            import.path == "src/CMakeLists.txt" && import.module == "src/lib/CMakeLists.txt"
        })
        .expect("relative CMake subdirectory should be indexed under caller directory");
    assert_eq!(
        src_subdir_import.resolution_state, "unresolved",
        "CMake add_subdirectory should not fall back to a root-level sibling: {:?}",
        cmake_subdir_scope.imports
    );
    let cmake_alias = parse_source_snapshot(
        "CMakeLists.txt",
        b"add_library(real src.cc)\nadd_library(alias ALIAS real)\nadd_subdirectory(${THIRD_PARTY_DIR}/foo)\n",
    );
    assert!(
        cmake_alias
            .references
            .iter()
            .any(|reference| reference.name == "real" && reference.kind == "target"),
        "CMake ALIAS operands should be indexed as target references: {:?}",
        cmake_alias.references
    );
    assert!(
        !cmake_alias
            .imports
            .iter()
            .any(|import| import.module.contains("THIRD_PARTY_DIR")),
        "CMake variable-expanded subdirectories should not become file imports: {:?}",
        cmake_alias.imports
    );
    let cmake_absolute_subdir = parse_source_snapshot(
        "src/CMakeLists.txt",
        b"add_subdirectory(/opt/vendor/lib build)\nadd_library(app src.cc)\n",
    );
    assert!(
        !cmake_absolute_subdir
            .imports
            .iter()
            .any(|import| import.module.contains("/opt/vendor/lib")),
        "CMake absolute add_subdirectory paths should not become repo-relative imports: {:?}",
        cmake_absolute_subdir.imports
    );
    assert!(
        cmake_absolute_subdir
            .symbols
            .iter()
            .any(|symbol| symbol.name == "app"),
        "CMake calls after absolute subdirectories should still be indexed: {:?}",
        cmake_absolute_subdir.symbols
    );
    let cmake_bracket_comment = parse_source_snapshot(
        "CMakeLists.txt",
        b"#[[\nadd_library(disabled disabled.cc)\ninclude(disabled.cmake)\n]]\nadd_library(enabled src.cc)\n",
    );
    assert!(
        !cmake_bracket_comment
            .symbols
            .iter()
            .any(|symbol| symbol.name == "disabled")
            && !cmake_bracket_comment
                .imports
                .iter()
                .any(|import| import.module == "disabled.cmake"),
        "CMake bracket comments must not emit disabled calls: {:?} {:?}",
        cmake_bracket_comment.symbols,
        cmake_bracket_comment.imports
    );
    assert!(
        cmake_bracket_comment
            .symbols
            .iter()
            .any(|symbol| symbol.name == "enabled"),
        "CMake calls after bracket comments should still be indexed: {:?}",
        cmake_bracket_comment.symbols
    );

    let markdown = parse_source_snapshot(
        "README.md",
        b"# Runtime\n    # install\n```sh\n# install dependencies\n```\n## Usage\n",
    );
    assert!(
        !markdown
            .symbols
            .iter()
            .any(|symbol| symbol.name == "install dependencies"),
        "Markdown fenced-code comments must not become headings: {:?}",
        markdown.symbols
    );
    assert!(
        !markdown
            .symbols
            .iter()
            .any(|symbol| symbol.name == "install"),
        "Markdown indented-code comments must not become headings: {:?}",
        markdown.symbols
    );
    assert!(
        markdown.symbols.iter().any(|symbol| symbol.name == "Usage"),
        "Markdown headings after fenced code should still be indexed: {:?}",
        markdown.symbols
    );
    assert!(
        markdown
            .chunks
            .iter()
            .any(|chunk| chunk.symbol_snapshot_id.is_none() && chunk.content.contains("## Usage")),
        "Markdown files with heading symbols should keep a file chunk for body text: {:?}",
        markdown.chunks
    );
    let properties = parse_source_snapshot(
        "app.properties",
        b"message = hello \\\n  world\nnext = yes\n",
    );
    assert!(
        !properties
            .symbols
            .iter()
            .any(|symbol| symbol.name == "world"),
        "continued Java properties value lines must not become keys: {:?}",
        properties.symbols
    );
    assert!(
        properties
            .symbols
            .iter()
            .any(|symbol| symbol.name == "next"),
        "Java properties keys after continuations should still be indexed: {:?}",
        properties.symbols
    );

    let jinja = parse_source_snapshot(
        "templates/page.html.j2",
        br#"{% include ["primary.html", "fallback.html"] %}"#,
    );
    assert!(
        jinja
            .imports
            .iter()
            .any(|import| import.module == "primary.html"),
        "first Jinja include-list template should be indexed: {:?}",
        jinja.imports
    );
    assert!(
        jinja
            .imports
            .iter()
            .any(|import| import.module == "fallback.html"),
        "fallback Jinja include-list template should be indexed: {:?}",
        jinja.imports
    );
    let jinja_template_root = parse_sources_snapshot(&[
        (
            "templates/pages/index.html.j2",
            br#"{% extends "layouts/base.html.j2" %}"# as &[u8],
        ),
        (
            "templates/layouts/base.html.j2",
            b"{% block body %}{% endblock %}" as &[u8],
        ),
    ]);
    let template_root_import = jinja_template_root
        .imports
        .iter()
        .find(|import| {
            import.path == "templates/pages/index.html.j2"
                && import.module == "layouts/base.html.j2"
        })
        .expect("Jinja template-root import should be indexed");
    assert_eq!(
        template_root_import.resolution_state, "resolved",
        "Jinja nested templates should resolve imports from the nearest template root: {:?}",
        jinja_template_root.imports
    );
    assert_eq!(
        template_root_import.target_hint.as_deref(),
        Some("templates/layouts/base.html.j2"),
        "Jinja template-root imports should target the sibling root directory: {:?}",
        jinja_template_root.imports
    );

    let xml = parse_source_snapshot(
        "pom.xml",
        br#"<root data-href = "bad.xml" relocation = "move.xml"><description>use href="prose.xml" in docs</description><![CDATA[<old href="cdata.xml"></old>]]><real href = "ok.xml" location = "extra.xml"/></root>"#,
    );
    assert!(
        !xml.symbols.iter().any(|symbol| symbol.name == "old"),
        "XML CDATA elements must not become symbols: {:?}",
        xml.symbols
    );
    assert!(
        !xml.imports.iter().any(|import| matches!(
            import.module.as_str(),
            "bad.xml" | "move.xml" | "cdata.xml" | "prose.xml"
        )),
        "XML prose, CDATA, and non-import attributes must not become imports: {:?}",
        xml.imports
    );
    assert!(
        xml.imports.iter().any(|import| import.module == "ok.xml")
            && xml
                .imports
                .iter()
                .any(|import| import.module == "extra.xml"),
        "XML imports outside CDATA and exact import attributes should still be indexed: {:?}",
        xml.imports
    );
    let xml_multiline = parse_source_snapshot(
        "pom.xml",
        br#"<project
  xsi:schemaLocation="urn:example schema/project.xsd"
  href="project.xml">
</project>"#,
    );
    assert!(
        xml_multiline
            .imports
            .iter()
            .any(|import| import.module == "schema/project.xsd")
            && xml_multiline
                .imports
                .iter()
                .any(|import| import.module == "project.xml"),
        "XML multiline start-tag imports should be indexed: {:?}",
        xml_multiline.imports
    );
    assert!(
        xml_multiline
            .symbols
            .iter()
            .any(|symbol| symbol.name == "project"),
        "XML multiline start tags should still emit element symbols: {:?}",
        xml_multiline.symbols
    );

    let malformed_starlark = parse_source_snapshot(
        "BUILD.bazel",
        br#"cc_library(name = "app")
load(
"#,
    );
    assert_eq!(
        malformed_starlark.files[0].parse_status,
        CodeParseStatus::Partial,
        "malformed configuration files should stay degraded even with partial facts"
    );
    let malformed_cmake = parse_source_snapshot("CMakeLists.txt", b"add_library(app src.cc)\n)\n");
    assert_eq!(
        malformed_cmake.files[0].parse_status,
        CodeParseStatus::Partial,
        "unbalanced CMake files should stay degraded even with target facts"
    );
    let balanced_invalid_cmake =
        parse_source_snapshot("CMakeLists.txt", b"add_library(app src.cc)\nnot_a_call\n");
    assert_eq!(
        balanced_invalid_cmake.files[0].parse_status,
        CodeParseStatus::Partial,
        "balanced CMake files with invalid top-level syntax should stay degraded"
    );
    let malformed_ninja = parse_source_snapshot("build.ninja", b"build app.o cc app.c\n");
    assert_eq!(
        malformed_ninja.files[0].parse_status,
        CodeParseStatus::Partial,
        "malformed Ninja manifests should stay degraded even when manual facts exist"
    );
    let malformed_go_template =
        parse_source_snapshot("templates/deployment.yaml", br#"{{ include "app.labels" ."#);
    assert_eq!(
        malformed_go_template.files[0].parse_status,
        CodeParseStatus::Partial,
        "malformed Go-template actions should stay degraded even when manual facts exist"
    );
    let make_scope = parse_sources_snapshot(&[
        ("app/Makefile", b"app: build\n" as &[u8]),
        ("tools/Makefile", b"build:\n" as &[u8]),
    ]);
    let cross_file_make_ref = make_scope
        .references
        .iter()
        .find(|reference| {
            reference.path == "app/Makefile"
                && reference.name == "build"
                && reference.kind == "target"
        })
        .expect("app Makefile should record build prerequisite");
    assert_eq!(
        cross_file_make_ref.target_symbol_snapshot_id, None,
        "Make target prerequisites should not resolve to unrelated Makefiles: {:?}",
        make_scope.references
    );
    assert_eq!(cross_file_make_ref.resolution_state, "unresolved");

    let make_variable_scope = parse_sources_snapshot(&[
        ("app/Makefile", b"all: $(OBJECTS)\n" as &[u8]),
        ("tools/Makefile", b"OBJECTS = tool.o\n" as &[u8]),
    ]);
    let cross_file_make_var = make_variable_scope
        .references
        .iter()
        .find(|reference| {
            reference.path == "app/Makefile"
                && reference.name == "OBJECTS"
                && reference.kind == "variable"
        })
        .expect("app Makefile should record OBJECTS prerequisite variable");
    assert_eq!(
        cross_file_make_var.target_symbol_snapshot_id, None,
        "Make variable references should not resolve to unrelated Makefiles: {:?}",
        make_variable_scope.references
    );
    assert_eq!(cross_file_make_var.resolution_state, "unresolved");

    let make_patterns = parse_source_snapshot(
        "Makefile",
        b"OBJECTS = app.o # $(OLD_OBJECTS)\nall: ${OBJECTS}\nobjects: %.o: %.c\n",
    );
    let brace_make_var = make_patterns
        .references
        .iter()
        .find(|reference| reference.name == "OBJECTS" && reference.kind == "variable")
        .expect("brace-style Make variable should be indexed");
    assert_eq!(
        brace_make_var.resolution_state, "resolved",
        "brace-style Make variables should resolve inside the same Makefile: {:?}",
        make_patterns.references
    );
    assert!(
        !make_patterns
            .references
            .iter()
            .any(|reference| matches!(reference.name.as_str(), "%.o:" | "%.c")),
        "Make static pattern rules should not index target patterns as prerequisites: {:?}",
        make_patterns.references
    );
    assert!(
        !make_patterns
            .references
            .iter()
            .any(|reference| reference.name == "OLD_OBJECTS"),
        "Make assignment comments must not become variable references: {:?}",
        make_patterns.references
    );
    let make_modifiers = parse_source_snapshot(
        "Makefile",
        b"SOURCES = app.c\nexport OBJECTS = $(SOURCES:.c=.o)\noverride CFLAGS += -O2\n",
    );
    assert!(
        make_modifiers
            .symbols
            .iter()
            .any(|symbol| symbol.name == "OBJECTS")
            && make_modifiers
                .symbols
                .iter()
                .any(|symbol| symbol.name == "CFLAGS"),
        "Make assignment modifiers should not hide variable symbols: {:?}",
        make_modifiers.symbols
    );
    let make_modifier_ref = make_modifiers
        .references
        .iter()
        .find(|reference| reference.name == "SOURCES" && reference.kind == "variable")
        .expect("Make substitution assignment should reference SOURCES");
    assert_eq!(
        make_modifier_ref.resolution_state, "resolved",
        "Make variable references inside modifier assignments should resolve: {:?}",
        make_modifiers.references
    );

    let make = parse_source_snapshot(
        "Makefile",
        b"define SCRIPT\necho: hi\nendef\ninclude $(GENERATED_MKS)\ninclude static.mk\nreal: dep\n",
    );
    assert!(
        !make.symbols.iter().any(|symbol| symbol.name == "echo"),
        "Make define body lines must not become rule targets: {:?}",
        make.symbols
    );
    assert!(
        !make
            .imports
            .iter()
            .any(|import| import.module.contains("GENERATED_MKS"))
            && make
                .imports
                .iter()
                .any(|import| import.module == "static.mk"),
        "Make variable-expanded includes should be skipped while static includes remain: {:?}",
        make.imports
    );

    let ninja = parse_source_snapshot("build.ninja", b"command = echo $$HOME ${real} $builddir\n");
    assert!(
        !ninja
            .references
            .iter()
            .any(|reference| reference.name == "HOME"),
        "Ninja escaped dollar literals must not become variable refs: {:?}",
        ninja.references
    );
    assert!(
        ninja
            .references
            .iter()
            .any(|reference| reference.name == "real")
            && ninja
                .references
                .iter()
                .any(|reference| reference.name == "builddir"),
        "Ninja real variable references should still be indexed: {:?}",
        ninja.references
    );
    let ninja_manifest = parse_source_snapshot(
        "build.ninja",
        b"rule cc\n  command = cc $in -o $out\nbuild app.o: cc app.c\n",
    );
    assert_eq!(
        ninja_manifest.files[0].parse_status,
        CodeParseStatus::Parsed,
        "valid Ninja manifests should not be degraded by the Make grammar fallback"
    );
    let ninja_dynamic_include = parse_source_snapshot(
        "build.ninja",
        b"builddir = out\nsubninja $builddir/rules.ninja\ninclude static.ninja\n",
    );
    assert!(
        !ninja_dynamic_include
            .imports
            .iter()
            .any(|import| import.module == "$builddir/rules.ninja")
            && ninja_dynamic_include
                .imports
                .iter()
                .any(|import| import.module == "static.ninja"),
        "Ninja variable-expanded includes should be skipped while static includes remain: {:?}",
        ninja_dynamic_include.imports
    );

    let dockerfile = parse_source_snapshot(
        "Dockerfile",
        b"ARG BASE_IMAGE=alpine\nFROM ${BASE_IMAGE} AS builder\nFROM alpine AS final\n",
    );
    assert!(
        !dockerfile
            .imports
            .iter()
            .any(|import| import.module.contains("BASE_IMAGE"))
            && dockerfile
                .imports
                .iter()
                .any(|import| import.module == "alpine"),
        "Dockerfile variable-expanded base images should not become imports: {:?}",
        dockerfile.imports
    );
    let docker_variants = parse_sources_snapshot(&[
        ("Dockerfile.dev", b"FROM alpine AS builder\n" as &[u8]),
        ("Dockerfile.prod", b"FROM alpine AS builder\n" as &[u8]),
    ]);
    let builder_ids = docker_variants
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "builder")
        .map(|symbol| symbol.canonical_symbol_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        builder_ids.len(),
        2,
        "Dockerfile variants should both expose their stage symbols: {:?}",
        docker_variants.symbols
    );
    assert!(
        builder_ids
            .iter()
            .any(|canonical_id| canonical_id.contains("Dockerfile.dev"))
            && builder_ids
                .iter()
                .any(|canonical_id| canonical_id.contains("Dockerfile.prod"))
            && builder_ids[0] != builder_ids[1],
        "Dockerfile variant suffixes should remain in canonical symbol ids: {:?}",
        builder_ids
    );
    let docker_copy_from = parse_source_snapshot(
        "Dockerfile",
        b"FROM alpine AS builder\nFROM scratch AS final\nCOPY --from=builder /out /app\nCOPY --from=nginx:alpine /etc/nginx /nginx\n",
    );
    let copy_stage_ref = docker_copy_from
        .references
        .iter()
        .find(|reference| reference.name == "builder" && reference.kind == "stage")
        .expect("Docker COPY --from should reference prior stages");
    assert_eq!(
        copy_stage_ref.resolution_state, "resolved",
        "Docker COPY --from stage references should resolve in the same file: {:?}",
        docker_copy_from.references
    );
    assert!(
        docker_copy_from
            .imports
            .iter()
            .any(|import| import.module == "nginx:alpine")
            && !docker_copy_from
                .imports
                .iter()
                .any(|import| import.module == "builder"),
        "Docker COPY --from should import external images but not prior stages: {:?}",
        docker_copy_from.imports
    );

    let go_template = parse_source_snapshot(
        "templates/deployment.yaml",
        br#"{{- /* {{ include "old.tpl" . }} */ -}}{{ include "current.tpl" . }}"#,
    );
    assert!(
        !go_template
            .imports
            .iter()
            .any(|import| import.module == "old.tpl")
            && go_template
                .imports
                .iter()
                .any(|import| import.module == "current.tpl"),
        "trimmed Go-template comments should not emit disabled imports: {:?}",
        go_template.imports
    );
    let dynamic_go_template = parse_source_snapshot(
        "templates/deployment.yaml",
        br#"{{ include .Template.Name . }} docs: "old.tpl"
{{ include "current.tpl" . }}"#,
    );
    assert!(
        !dynamic_go_template
            .imports
            .iter()
            .any(|import| import.module == "old.tpl")
            && dynamic_go_template
                .imports
                .iter()
                .any(|import| import.module == "current.tpl"),
        "Go-template imports must stop scanning at the closing action delimiter: {:?}",
        dynamic_go_template.imports
    );
    let control_only_go_template = parse_source_snapshot(
        "templates/NOTES.txt",
        br#"{{- if .Values.enabled -}}ready{{- end -}}"#,
    );
    assert_eq!(
        control_only_go_template.files[0].parse_status,
        CodeParseStatus::Parsed,
        "balanced Go-template control actions should not require extracted facts"
    );
    let go_template_block = parse_source_snapshot(
        "templates/layout.tpl",
        br#"{{ block "content" . }}fallback{{ end }}"#,
    );
    assert!(
        go_template_block
            .symbols
            .iter()
            .any(|symbol| symbol.name == "content" && symbol.kind == "template")
            && go_template_block
                .references
                .iter()
                .any(|reference| reference.name == "content" && reference.kind == "template")
            && go_template_block
                .imports
                .iter()
                .any(|import| import.module == "content"),
        "Go-template block actions should define and execute the named template: {:?} {:?} {:?}",
        go_template_block.symbols,
        go_template_block.references,
        go_template_block.imports
    );
    let templated_yaml = parse_source_snapshot(
        "templates/deployment.yaml",
        br#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ .Values.name }}
{{- if .Values.enabled }}
spec:
  replicas: 1
{{- end }}
{{ include "app.labels" . }}
"#,
    );
    assert_eq!(
        templated_yaml.files[0].parse_status,
        CodeParseStatus::Parsed,
        "valid Go-template manifests should not be degraded by the Jinja grammar fallback"
    );
    assert!(
        ["apiVersion", "kind", "metadata", "name", "spec", "replicas"]
            .iter()
            .all(|key| templated_yaml
                .symbols
                .iter()
                .any(|symbol| { symbol.name == *key && symbol.kind == "config_key" })),
        "templated YAML manifests should still expose static config keys: {:?}",
        templated_yaml.symbols
    );
    assert!(
        templated_yaml
            .references
            .iter()
            .any(|reference| reference.name == "app.labels" && reference.kind == "template"),
        "templated YAML manifests should still expose helper references: {:?}",
        templated_yaml.references
    );

    let helm_notes = parse_source_snapshot(
        "charts/app/templates/NOTES.txt",
        br#"{{ include "app.labels" . }}"#,
    );
    assert_eq!(helm_notes.files[0].language_id, "gotemplate");
    assert!(
        helm_notes
            .imports
            .iter()
            .any(|import| import.module == "app.labels"),
        "Helm NOTES.txt should be indexed as a Go template: {:?}",
        helm_notes.imports
    );

    let helm_template_scope = parse_sources_snapshot(&[
        (
            "charts/app/templates/deployment.yaml",
            br#"{{ include "app.labels" . }}"# as &[u8],
        ),
        (
            "charts/app/templates/_helpers.tpl",
            br#"{{ define "app.labels" }}{{ end }}"# as &[u8],
        ),
        (
            "charts/other/templates/_helpers.tpl",
            br#"{{ define "app.labels" }}{{ end }}"# as &[u8],
        ),
    ]);
    let app_helper = helm_template_scope
        .symbols
        .iter()
        .find(|symbol| {
            symbol.path == "charts/app/templates/_helpers.tpl" && symbol.name == "app.labels"
        })
        .expect("same-chart helper should be indexed");
    let app_template_reference = helm_template_scope
        .references
        .iter()
        .find(|reference| {
            reference.path == "charts/app/templates/deployment.yaml"
                && reference.name == "app.labels"
                && reference.kind == "template"
        })
        .expect("same-chart helper reference should be indexed");
    assert_eq!(
        app_template_reference.target_symbol_snapshot_id.as_deref(),
        Some(app_helper.symbol_snapshot_id.as_str()),
        "Helm template references should resolve inside the current chart before other charts: {:?}",
        helm_template_scope.references
    );

    let missing_local_template = parse_sources_snapshot(&[
        (
            "charts/app/templates/deployment.yaml",
            br#"{{ include "app.labels" . }}"# as &[u8],
        ),
        (
            "charts/other/templates/_helpers.tpl",
            br#"{{ define "app.labels" }}{{ end }}"# as &[u8],
        ),
    ]);
    let missing_local_reference = missing_local_template
        .references
        .iter()
        .find(|reference| {
            reference.path == "charts/app/templates/deployment.yaml"
                && reference.name == "app.labels"
                && reference.kind == "template"
        })
        .expect("cross-chart helper reference should be indexed");
    assert_eq!(
        missing_local_reference.target_symbol_snapshot_id, None,
        "Helm template references must not resolve across chart template roots: {:?}",
        missing_local_template.references
    );
    assert_eq!(missing_local_reference.resolution_state, "unresolved");
}

fn parse_source_snapshot(path: &str, source: &[u8]) -> CodeIndexSnapshot {
    parse_sources_snapshot(&[(path, source)])
}

fn parse_sources_snapshot(files: &[(&str, &[u8])]) -> CodeIndexSnapshot {
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

    for (path, source) in files {
        parse_indexed_file(&mut build, path, source).expect("file should parse");
    }

    build.finish()
}
