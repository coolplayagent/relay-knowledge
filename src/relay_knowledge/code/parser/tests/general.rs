use std::collections::BTreeSet;

use crate::domain::{CodeParseStatus, CodeRepositoryRegistration};

use super::*;

#[test]
fn tree_sitter_captures_symbols_references_imports_and_chunks() {
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
    let source = br#"
use std::time::Duration;

/// Runs retries.
fn retry_policy() {
    sleep(Duration::from_secs(1));
}
"#;

    parse_indexed_file(&mut build, "src/lib.rs", source).expect("file should parse");
    let snapshot = build.finish();

    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "retry_policy")
    );
    assert!(
        snapshot
            .references
            .iter()
            .any(|reference| reference.name == "sleep")
    );
    assert!(
        snapshot
            .imports
            .iter()
            .any(|import| import.module.contains("std::time"))
    );
    assert!(
        snapshot
            .chunks
            .iter()
            .any(|chunk| chunk.content.contains("retry_policy"))
    );
}

#[test]
fn feature_flags_are_extracted_from_full_symbol_bearing_files() {
    let snapshot = parse_source_snapshot(
        "src/app.py",
        br#"
import os

CHECKOUT_ENABLED = os.getenv("CHECKOUT_V2")

def run_checkout():
    execute_checkout()
"#,
    );

    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "run_checkout")
    );
    assert!(snapshot.feature_flags.iter().any(|record| {
        record.source_key == "CHECKOUT_V2" && record.edge_kind == "reads_config"
    }));
}

#[test]
fn python_tree_sitter_imports_resolve_local_symbols() {
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
        "src/relay_teams/connector/w3_models.py",
        br#"
class W3ConnectorSaveRequest:
    pass
"#,
    )
    .expect("model file should parse");
    parse_indexed_file(
        &mut build,
        "src/relay_teams/connector/service.py",
        br#"
from relay_teams.connector.w3_models import W3ConnectorSaveRequest

def build_request():
    return W3ConnectorSaveRequest()
"#,
    )
    .expect("service file should parse");

    let snapshot = build.finish();
    let import = snapshot
        .imports
        .iter()
        .find(|import| import.module.contains("W3ConnectorSaveRequest"))
        .expect("tree-sitter should collect the Python import statement");

    assert_eq!(import.resolution_state, "resolved");
    assert_eq!(import.confidence_tier, "inferred");
}

#[test]
fn python_tree_sitter_external_imports_do_not_match_local_symbol_names() {
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
        "src/local/session.py",
        br#"
class Session:
    pass
"#,
    )
    .expect("local file should parse");
    parse_indexed_file(
        &mut build,
        "src/client.py",
        br#"
from requests import Session

def build_client():
    return Session()
"#,
    )
    .expect("client file should parse");

    let snapshot = build.finish();
    let import = snapshot
        .imports
        .iter()
        .find(|import| import.module.contains("requests"))
        .expect("tree-sitter should collect the external Python import");

    assert_eq!(import.resolution_state, "unresolved");
    assert_eq!(import.confidence_tier, "ambiguous");
}

#[test]
fn python_tree_sitter_package_init_imports_resolve_package_modules() {
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
        "src/pkg/__init__.py",
        br#"
PACKAGE_NAME = "pkg"
"#,
    )
    .expect("package init should parse");
    parse_indexed_file(
        &mut build,
        "src/app.py",
        br#"
import pkg

def load():
    return pkg.PACKAGE_NAME
"#,
    )
    .expect("app file should parse");

    let snapshot = build.finish();
    let import = snapshot
        .imports
        .iter()
        .find(|import| import.module == "import pkg")
        .expect("tree-sitter should collect the package import");

    assert_eq!(import.resolution_state, "resolved");
    assert_eq!(import.confidence_tier, "inferred");
}

