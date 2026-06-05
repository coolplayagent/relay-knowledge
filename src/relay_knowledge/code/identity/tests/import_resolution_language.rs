use crate::{
    code::{SnapshotBuild, parse_indexed_file},
    domain::{CodeImportRecord, CodeIndexSnapshot, CodeRepositoryRegistration},
};

#[test]
fn javascript_import_resolution_handles_local_modules_and_external_packages() {
    let snapshot = parse_sources(&[
        ("src/utils/index.js", "export function buildRequest() {}\n"),
        ("src/sleep.js", "export function sleep() {}\n"),
        (
            "src/app.js",
            r#"
import { sleep } from "./sleep.js";
import { buildRequest } from "./utils";
import { Session } from "requests";

export function run() {
  sleep();
  buildRequest();
}
"#,
        ),
    ]);

    assert_import_state(&snapshot, "./sleep.js", "resolved");
    assert_import_state(&snapshot, "./utils", "resolved");
    assert_import_state(&snapshot, "requests", "unresolved");
}

#[test]
fn kotlin_import_resolution_handles_local_classes_and_wildcards() {
    let snapshot = parse_sources(&[
        (
            "src/main/kotlin/app/RetryPolicy.kt",
            r#"
package app

class RetryPolicy
"#,
        ),
        (
            "src/main/kotlin/worker/Worker.kt",
            r#"
package worker

import app.RetryPolicy
import app.*
import kotlin.time.Duration

fun run(): RetryPolicy = RetryPolicy()
"#,
        ),
    ]);

    assert_import_state(&snapshot, "app.RetryPolicy", "resolved");
    assert_import_state(&snapshot, "app.*", "resolved");
    assert_import_state(&snapshot, "kotlin.time.Duration", "unresolved");
}

#[test]
fn kotlin_import_resolution_strips_aliases() {
    let snapshot = parse_sources(&[
        (
            "src/main/kotlin/app/RetryPolicy.kt",
            r#"
package app

class RetryPolicy
"#,
        ),
        (
            "src/main/kotlin/worker/Worker.kt",
            r#"
package worker

import app.RetryPolicy as Retry

fun run(): Retry = Retry()
"#,
        ),
    ]);

    assert_import_state(&snapshot, "app.RetryPolicy as Retry", "resolved");
}

#[test]
fn kotlin_import_resolution_requires_symbols_inside_matching_files() {
    let snapshot = parse_sources(&[
        (
            "src/main/kotlin/app/Missing.kt",
            r#"
package app

class RetryPolicy
"#,
        ),
        (
            "src/main/kotlin/worker/Worker.kt",
            r#"
package worker

import app.Missing
"#,
        ),
    ]);

    assert_import_state(&snapshot, "app.Missing", "unresolved");
}

#[test]
fn kotlin_import_resolution_finds_top_level_symbols_in_package_files() {
    let snapshot = parse_sources(&[
        (
            "src/main/kotlin/app/Utils.kt",
            r#"
package app

fun buildRequest(): String = "ok"
"#,
        ),
        (
            "src/main/kotlin/worker/Worker.kt",
            r#"
package worker

import app.buildRequest

fun run(): String = buildRequest()
"#,
        ),
    ]);

    assert_import_state(&snapshot, "app.buildRequest", "resolved");
}

#[test]
fn kotlin_import_resolution_does_not_match_top_level_symbols_in_subpackages() {
    let snapshot = parse_sources(&[
        (
            "src/main/kotlin/app/internal/Utils.kt",
            r#"
package app.internal

fun buildRequest(): String = "ok"
"#,
        ),
        (
            "src/main/kotlin/worker/Worker.kt",
            r#"
package worker

import app.buildRequest
"#,
        ),
    ]);

    assert_import_state(&snapshot, "app.buildRequest", "unresolved");
}

#[test]
fn scala_import_resolution_handles_local_classes_and_wildcards() {
    let snapshot = parse_sources(&[
        (
            "src/main/scala/app/RetryPolicy.scala",
            r#"
package app

class RetryPolicy
"#,
        ),
        (
            "src/main/scala/worker/Worker.scala",
            r#"
package worker

import app.RetryPolicy
import app._
import scala.concurrent.Future

object Worker {
  def run(): RetryPolicy = new RetryPolicy()
}
"#,
        ),
    ]);

    assert_import_state(&snapshot, "app.RetryPolicy", "resolved");
    assert_import_state(&snapshot, "app._", "resolved");
    assert_import_state(&snapshot, "scala.concurrent.Future", "unresolved");
}

