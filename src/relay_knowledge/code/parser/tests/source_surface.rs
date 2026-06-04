use crate::domain::{CodeIndexSnapshot, CodeRepositoryRegistration};

use super::*;

#[test]
fn small_code_files_keep_uncovered_import_surface_chunks() {
    let snapshot = parse_source_snapshot(
        "src/lib.rs",
        br#"
use crate::runtime::Runtime;

pub fn run(runtime: Runtime) {
    runtime.start();
}
"#,
    );

    let file_chunk = snapshot
        .chunks
        .iter()
        .find(|chunk| chunk.symbol_snapshot_id.is_none())
        .expect("small file should keep uncovered import surface");

    assert!(file_chunk.content.contains("use crate::runtime::Runtime"));
    assert!(file_chunk.content.contains("runtime.start()"));
}

#[test]
fn fully_covered_code_files_do_not_add_redundant_file_chunks() {
    let snapshot = parse_source_snapshot("src/lib.rs", b"pub fn run() {}\n");

    assert!(
        snapshot
            .chunks
            .iter()
            .all(|chunk| chunk.symbol_snapshot_id.is_some()),
        "file chunks should not duplicate fully covered symbol chunks: {:?}",
        snapshot.chunks
    );
}

#[test]
fn csharp_member_invocations_are_indexed_as_calls() {
    let snapshot = parse_source_snapshot(
        "src/RuntimeService.cs",
        br#"
using System;

class BufferSink {
    byte[] RentBuffer(int size) => new byte[size];
    void Write(byte[] buffer) {}
}

class RuntimeService {
    void Dispatch(BufferSink sink, int size) {
        Func<byte[], byte[]> returnBuffer = rented => rented;
        var buffer = sink.RentBuffer(size);
        sink.Write(returnBuffer(buffer));
    }
}
"#,
    );

    assert_call(&snapshot, "RentBuffer");
    assert_call(&snapshot, "Write");
    assert_call(&snapshot, "returnBuffer");
}

#[test]
fn swift_member_invocations_are_indexed_as_calls() {
    let snapshot = parse_source_snapshot(
        "Sources/App/RequestPipeline.swift",
        br#"
import Foundation

protocol SessionClient {
    func request(url: URL) async throws -> Data
}

final class RequestPipeline {
    let client: SessionClient

    init(client: SessionClient) {
        self.client = client
    }

    func dispatch(url: URL) async throws -> Data {
        try await client.request(url: url)
    }
}
"#,
    );

    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "request" && symbol.kind == "method"),
        "Swift protocol request definition should be indexed: {:?}",
        snapshot.symbols
    );
    assert_call(&snapshot, "request");
}

fn assert_call(snapshot: &CodeIndexSnapshot, name: &str) {
    assert!(
        snapshot
            .references
            .iter()
            .any(|reference| reference.name == name && reference.kind == "call"),
        "{name} call should be indexed: {:?}",
        snapshot.references
    );
}

fn parse_source_snapshot(path: &str, source: &[u8]) -> CodeIndexSnapshot {
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