#[test]
fn mainstream_tree_sitter_languages_extract_symbols_imports_and_chunks() {
    let fixtures = [
        LanguageFixture {
            path: "src/app.js",
            source: br#"
import { sleep } from "./sleep.js";
export function retryPolicy() {
    return sleep(1);
}
"#,
            language_id: "javascript",
            symbol_name: "retryPolicy",
            import_fragment: Some("sleep"),
        },
        LanguageFixture {
            path: "src/view.jsx",
            source: br#"
export function RetryButton() {
    return <button onClick={retryPolicy}>Retry</button>;
}
"#,
            language_id: "jsx",
            symbol_name: "RetryButton",
            import_fragment: None,
        },
        LanguageFixture {
            path: "src/app.ts",
            source: br#"
import { sleep } from "./sleep";
export function retryPolicy(): void {
    sleep(1);
}
"#,
            language_id: "typescript",
            symbol_name: "retryPolicy",
            import_fragment: Some("sleep"),
        },
        LanguageFixture {
            path: "src/view.tsx",
            source: br#"
export function RetryView() {
    return <span>{retryPolicy()}</span>;
}
"#,
            language_id: "tsx",
            symbol_name: "RetryView",
            import_fragment: None,
        },
        LanguageFixture {
            path: "src/app.go",
            source: br#"
package retry

import "time"

func RetryPolicy() {
    time.Sleep(time.Second)
}
"#,
            language_id: "go",
            symbol_name: "RetryPolicy",
            import_fragment: Some("time"),
        },
        LanguageFixture {
            path: "src/RetryPolicy.java",
            source: br#"
package app;

import java.time.Duration;

class RetryPolicy {
    void run() {
        Duration.ofSeconds(1);
    }
}
"#,
            language_id: "java",
            symbol_name: "RetryPolicy",
            import_fragment: Some("java.time.Duration"),
        },
        LanguageFixture {
            path: "src/retry.c",
            source: br#"
#include <stdio.h>

void retry_policy(void) {
    puts("retry");
}
"#,
            language_id: "c",
            symbol_name: "retry_policy",
            import_fragment: Some("stdio"),
        },
        LanguageFixture {
            path: "src/retry.cpp",
            source: br#"
#include <string>

void retry_policy() {
    std::string message = "retry";
}
"#,
            language_id: "cpp",
            symbol_name: "retry_policy",
            import_fragment: Some("string"),
        },
        LanguageFixture {
            path: "src/RetryPolicy.cs",
            source: br#"
using System;

class RetryPolicy {
    void Run() {
        Console.WriteLine("retry");
    }
}
"#,
            language_id: "csharp",
            symbol_name: "RetryPolicy",
            import_fragment: Some("System"),
        },
        LanguageFixture {
            path: "src/retry.rb",
            source: br#"
require "time"

def retry_policy
  sleep 1
end
"#,
            language_id: "ruby",
            symbol_name: "retry_policy",
            import_fragment: None,
        },
        LanguageFixture {
            path: "src/retry.php",
            source: br#"
<?php
use DateTime;

function retry_policy() {
    return new DateTime();
}
"#,
            language_id: "php",
            symbol_name: "retry_policy",
            import_fragment: Some("DateTime"),
        },
        LanguageFixture {
            path: "src/RetryPolicy.kt",
            source: br#"
package app

import kotlin.time.Duration

fun retryPolicy() {
    println("retry")
}
"#,
            language_id: "kotlin",
            symbol_name: "retryPolicy",
            import_fragment: Some("kotlin.time.Duration"),
        },
        LanguageFixture {
            path: "src/RetryPolicy.scala",
            source: br#"
package app

import scala.concurrent.Future

object RetryPolicy {
  def run(): Unit = println("retry")
}
"#,
            language_id: "scala",
            symbol_name: "run",
            import_fragment: Some("scala.concurrent.Future"),
        },
        LanguageFixture {
            path: "src/RetryPolicy.swift",
            source: br#"
import Foundation

func retryPolicy() {
    print("retry")
}
"#,
            language_id: "swift",
            symbol_name: "retryPolicy",
            import_fragment: Some("Foundation"),
        },
        LanguageFixture {
            path: "scripts/retry.sh",
            source: br#"
retry_policy() {
  echo retry
}

retry_policy
"#,
            language_id: "bash",
            symbol_name: "retry_policy",
            import_fragment: None,
        },
    ];

    for fixture in fixtures {
        let snapshot = parse_source_snapshot(fixture.path, fixture.source);

        assert_eq!(
            snapshot.files[0].language_id, fixture.language_id,
            "{} should use the expected language id",
            fixture.path
        );
        assert_eq!(
            snapshot.files[0].parse_status,
            CodeParseStatus::Parsed,
            "{} should parse cleanly: {:?}",
            fixture.path,
            snapshot.diagnostics
        );
        assert!(
            snapshot
                .symbols
                .iter()
                .any(|symbol| symbol.name == fixture.symbol_name),
            "{} should expose symbol {}",
            fixture.path,
            fixture.symbol_name
        );
        assert!(
            snapshot
                .chunks
                .iter()
                .any(|chunk| chunk.content.contains(fixture.symbol_name)),
            "{} should create a retrievable chunk for {}",
            fixture.path,
            fixture.symbol_name
        );
        if let Some(import_fragment) = fixture.import_fragment {
            assert!(
                snapshot
                    .imports
                    .iter()
                    .any(|import| import.module.contains(import_fragment)),
                "{} should collect import fragment {}",
                fixture.path,
                import_fragment
            );
            assert!(
                snapshot
                    .imports
                    .iter()
                    .all(|import| import.module != "import"),
                "{} should not record bare import keyword tokens",
                fixture.path
            );
        }
    }
}

