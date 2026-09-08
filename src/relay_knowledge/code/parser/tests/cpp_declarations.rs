//! Real C++ tag captures preserve prototype versus implementation identity.
use super::*;

#[test]
fn cpp_declarators_preserve_prototypes_without_reclassifying_bodies() {
    let registration = crate::domain::CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        vec![],
        vec![],
    )
    .unwrap();
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
        "src/main.cpp",
        br#"
void helper();
void helper();
void leaf() {}
/** Outer documentation. */
void outer() {
 /** Inner prototype documentation. */
 void nested();
}
void helper() { leaf(); }
struct Worker { void run(); void inline_body() {} };
void Worker::run() { helper(); }
template<class T> void generic(T item);
template<class T> void defined(T item) { helper(); }
"#,
    )
    .unwrap();
    let snapshot = build.finish();
    let helpers = snapshot
        .symbols
        .iter()
        .filter(|s| s.name == "helper")
        .collect::<Vec<_>>();
    assert_eq!(helpers.len(), 3);
    assert_eq!(
        helpers
            .iter()
            .filter(|s| s.kind == "function_declaration")
            .count(),
        2
    );
    assert_eq!(
        helpers
            .iter()
            .filter(
                |s| crate::domain::code_call_targets::callable_definition_symbol(
                    &s.kind,
                    &s.signature
                )
            )
            .count(),
        1
    );
    let nested = snapshot
        .symbols
        .iter()
        .find(|s| s.name == "nested")
        .unwrap();
    assert_eq!(nested.kind, "function_declaration");
    assert_eq!(
        nested.doc_comment.as_deref(),
        Some("Inner prototype documentation.")
    );
    for name in ["run", "generic"] {
        assert!(
            snapshot
                .symbols
                .iter()
                .any(|s| s.name == name && s.kind == "function_declaration"),
            "{name}: {:?}",
            snapshot.symbols
        );
    }
    for name in ["inline_body", "defined"] {
        assert!(
            snapshot.symbols.iter().any(|s| s.name == name
                && crate::domain::code_call_targets::callable_definition_symbol(
                    &s.kind,
                    &s.signature
                )),
            "{name}: {:?}",
            snapshot.symbols
        );
    }
}
