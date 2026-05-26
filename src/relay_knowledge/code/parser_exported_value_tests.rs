use crate::domain::{CodeIndexSnapshot, CodeParseStatus, CodeRepositoryRegistration};

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

#[test]
fn typescript_function_value_declarations_are_symbols() {
    let snapshot = parse_source_snapshot(
        "src/connectors.ts",
        br#"
type W3ConnectorSaveRequest = { id: string };
const CONNECTOR_TIMEOUT_MS = 5000;

export const saveW3Connector = async (
    request: W3ConnectorSaveRequest,
): Promise<void> => {
    await client.save(request);
};

const normalizeConnector = function (
    request: W3ConnectorSaveRequest,
): W3ConnectorSaveRequest {
    return request;
};

class ConnectorService {
    saveLater = (request: W3ConnectorSaveRequest): void => {
        saveW3Connector(request);
    };
}

export const connectorActions = {
    persistW3Connector: async (request: W3ConnectorSaveRequest): Promise<void> => {
        await saveW3Connector(request);
    },
    timeoutMs: CONNECTOR_TIMEOUT_MS,
};

exports.retryW3Connector = function (request: W3ConnectorSaveRequest): void {
    saveW3Connector(request);
};

ConnectorService.prototype.flushW3Connector = async (
    request: W3ConnectorSaveRequest,
): Promise<void> => {
    await saveW3Connector(request);
};

module.exports = function hiddenDefault(request: W3ConnectorSaveRequest): void {
    saveW3Connector(request);
};

handlers[dynamicName] = async (request: W3ConnectorSaveRequest): Promise<void> => {
    await saveW3Connector(request);
};
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    for name in [
        "saveW3Connector",
        "normalizeConnector",
        "saveLater",
        "persistW3Connector",
        "retryW3Connector",
        "flushW3Connector",
    ] {
        let symbol = snapshot
            .symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("{name} should be extracted as a function symbol"));
        assert_eq!(symbol.kind, "function");
        assert!(symbol.signature.contains("W3ConnectorSaveRequest"));
    }
    let exported = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "saveW3Connector")
        .expect("exported function value should be extracted");
    assert!(
        exported
            .signature
            .starts_with("export const saveW3Connector")
    );
    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "CONNECTOR_TIMEOUT_MS")
    );
    assert!(!snapshot.symbols.iter().any(|symbol| matches!(
        symbol.name.as_str(),
        "timeoutMs" | "exports" | "dynamicName"
    )));
}

#[test]
fn typescript_exported_constructed_values_are_definition_symbols() {
    let large_body = (0..70)
        .map(|index| format!("  item{index}: buildItem({index}),\n"))
        .collect::<String>();
    let source = format!(
        r#"
const helper = Protocol.make({{}});

export const protocol = Protocol.make({{
  id: "openai-chat",
  stream: {{
    initial: () => ({{}}),
  }},
}});
export const route = new Route(protocol);
export const plain = makeProtocol({{}});
export const large = Layer.effect({{
{large_body}}});
"#
    );
    let snapshot = parse_source_snapshot("src/protocol.ts", source.as_bytes());
    let protocol = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "protocol")
        .expect("exported constructed protocol should be a definition symbol");
    let route = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "route")
        .expect("exported constructed route should be a definition symbol");

    assert_eq!(protocol.kind, "constant");
    assert!(protocol.signature.contains("Protocol.make"));
    assert_eq!(route.kind, "constant");
    assert!(route.signature.contains("new Route"));
    for ignored in ["helper", "plain", "large"] {
        assert!(
            !snapshot.symbols.iter().any(|symbol| symbol.name == ignored),
            "{ignored} should not be indexed as an exported constructed value",
        );
    }
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
