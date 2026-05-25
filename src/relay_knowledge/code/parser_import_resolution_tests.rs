use std::collections::BTreeSet;

use crate::domain::{
    CodeImportRecord, CodeIndexSnapshot, CodeParseStatus, CodeRepositoryRegistration,
};

use super::*;

#[test]
fn java_import_resolution_distinguishes_local_and_external_modules() {
    let snapshot = parse_sources(&[
        (
            "src/app/RetryPolicy.java",
            r#"
package app;

class RetryPolicy {
    static void run() {}
}
"#,
        ),
        (
            "src/app/Worker.java",
            r#"
package app;

import app.RetryPolicy;
import java.time.Duration;

class Worker {
    void run() {
        RetryPolicy.run();
        Duration.ofSeconds(1);
    }
}
"#,
        ),
    ]);

    assert_import_state(&snapshot, "app.RetryPolicy", "resolved");
    assert_import_state(&snapshot, "java.time.Duration", "unresolved");
}

#[test]
fn java_import_resolution_handles_maven_source_roots() {
    let snapshot = parse_sources(&[
        (
            "src/main/java/org/springframework/context/ApplicationContext.java",
            r#"
package org.springframework.context;

public interface ApplicationContext {}
"#,
        ),
        (
            "src/main/java/org/springframework/context/support/ContextLoader.java",
            r#"
package org.springframework.context.support;

import org.springframework.context.ApplicationContext;

class ContextLoader {
    ApplicationContext load() {
        return null;
    }
}
"#,
        ),
    ]);

    assert_import_state(
        &snapshot,
        "org.springframework.context.ApplicationContext",
        "resolved",
    );
}

#[test]
fn java_wildcard_import_resolution_targets_normalized_package_directory() {
    let snapshot = parse_sources(&[
        (
            "src/main/java/org/springframework/context/ApplicationContext.java",
            r#"
package org.springframework.context;

public interface ApplicationContext {}
"#,
        ),
        (
            "src/main/java/org/springframework/context/support/ContextLoader.java",
            r#"
package org.springframework.context.support;

import org.springframework.context.*;

class ContextLoader {
    ApplicationContext load() {
        return null;
    }
}
"#,
        ),
    ]);
    let import = import_containing(&snapshot, "org.springframework.context.*");

    assert_eq!(import.resolution_state, "resolved");
    assert_eq!(
        import.target_hint.as_deref(),
        Some("org/springframework/context")
    );
}

#[test]
fn java_static_import_reports_overloaded_members_as_ambiguous() {
    let snapshot = parse_sources(&[
        (
            "src/app/RetryPolicy.java",
            r#"
package app;

class RetryPolicy {
    static void run() {}
    static void run(int attempts) {}
}
"#,
        ),
        (
            "src/app/Worker.java",
            r#"
package app;

import static app.RetryPolicy.run;

class Worker {}
"#,
        ),
    ]);

    assert_import_state(&snapshot, "static app.RetryPolicy.run", "ambiguous");
}

#[test]
fn typescript_import_resolution_handles_relative_modules_and_index_files() {
    let snapshot = parse_sources(&[
        (
            "src/sleep.ts",
            r#"
export function sleep(): void {}
"#,
        ),
        (
            "src/utils/index.ts",
            r#"
export function buildRequest(): string {
    return "ok";
}
"#,
        ),
        (
            "src/app.ts",
            r#"
import { sleep } from "./sleep";
import { buildRequest } from "./utils";
export { sleep as exportedSleep } from "./sleep";

export function retryPolicy(): void {
    sleep();
    buildRequest();
}
"#,
        ),
    ]);

    assert_import_state(&snapshot, "./sleep", "resolved");
    assert_import_state(&snapshot, "./utils", "resolved");
    assert_import_state(
        &snapshot,
        "export { sleep as exportedSleep } from \"./sleep\"",
        "resolved",
    );
}