#[test]
fn configuration_languages_extract_nested_keys_as_symbols() {
    let json = parse_source_snapshot(
        "package.json",
        br#"{"scripts":{"build":"vite build"},"dependencies":{"react":"^18"}}"#,
    );
    assert_eq!(json.files[0].language_id, "json");
    assert_eq!(json.files[0].parse_status, CodeParseStatus::Parsed);
    assert_symbol_and_chunk(&json, "scripts.build");
    assert_symbol_and_chunk(&json, "dependencies.react");

    let yaml = parse_source_snapshot(
        ".github/workflows/ci.yml",
        b"jobs:\n  build:\n    steps:\n      - uses: actions/checkout@v4\nservices:\n  api:\n    image: ghcr.io/org/app:1.2.3\n",
    );
    assert_eq!(yaml.files[0].language_id, "yaml");
    assert_eq!(yaml.files[0].parse_status, CodeParseStatus::Parsed);
    assert_symbol_and_chunk(&yaml, "jobs.build");
    assert_symbol_and_chunk(&yaml, "services.api.image");

    let properties = parse_source_snapshot(
        "src/main/resources/application.properties",
        b"spring.datasource.url=jdbc:postgresql://localhost/app\nlogging.level.root: INFO\n",
    );
    assert_eq!(properties.files[0].language_id, "properties");
    assert_eq!(properties.files[0].parse_status, CodeParseStatus::Parsed);
    assert_symbol_and_chunk(&properties, "spring.datasource.url");
    assert_symbol_and_chunk(&properties, "logging.level.root");
}

#[test]
fn dependency_only_lockfiles_emit_sbom_without_chunks() {
    let snapshot = parse_source_snapshot(
        "package-lock.json",
        br#"{"packages":{"node_modules/react":{"version":"18.2.0"}}}"#,
    );

    assert_eq!(snapshot.files[0].language_id, "unknown");
    assert!(snapshot.symbols.is_empty());
    assert!(snapshot.chunks.is_empty());
    assert!(
        snapshot
            .dependencies
            .iter()
            .any(|dependency| dependency.package_name == "react")
    );

    let uv = parse_source_snapshot(
        "uv.lock",
        br#"[[package]]
name = "httpx"
version = "0.27.2"
source = { registry = "https://pypi.org/simple" }
"#,
    );
    assert_eq!(uv.files[0].language_id, "unknown");
    assert!(uv.chunks.is_empty());
    assert!(
        uv.dependencies
            .iter()
            .any(|dependency| dependency.package_name == "httpx")
    );
}

fn assert_symbol_and_chunk(snapshot: &crate::domain::CodeIndexSnapshot, name: &str) {
    assert!(
        snapshot.symbols.iter().any(|symbol| symbol.name == name),
        "missing config symbol {name}; got {:?}",
        snapshot
            .symbols
            .iter()
            .map(|symbol| symbol.name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        snapshot.chunks.iter().any(|chunk| chunk
            .content
            .contains(name.rsplit('.').next().unwrap_or(name))),
        "missing chunk for {name}; got {:?}",
        snapshot
            .chunks
            .iter()
            .map(|chunk| chunk.content.as_str())
            .collect::<Vec<_>>()
    );
}

struct LanguageFixture {
    path: &'static str,
    source: &'static [u8],
    language_id: &'static str,
    symbol_name: &'static str,
    import_fragment: Option<&'static str>,
}

#[test]
fn syntax_error_files_are_partial_and_keep_reliable_facts() {
    let snapshot = parse_source_snapshot(
        "src/lib.rs",
        br#"
fn retry_policy() -> u32 {
    let broken = ;
    3
}
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Partial);
    assert!(
        snapshot
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("error nodes"))
    );
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "retry_policy")
    );
    assert!(
        snapshot
            .chunks
            .iter()
            .any(|chunk| chunk.content.contains("retry_policy"))
    );
}