#[test]
fn scala_import_resolution_expands_selectors() {
    let snapshot = parse_sources(&[
        (
            "src/main/scala/app/RetryPolicy.scala",
            "package app\nclass RetryPolicy\n",
        ),
        (
            "src/main/scala/app/Backoff.scala",
            "package app\nclass Backoff\n",
        ),
        (
            "src/main/scala/worker/Worker.scala",
            r#"
package worker

import app.{RetryPolicy, Backoff}

object Worker {
  def run(): RetryPolicy = new RetryPolicy()
}
"#,
        ),
    ]);

    assert_import_state(&snapshot, "app.{RetryPolicy, Backoff}", "resolved");
}

#[test]
fn scala_import_resolution_handles_object_member_imports() {
    let snapshot = parse_sources(&[
        (
            "src/main/scala/app/Helpers.scala",
            r#"
package app

object Helpers {
  def buildRequest(): String = "ok"
}
"#,
        ),
        (
            "src/main/scala/worker/Worker.scala",
            r#"
package worker

import app.Helpers.buildRequest
"#,
        ),
    ]);

    assert_import_state(&snapshot, "app.Helpers.buildRequest", "resolved");
}

#[test]
fn csharp_import_resolution_handles_local_namespaces_and_external_namespaces() {
    let snapshot = parse_sources(&[
        (
            "src/App/RetryPolicy.cs",
            r#"
namespace App {
    public class RetryPolicy {}
}
"#,
        ),
        (
            "src/Worker.cs",
            r#"
using App;
using static App.RetryPolicy;
using System;

class Worker {
    RetryPolicy Run() {
        return new RetryPolicy();
    }
}
"#,
        ),
    ]);

    assert_import_state(&snapshot, "using App;", "resolved");
    assert_import_state(&snapshot, "using static App.RetryPolicy;", "resolved");
    assert_import_state(&snapshot, "using System;", "unresolved");
}

#[test]
fn csharp_import_resolution_handles_aliases_and_nested_namespaces() {
    let snapshot = parse_sources(&[
        (
            "src/App/Services/Client.cs",
            r#"
namespace App.Services {
    public class Client {}
}
"#,
        ),
        (
            "src/Worker.cs",
            r#"
using App.Services;
using ClientAlias = App.Services.Client;

class Worker {
    Client Run() {
        return new ClientAlias();
    }
}
"#,
        ),
    ]);

    assert_import_state(&snapshot, "using App.Services;", "resolved");
    assert_import_state(
        &snapshot,
        "using ClientAlias = App.Services.Client;",
        "resolved",
    );
}

#[test]
fn csharp_import_resolution_keeps_ordinary_dotted_using_namespace_only() {
    let snapshot = parse_sources(&[
        (
            "src/App/RetryPolicy.cs",
            r#"
namespace App {
    public class RetryPolicy {}
}
"#,
        ),
        (
            "src/Worker.cs",
            r#"
using App.RetryPolicy;
"#,
        ),
    ]);

    assert_import_state(&snapshot, "using App.RetryPolicy;", "unresolved");
}

#[test]
fn csharp_alias_resolution_ignores_non_csharp_symbols() {
    let snapshot = parse_sources(&[
        (
            "src/App/Client.php",
            r#"
<?php
namespace App;

class Client {}
"#,
        ),
        (
            "src/Worker.cs",
            r#"
using ClientAlias = App.Client;
"#,
        ),
    ]);

    assert_import_state(&snapshot, "using ClientAlias = App.Client;", "unresolved");
}

#[test]
fn csharp_static_using_requires_a_type_not_namespace_directory() {
    let snapshot = parse_sources(&[
        (
            "src/App/Helpers/Client.cs",
            r#"
namespace App.Helpers;

public class Client {}
"#,
        ),
        (
            "src/Worker.cs",
            r#"
using static App.Helpers;
"#,
        ),
    ]);

    assert_import_state(&snapshot, "using static App.Helpers;", "unresolved");
}