#[test]
fn typescript_external_package_imports_do_not_match_local_symbol_names() {
    let snapshot = parse_sources(&[
        (
            "src/local/session.ts",
            r#"
export class Session {}
"#,
        ),
        (
            "src/client.ts",
            r#"
import { Session } from "requests";

export function buildClient(): Session {
    return new Session();
}
"#,
        ),
    ]);

    assert_import_state(&snapshot, "requests", "unresolved");
}

#[test]
fn typescript_import_specifiers_do_not_strip_source_like_roots() {
    let snapshot = parse_sources(&[
        (
            "src/session.ts",
            r#"
export class Session {}
"#,
        ),
        (
            "src/client.ts",
            r#"
import { Session } from "lib/session";

export function buildClient(): Session {
    return new Session();
}
"#,
        ),
    ]);

    assert_import_state(&snapshot, "lib/session", "unresolved");
}

#[test]
fn typescript_dynamic_imports_use_string_specifier_identity() {
    let snapshot = parse_sources(&[
        ("src/lazy/alpha.ts", "export function alpha(): void {}"),
        ("src/lazy/beta.ts", "export function beta(): void {}"),
        (
            "src/loader.ts",
            r#"
export async function loadLazy() {
    const moduleName = "./lazy/runtime";
    return Promise.all([
        import("./lazy/alpha"),
        import("./lazy/beta"),
        import(moduleName),
    ]);
}
"#,
        ),
    ]);
    let loader_imports = snapshot
        .imports
        .iter()
        .filter(|import| import.path == "src/loader.ts")
        .collect::<Vec<_>>();
    let ids = loader_imports
        .iter()
        .map(|import| import.import_id.as_str())
        .collect::<BTreeSet<_>>();

    assert_eq!(loader_imports.len(), 2);
    assert_eq!(ids.len(), loader_imports.len());
    assert!(
        loader_imports
            .iter()
            .any(|import| import.module == "import(\"./lazy/alpha\")")
    );
    assert!(
        loader_imports
            .iter()
            .any(|import| import.module == "import(\"./lazy/beta\")")
    );
    assert!(
        !loader_imports
            .iter()
            .any(|import| import.module == "import" || import.module.contains("moduleName"))
    );
    assert_import_state(&snapshot, "./lazy/alpha", "resolved");
    assert_import_state(&snapshot, "./lazy/beta", "resolved");
}

#[test]
fn python_import_resolution_does_not_strip_vendor_as_source_root() {
    let snapshot = parse_sources(&[
        (
            "vendor/pkg/foo.py",
            r#"
class VendorThing:
    pass
"#,
        ),
        (
            "src/app.py",
            r#"
from pkg.foo import VendorThing

def build():
    return VendorThing()
"#,
        ),
    ]);

    assert_import_state(&snapshot, "from pkg.foo import VendorThing", "unresolved");
}

#[test]
fn python_import_resolution_handles_external_deps_source_roots() {
    let snapshot = parse_sources(&[
        (
            "external_deps/python_sdk/session_client.py",
            r#"
class ExternalSessionClient:
    pass
"#,
        ),
        (
            "src/app.py",
            r#"
from python_sdk.session_client import ExternalSessionClient

def build():
    return ExternalSessionClient()
"#,
        ),
    ]);

    assert_import_state(
        &snapshot,
        "from python_sdk.session_client import ExternalSessionClient",
        "resolved",
    );
}

#[test]
fn typescript_import_resolution_handles_external_deps_source_roots() {
    let snapshot = parse_sources(&[
        (
            "external_deps/ts_sdk/sessionClient.ts",
            r#"
export class ExternalSessionClient {}
"#,
        ),
        (
            "src/app.ts",
            r#"
import { ExternalSessionClient } from "ts_sdk/sessionClient";

export function buildClient(): ExternalSessionClient {
    return new ExternalSessionClient();
}
"#,
        ),
    ]);

    assert_import_state(&snapshot, "ts_sdk/sessionClient", "resolved");
}

