use crate::domain::{CodeIndexSnapshot, CodeRepositoryRegistration};

use super::*;

#[test]
fn javascript_exported_object_values_are_definition_symbols() {
    let large_body = large_object_body();
    let source = format!(
        r#"
const localState = {{
  hidden: true,
}};

export const state = {{
  currentSessionId: null,
  currentWorkspaceId: null,
}};
export const views = [state];
export const largeState = {{
{large_body}}};
"#
    );
    let snapshot = parse_source_snapshot("src/state.js", source.as_bytes());

    assert_exported_constant(&snapshot, "state", "state = {");
    assert_exported_constant(&snapshot, "views", "[state]");
    assert!(snapshot.chunks.iter().any(|chunk| {
        chunk.content.contains("currentSessionId") && chunk.content.contains("currentWorkspaceId")
    }));
    assert_missing_symbols(&snapshot, ["localState", "largeState"]);
}

#[test]
fn typescript_exported_object_values_are_definition_symbols() {
    let source = br#"
type Route = { id: string };
const localState = {
  hidden: true,
};

export const state = {
  currentSessionId: null,
  activeRunId: null,
} as const;
export const routes = [state] satisfies Route[];
"#;
    let snapshot = parse_source_snapshot("src/state.ts", source);

    assert_exported_constant(&snapshot, "state", "state = {");
    assert_exported_constant(&snapshot, "routes", "[state]");
    assert!(snapshot.chunks.iter().any(|chunk| {
        chunk.content.contains("currentSessionId") && chunk.content.contains("activeRunId")
    }));
    assert_missing_symbols(&snapshot, ["localState"]);
}

#[test]
fn typescript_exported_declarations_preserve_source_surface() {
    let snapshot = parse_source_snapshot(
        "src/protocol.ts",
        br#"
export type PayloadProjector<TPayload> = (payload: TPayload) => TPayload;

export interface StreamTransport {
    send(payload: string): Promise<void>;
}

export class ProviderPanel {
    render(): void {}
}

export function makeProvider(): StreamTransport {
    return { send: async () => undefined };
}
"#,
    );

    assert_symbol_signature_starts_with(
        &snapshot,
        "PayloadProjector",
        "export type PayloadProjector",
    );
    assert_symbol_signature_starts_with(
        &snapshot,
        "StreamTransport",
        "export interface StreamTransport",
    );
    assert_symbol_signature_starts_with(&snapshot, "ProviderPanel", "export class ProviderPanel");
    assert_symbol_signature_starts_with(&snapshot, "makeProvider", "export function makeProvider");
}

#[test]
fn typescript_exported_function_values_use_one_exported_symbol_surface() {
    let snapshot = parse_source_snapshot(
        "src/protocol.ts",
        br#"
export type PayloadProjector<TPayload> = (payload: TPayload) => TPayload;
export const trimPayload: PayloadProjector<string> = (payload) => payload.trim();
"#,
    );
    let symbols = snapshot
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "trimPayload")
        .collect::<Vec<_>>();

    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].kind, "function");
    assert!(symbols[0].signature.starts_with("export const trimPayload"));
}

fn assert_exported_constant(snapshot: &CodeIndexSnapshot, name: &str, signature: &str) {
    let symbol = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("{name} should be extracted as a constant symbol"));

    assert_eq!(symbol.kind, "constant");
    assert!(symbol.signature.contains(signature));
    assert!(
        snapshot
            .chunks
            .iter()
            .any(|chunk| chunk.symbol_snapshot_id == Some(symbol.symbol_snapshot_id.clone())),
        "{name} should create a retrievable symbol chunk",
    );
}

fn assert_symbol_signature_starts_with(
    snapshot: &CodeIndexSnapshot,
    name: &str,
    expected_prefix: &str,
) {
    let symbol = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("{name} should be extracted as a symbol"));

    assert!(
        symbol.signature.starts_with(expected_prefix),
        "{name} signature should start with {expected_prefix:?}, got {:?}",
        symbol.signature
    );
}

fn assert_missing_symbols<const N: usize>(snapshot: &CodeIndexSnapshot, names: [&str; N]) {
    for name in names {
        assert!(
            !snapshot.symbols.iter().any(|symbol| symbol.name == name),
            "{name} should not be indexed as an exported object value",
        );
    }
}

fn large_object_body() -> String {
    (0..70)
        .map(|index| format!("  item{index}: buildItem({index}),\n"))
        .collect::<String>()
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