#[test]
fn parser_panics_are_recorded_as_failed_file_diagnostics() {
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
    let content = "fn retry_policy() {}\n";

    parse_syntax_file(
        &mut build,
        SyntaxFileInput {
            path: "src/lib.rs",
            file_id: "file",
            language: LanguageSpec {
                id: "rust",
                language: || panic!("parser boom"),
                tags_query: "",
            },
            blob_hash: "hash",
            byte_len: content.len(),
            line_count: 1,
            content,
        },
    )
    .expect("parser failure should be isolated");
    let snapshot = build.finish();

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Failed);
    assert!(
        snapshot
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("tree-sitter parse failed"))
    );
    assert!(snapshot.symbols.is_empty());
}

#[test]
fn multiline_symbol_signatures_keep_parameter_types_searchable() {
    let snapshot = parse_source_snapshot(
        "src/relay_teams/connector/service.py",
        br#"
class W3ConnectorSaveRequest:
    pass

class ConnectorService:
    async def save_w3_connector(
        self,
        request: W3ConnectorSaveRequest,
    ) -> None:
        pass
"#,
    );
    let symbol = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "save_w3_connector")
        .expect("method symbol should be extracted");

    assert!(symbol.signature.contains("W3ConnectorSaveRequest"));
    assert!(!symbol.signature.contains("pass"));
}

#[test]
fn python_type_annotations_emit_type_references() {
    let snapshot = parse_source_snapshot(
        "src/relay_teams/connector/service.py",
        br#"
class W3ConnectorSaveRequest:
    pass

class ConnectorService:
    async def save_w3_connector(
        self,
        request: W3ConnectorSaveRequest,
    ) -> None:
        pass
"#,
    );

    assert!(snapshot.references.iter().any(|reference| {
        reference.name == "W3ConnectorSaveRequest"
            && reference.kind == "type"
            && reference.line_range.start == 8
    }));
}

#[test]
fn go_type_signatures_keep_declaration_keyword() {
    let snapshot = parse_source_snapshot(
        "processor/worker.go",
        br#"
package processor

type EventProcessor interface {
    Process(event string) error
}
"#,
    );
    let symbol = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "EventProcessor")
        .expect("Go interface symbol should be extracted");

    assert!(
        symbol
            .signature
            .starts_with("type EventProcessor interface")
    );
}

#[test]
fn typescript_imports_have_site_stable_ids_and_skip_import_expression_tokens() {
    let snapshot = parse_source_snapshot(
        "src/runtime.ts",
        br#"
import "./polyfill"; import "./polyfill";
const root = new URL(".", import.meta.url);
if (import.meta.env.DEV) await import("./dev");
if (import.meta.env.TEST) await import(runtimeModule);

export function runtimeRoot(): string {
    return root.href;
}
"#,
    );
    let modules = snapshot
        .imports
        .iter()
        .map(|import| import.module.as_str())
        .collect::<Vec<_>>();
    let ids = snapshot
        .imports
        .iter()
        .map(|import| import.import_id.as_str())
        .collect::<BTreeSet<_>>();

    assert_eq!(snapshot.imports.len(), 3);
    assert_eq!(ids.len(), snapshot.imports.len());
    assert_eq!(
        modules
            .iter()
            .filter(|module| module.contains("\"./polyfill\""))
            .count(),
        2
    );
    assert!(modules.contains(&"await import(\"./dev\")"));
    assert!(!modules.contains(&"import"));
    assert!(
        !modules
            .iter()
            .any(|module| module.contains("runtimeModule"))
    );
}

#[test]
fn long_multibyte_symbol_signatures_truncate_on_utf8_boundary() {
    let mut source = "def retry_policy(value=\"".to_owned();
    source.push_str(&"\u{00e9}".repeat(300));
    source.push_str("\"):\n    pass\n");

    let snapshot = parse_source_snapshot("src/app.py", source.as_bytes());
    let symbol = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "retry_policy")
        .expect("function symbol should be extracted");

    assert!(symbol.signature.len() <= 512);
    assert!(symbol.signature.is_char_boundary(symbol.signature.len()));
}