#[test]
fn go_import_resolution_splits_blocks_and_handles_staging_source_roots() {
    let snapshot = parse_sources(&[
        (
            "staging/src/k8s.io/client-go/informers/factory.go",
            r#"
package informers

type SharedInformerFactory interface {}
"#,
        ),
        (
            "pkg/kubeapiserver/authorizer/config.go",
            r#"
package authorizer

import (
    "context"
    informers "k8s.io/client-go/informers"
    // "not/local"
)

var _ informers.SharedInformerFactory
var _ context.Context
"#,
        ),
    ]);

    assert_import_state(&snapshot, "k8s.io/client-go/informers", "resolved");
    assert_import_state(&snapshot, "context", "unresolved");
    assert!(
        snapshot
            .imports
            .iter()
            .any(|import| import.module == "informers k8s.io/client-go/informers")
    );
    assert!(
        !snapshot
            .imports
            .iter()
            .any(|import| import.module.contains("not/local"))
    );
}

#[test]
fn go_import_resolution_preserves_explicit_vendor_module_roots() {
    let snapshot = parse_sources(&[
        (
            "vendor/k8s.io/client-go/informers/factory.go",
            r#"
package informers

type SharedInformerFactory interface {}
"#,
        ),
        (
            "pkg/kubeapiserver/authorizer/config.go",
            r#"
package authorizer

import informers "k8s.io/client-go/informers"

var _ informers.SharedInformerFactory
"#,
        ),
    ]);

    assert_import_state(&snapshot, "k8s.io/client-go/informers", "resolved");
}

#[test]
fn go_import_resolution_handles_plugin_module_roots() {
    let snapshot = parse_sources(&[
        (
            "plugins/example.com/nonstandard/session/client.go",
            r#"
package session

type ExternalSessionClient interface {}
"#,
        ),
        (
            "cmd/app/main.go",
            r#"
package main

import session "example.com/nonstandard/session"

var _ session.ExternalSessionClient
"#,
        ),
    ]);

    assert_import_state(&snapshot, "example.com/nonstandard/session", "resolved");
}

#[test]
fn java_import_resolution_handles_nested_module_source_roots() {
    let snapshot = parse_sources(&[
        (
            "modules/java_sdk/src/main/java/example/SessionClient.java",
            r#"
package example;

public class SessionClient {}
"#,
        ),
        (
            "src/main/java/app/Worker.java",
            r#"
package app;

import example.SessionClient;

class Worker {
    SessionClient client() {
        return null;
    }
}
"#,
        ),
    ]);

    assert_import_state(&snapshot, "example.SessionClient", "resolved");
}

#[test]
fn ruby_require_relative_import_resolution_targets_local_files() {
    let snapshot = parse_sources(&[
        (
            "lib/app/extensions.rb",
            r#"
module App
  module Extensions
  end
end
"#,
        ),
        (
            "lib/app/controller.rb",
            r#"
require_relative "extensions"

module App
  class Controller
    include Extensions
  end
end
"#,
        ),
    ]);
    let import = import_containing(&snapshot, "require_relative \"extensions\"");

    assert_eq!(import.resolution_state, "resolved");
    assert_eq!(import.target_hint.as_deref(), Some("lib/app/extensions.rb"));
}

#[test]
fn bash_source_imports_preserve_shellcheck_context_and_resolve_targets() {
    let snapshot = parse_sources(&[
        ("lib/runtime.sh", "rk_runtime_dispatch() { :; }\n"),
        (
            "bin/install.sh",
            r#"
SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/runtime.sh
. "$SCRIPT_DIR/../lib/runtime.sh"
"#,
        ),
    ]);
    let import = import_containing(&snapshot, ". \"$SCRIPT_DIR/../lib/runtime.sh\"");

    assert!(
        import
            .module
            .contains("# shellcheck source=../lib/runtime.sh")
    );
    assert_eq!(import.resolution_state, "resolved");
    assert_eq!(import.target_hint.as_deref(), Some("lib/runtime.sh"));
}