#[test]
fn csharp_import_resolution_requires_indexed_namespace_symbols() {
    let snapshot = parse_sources(&[
        (
            "src/App/Client.cs",
            r#"
namespace Other;

public class Client {}
"#,
        ),
        (
            "src/Worker.cs",
            r#"
using App;
"#,
        ),
    ]);

    assert_import_state(&snapshot, "using App;", "unresolved");
}

#[test]
fn csharp_import_resolution_handles_global_using_directives() {
    let snapshot = parse_sources(&[
        (
            "src/App/RetryPolicy.cs",
            r#"
namespace App {
    public class RetryPolicy {}
}
"#,
        ),
        (
            "src/Worker.cs",
            r#"
global using App;
global using static App.RetryPolicy;
"#,
        ),
    ]);

    assert_import_state(&snapshot, "global using App;", "resolved");
    assert_import_state(
        &snapshot,
        "global using static App.RetryPolicy;",
        "resolved",
    );
}

#[test]
fn csharp_static_using_requires_type_symbols() {
    let snapshot = parse_sources(&[
        (
            "src/App/Helpers.cs",
            r#"
namespace App;

public class Other {
    public static void Helpers() {}
}
"#,
        ),
        (
            "src/Worker.cs",
            r#"
using static App.Helpers;
"#,
        ),
    ]);

    assert_import_state(&snapshot, "using static App.Helpers;", "unresolved");
}

#[test]
fn csharp_alias_using_requires_type_or_namespace_targets() {
    let snapshot = parse_sources(&[
        (
            "src/App/Helpers.cs",
            r#"
namespace App;

public class Other {
    public void Helpers() {}
}
"#,
        ),
        (
            "src/Worker.cs",
            r#"
using Alias = App.Helpers;
"#,
        ),
    ]);

    assert_import_state(&snapshot, "using Alias = App.Helpers;", "unresolved");
}

#[test]
fn php_import_resolution_handles_local_namespace_uses_and_external_packages() {
    let snapshot = parse_sources(&[
        (
            "src/App/SessionClient.php",
            r#"
<?php
namespace App;

class SessionClient {}
"#,
        ),
        (
            "src/Worker.php",
            r#"
<?php
use App\SessionClient;
use Vendor\Requests\Session;

function build(): SessionClient {
    return new SessionClient();
}
"#,
        ),
    ]);

    assert_import_state(&snapshot, r"use App\SessionClient;", "resolved");
    assert_import_state(&snapshot, r"use Vendor\Requests\Session;", "unresolved");
}

#[test]
fn php_import_resolution_expands_grouped_use_declarations() {
    let snapshot = parse_sources(&[
        (
            "src/App/SessionClient.php",
            "<?php\nnamespace App;\nclass SessionClient {}\n",
        ),
        (
            "src/App/RetryPolicy.php",
            "<?php\nnamespace App;\nclass RetryPolicy {}\n",
        ),
        (
            "src/Worker.php",
            r#"
<?php
use App\{SessionClient, RetryPolicy};

function build(): SessionClient {
    return new SessionClient();
}
"#,
        ),
    ]);

    assert_import_state(
        &snapshot,
        r"use App\{SessionClient, RetryPolicy};",
        "resolved",
    );
}

#[test]
fn php_import_resolution_handles_grouped_aliases() {
    let snapshot = parse_sources(&[
        (
            "src/App/SessionClient.php",
            "<?php\nnamespace App;\nclass SessionClient {}\n",
        ),
        (
            "src/App/RetryPolicy.php",
            "<?php\nnamespace App;\nclass RetryPolicy {}\n",
        ),
        (
            "src/Worker.php",
            r#"
<?php
use App\{SessionClient as Client, RetryPolicy};

function build(): Client {
    return new Client();
}
"#,
        ),
    ]);

    assert_import_state(
        &snapshot,
        r"use App\{SessionClient as Client, RetryPolicy};",
        "resolved",
    );
}

