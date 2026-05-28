use crate::{
    code::{SnapshotBuild, parse_indexed_file},
    domain::{CodeImportRecord, CodeIndexSnapshot, CodeRepositoryRegistration},
};

#[test]
fn rust_import_resolution_handles_aliased_self_selectors() {
    let snapshot = parse_sources(&[
        ("src/client.rs", "pub struct SessionClient;\n"),
        (
            "src/lib.rs",
            r#"
use crate::client::{self as client_mod, SessionClient};
"#,
        ),
    ]);

    assert_import_state(
        &snapshot,
        "use crate::client::{self as client_mod, SessionClient};",
        "resolved",
    );
}

#[test]
fn csharp_import_resolution_strips_global_alias_qualifiers() {
    let snapshot = parse_sources(&[
        (
            "src/App/Client.cs",
            r#"
namespace App;

public class Client {}
"#,
        ),
        (
            "src/Worker.cs",
            r#"
global using ClientAlias = global::App.Client;
using static global::App.Client;
"#,
        ),
    ]);

    assert_import_state(
        &snapshot,
        "global using ClientAlias = global::App.Client;",
        "resolved",
    );
    assert_import_state(&snapshot, "using static global::App.Client;", "resolved");
}

#[test]
fn csharp_import_resolution_handles_unqualified_alias_targets() {
    let snapshot = parse_sources(&[
        ("src/Client.cs", "public class Client {}\n"),
        (
            "src/Worker.cs",
            r#"
using ClientAlias = Client;
using static Client;
"#,
        ),
    ]);

    assert_import_state(&snapshot, "using ClientAlias = Client;", "resolved");
    assert_import_state(&snapshot, "using static Client;", "resolved");
}

#[test]
fn swift_import_resolution_reports_duplicate_modules_as_ambiguous() {
    let snapshot = parse_sources(&[
        (
            "Sources/Networking/Client.swift",
            "public struct Client {}\n",
        ),
        (
            "plugins/Networking/PluginClient.swift",
            "public struct PluginClient {}\n",
        ),
        ("Sources/App/App.swift", "import Networking\n"),
    ]);

    assert_import_state(&snapshot, "Networking", "ambiguous");
}

#[test]
fn javascript_import_resolution_rejects_ts_only_modules() {
    let snapshot = parse_sources(&[
        ("src/client.ts", "export class Client {}\n"),
        (
            "src/app.js",
            r#"
import { Client } from "./client";
"#,
        ),
    ]);

    assert_import_state(&snapshot, "./client", "unresolved");
}

#[test]
fn javascript_import_resolution_preserves_explicit_extensions() {
    let snapshot = parse_sources(&[
        ("src/client.jsx", "export class Client {}\n"),
        ("src/app.js", "import { Client } from \"./client.js\";\n"),
    ]);

    assert_import_state(&snapshot, "./client.js", "unresolved");
}

#[test]
fn typescript_import_resolution_treats_default_re_exports_as_module_edges() {
    let snapshot = parse_sources(&[
        ("src/client.ts", "export default class Client {}\n"),
        (
            "src/index.ts",
            "export { default as Client } from \"./client\";\n",
        ),
    ]);

    assert_import_state(&snapshot, "./client", "resolved");
}

#[test]
fn php_import_resolution_splits_nested_grouped_use_names() {
    let snapshot = parse_sources(&[
        (
            "src/App/Services/SessionClient.php",
            "<?php\nnamespace App\\Services;\nclass SessionClient {}\n",
        ),
        (
            "src/Worker.php",
            r#"
<?php
use App\{Services\SessionClient};
"#,
        ),
    ]);

    assert_import_state(&snapshot, r"use App\{Services\SessionClient};", "resolved");
}

#[test]
fn php_import_resolution_strips_uppercase_alias_keywords() {
    let snapshot = parse_sources(&[
        (
            "src/App/SessionClient.php",
            "<?php\nnamespace App;\nclass SessionClient {}\n",
        ),
        (
            "src/Worker.php",
            r#"
<?php
use App\SessionClient AS Client;
"#,
        ),
    ]);

    assert_import_state(&snapshot, r"use App\SessionClient AS Client;", "resolved");
}

