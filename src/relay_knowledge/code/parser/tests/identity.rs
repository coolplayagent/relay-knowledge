use crate::domain::{
    CodeRepositoryRegistration, RepositoryCodeRange, RepositoryCodeReferenceRecord,
    RepositoryCodeSymbolRecord,
};

use super::*;

#[test]
fn nested_symbols_receive_stable_canonical_qualified_names() {
    let mut build = snapshot_build();
    let source = br#"
class RetryPolicy {
    void run() {
        backoff();
    }

    void backoff() {}
}
"#;

    parse_indexed_file(&mut build, "src/RetryPolicy.java", source).expect("file should parse");
    let snapshot = build.finish();
    let method = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "run")
        .expect("method should be extracted");
    let call = snapshot
        .references
        .iter()
        .find(|reference| reference.name == "backoff")
        .expect("call should be extracted");

    assert_eq!(method.qualified_name, "src::RetryPolicy::RetryPolicy.run");
    assert_eq!(
        method.canonical_symbol_id,
        "repo://repo/src::RetryPolicy::RetryPolicy.run"
    );
    assert_eq!(call.resolution_state, "resolved");
    assert_eq!(call.confidence_tier, "inferred");
}

#[test]
fn nested_functions_are_canonical_symbol_containers() {
    let mut build = snapshot_build();
    let source = br#"
def outer_a():
    def inner():
        return 1
    return inner()

def outer_b():
    def inner():
        return 2
    return inner()
"#;

    parse_indexed_file(&mut build, "src/nested.py", source).expect("file should parse");
    let snapshot = build.finish();
    let inner_names = snapshot
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "inner")
        .map(|symbol| symbol.canonical_symbol_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(inner_names.len(), 2);
    assert!(inner_names.contains(&"repo://repo/src::nested::outer_a.inner"));
    assert!(inner_names.contains(&"repo://repo/src::nested::outer_b.inner"));
}

#[test]
fn rust_tag_and_manual_symbols_share_one_snapshot_identity() {
    let snapshot = parse_snapshot(
        "src/auth.rs",
        br#"
trait S3Auth { async fn get_secret_key(&self); }
struct IAMAuth;
impl S3Auth for IAMAuth { async fn get_secret_key(&self) {} }
impl IAMAuth { fn new() -> Self { IAMAuth } }
"#,
    );

    for (index, symbol) in snapshot.symbols.iter().enumerate() {
        assert!(
            snapshot
                .symbols
                .iter()
                .skip(index + 1)
                .all(|other| other.symbol_snapshot_id != symbol.symbol_snapshot_id),
            "duplicate symbol_snapshot_id for {} at {:?}",
            symbol.name,
            symbol.line_range
        );
    }
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "get_secret_key")
    );
}

#[test]
fn duplicate_symbol_snapshot_identity_keeps_single_symbol_record() {
    let mut output = FileParseOutput::new();

    records::upsert_symbol(&mut output, duplicate_identity_symbol("class"));
    records::upsert_symbol(&mut output, duplicate_identity_symbol("module"));

    assert_eq!(output.symbols.len(), 1);
    assert_eq!(output.symbols[0].symbol_snapshot_id, "symbol:Session");
}

#[test]
fn duplicate_reference_identity_keeps_single_reference_record() {
    let mut output = FileParseOutput::new();

    records::upsert_reference(&mut output, duplicate_reference("call"));
    records::upsert_reference(&mut output, duplicate_reference("implementation"));

    assert_eq!(output.references.len(), 1);
    assert_eq!(output.references[0].reference_id, "reference:run");
}

#[test]
fn extracted_unresolved_references_start_with_low_confidence() {
    let mut build = snapshot_build();

    parse_indexed_file(&mut build, "src/app.rs", b"fn run() { missing(); }\n")
        .expect("file should parse");
    let reference = build
        .references
        .iter()
        .find(|reference| reference.name == "missing")
        .expect("call reference should be extracted before finalization");

    assert_eq!(reference.resolution_state, "unresolved");
    assert_eq!(reference.confidence_basis_points, 2_500);
    assert_eq!(reference.confidence_tier, "ambiguous");
}

