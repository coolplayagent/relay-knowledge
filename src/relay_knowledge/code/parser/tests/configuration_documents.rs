use crate::domain::{CodeParseStatus, CodeRepositoryRegistration};

use super::*;

#[test]
fn conf_files_reuse_ini_tree_sitter_indexing() {
    let snapshot = parse_source_snapshot(
        "config/service.conf",
        b"[server]\nenabled=true\nport=8080\n[server.tls]\ncert=server.pem\n",
    );

    assert_eq!(snapshot.files[0].language_id, "ini");
    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert_symbol(&snapshot, "server", "section");
    assert_symbol(&snapshot, "server.enabled", "config");
    assert_symbol(&snapshot, "server.port", "config");
    assert_symbol(&snapshot, "server.tls.cert", "config");
    assert!(
        snapshot
            .feature_flags
            .iter()
            .any(|record| record.source_key == "enabled" && record.edge_kind == "defines_config"),
        ".conf boolean keys should feed feature-flag definitions: {:?}",
        snapshot.feature_flags
    );
}

#[test]
fn markdown_tree_sitter_extracts_headings_and_local_links() {
    let snapshot = parse_source_snapshot(
        "README.md",
        br#"# Runtime Guide

Install Notes
=============

# C#
# #breaking
Install
Notes
=====

[local](docs/install.md#setup) ![diagram](assets/diagram.png)
[![nested](assets/thumb.png)](docs/details.md)
[query](docs/query.md?plain=1#L4) ![raw](assets/raw.png?raw=1)
[external](https://example.com) [mail](mailto:ops@example.com) [anchor](#usage)
[bucket](s3://example-bucket/key) [r2](r2://bucket/key)

[ref]: docs/reference.md "Reference"

```md
# Disabled
[disabled](docs/disabled.md)
```
"#,
    );

    assert_eq!(snapshot.files[0].language_id, "markdown");
    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert_symbol(&snapshot, "Runtime Guide", "heading");
    assert_symbol(&snapshot, "Install Notes", "heading");
    assert_symbol(&snapshot, "C#", "heading");
    assert_symbol(&snapshot, "#breaking", "heading");
    assert_no_symbol(&snapshot, "Disabled", "heading");
    assert_import(&snapshot, "docs/install.md");
    assert_import(&snapshot, "assets/diagram.png");
    assert_import(&snapshot, "assets/thumb.png");
    assert_import(&snapshot, "docs/details.md");
    assert_import(&snapshot, "docs/query.md");
    assert_import(&snapshot, "assets/raw.png");
    assert_import(&snapshot, "docs/reference.md");
    assert_no_import(&snapshot, "docs/query.md?plain=1");
    assert_no_import(&snapshot, "assets/raw.png?raw=1");
    assert_no_import(&snapshot, "https://example.com");
    assert_no_import(&snapshot, "mailto:ops@example.com");
    assert_no_import(&snapshot, "s3://example-bucket/key");
    assert_no_import(&snapshot, "r2://bucket/key");
    assert_no_import(&snapshot, "#usage");
    assert_no_import(&snapshot, "docs/disabled.md");
}

#[test]
fn markdown_local_imports_resolve_to_indexed_files() {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        5,
        0,
    );

    parse_indexed_file(
        &mut build,
        "README.md",
        br#"[reference](docs/reference.md#install)
[query](docs/query.md?plain=1#L4)
[escaped](docs/My%20Guide.md)
[punctuation](docs/API\).md)"#,
    )
    .expect("markdown importer should parse");
    parse_indexed_file(&mut build, "docs/reference.md", b"# Install\n")
        .expect("markdown target should parse");
    parse_indexed_file(&mut build, "docs/query.md", b"# Query\n")
        .expect("query target should parse");
    parse_indexed_file(&mut build, "docs/My Guide.md", b"# Escaped\n")
        .expect("escaped target should parse");
    parse_indexed_file(&mut build, "docs/API).md", b"# API\n")
        .expect("punctuation target should parse");

    let snapshot = build.finish();
    let import = snapshot
        .imports
        .iter()
        .find(|import| import.module == "docs/reference.md")
        .expect("Markdown import should be indexed");

    assert_eq!(import.resolution_state, "resolved", "{import:?}");
    assert_eq!(import.target_hint.as_deref(), Some("docs/reference.md"));

    let query_import = snapshot
        .imports
        .iter()
        .find(|import| import.module == "docs/query.md")
        .expect("Markdown import should strip query parameters");
    assert_eq!(
        query_import.resolution_state, "resolved",
        "{query_import:?}"
    );
    assert_eq!(query_import.target_hint.as_deref(), Some("docs/query.md"));

    let escaped_import = snapshot
        .imports
        .iter()
        .find(|import| import.module == "docs/My Guide.md")
        .expect("Markdown import should decode percent-escaped path bytes");
    assert_eq!(
        escaped_import.resolution_state, "resolved",
        "{escaped_import:?}"
    );
    assert_eq!(
        escaped_import.target_hint.as_deref(),
        Some("docs/My Guide.md")
    );

    let punctuation_import = snapshot
        .imports
        .iter()
        .find(|import| import.module == "docs/API).md")
        .expect("Markdown import should decode backslash-escaped punctuation");
    assert_eq!(
        punctuation_import.resolution_state, "resolved",
        "{punctuation_import:?}"
    );
    assert_eq!(
        punctuation_import.target_hint.as_deref(),
        Some("docs/API).md")
    );
}

#[test]
fn markdown_root_relative_imports_resolve_from_repository_root() {
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

    parse_indexed_file(
        &mut build,
        "docs/README.md",
        br#"[guide](/guide.md#install)"#,
    )
    .expect("markdown importer should parse");
    parse_indexed_file(&mut build, "guide.md", b"# Install\n")
        .expect("markdown target should parse");

    let snapshot = build.finish();
    let import = snapshot
        .imports
        .iter()
        .find(|import| import.module == "/guide.md")
        .expect("root-relative Markdown import should be indexed");

    assert_eq!(import.resolution_state, "resolved", "{import:?}");
    assert_eq!(import.target_hint.as_deref(), Some("guide.md"));
}

#[test]
fn markdown_directory_imports_resolve_to_index_documents() {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        4,
        0,
    );

    parse_indexed_file(
        &mut build,
        "README.md",
        br#"[docs](docs/) [api](api) [manuals](manuals/)"#,
    )
    .expect("markdown importer should parse");
    parse_indexed_file(&mut build, "docs/README.md", b"# Docs\n")
        .expect("docs README should parse");
    parse_indexed_file(&mut build, "api/index.md", b"# API\n").expect("api index should parse");
    parse_indexed_file(&mut build, "manuals/README.markdown", b"# Manuals\n")
        .expect("manuals README should parse");

    let snapshot = build.finish();
    let docs_import = snapshot
        .imports
        .iter()
        .find(|import| import.module == "docs/")
        .expect("directory Markdown import should be indexed");
    assert_eq!(docs_import.resolution_state, "resolved", "{docs_import:?}");
    assert_eq!(docs_import.target_hint.as_deref(), Some("docs/README.md"));

    let api_import = snapshot
        .imports
        .iter()
        .find(|import| import.module == "api")
        .expect("extensionless directory Markdown import should be indexed");
    assert_eq!(api_import.resolution_state, "resolved", "{api_import:?}");
    assert_eq!(api_import.target_hint.as_deref(), Some("api/index.md"));

    let manuals_import = snapshot
        .imports
        .iter()
        .find(|import| import.module == "manuals/")
        .expect(".markdown directory import should be indexed");
    assert_eq!(
        manuals_import.resolution_state, "resolved",
        "{manuals_import:?}"
    );
    assert_eq!(
        manuals_import.target_hint.as_deref(),
        Some("manuals/README.markdown")
    );
}

#[test]
fn markdown_root_directory_imports_resolve_to_root_index_documents() {
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

    parse_indexed_file(&mut build, "docs/guide.md", br#"[home](..) [root](/)"#)
        .expect("markdown importer should parse");
    parse_indexed_file(&mut build, "README.md", b"# Home\n").expect("root README should parse");

    let snapshot = build.finish();
    let parent_import = snapshot
        .imports
        .iter()
        .find(|import| import.module == "..")
        .expect("parent directory Markdown import should be indexed");
    assert_eq!(
        parent_import.resolution_state, "resolved",
        "{parent_import:?}"
    );
    assert_eq!(parent_import.target_hint.as_deref(), Some("README.md"));

    let root_import = snapshot
        .imports
        .iter()
        .find(|import| import.module == "/")
        .expect("root directory Markdown import should be indexed");
    assert_eq!(root_import.resolution_state, "resolved", "{root_import:?}");
    assert_eq!(root_import.target_hint.as_deref(), Some("README.md"));
}

#[test]
fn markdown_relative_imports_do_not_fall_back_to_repository_root() {
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

    parse_indexed_file(&mut build, "docs/README.md", br#"[guide](guide.md)"#)
        .expect("markdown importer should parse");
    parse_indexed_file(&mut build, "guide.md", b"# Root Guide\n").expect("root guide should parse");

    let snapshot = build.finish();
    let import = snapshot
        .imports
        .iter()
        .find(|import| import.module == "guide.md")
        .expect("relative Markdown import should be indexed");

    assert_eq!(import.resolution_state, "unresolved", "{import:?}");
    assert_eq!(import.target_hint.as_deref(), Some("guide.md"));
}

#[test]
fn json_tree_sitter_paths_survive_arrays_and_partial_input() {
    let snapshot = parse_source_snapshot(
        "config/runtime.json",
        br#"{"server":{"port":8080},"containers":[{"name":"app"}],"matrix":[[{"name":"nested"}]]}"#,
    );

    assert_eq!(snapshot.files[0].language_id, "json");
    assert_symbol(&snapshot, "server.port", "config");
    assert_symbol(&snapshot, "containers[].name", "config");
    assert_symbol(&snapshot, "matrix[][].name", "config");
    assert_no_symbol(&snapshot, "containers.name", "config");

    let partial = parse_source_snapshot("config/partial.json", br#"{"enabled": true"#);
    assert_eq!(partial.files[0].parse_status, CodeParseStatus::Partial);
    assert_symbol(&partial, "enabled", "config");
    assert!(
        partial.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("tree-sitter produced error nodes")
        }),
        "partial JSON should keep parse diagnostics: {:?}",
        partial.diagnostics
    );
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

fn assert_symbol(snapshot: &crate::domain::CodeIndexSnapshot, name: &str, kind: &str) {
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == name && symbol.kind == kind),
        "{kind} symbol {name} should be indexed: {:?}",
        snapshot.symbols
    );
}

fn assert_no_symbol(snapshot: &crate::domain::CodeIndexSnapshot, name: &str, kind: &str) {
    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == name && symbol.kind == kind),
        "{kind} symbol {name} should not be indexed: {:?}",
        snapshot.symbols
    );
}

fn assert_import(snapshot: &crate::domain::CodeIndexSnapshot, module: &str) {
    assert!(
        snapshot
            .imports
            .iter()
            .any(|import| import.module == module),
        "import {module} should be indexed: {:?}",
        snapshot.imports
    );
}

fn assert_no_import(snapshot: &crate::domain::CodeIndexSnapshot, module: &str) {
    assert!(
        !snapshot
            .imports
            .iter()
            .any(|import| import.module == module),
        "import {module} should not be indexed: {:?}",
        snapshot.imports
    );
}