#[test]
fn manual_call_extraction_preserves_same_line_calls() {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    let content = "fn run() { foo(); foo(); }\n";
    let language = detect_language("src/lib.rs").expect("rust should be configured");
    let parsed = parse_tree(language, content).expect("source should parse");
    let context = FileParseContext {
        build: &build,
        path: "src/lib.rs",
        file_id: "file",
        language_id: language.id,
        content,
    };
    let mut output = FileParseOutput::new();

    collect_manual_nodes(&context, parsed.root_node(), &[], &[], &mut output)
        .expect("manual extraction should succeed");
    let foo_ranges = output
        .references
        .iter()
        .filter(|reference| reference.name == "foo")
        .map(|reference| reference.byte_range.clone())
        .collect::<Vec<_>>();

    assert_eq!(foo_ranges.len(), 2);
    assert_ne!(foo_ranges[0], foo_ranges[1]);
}

#[test]
fn rust_attributes_are_not_doc_comments() {
    let snapshot = parse_source_snapshot(
        "src/lib.rs",
        br#"
#[derive(Debug)]
pub struct Settings;
"#,
    );
    let settings = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Settings")
        .expect("struct symbol should be extracted");

    assert_eq!(settings.doc_comment, None);
}

#[test]
fn python_hash_comments_are_doc_comments() {
    let snapshot = parse_source_snapshot(
        "src/app.py",
        br#"
# Runs the worker.
def run_worker():
    pass
"#,
    );
    let worker = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "run_worker")
        .expect("function symbol should be extracted");

    assert_eq!(worker.doc_comment.as_deref(), Some("Runs the worker."));
}

#[test]
fn text_only_files_keep_bm25_fallback_chunks() {
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

    parse_indexed_file(&mut build, "README.txt", b"RetryPolicy appears in docs")
        .expect("file should index as text");
    let snapshot = build.finish();

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::TextOnly);
    assert_eq!(snapshot.chunks.len(), 1);
    assert!(snapshot.diagnostics[0].message.contains("grammar"));
}

#[test]
fn invalid_utf8_files_degrade_to_lossy_text_chunks() {
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

    parse_indexed_file(
        &mut build,
        "src/lib.rs",
        b"fn retry_policy() {}\n\xff\nfn caller() {}",
    )
    .expect("invalid utf8 should degrade instead of failing");
    let snapshot = build.finish();

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::TextOnly);
    assert!(snapshot.diagnostics[0].message.contains("not valid UTF-8"));
    assert!(snapshot.chunks[0].content.contains("retry_policy"));
}

#[test]
fn generated_record_ids_are_scoped_by_repository() {
    let first = parse_fixture_snapshot("repo-a");
    let second = parse_fixture_snapshot("repo-b");

    assert_ne!(
        first.references[0].reference_id,
        second.references[0].reference_id
    );
    assert_eq!(
        first
            .references
            .iter()
            .filter(|reference| reference.name == "retry_policy")
            .count(),
        2
    );
    assert_ne!(first.imports[0].import_id, second.imports[0].import_id);
    assert_ne!(first.chunks[0].chunk_id, second.chunks[0].chunk_id);
    assert_ne!(first.calls[0].call_id, second.calls[0].call_id);
}

#[test]
fn oversized_files_truncate_on_utf8_boundary() {
    let mut bytes = vec![b'a'; MAX_TEXT_FILE_BYTES - 1];
    bytes.extend("é".as_bytes());
    bytes.extend(b"tail");

    let (status, reason, content) =
        validate_text_content("src/lib.rs", &bytes, detect_language("src/lib.rs"))
            .expect("oversized utf8 should degrade");
    let content = content.expect("oversized file should keep fallback content");

    assert_eq!(status, CodeParseStatus::TextOnly);
    assert!(
        reason
            .expect("reason should explain budget")
            .contains("exceeds")
    );
    assert_eq!(content.len(), MAX_TEXT_FILE_BYTES - 1);
    assert!(!content.contains('\u{fffd}'));
}

fn parse_fixture_snapshot(repository_id: &str) -> crate::domain::CodeIndexSnapshot {
    parse_source_snapshot_for_repository(
        repository_id,
        "src/main.rs",
        br#"
use crate::retry_policy;

fn run_worker() {
    retry_policy(); retry_policy();
}
"#,
    )
}

fn parse_source_snapshot(path: &str, source: &[u8]) -> crate::domain::CodeIndexSnapshot {
    parse_source_snapshot_for_repository("repo", path, source)
}

fn parse_source_snapshot_for_repository(
    repository_id: &str,
    path: &str,
    source: &[u8],
) -> crate::domain::CodeIndexSnapshot {
    let registration = CodeRepositoryRegistration::new(
        repository_id,
        "alias",
        "/tmp/repo",
        Vec::new(),
        Vec::new(),
    )
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