#[test]
fn typescript_function_factory_members_are_call_containers() {
    let snapshot = parse_snapshot(
        "src/agent.ts",
        br#"
export const layer = Service.of({
  generate: Effect.fn("Agent.generate")(function* (input: Input) {
    const params = buildParams(input)
    return yield* Effect.promise(() => generateObject(params).then((r) => r.object))
  }),
})
"#,
    );
    let generate = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "generate")
        .expect("function factory object member should be indexed as a symbol");
    let call = snapshot
        .calls
        .iter()
        .find(|call| call.callee_name == "generateObject")
        .expect("generateObject call should be indexed");
    let chunk = snapshot
        .chunks
        .iter()
        .find(|chunk| chunk.symbol_snapshot_id.as_deref() == Some(&generate.symbol_snapshot_id))
        .expect("function factory member should have a retrievable chunk");

    assert_eq!(call.caller_name.as_deref(), Some("generate"));
    assert!(chunk.content.contains("generateObject(params)"));
}

#[test]
fn typescript_data_transform_members_are_not_function_factory_symbols() {
    let snapshot = parse_snapshot(
        "src/converter.ts",
        br#"
const converted = {
  tool_calls: content.filter((c) => c.type === "tool_use").map((c) => ({ id: c.id })),
}
"#,
    );

    assert!(
        snapshot
            .symbols
            .iter()
            .all(|symbol| symbol.name != "tool_calls")
    );
}

#[test]
fn java_object_creation_expressions_are_constructor_call_edges() {
    let snapshot = parse_snapshot(
        "src/app/Worker.java",
        br#"
class Box<T> {}
class Session {}

class Worker {
    Box<Session> create() {
        return new Box<Session>();
    }
}
"#,
    );
    let call = snapshot
        .calls
        .iter()
        .find(|call| call.callee_name == "Box")
        .expect("generic object construction should be indexed as a call to the constructed type");

    assert_eq!(call.caller_name.as_deref(), Some("create"));
    assert_eq!(call.resolution_state, "resolved");
    assert!(
        !snapshot
            .calls
            .iter()
            .any(|call| call.callee_name == "Session"),
        "generic type arguments must not become constructor callees",
    );
}

fn parse_snapshot(path: &str, source: &[u8]) -> crate::domain::CodeIndexSnapshot {
    let mut build = snapshot_build();
    parse_indexed_file(&mut build, path, source).expect("file should parse");
    build.finish()
}

fn snapshot_build() -> SnapshotBuild {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    )
}

fn duplicate_identity_symbol(kind: &str) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        symbol_snapshot_id: "symbol:Session".to_owned(),
        canonical_symbol_id: "src::Session".to_owned(),
        file_id: "file:Session".to_owned(),
        path: "src/Session.swift".to_owned(),
        language_id: "swift".to_owned(),
        name: "Session".to_owned(),
        qualified_name: "src::Session".to_owned(),
        kind: kind.to_owned(),
        signature: "class Session".to_owned(),
        doc_comment: None,
        byte_range: RepositoryCodeRange::new("byte_range", 0, 14)
            .expect("byte range should validate"),
        line_range: RepositoryCodeRange::new("line_range", 1, 1)
            .expect("line range should validate"),
    }
}

fn duplicate_reference(kind: &str) -> RepositoryCodeReferenceRecord {
    RepositoryCodeReferenceRecord {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        reference_id: "reference:run".to_owned(),
        file_id: "file:run".to_owned(),
        path: "src/lib.rs".to_owned(),
        name: "run".to_owned(),
        kind: kind.to_owned(),
        target_symbol_snapshot_id: None,
        target_hint: Some("run".to_owned()),
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 5_000,
        confidence_tier: "ambiguous".to_owned(),
        byte_range: RepositoryCodeRange::new("byte_range", 10, 13)
            .expect("byte range should validate"),
        line_range: RepositoryCodeRange::new("line_range", 2, 2)
            .expect("line range should validate"),
    }
}