#[test]
fn php_import_resolution_handles_leading_separators_and_multiple_clauses() {
    let snapshot = parse_sources(&[
        (
            "src/App/SessionClient.php",
            "<?php\nnamespace App;\nclass SessionClient {}\n",
        ),
        (
            "src/App/RetryPolicy.php",
            "<?php\nnamespace App;\nclass RetryPolicy {}\n",
        ),
        (
            "src/Worker.php",
            r#"
<?php
use \App\SessionClient, App\RetryPolicy;

function build(): SessionClient {
    return new SessionClient();
}
"#,
        ),
    ]);

    assert_import_state(
        &snapshot,
        r"use \App\SessionClient, App\RetryPolicy;",
        "resolved",
    );
}

#[test]
fn php_import_resolution_filters_namespace_fallback_by_language() {
    let snapshot = parse_sources(&[
        (
            "src/App/SessionClient.cs",
            r#"
namespace App;

public class SessionClient {}
"#,
        ),
        (
            "src/Worker.php",
            r#"
<?php
use App\SessionClient;
"#,
        ),
    ]);

    assert_import_state(&snapshot, r"use App\SessionClient;", "unresolved");
}

#[test]
fn php_import_resolution_preserves_function_import_kind() {
    let snapshot = parse_sources(&[
        (
            "src/App/SessionClient.php",
            "<?php\nnamespace App;\nclass SessionClient {}\n",
        ),
        (
            "src/App/build_session.php",
            "<?php\nnamespace App;\nfunction build_session() { return null; }\n",
        ),
        (
            "src/Worker.php",
            r#"
<?php
use function App\SessionClient;
use function App\build_session;
"#,
        ),
    ]);

    assert_import_state(&snapshot, r"use function App\SessionClient;", "unresolved");
    assert_import_state(&snapshot, r"use function App\build_session;", "resolved");
}

#[test]
fn php_import_resolution_finds_functions_in_namespace_helper_files() {
    let snapshot = parse_sources(&[
        (
            "src/App/helpers.php",
            "<?php\nnamespace App;\nfunction build_session() { return null; }\n",
        ),
        (
            "src/Worker.php",
            r#"
<?php
use function App\build_session;
"#,
        ),
    ]);

    assert_import_state(&snapshot, r"use function App\build_session;", "resolved");
}

#[test]
fn php_import_resolution_handles_global_namespace_uses() {
    let snapshot = parse_sources(&[
        ("src/SessionClient.php", "<?php\nclass SessionClient {}\n"),
        (
            "src/Worker.php",
            r#"
<?php
namespace Worker;

use SessionClient;
"#,
        ),
    ]);

    assert_import_state(&snapshot, "use SessionClient;", "resolved");
}

#[test]
fn php_import_resolution_preserves_grouped_item_kinds() {
    let snapshot = parse_sources(&[
        (
            "src/App/SessionClient.php",
            "<?php\nnamespace App;\nclass SessionClient {}\n",
        ),
        (
            "src/App/build_session.php",
            "<?php\nnamespace App;\nfunction build_session() { return null; }\n",
        ),
        (
            "src/Worker.php",
            r#"
<?php
use App\{SessionClient, function build_session};
"#,
        ),
    ]);

    assert_import_state(
        &snapshot,
        r"use App\{SessionClient, function build_session};",
        "resolved",
    );
}

#[test]
fn rust_import_resolution_handles_crate_modules_and_external_crates() {
    let snapshot = parse_sources(&[
        (
            "src/client.rs",
            r#"
pub struct SessionClient;
"#,
        ),
        (
            "src/lib.rs",
            r#"
use crate::client::SessionClient;
use serde::Serialize;

pub fn build() -> SessionClient {
    SessionClient
}
"#,
        ),
    ]);

    assert_import_state(&snapshot, "use crate::client::SessionClient;", "resolved");
    assert_import_state(&snapshot, "use serde::Serialize;", "unresolved");
}

#[test]
fn rust_import_resolution_keeps_missing_symbols_unresolved() {
    let snapshot = parse_sources(&[
        ("src/client.rs", "pub struct SessionClient;\n"),
        (
            "src/lib.rs",
            r#"
use crate::client::Missing;
"#,
        ),
    ]);

    assert_import_state(&snapshot, "use crate::client::Missing;", "unresolved");
}

