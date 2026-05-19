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