#[test]
fn php_import_resolution_parses_uppercase_use_kinds() {
    let snapshot = parse_sources(&[
        (
            "src/App/helpers.php",
            "<?php\nnamespace App;\nfunction build_session() { return null; }\n",
        ),
        (
            "src/Worker.php",
            r#"
<?php
use FUNCTION App\build_session;
"#,
        ),
    ]);

    assert_import_state(&snapshot, r"use FUNCTION App\build_session;", "resolved");
}

#[test]
fn kotlin_wildcard_import_requires_declared_package() {
    let snapshot = parse_sources(&[
        (
            "src/main/kotlin/app/RetryPolicy.kt",
            r#"
package other

class RetryPolicy
"#,
        ),
        (
            "src/main/kotlin/Worker.kt",
            r#"
package worker

import app.*
"#,
        ),
    ]);

    assert_import_state(&snapshot, "app.*", "unresolved");
}

#[test]
fn rust_import_resolution_handles_unprefixed_crate_root_uses() {
    let snapshot = parse_sources(&[
        ("src/client.rs", "pub struct SessionClient;\n"),
        (
            "src/lib.rs",
            r#"
use client::SessionClient;
"#,
        ),
    ]);

    assert_import_state(&snapshot, "use client::SessionClient;", "resolved");
}

#[test]
fn rust_import_resolution_rejects_nested_crate_roots_as_modules() {
    let snapshot = parse_sources(&[
        ("src/foo/lib.rs", "pub struct Bar;\n"),
        ("src/lib.rs", "use crate::foo::Bar;\n"),
    ]);

    assert_import_state(&snapshot, "use crate::foo::Bar;", "unresolved");
}

#[test]
fn scala_import_resolution_splits_multi_imports_and_exclusions() {
    let snapshot = parse_sources(&[
        (
            "src/main/scala/app/Client.scala",
            "package app\nclass Client\n",
        ),
        (
            "src/main/scala/app/RetryPolicy.scala",
            "package app\nclass RetryPolicy\n",
        ),
        (
            "src/main/scala/worker/Worker.scala",
            r#"
package worker

import app.Client, app.RetryPolicy
import app.{Missing => _, Client}
"#,
        ),
    ]);

    assert_import_state(&snapshot, "import app.Client, app.RetryPolicy", "resolved");
    assert_import_state(&snapshot, "import app.{Missing => _, Client}", "resolved");
}

#[test]
fn csharp_import_resolution_matches_struct_alias_targets() {
    let snapshot = parse_sources(&[
        (
            "src/App/Vector.cs",
            r#"
namespace App;

public struct Vector {}
"#,
        ),
        (
            "src/Worker.cs",
            r#"
using Vec = App.Vector;
using static App.Vector;
"#,
        ),
    ]);

    assert_import_state(&snapshot, "using Vec = App.Vector;", "resolved");
    assert_import_state(&snapshot, "using static App.Vector;", "resolved");
}

#[test]
fn php_import_resolution_matches_namespace_symbols_in_helper_files() {
    let snapshot = parse_sources(&[
        (
            "src/App/models.php",
            "<?php\nnamespace App;\nclass Foo {}\n",
        ),
        ("src/Worker.php", "<?php\nuse App\\Foo;\n"),
    ]);

    assert_import_state(&snapshot, r"use App\Foo;", "resolved");
}

fn parse_sources(sources: &[(&str, &str)]) -> CodeIndexSnapshot {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        sources.len(),
        0,
    );
    for (path, source) in sources {
        parse_indexed_file(&mut build, path, source.as_bytes()).expect("file should parse");
    }

    build.finish()
}

fn assert_import_state(snapshot: &CodeIndexSnapshot, fragment: &str, state: &str) {
    let import = import_containing(snapshot, fragment);

    assert_eq!(import.resolution_state, state, "{fragment}");
}

fn import_containing<'a>(snapshot: &'a CodeIndexSnapshot, fragment: &str) -> &'a CodeImportRecord {
    snapshot
        .imports
        .iter()
        .find(|import| import.module.contains(fragment))
        .unwrap_or_else(|| panic!("import containing {fragment} should exist"))
}