#[test]
fn cpp_import_resolution_handles_local_includes_and_using_declarations() {
    let snapshot = parse_sources(&[
        (
            "src/retry.hpp",
            r#"
namespace app {
void RetryPolicy() {}
}
"#,
        ),
        (
            "src/retry.cpp",
            r#"
#include "retry.hpp"
#include <string>
using app::RetryPolicy;

void run() {
    RetryPolicy();
}
"#,
        ),
    ]);

    assert_import_state(&snapshot, "retry.hpp", "resolved");
    assert_import_state(&snapshot, "<string>", "unresolved");
    assert_import_state(&snapshot, "using app::RetryPolicy", "resolved");
}

#[test]
fn c_include_resolution_normalizes_relative_header_paths() {
    let snapshot = parse_sources(&[
        (
            "include/driver.h",
            r#"
void register_driver(void);
"#,
        ),
        (
            "src/platform/driver.c",
            r#"
#include "../../include/driver.h"

void load_driver(void) {
    register_driver();
}
"#,
        ),
    ]);
    let import = import_containing(&snapshot, "../../include/driver.h");

    assert_eq!(import.resolution_state, "resolved");
    assert_eq!(import.target_hint.as_deref(), Some("include/driver.h"));
}

#[test]
fn quoted_include_resolution_preserves_source_directory_precedence() {
    let snapshot = parse_sources(&[
        (
            "foo.h",
            r#"
void root_header(void);
"#,
        ),
        (
            "src/foo.h",
            r#"
void source_header(void);
"#,
        ),
        (
            "src/use.cpp",
            r#"
#include "foo.h"
"#,
        ),
    ]);
    let import = import_containing(&snapshot, "\"foo.h\"");

    assert_eq!(import.resolution_state, "resolved");
    assert_eq!(import.target_hint.as_deref(), Some("src/foo.h"));
}

#[test]
fn angle_include_resolution_does_not_probe_source_directory_first() {
    let snapshot = parse_sources(&[
        (
            "include/driver.h",
            r#"
void include_driver(void);
"#,
        ),
        (
            "src/driver.h",
            r#"
void private_driver(void);
"#,
        ),
        (
            "src/platform/driver.h",
            r#"
void platform_driver(void);
"#,
        ),
        (
            "src/platform/driver.c",
            r#"
#include <driver.h>
"#,
        ),
    ]);
    let import = import_containing(&snapshot, "<driver.h>");

    assert_eq!(import.resolution_state, "resolved");
    assert_eq!(import.target_hint.as_deref(), Some("include/driver.h"));
}

#[test]
fn cpp_include_resolution_handles_external_include_roots() {
    let snapshot = parse_sources(&[
        (
            "external_deps/cpp_sdk/include/session_client.hpp",
            r#"
void external_session_client(void);
"#,
        ),
        (
            "src/app.cpp",
            r#"
#include <session_client.hpp>

void run() {
    external_session_client();
}
"#,
        ),
    ]);
    let import = import_containing(&snapshot, "<session_client.hpp>");

    assert_eq!(import.resolution_state, "resolved");
    assert_eq!(
        import.target_hint.as_deref(),
        Some("external_deps/cpp_sdk/include/session_client.hpp")
    );
}

#[test]
fn cpp_using_declarations_keep_ambiguous_symbols_visible() {
    let snapshot = parse_sources(&[
        (
            "src/a.cpp",
            r#"
namespace app {
void RetryPolicy() {}
}
"#,
        ),
        (
            "src/b.cpp",
            r#"
namespace app {
void RetryPolicy() {}
}
"#,
        ),
        (
            "src/use.cpp",
            r#"
using app::RetryPolicy;
"#,
        ),
    ]);

    assert_import_state(&snapshot, "using app::RetryPolicy", "ambiguous");
}

#[test]
fn deep_syntax_trees_are_walked_without_recursive_parser_helpers() {
    let mut source = String::from("function root() {\n");
    for _ in 0..2_048 {
        source.push_str("if (true) {\n");
    }
    source.push_str("sleep();\n");
    for _ in 0..2_048 {
        source.push_str("}\n");
    }
    source.push_str("}\n");

    let snapshot = parse_sources(&[("src/deep.js", &source)]);

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert!(
        snapshot
            .references
            .iter()
            .any(|reference| reference.name == "sleep")
    );
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