#[test]
fn rust_import_resolution_handles_module_imports_and_file_module_super() {
    let snapshot = parse_sources(&[
        ("src/client.rs", "pub struct SessionClient;\n"),
        ("src/foo/baz.rs", "pub struct Thing;\n"),
        (
            "src/foo/bar.rs",
            r#"
use super::baz::Thing;
"#,
        ),
        (
            "src/lib.rs",
            r#"
use crate::client;
"#,
        ),
    ]);

    assert_import_state(&snapshot, "use crate::client;", "resolved");
    assert_import_state(&snapshot, "use super::baz::Thing;", "resolved");
}

#[test]
fn rust_import_resolution_handles_re_exports_use_trees_and_globs() {
    let snapshot = parse_sources(&[
        ("src/client.rs", "pub struct SessionClient;\n"),
        ("src/retry.rs", "pub struct RetryPolicy;\n"),
        (
            "src/lib.rs",
            r#"
pub use crate::client::SessionClient;
use crate::{client::SessionClient, retry::RetryPolicy};
use crate::client::*;
"#,
        ),
    ]);

    assert_import_state(
        &snapshot,
        "pub use crate::client::SessionClient;",
        "resolved",
    );
    assert_import_state(
        &snapshot,
        "use crate::{client::SessionClient, retry::RetryPolicy};",
        "resolved",
    );
    assert_import_state(&snapshot, "use crate::client::*;", "resolved");
}

#[test]
fn rust_import_resolution_handles_self_selectors_in_use_trees() {
    let snapshot = parse_sources(&[
        ("src/client.rs", "pub struct SessionClient;\n"),
        (
            "src/lib.rs",
            r#"
use crate::client::{self, SessionClient};
"#,
        ),
    ]);

    assert_import_state(
        &snapshot,
        "use crate::client::{self, SessionClient};",
        "resolved",
    );
}

#[test]
fn rust_import_resolution_strips_aliases_inside_use_trees() {
    let snapshot = parse_sources(&[
        (
            "src/client.rs",
            r#"
pub struct SessionClient;
pub struct RetryPolicy;
"#,
        ),
        (
            "src/lib.rs",
            r#"
use crate::client::{SessionClient as Client, RetryPolicy};
"#,
        ),
    ]);

    assert_import_state(
        &snapshot,
        "use crate::client::{SessionClient as Client, RetryPolicy};",
        "resolved",
    );
}

#[test]
fn rust_import_resolution_expands_nested_use_trees() {
    let snapshot = parse_sources(&[
        (
            "src/client/transport.rs",
            r#"
pub struct HttpClient;
pub struct RetryPolicy;
"#,
        ),
        (
            "src/lib.rs",
            r#"
use crate::client::{transport::{HttpClient, RetryPolicy}};
"#,
        ),
    ]);

    assert_import_state(
        &snapshot,
        "use crate::client::{transport::{HttpClient, RetryPolicy}};",
        "resolved",
    );
}

#[test]
fn swift_import_resolution_handles_local_modules_and_external_modules() {
    let snapshot = parse_sources(&[
        (
            "Sources/Networking/Client.swift",
            r#"
public struct Client {}
"#,
        ),
        (
            "Sources/App/App.swift",
            r#"
import Networking
import Foundation

func run() -> Client {
    Client()
}
"#,
        ),
    ]);

    assert_import_state(&snapshot, "Networking", "resolved");
    assert_import_state(&snapshot, "Foundation", "unresolved");
}

#[test]
fn swift_import_resolution_handles_qualified_and_nested_module_imports() {
    let snapshot = parse_sources(&[
        (
            "Sources/Networking/HTTP/Client.swift",
            r#"
public struct Client {}
"#,
        ),
        (
            "Sources/App/App.swift",
            r#"
import struct Networking.Client

func run() -> Client {
    Client()
}
"#,
        ),
    ]);

    assert_import_state(&snapshot, "Networking.Client", "resolved");
}

#[test]
fn swift_import_resolution_validates_qualified_and_attributed_imports() {
    let snapshot = parse_sources(&[
        (
            "Sources/Networking/HTTP/Client.swift",
            r#"
public struct Client {}
"#,
        ),
        (
            "Sources/App/App.swift",
            r#"
@testable import Networking
import struct Networking.Missing
import func Networking.Client
"#,
        ),
    ]);

    assert_import_state(&snapshot, "Networking", "resolved");
    assert_import_state(&snapshot, "Networking.Missing", "unresolved");
    assert_import_state(&snapshot, "Networking.Client", "unresolved");
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
